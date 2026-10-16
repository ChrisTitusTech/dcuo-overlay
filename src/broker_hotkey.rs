//! One-shot, explicitly armed price typing. No mouse, focus, selection or submit events.
use crate::process;
use std::{
    collections::HashSet,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, ConnectionExt, KEY_PRESS_EVENT, KEY_RELEASE_EVENT, MapState},
    rust_connection::RustConnection,
};
const LIMIT: Duration = Duration::from_secs(60);
fn err(e: impl std::fmt::Display) -> String {
    format!("Price hotkey unavailable: {e}")
}

pub struct Hotkey {
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<(), String>>,
    completed: bool,
}
impl Hotkey {
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        if self.completed {
            return None;
        }
        match self.result.try_recv() {
            Ok(result) => {
                self.completed = true;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.completed = true;
                Some(Err("Price hotkey worker stopped unexpectedly".into()))
            }
        }
    }
}
impl Drop for Hotkey {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
pub fn arm(pid: i32, price: u64) -> Result<Hotkey, String> {
    if pid <= 0 || price == 0 || price > u64::from(u32::MAX) {
        return Err(
            "Price hotkey requires a selected game and price between 1 and 4294967295".into(),
        );
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let (send, result) = mpsc::channel();
    let started = Instant::now();
    thread::Builder::new()
        .name("broker-price-hotkey".into())
        .spawn(move || {
            let _ = send.send(run(pid, price, &worker_cancel, started));
        })
        .map_err(err)?;
    Ok(Hotkey {
        cancel,
        result,
        completed: false,
    })
}
fn budget(cancel: &AtomicBool, started: Instant) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("Price hotkey cancelled".into())
    } else if started.elapsed() >= LIMIT {
        Err("Price hotkey expired after 60 seconds".into())
    } else {
        Ok(())
    }
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
fn focused(c: &RustConnection, w: u32, root: u32) -> Result<(), String> {
    let active = property(c, root, b"_NET_ACTIVE_WINDOW", AtomEnum::WINDOW.into(), 1)?;
    if active.as_slice() != w.to_ne_bytes() {
        return Err("Game window lost active focus".into());
    }
    let mut focus = c
        .get_input_focus()
        .map_err(err)?
        .reply()
        .map_err(err)?
        .focus;
    for _ in 0..16 {
        if focus == w {
            return Ok(());
        }
        if focus == 0 || focus == root {
            break;
        }
        focus = c
            .query_tree(focus)
            .map_err(err)?
            .reply()
            .map_err(err)?
            .parent;
    }
    Err("Keyboard focus is outside the game window".into())
}
fn no_modifiers(c: &RustConnection, root: u32) -> Result<(), String> {
    // Physical key state alone misses locks and accessibility-latched modifiers.
    // QueryPointer reports the effective core modifier state, including those.
    let pointer = c.query_pointer(root).map_err(err)?.reply().map_err(err)?;
    if u16::from(pointer.mask) & 0xff != 0 {
        return Err("Release modifiers and turn off Caps Lock, Num Lock and sticky/latched modifiers before price typing".into());
    }
    let keys = c.query_keymap().map_err(err)?.reply().map_err(err)?.keys;
    let modifiers = c
        .get_modifier_mapping()
        .map_err(err)?
        .reply()
        .map_err(err)?;
    if modifiers
        .keycodes
        .iter()
        .any(|code| *code != 0 && keys[usize::from(*code) / 8] & (1 << (*code % 8)) != 0)
    {
        return Err("Release held keyboard modifiers before price typing".into());
    }
    Ok(())
}
fn down(keys: &[u8; 32], code: u8) -> bool {
    keys[usize::from(code) / 8] & (1 << (code % 8)) != 0
}
#[derive(Default)]
struct Edge {
    ready: bool,
    previous: bool,
}
impl Edge {
    fn update(&mut self, chord: bool, released: bool) -> bool {
        // An already held shortcut on arm is never an activation.
        if released {
            self.ready = true;
        }
        let fire = self.ready && chord && !self.previous;
        self.previous = chord;
        fire
    }
}
fn digit_codes(min: u8, width: usize, map: &[u32]) -> Result<[u8; 10], String> {
    if width == 0 || !map.len().is_multiple_of(width) {
        return Err("Invalid keyboard map".into());
    }
    let mut codes = [0; 10];
    for (digit, slot) in codes.iter_mut().enumerate() {
        let symbol = u32::from(b'0') + digit as u32;
        let matches: Vec<_> = map
            .chunks_exact(width)
            .enumerate()
            .filter(|(_, row)| {
                row[0] == symbol && row.iter().step_by(2).all(|s| *s == 0 || *s == symbol)
            })
            .collect();
        if matches.len() != 1 {
            return Err("Digits require unique unshifted ASCII keyboard mappings".into());
        }
        *slot = u8::try_from(usize::from(min) + matches[0].0).map_err(err)?;
    }
    Ok(codes)
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
fn send_digit(c: &RustConnection, root: u32, code: u8) -> Result<(), String> {
    let pressed = c
        .xtest_fake_input(KEY_PRESS_EVENT, code, 0, root, 0, 0, 0)
        .map_err(err)
        .and_then(|cookie| cookie.check().map_err(err));
    thread::sleep(Duration::from_millis(35));
    // Always release exactly the digit we attempted, including on cancellation/errors.
    let released = c
        .xtest_fake_input(KEY_RELEASE_EVENT, code, 0, root, 0, 0, 0)
        .map_err(err)
        .and_then(|cookie| cookie.check().map_err(err));
    let flushed = c.flush().map_err(err);
    pressed?;
    released?;
    flushed?;
    Ok(())
}
fn run(pid: i32, price: u64, cancel: &AtomicBool, started: Instant) -> Result<(), String> {
    budget(cancel, started)?;
    let candidate = process::candidate(pid).map_err(err)?;
    if process::executable_metadata(&candidate)
        .map_err(err)?
        .sha256
        != crate::native::PROFILE_ID
    {
        return Err("Unsupported game build for price hotkey".into());
    }
    let owners = ancestry(pid)?;
    if owners.first() != Some(&candidate.identity) {
        return Err("Selected game changed".into());
    }
    let (c, screen) = x11rb::connect(None).map_err(err)?;
    c.xtest_get_version(2, 2)
        .map_err(err)?
        .reply()
        .map_err(err)?;
    let root = c.setup().roots[screen].root;
    let windows = clients(&c, root, &owners)?;
    if windows.len() != 1 {
        return Err("Expected one game window belonging to the selected process".into());
    }
    let window = windows[0];
    let min = c.setup().min_keycode;
    let count = c
        .setup()
        .max_keycode
        .checked_sub(min)
        .and_then(|n| n.checked_add(1))
        .ok_or("Invalid keycode range")?;
    let mapping = c
        .get_keyboard_mapping(min, count)
        .map_err(err)?
        .reply()
        .map_err(err)?;
    let width = usize::from(mapping.keysyms_per_keycode);
    let digits = digit_codes(min, width, &mapping.keysyms)?;
    let symbol_codes = |symbols: &[u32]| -> Vec<u8> {
        mapping
            .keysyms
            .chunks_exact(width)
            .enumerate()
            .filter(|(_, row)| symbols.contains(&row[0]))
            .map(|(i, _)| min + i as u8)
            .collect()
    };
    let ctrl = symbol_codes(&[0xffe3, 0xffe4]);
    let alt = symbol_codes(&[0xffe9, 0xffea]);
    let p = symbol_codes(&[u32::from(b'p')]);
    if ctrl.is_empty() || alt.is_empty() || p.len() != 1 {
        return Err("Unsupported Ctrl+Alt+P keyboard mapping".into());
    }
    let validate = || -> Result<(), String> {
        budget(cancel, started)?;
        let current = process::candidate(pid).map_err(err)?;
        if current.identity != candidate.identity
            || current.mapping != candidate.mapping
            || ancestry(pid)? != owners
        {
            return Err("Game process identity or executable mapping changed".into());
        }
        if clients(&c, root, &owners)? != windows {
            return Err("Game window identity became ambiguous".into());
        }
        if c.get_window_attributes(window)
            .map_err(err)?
            .reply()
            .map_err(err)?
            .map_state
            != MapState::VIEWABLE
        {
            return Err("Game window is not visible".into());
        }
        focused(&c, window, root)?;
        budget(cancel, started)
    };
    let mut edge = Edge::default();
    loop {
        budget(cancel, started)?;
        let keys = c.query_keymap().map_err(err)?.reply().map_err(err)?.keys;
        let any = |codes: &[u8]| codes.iter().any(|code| down(&keys, *code));
        if edge.update(
            any(&ctrl) && any(&alt) && any(&p),
            !any(&ctrl) && !any(&alt) && !any(&p),
        ) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    // Wait for the user's own releases. Never synthesize modifier releases.
    loop {
        validate()?;
        let keys = c.query_keymap().map_err(err)?.reply().map_err(err)?.keys;
        if ctrl
            .iter()
            .chain(&alt)
            .chain(&p)
            .all(|code| !down(&keys, *code))
        {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if process::executable_metadata(&process::candidate(pid).map_err(err)?)
        .map_err(err)?
        .sha256
        != crate::native::PROFILE_ID
    {
        return Err("Game executable changed".into());
    }
    for byte in price.to_string().bytes() {
        validate()?;
        no_modifiers(&c, root)?;
        let latest = c
            .get_keyboard_mapping(min, count)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        if latest.keysyms_per_keycode != mapping.keysyms_per_keycode
            || latest.keysyms != mapping.keysyms
        {
            return Err("Keyboard mapping changed while armed".into());
        }
        let code = digits[usize::from(byte - b'0')];
        if c.query_keymap()
            .map_err(err)?
            .reply()
            .map_err(err)?
            .keys
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err("Release all held keyboard keys before price typing".into());
        }
        budget(cancel, started)?;
        send_digit(&c, root, code)?;
        validate()?;
        no_modifiers(&c, root)?;
        thread::sleep(Duration::from_millis(35));
    }
    budget(cancel, started)
}
#[cfg(test)]
mod tests {
    use super::*;
    // Run only under a private server:
    // xvfb-run -a env DCUO_HOTKEY_XVFB=1 cargo test --lib broker_hotkey::tests::xvfb_digits -- --ignored
    #[test]
    #[ignore = "requires explicitly opted-in private Xvfb display"]
    fn xvfb_digits() {
        assert_eq!(std::env::var("DCUO_HOTKEY_XVFB").as_deref(), Ok("1"));
        use x11rb::protocol::{
            Event,
            xproto::{CreateWindowAux, EventMask, InputFocus, WindowClass},
        };
        let (c, screen) = x11rb::connect(None).unwrap();
        let root = c.setup().roots[screen].root;
        let window = c.generate_id().unwrap();
        c.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            200,
            100,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().event_mask(
                EventMask::KEY_PRESS
                    | EventMask::KEY_RELEASE
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE,
            ),
        )
        .unwrap()
        .check()
        .unwrap();
        c.map_window(window).unwrap().check().unwrap();
        c.set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        let min = c.setup().min_keycode;
        let count = c.setup().max_keycode - min + 1;
        let mapping = c.get_keyboard_mapping(min, count).unwrap().reply().unwrap();
        let codes = digit_codes(
            min,
            usize::from(mapping.keysyms_per_keycode),
            &mapping.keysyms,
        )
        .unwrap();
        let price = "4294967295";
        for digit in price.bytes() {
            send_digit(&c, root, codes[usize::from(digit - b'0')]).unwrap();
        }
        // Round-trip ensures all synthetic events have reached this receiving client.
        c.get_input_focus().unwrap().reply().unwrap();
        let mut received = Vec::new();
        while let Some(event) = c.poll_for_event().unwrap() {
            match event {
                Event::KeyPress(event) => received.push((true, event.detail)),
                Event::KeyRelease(event) => received.push((false, event.detail)),
                Event::ButtonPress(_) | Event::ButtonRelease(_) => panic!("unexpected mouse event"),
                _ => {}
            }
        }
        let expected: Vec<_> = price
            .bytes()
            .flat_map(|digit| {
                let code = codes[usize::from(digit - b'0')];
                [(true, code), (false, code)]
            })
            .collect();
        assert_eq!(
            received, expected,
            "only the exact decimal digit events are allowed"
        );
        assert!(
            c.query_keymap()
                .unwrap()
                .reply()
                .unwrap()
                .keys
                .iter()
                .all(|byte| *byte == 0)
        );
        // Exercise the production guard against physically held Shift, then a
        // lock that remains effective after every physical key has been released.
        let symbol_code = |symbol| {
            let index = mapping
                .keysyms
                .chunks_exact(usize::from(mapping.keysyms_per_keycode))
                .position(|row| row[0] == symbol)
                .unwrap();
            min + u8::try_from(index).unwrap()
        };
        let shift = symbol_code(0xffe1);
        c.xtest_fake_input(KEY_PRESS_EVENT, shift, 0, root, 0, 0, 0)
            .unwrap()
            .check()
            .unwrap();
        let held = no_modifiers(&c, root);
        c.xtest_fake_input(KEY_RELEASE_EVENT, shift, 0, root, 0, 0, 0)
            .unwrap()
            .check()
            .unwrap();
        assert!(held.is_err(), "physically held Shift must be rejected");
        no_modifiers(&c, root).unwrap();
        let caps = symbol_code(0xffe5);
        send_digit(&c, root, caps).unwrap();
        let locked = no_modifiers(&c, root);
        let physical_keys = c.query_keymap().unwrap().reply().unwrap().keys;
        // Restore the private server's state before assertions can panic.
        send_digit(&c, root, caps).unwrap();
        assert!(physical_keys.iter().all(|byte| *byte == 0));
        assert!(
            locked.is_err(),
            "effective Caps Lock must be rejected with no physical keys held"
        );
        no_modifiers(&c, root).unwrap();
        c.destroy_window(window).unwrap().check().unwrap();
    }
    #[test]
    fn polling_is_nonblocking_and_drop_cancels() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (send, result) = mpsc::channel();
        let mut hotkey = Hotkey {
            cancel: cancel.clone(),
            result,
            completed: false,
        };
        assert!(hotkey.poll().is_none());
        send.send(Ok(())).unwrap();
        assert_eq!(hotkey.poll(), Some(Ok(())));
        assert!(hotkey.poll().is_none());
        drop(hotkey);
        assert!(cancel.load(Ordering::Acquire));
    }
    #[test]
    fn stopped_worker_is_reported_once() {
        let (send, result) = mpsc::channel();
        drop(send);
        let mut hotkey = Hotkey {
            cancel: Arc::new(AtomicBool::new(false)),
            result,
            completed: false,
        };
        assert!(hotkey.poll().unwrap().is_err());
        assert!(hotkey.poll().is_none());
    }
    #[test]
    fn held_shortcut_requires_full_release_and_new_press() {
        let mut edge = Edge::default();
        assert!(!edge.update(true, false));
        assert!(!edge.update(false, false));
        assert!(!edge.update(true, false));
        assert!(!edge.update(false, true));
        assert!(edge.update(true, false));
        assert!(!edge.update(true, false));
    }
    #[test]
    fn deadline_and_cancellation_stop_work() {
        let cancel = AtomicBool::new(false);
        assert!(budget(&cancel, Instant::now()).is_ok());
        assert!(budget(&cancel, Instant::now() - LIMIT).is_err());
        cancel.store(true, Ordering::Release);
        assert!(budget(&cancel, Instant::now()).is_err());
        assert!(arm(0, 10).is_err());
        assert!(arm(123, 0).is_err());
        assert!(arm(123, u64::from(u32::MAX) + 1).is_err());
    }
    #[test]
    fn digits_require_unique_unshifted_mapping_in_each_group() {
        let mut map: Vec<u32> = (b'0'..=b'9')
            .flat_map(|b| [u32::from(b), u32::from(b'!'), 0, 0])
            .collect();
        assert_eq!(
            digit_codes(20, 4, &map).unwrap(),
            [20, 21, 22, 23, 24, 25, 26, 27, 28, 29]
        );
        map.extend_from_slice(&[u32::from(b'0'), 0, 0, 0]);
        assert!(digit_codes(20, 4, &map).is_err());
        map.truncate(40);
        map[2] = u32::from(b'x');
        assert!(digit_codes(20, 4, &map).is_err());
        assert!(digit_codes(20, 0, &map).is_err());
        assert!(digit_codes(20, 3, &map).is_err());
    }
}
