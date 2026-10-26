//! Bounded, read-only OCR observations. No pixels or OCR output are persisted.
use crate::process;
use std::{
    collections::HashSet,
    io::{Cursor, Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, ConnectionExt, ImageFormat, ImageOrder, MapState},
    rust_connection::RustConnection,
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
pub struct Monitor {
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<Screen, String>>,
}
impl Monitor {
    pub fn start(pid: i32) -> Result<Self, String> {
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (send, result) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("broker-screen".into())
            .spawn(move || {
                let binding = (|| -> Result<Vec<process::Identity>, String> {
                    let candidate = process::candidate(pid).map_err(err)?;
                    if process::executable_metadata(&candidate)
                        .map_err(err)?
                        .sha256
                        != crate::native::PROFILE_ID
                    {
                        return Err(err("Unsupported game build"));
                    }
                    let owners = ancestry(pid)?;
                    if owners.first() != Some(&candidate.identity) {
                        return Err(err("Selected game changed"));
                    }
                    Ok(owners)
                })();
                let owners = match binding {
                    Ok(owners) => owners,
                    Err(error) => {
                        let _ = send.try_send(Err(error));
                        return;
                    }
                };
                while !worker_cancel.load(Ordering::Acquire) {
                    let result = observe(pid, &owners, &worker_cancel);
                    if send
                        .try_send(result)
                        .is_err_and(|e| matches!(e, mpsc::TrySendError::Disconnected(_)))
                    {
                        break;
                    }
                    for _ in 0..20 {
                        if worker_cancel.load(Ordering::Acquire) {
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
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(err("Screen worker stopped"))),
        }
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
fn err(e: impl std::fmt::Display) -> String {
    format!("Broker screen unavailable: {e}")
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
fn observe(pid: i32, owners: &[process::Identity], cancel: &AtomicBool) -> Result<Screen, String> {
    if ancestry(pid)? != owners {
        return Err(err("Game session or ancestry changed"));
    }
    let (c, screen) = x11rb::connect(None).map_err(err)?;
    let root = c.setup().roots[screen].root;
    let windows = clients(&c, root, owners)?;
    if windows.len() != 1 {
        return Err(err("Expected one bound game window"));
    }
    let w = windows[0];
    let g = c.get_geometry(w).map_err(err)?.reply().map_err(err)?;
    if !geometry(g.width, g.height, HEADER)
        || !geometry(g.width, g.height, NAME)
        || !geometry(g.width, g.height, CHECKBOX)
    {
        return Err(err("Requires proven 2560x1440 layout"));
    }
    if c.get_window_attributes(w)
        .map_err(err)?
        .reply()
        .map_err(err)?
        .map_state
        != MapState::VIEWABLE
    {
        return Err(err("Game window is not viewable"));
    }
    let observed_at = Instant::now();
    let checkbox_pixels = capture_rgb(&c, w, CHECKBOX)?;
    let title = parse_tsv(&ocr(&capture(&c, w, HEADER)?, cancel)?, HEADER)?;
    let panel = match title.as_deref() {
        Some("Tradable Inventory") => Panel::Tradable,
        Some("Browse") => Panel::Browse,
        _ => Panel::Other,
    };
    let usable_by_me = if panel == Panel::Other {
        None
    } else {
        classify_checkbox(checkbox_pixels.as_raw())
    };
    let selected_name = if panel == Panel::Tradable {
        parse_tsv(&ocr(&capture(&c, w, NAME)?, cancel)?, NAME)?
    } else {
        None
    };
    if ancestry(pid)? != owners || clients(&c, root, owners)? != windows {
        return Err(err("Game changed during observation"));
    }
    Ok(Screen {
        panel,
        selected_name,
        usable_by_me,
        observed_at,
    })
}
fn capture(c: &RustConnection, window: u32, r: Roi) -> Result<Vec<u8>, String> {
    let image = capture_rgb(c, window, r)?;
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(err)?;
    Ok(png.into_inner())
}
fn capture_rgb(c: &RustConnection, window: u32, r: Roi) -> Result<image::RgbImage, String> {
    let a = c
        .get_window_attributes(window)
        .map_err(err)?
        .reply()
        .map_err(err)?;
    let visual = c
        .setup()
        .roots
        .iter()
        .flat_map(|s| &s.allowed_depths)
        .flat_map(|d| &d.visuals)
        .find(|v| v.visual_id == a.visual)
        .ok_or_else(|| err("Unknown visual"))?;
    let reply = c
        .get_image(ImageFormat::Z_PIXMAP, window, r.x, r.y, r.w, r.h, u32::MAX)
        .map_err(err)?
        .reply()
        .map_err(err)?;
    let format = c
        .setup()
        .pixmap_formats
        .iter()
        .find(|f| f.depth == reply.depth)
        .ok_or_else(|| err("Unknown pixel format"))?;
    if reply.depth != 24
        || format.bits_per_pixel != 32
        || visual.red_mask != 0xff0000
        || visual.green_mask != 0xff00
        || visual.blue_mask != 0xff
    {
        return Err(err("Unsupported capture pixel format"));
    }
    let expected = usize::from(r.w) * usize::from(r.h) * 4;
    if reply.data.len() != expected {
        return Err(err("Invalid bounded capture length"));
    }
    let mut rgb = Vec::with_capacity(expected / 4 * 3);
    for p in reply.data.chunks_exact(4) {
        let pixel = if c.setup().image_byte_order == ImageOrder::LSB_FIRST {
            u32::from_le_bytes(p.try_into().map_err(err)?)
        } else {
            u32::from_be_bytes(p.try_into().map_err(err)?)
        };
        rgb.extend_from_slice(&[
            ((pixel >> 16) & 255) as u8,
            ((pixel >> 8) & 255) as u8,
            (pixel & 255) as u8,
        ]);
    }
    image::RgbImage::from_raw(u32::from(r.w), u32::from(r.h), rgb)
        .ok_or_else(|| err("Invalid crop"))
}

fn nonblocking(fd: std::os::fd::RawFd) -> Result<(), String> {
    // Only changes flags on this worker's owned subprocess pipe.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(err(std::io::Error::last_os_error()));
    }
    Ok(())
}
fn ocr(png: &[u8], cancel: &AtomicBool) -> Result<String, String> {
    use std::os::fd::AsRawFd;
    let mut child = Command::new("tesseract")
        .args(["stdin", "stdout", "-l", "eng", "--psm", "7", "tsv"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(err)?;
    let result = (|| {
        let input = child.stdin.take().ok_or_else(|| err("Missing OCR input"))?;
        let mut output = child
            .stdout
            .take()
            .ok_or_else(|| err("Missing OCR output"))?;
        nonblocking(input.as_raw_fd())?;
        nonblocking(output.as_raw_fd())?;
        let start = Instant::now();
        let mut sent = 0;
        let mut bytes = Vec::new();
        let mut input = Some(input);
        let mut eof = false;
        loop {
            if cancel.load(Ordering::Acquire) || start.elapsed() > Duration::from_secs(2) {
                return Err(err("OCR cancelled or timed out"));
            }
            if let Some(pipe) = input.as_mut() {
                match pipe.write(&png[sent..]) {
                    Ok(n) => sent += n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(err(e)),
                }
                if sent == png.len() {
                    input = None;
                }
            }
            let mut buf = [0; 4096];
            loop {
                match output.read(&mut buf) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        if bytes.len() + n > 65536 {
                            return Err(err("OCR output exceeds bound"));
                        }
                        bytes.extend_from_slice(&buf[..n]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(err(e)),
                }
            }
            if let Some(status) = child.try_wait().map_err(err)? {
                if !status.success() {
                    return Err(err("OCR process failed"));
                }
                if eof {
                    return String::from_utf8(bytes).map_err(err);
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
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
fn identity(pid: i32) -> Result<(process::Identity, i32), String> {
    let stat = process::bounded_text(Path::new(&format!("/proc/{pid}/stat"))).map_err(err)?;
    let id = process::parse_stat(pid, &stat).map_err(err)?;
    let end = stat.rfind(')').ok_or("Invalid process ancestry")?;
    let parent = stat[end + 1..]
        .split_whitespace()
        .nth(1)
        .ok_or("Missing parent PID")?
        .parse::<i32>()
        .map_err(err)?;
    Ok((id, parent))
}
fn ancestry(pid: i32) -> Result<Vec<process::Identity>, String> {
    if pid <= 0 {
        return Err("Invalid selected game PID".into());
    }
    let mut result = Vec::new();
    let mut current = pid;
    let mut visited = HashSet::new();
    for _ in 0..16 {
        if !visited.insert(current) {
            return Err("Cyclic process ancestry".into());
        }
        let (id, parent) = identity(current)?;
        result.push(id);
        if parent <= 0 {
            return Ok(result);
        }
        current = parent;
    }
    Err("Process ancestry exceeds bound".into())
}
fn atom(c: &RustConnection, name: &[u8]) -> Result<u32, String> {
    Ok(c.intern_atom(false, name)
        .map_err(err)?
        .reply()
        .map_err(err)?
        .atom)
}
fn property(
    c: &RustConnection,
    w: u32,
    name: &[u8],
    kind: u32,
    len: u32,
) -> Result<Vec<u8>, String> {
    let reply = c
        .get_property(false, w, atom(c, name)?, kind, 0, len)
        .map_err(err)?
        .reply()
        .map_err(err)?;
    if reply.bytes_after != 0 {
        return Err("Window property exceeds bound".into());
    }
    Ok(reply.value)
}
fn window_pid(c: &RustConnection, w: u32) -> Result<Option<i32>, String> {
    let bytes = property(c, w, b"_NET_WM_PID", AtomEnum::CARDINAL.into(), 1)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let bytes: [u8; 4] = bytes
        .try_into()
        .map_err(|_| "Invalid window PID property")?;
    Ok(i32::try_from(u32::from_ne_bytes(bytes)).ok())
}
fn game_title(c: &RustConnection, w: u32) -> Result<bool, String> {
    let mut bytes = property(c, w, b"_NET_WM_NAME", atom(c, b"UTF8_STRING")?, 256)?;
    if bytes.is_empty() {
        bytes = property(c, w, b"WM_NAME", AtomEnum::STRING.into(), 256)?;
    }
    let title = String::from_utf8(bytes).map_err(err)?.to_ascii_lowercase();
    Ok(title.contains("dc universe online") || title.contains("dcuniverseonline"))
}
fn clients(
    c: &RustConnection,
    root: u32,
    owners: &[process::Identity],
) -> Result<Vec<u32>, String> {
    let bytes = property(c, root, b"_NET_CLIENT_LIST", AtomEnum::WINDOW.into(), 4096)?;
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return Err("No bounded desktop client list".into());
    }
    let mut result = Vec::new();
    for value in bytes.chunks_exact(4) {
        let w = u32::from_ne_bytes(value.try_into().map_err(err)?);
        if window_pid(c, w)?.is_some_and(|pid| owners.iter().any(|owner| owner.pid == pid))
            && game_title(c, w)?
        {
            result.push(w);
        }
    }
    result.sort_unstable();
    result.dedup();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tsv(words: &[(&str, &str, u32, u32)]) -> String {
        let mut text = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n".to_owned();
        for (i, (word, confidence, x, line)) in words.iter().enumerate() {
            text.push_str(&format!(
                "5\t1\t1\t1\t{line}\t{}\t{x}\t2\t20\t20\t{confidence}\t{word}\n",
                i + 1
            ));
        }
        text
    }
    #[test]
    fn reconstructs_exact_words() {
        assert_eq!(
            parse_tsv(
                &tsv(&[("Promethium", "91.5", 2, 1), ("Research", "95", 30, 1)]),
                NAME
            )
            .unwrap()
            .as_deref(),
            Some("Promethium Research")
        );
    }
    #[test]
    fn rejects_uncertain_ambiguous_and_overlapping_words() {
        for words in [
            vec![("Name", "79.9", 2, 1)],
            vec![("Name", "NaN", 2, 1)],
            vec![("A", "99", 2, 1), ("B", "99", 30, 2)],
            vec![("A", "99", 2, 1), ("B", "99", 10, 1)],
            vec![("A", "99", 560, 1)],
        ] {
            assert!(parse_tsv(&tsv(&words), NAME).is_err());
        }
    }
    #[test]
    fn empty_is_absent_and_malformed_fails() {
        assert!(parse_tsv(&tsv(&[]), NAME).unwrap().is_none());
        assert!(parse_tsv("header\nbad", NAME).is_err());
        assert!(parse_tsv(&tsv(&[("é", "99", 2, 1)]), NAME).is_err());
    }
    #[test]
    fn checkbox_requires_clear_color_and_exact_size() {
        assert_eq!(classify_checkbox(&[180, 237, 61].repeat(36)), Some(true));
        assert_eq!(classify_checkbox(&[70, 125, 158].repeat(36)), Some(false));
        assert_eq!(classify_checkbox(&[100, 100, 100].repeat(36)), None);
        assert_eq!(classify_checkbox(&[70, 125, 158].repeat(35)), None);
        assert!(geometry(2560, 1440, CHECKBOX));
    }
    #[test]
    fn only_proven_geometry() {
        assert!(geometry(2560, 1440, HEADER));
        assert!(geometry(2560, 1440, NAME));
        assert!(!geometry(1920, 1080, HEADER));
        assert!(!geometry(
            2560,
            1440,
            Roi {
                x: 2550,
                y: 0,
                w: 20,
                h: 20
            }
        ));
    }
}
