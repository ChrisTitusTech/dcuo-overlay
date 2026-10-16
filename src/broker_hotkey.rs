//! One-shot Browse search in the user-confirmed focused search field. No mouse or focus events.
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
    format!("Browse hotkey unavailable: {e}")
}

#[derive(Debug, PartialEq)]
pub enum Event {
    Started(Instant),
    ReadyToSearch(Instant),
    Finished(Result<(), String>),
}
pub struct Hotkey {
    cancel: Arc<AtomicBool>,
    search_permit: Arc<AtomicBool>,
    result: Receiver<Event>,
    completed: bool,
}
impl Hotkey {
    pub fn confirm_search(&self) {
        self.search_permit.store(true, Ordering::Release);
    }
    pub fn poll(&mut self) -> Option<Event> {
        if self.completed {
            return None;
        }
        match self.result.try_recv() {
            Ok(result) => {
                self.completed = matches!(result, Event::Finished(_));
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.completed = true;
                Some(Event::Finished(Err(
                    "Browse hotkey worker stopped unexpectedly".into(),
                )))
            }
        }
    }
}
impl Drop for Hotkey {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 96 && name.bytes().all(|b| (32..=126).contains(&b))
}
pub fn arm(pid: i32, name: &str) -> Result<Hotkey, String> {
    if pid <= 0 || !valid_name(name) {
        return Err(
            "Browse hotkey requires a selected game and 1-96 printable ASCII item characters"
                .into(),
        );
    }
    arm_action(pid, name.to_owned(), true, false)
}
/// Paste an item name, then await a fresh baseline acknowledgement; never Return.
pub fn arm_text(pid: i32, name: &str) -> Result<Hotkey, String> {
    if pid <= 0 || !valid_name(name) {
        return Err("Invalid item name or game session".into());
    }
    arm_action(pid, name.to_owned(), false, true)
}
pub fn arm_price(pid: i32, price: u64) -> Result<Hotkey, String> {
    if pid <= 0 || price == 0 || price > u64::from(u32::MAX) {
        return Err(
            "Price hotkey requires a selected game and price between 1 and 4294967295".into(),
        );
    }
    arm_action(pid, price.to_string(), false, false)
}
fn arm_action(pid: i32, name: String, submit: bool, settle_after: bool) -> Result<Hotkey, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let search_permit = Arc::new(AtomicBool::new(false));
    let worker_permit = search_permit.clone();
    let (send, result) = mpsc::channel();
    let started = Instant::now();
    thread::Builder::new()
        .name("broker-search-hotkey".into())
        .spawn(move || {
            let result = run(
                pid,
                &name,
                (submit, settle_after),
                &worker_cancel,
                &worker_permit,
                started,
                &send,
            );
            let _ = send.send(Event::Finished(result));
        })
        .map_err(err)?;
    Ok(Hotkey {
        cancel,
        search_permit,
        result,
        completed: false,
    })
}
fn budget(cancel: &AtomicBool, started: Instant) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("Browse hotkey cancelled".into())
    } else if started.elapsed() >= LIMIT {
        Err("Browse hotkey expired after 60 seconds".into())
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
        return Err("Release modifiers and turn off Caps Lock, Num Lock and sticky/latched modifiers before Browse search".into());
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
        return Err("Release held keyboard modifiers before Browse search".into());
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
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stroke {
    code: u8,
    shift: bool,
}
fn resolve(min: u8, width: usize, map: &[u32], symbol: u32) -> Result<Stroke, String> {
    if width < 2 || !map.len().is_multiple_of(width) {
        return Err("Invalid keyboard map".into());
    }
    let mut matches = Vec::new();
    for (index, row) in map.chunks_exact(width).enumerate() {
        for level in 0..2 {
            if row[level] == symbol
                && row
                    .iter()
                    .skip(level)
                    .step_by(2)
                    .all(|s| *s == 0 || *s == symbol)
            {
                let stroke = Stroke {
                    code: u8::try_from(usize::from(min) + index).map_err(err)?,
                    shift: level == 1,
                };
                // Same symbol at both levels can always use the unshifted level.
                if level == 0 || row[0] != symbol {
                    matches.push(stroke);
                }
            }
        }
    }
    if matches.len() != 1 {
        return Err(format!(
            "Search character {symbol:#x} requires a unique ASCII keyboard mapping"
        ));
    }
    Ok(matches[0])
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
fn send_key(c: &RustConnection, root: u32, code: u8) -> Result<(), String> {
    let pressed = c
        .xtest_fake_input(KEY_PRESS_EVENT, code, 0, root, 0, 0, 0)
        .map_err(err)
        .and_then(|cookie| cookie.check().map_err(err));
    thread::sleep(Duration::from_millis(35));
    // Always release exactly the key we attempted, including on errors.
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
fn send_stroke(c: &RustConnection, root: u32, stroke: Stroke, modifier: u8) -> Result<(), String> {
    if !stroke.shift {
        return send_key(c, root, stroke.code);
    }
    let pressed = c
        .xtest_fake_input(KEY_PRESS_EVENT, modifier, 0, root, 0, 0, 0)
        .map_err(err)
        .and_then(|cookie| cookie.check().map_err(err));
    let typed = if pressed.is_ok() {
        send_key(c, root, stroke.code)
    } else {
        Ok(())
    };
    // Release only our own synthetic modifier, even if key input failed.
    let released = c
        .xtest_fake_input(KEY_RELEASE_EVENT, modifier, 0, root, 0, 0, 0)
        .map_err(err)
        .and_then(|cookie| cookie.check().map_err(err));
    let flushed = c.flush().map_err(err);
    pressed?;
    typed?;
    released?;
    flushed
}
fn input_sequence(
    strokes: Vec<Stroke>,
    select: Stroke,
    enter: Option<Stroke>,
    control: u8,
    shift: u8,
) -> Vec<(Stroke, u8)> {
    std::iter::once((
        Stroke {
            code: select.code,
            shift: true,
        },
        control,
    ))
    .chain(strokes.into_iter().map(|key| (key, shift)))
    .chain(enter.into_iter().map(|key| (key, shift)))
    .collect()
}
fn wait_for_search_permit(
    cancel: &AtomicBool,
    started: Instant,
    ready: Instant,
    permit: &AtomicBool,
) -> Result<(), String> {
    loop {
        budget(cancel, started)?;
        if ready.elapsed() >= Duration::from_secs(10) {
            return Err("Browse search confirmation timed out after 10 seconds".into());
        }
        if permit.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn run(
    pid: i32,
    name: &str,
    behavior: (bool, bool),
    cancel: &AtomicBool,
    search_permit: &AtomicBool,
    started: Instant,
    events: &mpsc::Sender<Event>,
) -> Result<(), String> {
    let (submit, settle_after) = behavior;
    budget(cancel, started)?;
    let candidate = process::candidate(pid).map_err(err)?;
    if process::executable_metadata(&candidate)
        .map_err(err)?
        .sha256
        != crate::native::PROFILE_ID
    {
        return Err("Unsupported game build for Browse hotkey".into());
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
    let strokes = name
        .bytes()
        .map(|b| resolve(min, width, &mapping.keysyms, u32::from(b)))
        .collect::<Result<Vec<_>, _>>()?;
    let select = resolve(min, width, &mapping.keysyms, u32::from(b'a'))?;
    let enter = if submit {
        Some(resolve(min, width, &mapping.keysyms, 0xff0d)?)
    } else {
        None
    };
    if select.shift || enter.is_some_and(|key| key.shift) {
        return Err("Unsupported selection or Return mapping".into());
    }
    let shift = resolve(min, width, &mapping.keysyms, 0xffe1)?;
    let control = resolve(min, width, &mapping.keysyms, 0xffe3)?;
    if shift.shift || control.shift {
        return Err("Unsupported modifier mapping".into());
    }
    let modifiers = c
        .get_modifier_mapping()
        .map_err(err)?
        .reply()
        .map_err(err)?;
    let modifier_width = modifiers.keycodes.len() / 8;
    if modifier_width == 0
        || !modifiers.keycodes[..modifier_width].contains(&shift.code)
        || !modifiers.keycodes[2 * modifier_width..3 * modifier_width].contains(&control.code)
        || modifiers
            .keycodes
            .chunks_exact(modifier_width)
            .enumerate()
            .any(|(index, codes)| {
                (index != 0 && codes.contains(&shift.code))
                    || (index != 2 && codes.contains(&control.code))
            })
    {
        return Err("Shift and Control modifier mapping is not verified".into());
    }
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
    let mut input_started = false;
    // Ctrl+A replaces only the explicitly confirmed focused Browse search field.
    let sequence = input_sequence(strokes, select, enter, control.code, shift.code);
    for (stroke, modifier) in sequence {
        if enter.is_some_and(|key| key == stroke) {
            search_permit.store(false, Ordering::Release);
            let ready = Instant::now();
            events.send(Event::ReadyToSearch(ready)).map_err(err)?;
            wait_for_search_permit(cancel, started, ready, search_permit)?;
        }
        validate()?;
        no_modifiers(&c, root)?;
        let latest = c
            .get_keyboard_mapping(min, count)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        let latest_modifiers = c
            .get_modifier_mapping()
            .map_err(err)?
            .reply()
            .map_err(err)?;
        if latest.keysyms_per_keycode != mapping.keysyms_per_keycode
            || latest.keysyms != mapping.keysyms
            || latest_modifiers.keycodes != modifiers.keycodes
        {
            return Err("Keyboard mapping changed while armed".into());
        }
        if c.query_keymap()
            .map_err(err)?
            .reply()
            .map_err(err)?
            .keys
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err("Release all held keyboard keys before Browse search".into());
        }
        budget(cancel, started)?;
        if !input_started {
            events.send(Event::Started(Instant::now())).map_err(err)?;
            input_started = true;
        }
        send_stroke(&c, root, stroke, modifier)?;
        validate()?;
        no_modifiers(&c, root)?;
        thread::sleep(Duration::from_millis(35));
    }
    if settle_after {
        search_permit.store(false, Ordering::Release);
        events
            .send(Event::ReadyToSearch(Instant::now()))
            .map_err(err)?;
        wait_for_search_permit(cancel, started, Instant::now(), search_permit)?;
    }
    budget(cancel, started)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires explicitly opted-in private Xvfb display"]
    fn xvfb_search_events() {
        assert_eq!(std::env::var("DCUO_HOTKEY_XVFB").as_deref(), Ok("1"));
        use x11rb::protocol::{
            Event as XEvent,
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
        let map = c
            .get_keyboard_mapping(min, c.setup().max_keycode - min + 1)
            .unwrap()
            .reply()
            .unwrap();
        let resolve = |symbol| {
            super::resolve(
                min,
                usize::from(map.keysyms_per_keycode),
                &map.keysyms,
                symbol,
            )
            .unwrap()
        };
        let shift = resolve(0xffe1).code;
        let ctrl = resolve(0xffe3).code;
        let mut sequence = input_sequence(
            "Ab Name: X"
                .bytes()
                .map(|b| resolve(u32::from(b)))
                .collect(),
            resolve(97),
            Some(resolve(0xff0d)),
            ctrl,
            shift,
        );
        let price = input_sequence(
            "12345".bytes().map(|b| resolve(u32::from(b))).collect(),
            resolve(97),
            None,
            ctrl,
            shift,
        );
        assert_eq!(price.len(), 6);
        assert!(
            price
                .iter()
                .all(|(key, _)| key.code != resolve(0xff0d).code)
        );
        sequence.extend(price);
        let mut expected = Vec::new();
        for (stroke, modifier) in sequence {
            no_modifiers(&c, root).unwrap();
            if stroke.shift {
                expected.push((true, modifier));
            }
            expected.extend([(true, stroke.code), (false, stroke.code)]);
            if stroke.shift {
                expected.push((false, modifier));
            }
            send_stroke(&c, root, stroke, modifier).unwrap();
        }
        c.get_input_focus().unwrap().reply().unwrap();
        let mut actual = Vec::new();
        while let Some(event) = c.poll_for_event().unwrap() {
            match event {
                XEvent::KeyPress(e) => actual.push((true, e.detail)),
                XEvent::KeyRelease(e) => actual.push((false, e.detail)),
                XEvent::ButtonPress(_) | XEvent::ButtonRelease(_) => panic!("mouse event"),
                _ => {}
            }
        }
        assert_eq!(actual, expected);
        assert!(
            send_stroke(
                &c,
                root,
                Stroke {
                    code: 0,
                    shift: true
                },
                shift
            )
            .is_err()
        );
        no_modifiers(&c, root).unwrap();
        assert!(
            c.query_keymap()
                .unwrap()
                .reply()
                .unwrap()
                .keys
                .iter()
                .all(|b| *b == 0)
        );
        c.xtest_fake_input(KEY_PRESS_EVENT, shift, 0, root, 0, 0, 0)
            .unwrap()
            .check()
            .unwrap();
        let held = no_modifiers(&c, root);
        c.xtest_fake_input(KEY_RELEASE_EVENT, shift, 0, root, 0, 0, 0)
            .unwrap()
            .check()
            .unwrap();
        assert!(held.is_err());
        let caps = resolve(0xffe5).code;
        send_key(&c, root, caps).unwrap();
        let locked = no_modifiers(&c, root);
        send_key(&c, root, caps).unwrap();
        assert!(locked.is_err());
        no_modifiers(&c, root).unwrap();
        c.destroy_window(window).unwrap().check().unwrap();
    }
    #[test]
    fn mapping_and_names_fail_closed() {
        let map = [97, 65, 97, 65, 59, 58, 59, 58, 32, 32, 32, 32];
        assert_eq!(
            resolve(20, 4, &map, 65).unwrap(),
            Stroke {
                code: 20,
                shift: true
            }
        );
        assert_eq!(
            resolve(20, 4, &map, 58).unwrap(),
            Stroke {
                code: 21,
                shift: true
            }
        );
        assert!(!resolve(20, 4, &map, 32).unwrap().shift);
        assert!(resolve(20, 0, &map, 65).is_err());
        assert!(resolve(20, 4, &[97, 65, 98, 65], 97).is_err());
        assert!(resolve(20, 2, &[97, 65, 97, 65], 65).is_err());
        assert!(valid_name("A Name: Test"));
        for name in ["", "  ", "a\n", "caf\u{e9}"] {
            assert!(!valid_name(name));
        }
        assert!(!valid_name(&"a".repeat(97)));
        assert!(arm(0, "Name").is_err());
        assert!(arm_price(0, 1).is_err());
        assert!(arm_price(1, 0).is_err());
        assert!(arm_price(1, u64::from(u32::MAX) + 1).is_err());
    }
    #[test]
    fn event_completion_is_once() {
        let (send, result) = mpsc::channel();
        let mut hotkey = Hotkey {
            cancel: Arc::new(AtomicBool::new(false)),
            search_permit: Arc::new(AtomicBool::new(false)),
            result,
            completed: false,
        };
        assert!(hotkey.poll().is_none());
        hotkey.confirm_search();
        assert!(hotkey.search_permit.load(Ordering::Acquire));
        let now = Instant::now();
        send.send(Event::Started(now)).unwrap();
        send.send(Event::Finished(Ok(()))).unwrap();
        assert_eq!(hotkey.poll(), Some(Event::Started(now)));
        assert_eq!(hotkey.poll(), Some(Event::Finished(Ok(()))));
        drop(send);
        assert!(hotkey.poll().is_none());
    }
    #[test]
    fn search_confirmation_is_bounded_and_consumed() {
        let cancel = AtomicBool::new(false);
        let permit = AtomicBool::new(false);
        let now = Instant::now();
        assert!(
            wait_for_search_permit(&cancel, now, now - Duration::from_secs(10), &permit).is_err()
        );
        assert!(wait_for_search_permit(&cancel, now - LIMIT, now, &permit).is_err());
        cancel.store(true, Ordering::Release);
        assert!(wait_for_search_permit(&cancel, now, now, &permit).is_err());
        cancel.store(false, Ordering::Release);
        permit.store(true, Ordering::Release);
        wait_for_search_permit(&cancel, now, now, &permit).unwrap();
        assert!(!permit.load(Ordering::Acquire));
    }
    #[test]
    fn stopped_worker_is_reported_once() {
        let (send, result) = mpsc::channel();
        drop(send);
        let mut hotkey = Hotkey {
            cancel: Arc::new(AtomicBool::new(false)),
            search_permit: Arc::new(AtomicBool::new(false)),
            result,
            completed: false,
        };
        assert!(matches!(hotkey.poll(), Some(Event::Finished(Err(_)))));
        assert!(hotkey.poll().is_none());
    }
    #[test]
    fn release_edge_and_budget() {
        let mut edge = Edge::default();
        assert!(!edge.update(true, false));
        assert!(!edge.update(false, true));
        assert!(edge.update(true, false));
        assert!(!edge.update(true, false));
        let cancel = AtomicBool::new(false);
        assert!(budget(&cancel, Instant::now() - LIMIT).is_err());
        cancel.store(true, Ordering::Release);
        assert!(budget(&cancel, Instant::now()).is_err());
    }
}
