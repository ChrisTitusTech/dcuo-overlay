//! Bounded Windows client-region capture and opt-in installed Tesseract OCR.
use crate::process;
use std::{
    io::{Cursor, Read, Write},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::*,
    UI::WindowsAndMessaging::{
        GetClientRect, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    },
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Tradable,
    Browse,
    Other,
}
#[derive(Debug)]
pub struct Screen {
    pub panel: Panel,
    pub selected_name: Option<String>,
    pub usable_by_me: Option<bool>,
    pub observed_at: Instant,
}
#[derive(Clone, Copy)]
struct Roi {
    x: i16,
    y: i16,
    w: u16,
    h: u16,
}
// Coordinates proven only for the recorded 2560x1440 English Broker layout.
const HEADER: Roi = Roi {
    x: 1050,
    y: 375,
    w: 500,
    h: 48,
};
const NAME: Roi = Roi {
    x: 1050,
    y: 985,
    w: 565,
    h: 43,
};
const CHECKBOX: Roi = Roi {
    x: 660,
    y: 505,
    w: 6,
    h: 6,
};
fn classify_checkbox(pixels: &[u8]) -> Option<bool> {
    if pixels.len() != 36 * 3 {
        return None;
    }
    let (mut red, mut green, mut blue) = (0u32, 0u32, 0u32);
    for p in pixels.chunks_exact(3) {
        red += u32::from(p[0]);
        green += u32::from(p[1]);
        blue += u32::from(p[2]);
    }
    red /= 36;
    green /= 36;
    blue /= 36;
    if green > 150 && green > blue + 40 {
        Some(true)
    } else if blue > green + 10 && blue > red + 20 {
        Some(false)
    } else {
        None
    }
}
fn geometry(w: u16, h: u16, roi: Roi) -> bool {
    w == 2560
        && h == 1440
        && roi.x >= 0
        && roi.y >= 0
        && u32::from(roi.x as u16) + u32::from(roi.w) <= u32::from(w)
        && u32::from(roi.y as u16) + u32::from(roi.h) <= u32::from(h)
}
fn parse_tsv(tsv: &str, roi: Roi) -> Result<Option<String>, String> {
    if tsv.lines().next()
        != Some(
            "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext",
        )
    {
        return Err(err("Missing OCR TSV header"));
    }
    let mut words = Vec::new();
    let mut line = None;
    let mut right = 0;
    for row in tsv.lines().skip(1) {
        let c: Vec<_> = row.splitn(12, '\t').collect();
        if c.len() != 12 {
            return Err(err("Malformed OCR TSV"));
        }
        if c[0] != "5" || c[11].trim().is_empty() {
            continue;
        }
        let key = (c[1], c[2], c[3], c[4]);
        if line.is_some_and(|l| l != key) {
            return Err(err("Ambiguous OCR lines"));
        }
        line = Some(key);
        let number = |i: usize| c[i].parse::<u32>().map_err(err);
        let (x, y, w, h) = (number(6)?, number(7)?, number(8)?, number(9)?);
        let confidence = c[10].parse::<f32>().map_err(err)?;
        if !confidence.is_finite()
            || !(80.0..=100.0).contains(&confidence)
            || x < right
            || w == 0
            || h == 0
            || x.checked_add(w).is_none_or(|v| v > u32::from(roi.w))
            || y.checked_add(h).is_none_or(|v| v > u32::from(roi.h))
        {
            return Err(err("Uncertain or out-of-bounds OCR"));
        }
        right = x + w;
        words.push(c[11]);
    }
    if words.is_empty() {
        return Ok(None);
    }
    let text = words.join(" ");
    if text.len() > 96 || !text.bytes().all(|b| (32..=126).contains(&b)) {
        return Err(err("Invalid OCR name"));
    }
    Ok(Some(text))
}

fn err(e: impl std::fmt::Display) -> String {
    format!("Broker screen unavailable: {e}")
}
pub struct Monitor {
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<Screen, String>>,
}
impl Monitor {
    pub fn start(pid: i32) -> Result<Self, String> {
        if pid <= 0 {
            return Err(err("Invalid selected game PID"));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        let (send, result) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("broker-screen".into())
            .spawn(move || {
                let binding = (|| {
                    let candidate = process::candidate(pid).map_err(err)?;
                    if process::executable_metadata(&candidate)
                        .map_err(err)?
                        .sha256
                        != crate::native::PROFILE_ID
                    {
                        return Err(err("Unsupported game build"));
                    }
                    Ok(candidate)
                })();
                let candidate = match binding {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = send.try_send(Err(e));
                        return;
                    }
                };
                while !c.load(Ordering::Acquire) {
                    let result = observe(pid, &candidate, &c);
                    if send
                        .try_send(result)
                        .is_err_and(|e| matches!(e, mpsc::TrySendError::Disconnected(_)))
                    {
                        break;
                    }
                    for _ in 0..20 {
                        if c.load(Ordering::Acquire) {
                            return;
                        }
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            })
            .map_err(err)?;
        Ok(Self { cancel, result })
    }
    pub fn poll(&self) -> Option<Result<Screen, String>> {
        match self.result.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(err("Screen worker stopped"))),
        }
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release)
    }
}
fn bound_window(pid: i32) -> Result<HWND, String> {
    crate::broker_hotkey::session_focused(pid)?;
    unsafe {
        let w = GetForegroundWindow();
        let mut owner = 0;
        GetWindowThreadProcessId(w, &mut owner);
        let mut r: RECT = std::mem::zeroed();
        if w.is_null()
            || owner != pid as u32
            || IsIconic(w) != 0
            || IsWindowVisible(w) == 0
            || GetClientRect(w, &mut r) == 0
            || r.right - r.left != 2560
            || r.bottom - r.top != 1440
        {
            return Err(err(
                "Requires foreground game with proven 2560x1440 client layout",
            ));
        }
        Ok(w)
    }
}
fn observe(pid: i32, binding: &process::Candidate, cancel: &AtomicBool) -> Result<Screen, String> {
    let validate = || -> Result<HWND, String> {
        let candidate = process::candidate(pid).map_err(err)?;
        if candidate.identity != binding.identity || candidate.mapping != binding.mapping {
            return Err(err("Selected game changed"));
        }
        bound_window(pid)
    };
    let w = validate()?;
    let observed_at = Instant::now();
    let checkbox = capture(w, CHECKBOX)?;
    let ocr_roi = |r: Roi| -> Result<Option<String>, String> {
        validate()?;
        let rgb = capture(w, r)?;
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(rgb)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(err)?;
        parse_tsv(&ocr(&png.into_inner(), cancel)?, r)
    };
    let title = ocr_roi(HEADER)?;
    let panel = match title.as_deref() {
        Some("Tradable Inventory") => Panel::Tradable,
        Some("Browse") => Panel::Browse,
        _ => Panel::Other,
    };
    let usable_by_me = if panel == Panel::Other {
        None
    } else {
        classify_checkbox(checkbox.as_raw())
    };
    let selected_name = if panel == Panel::Tradable {
        ocr_roi(NAME)?
    } else {
        None
    };
    if validate()? != w {
        return Err(err("Game window changed during observation"));
    }
    Ok(Screen {
        panel,
        selected_name,
        usable_by_me,
        observed_at,
    })
}
fn capture(window: HWND, r: Roi) -> Result<image::RgbImage, String> {
    if !geometry(2560, 1440, r) {
        return Err(err("Invalid bounded crop"));
    }
    // All GDI handles are worker-owned and released on every return path.
    unsafe {
        let dc = GetDC(window);
        if dc.is_null() {
            return Err(err("GetDC failed"));
        }
        let memory = CreateCompatibleDC(dc);
        if memory.is_null() {
            ReleaseDC(window, dc);
            return Err(err("CreateCompatibleDC failed"));
        }
        let bitmap = CreateCompatibleBitmap(dc, i32::from(r.w), i32::from(r.h));
        if bitmap.is_null() {
            DeleteDC(memory);
            ReleaseDC(window, dc);
            return Err(err("CreateCompatibleBitmap failed"));
        }
        let old = SelectObject(memory, bitmap);
        let result = (|| {
            if old.is_null() || old as isize == -1 {
                return Err(err("SelectObject failed"));
            }
            if BitBlt(
                memory,
                0,
                0,
                i32::from(r.w),
                i32::from(r.h),
                dc,
                i32::from(r.x),
                i32::from(r.y),
                SRCCOPY,
            ) == 0
            {
                return Err(err("Client capture failed"));
            }
            // GetDIBits requires the bitmap to be deselected.
            SelectObject(memory, old);
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = i32::from(r.w);
            info.bmiHeader.biHeight = -i32::from(r.h);
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB;
            let mut bytes = vec![0u8; usize::from(r.w) * usize::from(r.h) * 4];
            if GetDIBits(
                dc,
                bitmap,
                0,
                u32::from(r.h),
                bytes.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            ) != i32::from(r.h)
            {
                return Err(err("Incomplete client capture"));
            }
            let rgb = bytes
                .chunks_exact(4)
                .flat_map(|b| [b[2], b[1], b[0]])
                .collect();
            image::RgbImage::from_raw(u32::from(r.w), u32::from(r.h), rgb)
                .ok_or_else(|| err("Invalid crop"))
        })();
        SelectObject(memory, old);
        DeleteObject(bitmap);
        DeleteDC(memory);
        ReleaseDC(window, dc);
        result
    }
}
fn ocr(png: &[u8], cancel: &AtomicBool) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let mut child = Command::new("tesseract.exe")
        .args(["stdin", "stdout", "-l", "eng", "--psm", "7", "tsv"])
        .creation_flags(0x08000000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            err(format!(
                "Install Tesseract with English language data and add it to PATH: {e}"
            ))
        })?;
    let mut input = child.stdin.take().ok_or_else(|| err("Missing OCR input"))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| err("Missing OCR output"))?;
    let bytes = png.to_vec();
    let writer = thread::spawn(move || input.write_all(&bytes));
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        output.take(65537).read_to_end(&mut bytes).map(|_| bytes)
    });
    let start = Instant::now();
    let result = loop {
        if cancel.load(Ordering::Acquire) || start.elapsed() > Duration::from_secs(2) {
            break Err(err("OCR cancelled or timed out"));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err(err("OCR failed"))
                };
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(e) => break Err(err(e)),
        }
    };
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let written = writer.join().map_err(|_| err("OCR input worker stopped"))?;
    let bytes = reader
        .join()
        .map_err(|_| err("OCR output worker stopped"))?
        .map_err(err)?;
    result?;
    written.map_err(err)?;
    if bytes.len() > 65536 {
        return Err(err("OCR output exceeds bound"));
    }
    String::from_utf8(bytes).map_err(err)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crop_bounds() {
        assert!(geometry(2560, 1440, HEADER));
        assert!(!geometry(1920, 1080, HEADER));
        assert!(!geometry(
            2560,
            1440,
            Roi {
                x: 2559,
                y: 0,
                w: 2,
                h: 1
            }
        ));
    }
    #[test]
    fn uncertain_pixels_fail_closed() {
        assert_eq!(classify_checkbox(&[0; 108]), None);
    }
}

#[cfg(test)]
mod desktop_tests {
    use super::*;
    /// Run only on an isolated interactive Windows test desktop, never alongside a game.
    #[test]
    #[ignore = "requires DCUO_WINDOWS_DESKTOP_TEST=1 on an isolated interactive Windows desktop"]
    fn native_test_window_capture() {
        assert_eq!(
            std::env::var("DCUO_WINDOWS_DESKTOP_TEST").as_deref(),
            Ok("1")
        );
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WS_OVERLAPPEDWINDOW,
        };
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let title: Vec<u16> = "DCUO isolated capture test\0".encode_utf16().collect();
        unsafe {
            let w = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!w.is_null());
            let dc = GetDC(w);
            assert!(!dc.is_null());
            let brush = CreateSolidBrush(0x00332211);
            let rect = RECT {
                left: 0,
                top: 0,
                right: 16,
                bottom: 16,
            };
            assert_ne!(FillRect(dc, &rect, brush), 0);
            DeleteObject(brush);
            ReleaseDC(w, dc);
            let result = capture(
                w,
                Roi {
                    x: 0,
                    y: 0,
                    w: 8,
                    h: 8,
                },
            );
            DestroyWindow(w);
            let rgb = result.expect("bounded native test-window capture");
            assert_eq!(rgb.dimensions(), (8, 8));
            assert!(rgb.pixels().all(|p| p.0 == [0x11, 0x22, 0x33]));
        }
    }
}
