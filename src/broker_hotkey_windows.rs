//! Guarded, one-shot Windows keyboard input. Never activates or raises a window.
use crate::process;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::UI::{
    Input::KeyboardAndMouse::*,
    WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    },
};
#[derive(Debug, PartialEq)]
pub enum Event {
    Listening,
    Detected,
    Pressed(Instant),
    Started(Instant),
    ReadyToSearch(Instant),
    Finished(Result<(), String>),
}
pub struct Hotkey {
    cancel: Arc<AtomicBool>,
    permit: Arc<AtomicBool>,
    result: Receiver<Event>,
    completed: bool,
}
impl Hotkey {
    pub fn confirm_search(&self) {
        self.permit.store(true, Ordering::Release)
    }
    pub fn poll(&mut self) -> Option<Event> {
        if self.completed {
            return None;
        }
        match self.result.try_recv() {
            Ok(e) => {
                self.completed = matches!(e, Event::Finished(_));
                Some(e)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => {
                self.completed = true;
                Some(Event::Finished(Err("Browse hotkey worker stopped".into())))
            }
        }
    }
}
impl Drop for Hotkey {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release)
    }
}
fn valid(pid: i32, text: &str) -> bool {
    pid > 0
        && !text.trim().is_empty()
        && text.len() <= 96
        && text.bytes().all(|b| (32..=126).contains(&b))
}
pub fn session_focused(pid: i32) -> Result<(), String> {
    if pid <= 0 {
        return Err("Invalid game PID".into());
    }
    unsafe {
        let w = GetForegroundWindow();
        let mut owner = 0;
        GetWindowThreadProcessId(w, &mut owner);
        if w.is_null() || owner != pid as u32 || IsWindowVisible(w) == 0 || IsIconic(w) != 0 {
            return Err("Selected game must remain the foreground window".into());
        }
    }
    Ok(())
}
pub fn arm(pid: i32, name: &str) -> Result<Hotkey, String> {
    start(pid, name, true, false, false, false)
}
pub fn arm_text(pid: i32, name: &str) -> Result<Hotkey, String> {
    start(pid, name, false, true, false, false)
}
pub fn arm_price(pid: i32, price: u64) -> Result<Hotkey, String> {
    price_action(pid, price, false)
}
pub fn trigger_price(pid: i32, price: u64) -> Result<Hotkey, String> {
    price_action(pid, price, true)
}
fn price_action(pid: i32, price: u64, triggered: bool) -> Result<Hotkey, String> {
    if price == 0 || price > u32::MAX as u64 {
        return Err("Price must be between 1 and 4294967295".into());
    }
    start(pid, &price.to_string(), false, false, triggered, false)
}
pub fn trigger_text(pid: i32, name: &str) -> Result<Hotkey, String> {
    start(pid, name, false, true, true, false)
}
pub fn listen(pid: i32) -> Result<Hotkey, String> {
    start(pid, "", false, false, false, true)
}
fn down(key: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(key as i32) < 0 }
}
fn released() -> bool {
    !(1..=254).any(down)
}
fn quiet() -> Result<(), String> {
    if !released() || unsafe { GetKeyState(VK_CAPITAL as i32) } & 1 != 0 {
        return Err("Release all keys and turn off Caps Lock before Browse input".into());
    }
    Ok(())
}
fn stroke(key: VIRTUAL_KEY, unicode: bool, control: bool) -> Result<(), String> {
    let input = |vk, scan, flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let flags = if unicode { KEYEVENTF_UNICODE } else { 0 };
    let (vk, scan) = if unicode { (0, key) } else { (key, 0) };
    let mut keys = Vec::new();
    if control {
        keys.push(input(VK_CONTROL, 0, 0));
    }
    keys.push(input(vk, scan, flags));
    keys.push(input(vk, scan, flags | KEYEVENTF_KEYUP));
    if control {
        keys.push(input(VK_CONTROL, 0, KEYEVENTF_KEYUP));
    }
    let sent = unsafe {
        SendInput(
            keys.len() as u32,
            keys.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent != keys.len() as u32 {
        // Release only synthetic keys, including a partially accepted batch.
        let mut release = vec![input(vk, scan, flags | KEYEVENTF_KEYUP)];
        if control {
            release.push(input(VK_CONTROL, 0, KEYEVENTF_KEYUP));
        }
        unsafe {
            SendInput(
                release.len() as u32,
                release.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            );
        }
        return Err(
            "Windows rejected keyboard input (check equal application privilege levels)".into(),
        );
    }
    Ok(())
}
// Foreground ownership matters when accepting input, not while waiting idle.
fn await_chord(
    mut identity: impl FnMut() -> Result<(), String>,
    mut keys: impl FnMut() -> (bool, bool),
    mut focus: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let mut ready = false;
    loop {
        identity()?;
        let (released, chord) = keys();
        if released {
            ready = true;
        }
        if ready && chord {
            focus()?;
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn start(
    pid: i32,
    text: &str,
    submit: bool,
    settle: bool,
    triggered: bool,
    listen: bool,
) -> Result<Hotkey, String> {
    if pid <= 0 || (!listen && !valid(pid, text)) {
        return Err("Select a game and 1-96 printable ASCII characters".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let permit = Arc::new(AtomicBool::new(false));
    let (send, result) = mpsc::channel();
    let c = cancel.clone();
    let p = permit.clone();
    let text = text.to_owned();
    thread::Builder::new()
        .name("broker-hotkey".into())
        .spawn(move || {
            let result = (|| {
                let started = Instant::now();
                let candidate = process::candidate(pid).map_err(|e| e.to_string())?;
                if process::executable_metadata(&candidate)
                    .map_err(|e| e.to_string())?
                    .sha256
                    != crate::native::PROFILE_ID
                {
                    return Err("Unsupported game build for Browse input".into());
                }
                let check_identity = || -> Result<(), String> {
                    if c.load(Ordering::Acquire) || started.elapsed() >= Duration::from_secs(60) {
                        return Err("Browse input cancelled or expired".into());
                    }
                    let current = process::candidate(pid).map_err(|e| e.to_string())?;
                    if current.identity != candidate.identity
                        || current.mapping != candidate.mapping
                    {
                        return Err("Game process changed".into());
                    }
                    Ok(())
                };
                let check = || -> Result<(), String> {
                    check_identity()?;
                    session_focused(pid)
                };
                send.send(Event::Listening).map_err(|e| e.to_string())?;
                if !triggered {
                    await_chord(
                        check_identity,
                        || {
                            (
                                released(),
                                down(VK_CONTROL) && down(VK_MENU) && down(b'P' as u16),
                            )
                        },
                        || session_focused(pid),
                    )?;
                    send.send(Event::Detected).map_err(|e| e.to_string())?;
                }
                while !released() {
                    check()?;
                    thread::sleep(Duration::from_millis(10));
                }
                check()?;
                if listen {
                    send.send(Event::Pressed(Instant::now()))
                        .map_err(|e| e.to_string())?;
                    return Ok(());
                }
                let confirm = || -> Result<(), String> {
                    p.store(false, Ordering::Release);
                    let ready = Instant::now();
                    send.send(Event::ReadyToSearch(ready))
                        .map_err(|e| e.to_string())?;
                    loop {
                        check()?;
                        if ready.elapsed() >= Duration::from_secs(10) {
                            return Err("Browse confirmation timed out".into());
                        }
                        if p.swap(false, Ordering::AcqRel) {
                            return Ok(());
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                };
                check()?;
                quiet()?;
                send.send(Event::Started(Instant::now()))
                    .map_err(|e| e.to_string())?;
                stroke(b'A' as u16, false, true)?;
                for ch in text.encode_utf16() {
                    check()?;
                    quiet()?;
                    stroke(ch, true, false)?;
                    thread::sleep(Duration::from_millis(35));
                    check()?;
                }
                if submit {
                    confirm()?;
                    check()?;
                    quiet()?;
                    stroke(VK_RETURN, false, false)?;
                }
                if settle {
                    confirm()?;
                }
                check()
            })();
            let _ = send.send(Event::Finished(result));
        })
        .map_err(|e| e.to_string())?;
    Ok(Hotkey {
        cancel,
        permit,
        result,
        completed: false,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_focus_loss_does_not_terminate_listener() {
        use std::cell::Cell;
        let tick = Cell::new(0);
        let identity_checks = Cell::new(0);
        let focus_checks = Cell::new(0);
        await_chord(
            || {
                identity_checks.set(identity_checks.get() + 1);
                Ok(())
            },
            || {
                tick.set(tick.get() + 1);
                // Released and waiting while another application is focused.
                (tick.get() == 1, tick.get() == 4)
            },
            || {
                focus_checks.set(focus_checks.get() + 1);
                if tick.get() < 4 {
                    Err("another application has focus".into())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap();
        assert_eq!(identity_checks.get(), 4);
        assert_eq!(focus_checks.get(), 1);
    }
    #[test]
    fn chord_requires_focus_and_idle_checks_identity() {
        let mut tick = 0;
        assert!(
            await_chord(
                || Ok(()),
                || {
                    tick += 1;
                    (tick == 1, tick == 2)
                },
                || Err("game not focused".into())
            )
            .is_err()
        );
        assert!(
            await_chord(
                || Err("session changed".into()),
                || panic!("keys must not be read after invalidation"),
                || panic!("focus is irrelevant while idle")
            )
            .is_err()
        );
    }
    #[test]
    fn input_bounds() {
        assert!(!valid(0, "item"));
        assert!(!valid(1, "\n"));
        assert!(!valid(1, &"x".repeat(97)));
        assert!(valid(1, "Item: Test"));
        assert!(arm_price(1, 0).is_err());
        assert!(arm_price(1, u32::MAX as u64 + 1).is_err());
    }
}

#[cfg(test)]
mod desktop_tests {
    use super::*;
    #[test]
    #[ignore = "requires DCUO_WINDOWS_DESKTOP_TEST=1 on an isolated interactive Windows desktop"]
    fn native_test_window_unicode_input() {
        assert_eq!(
            std::env::var("DCUO_WINDOWS_DESKTOP_TEST").as_deref(),
            Ok("1")
        );
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let class: Vec<u16> = "EDIT\0".encode_utf16().collect();
        let title: Vec<u16> = "\0".encode_utf16().collect();
        unsafe {
            let w = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                0,
                0,
                320,
                100,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!w.is_null());
            let result = (|| -> Result<(), String> {
                if SetForegroundWindow(w) == 0 || GetForegroundWindow() != w {
                    return Err("Isolated test window could not receive foreground focus".into());
                }
                SetFocus(w);
                quiet()?;
                // This opt-in test targets only the newly created EDIT window.
                if GetForegroundWindow() != w {
                    return Err("Test window lost foreground focus".into());
                }
                stroke(b'Z' as u16, true, false)?;
                let deadline = Instant::now();
                let mut text = [0u16; 8];
                loop {
                    let mut message: MSG = std::mem::zeroed();
                    while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                    let len = GetWindowTextW(w, text.as_mut_ptr(), text.len() as i32);
                    if len == 1 && text[0] == b'Z' as u16 {
                        return Ok(());
                    }
                    if deadline.elapsed() > Duration::from_secs(2) {
                        return Err("Unicode input did not reach isolated test window".into());
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            })();
            DestroyWindow(w);
            result.unwrap();
        }
    }
}
