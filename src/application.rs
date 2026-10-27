//! Native desktop companion with explicitly sourced verification evidence.

use crate::{
    api::DEFAULT_BASE_URL,
    map_view::MapView,
    maps::{Context, LocationConfidence, LocationSample, MarkerGate, Point},
    session::Controller,
    settings::{self, Settings},
    spawn_cache::SpawnCache,
};
use std::io::Read;
use std::time::{Duration, Instant};

use eframe::egui;
#[cfg(target_os = "linux")]
use winit::platform::x11::EventLoopBuilderExtX11;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(target_os = "linux")]
use x11rb::connection::Connection;
#[cfg(target_os = "linux")]
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, MapState, PropMode};
#[cfg(target_os = "linux")]
use x11rb::wrapper::ConnectionExt as _;

const TITLE: &str = "DCUO Companion";
#[cfg(target_os = "linux")]
const WM_CLASS: &[u8] = b"dcuo-companion\0DcuoCompanion\0";
type SetupError = Box<dyn std::error::Error + Send + Sync>;
const HELP: &str = "DCUO Companion - native companion window
Usage: dcuo-companion [--smoke-seconds SECONDS] [--position X Y]
       [--pid PID] [--reference-stdin] [--world ID] [--api-base URL]

  --smoke-seconds SECONDS  Close after 1..3600 seconds.
  --position X Y          Initial physical-pixel window placement hint.
  --pid PID               Connect to an explicit positive process ID.
  --reference-stdin       Read an exact reference name from stdin (64 characters).
  --world ID              Reference world ID (saved preference, otherwise 2).
  --api-base URL          Reference service (saved preference, otherwise https://dcuo.bot/api).
  --help                  Show help.
";

// Keep every evidence row present and single-line so sample loss and long
// diagnostics cannot move the map. Full text remains available on hover.
fn evidence_line(ui: &mut egui::Ui, text: impl Into<String>, small: bool) {
    let text = text.into().replace(['\n', '\r'], " ");
    let rich = egui::RichText::new(&text);
    let rich = if small { rich.small() } else { rich };
    ui.add(egui::Label::new(rich).truncate())
        .on_hover_text(text);
}

fn map_evidence_header(ui: &mut egui::Ui, view: &crate::session::View) {
    ui.add(
        egui::Label::new(egui::RichText::new("Experimental location evidence").heading())
            .truncate(),
    );
    evidence_line(ui, &view.native_status, false);
    let (age, position, area, area_status) = if let Some(sample) = &view.native {
        (
            format!(
                "Memory sample age: {:.2}s",
                sample.observed_at.elapsed().as_secs_f64()
            ),
            format!(
                "Position (experimental): {:.1}, {:.1}, {:.1}",
                sample.position[0], sample.position[1], sample.position[2]
            ),
            format!(
                "Streaming area evidence: {}",
                sample
                    .area_key
                    .as_deref()
                    .unwrap_or("Unavailable / ambiguous")
            ),
            sample.area_status.clone(),
        )
    } else {
        (
            "Memory sample age: unavailable (no fresh sample)".into(),
            "Position (experimental): unavailable".into(),
            "Streaming area evidence: unavailable".into(),
            "No fresh experimental position sample available.".into(),
        )
    };
    for text in [age, position, area] {
        evidence_line(ui, text, false);
    }
    evidence_line(ui, area_status, true);
    evidence_line(
        ui,
        "Position and streaming evidence do not establish map calibration, floor or current-character identity. The experimental marker is a preview, not a fully validated profile.",
        true,
    );
}

fn repaint_delay(
    configured: Duration,
    connected: bool,
    observed: Option<Instant>,
    now: Instant,
) -> Duration {
    let mut delay = if connected {
        configured.min(Duration::from_millis(250))
    } else {
        configured
    };
    if let Some(observed) = observed {
        let age = now
            .checked_duration_since(observed)
            .unwrap_or(Duration::from_secs(1));
        delay = delay.min(Duration::from_secs(1).saturating_sub(age));
    }
    delay
}

struct Options {
    smoke: Option<Duration>,
    position: Option<(i32, i32)>,
    pid: Option<i32>,
    reference_stdin: bool,
    world: String,
    api_base: String,
    world_explicit: bool,
    base_explicit: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            smoke: None,
            position: None,
            pid: None,
            reference_stdin: false,
            world: "2".into(),
            api_base: DEFAULT_BASE_URL.into(),
            world_explicit: false,
            base_explicit: false,
        }
    }
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<Option<Options>, String> {
    let mut args = args.into_iter();
    let mut options = Options::default();
    let mut world_seen = false;
    let mut base_seen = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--smoke-seconds" if options.smoke.is_none() => {
                let seconds = args
                    .next()
                    .ok_or("--smoke-seconds requires a value")?
                    .parse::<u64>()
                    .map_err(|_| "--smoke-seconds requires an integer in 1..3600")?;
                if !(1..=3600).contains(&seconds) {
                    return Err("--smoke-seconds must be in 1..3600".into());
                }
                options.smoke = Some(Duration::from_secs(seconds));
            }
            "--position" if options.position.is_none() => {
                let mut coordinate = || {
                    args.next()
                        .ok_or("--position requires X and Y")?
                        .parse::<i32>()
                        .map_err(|_| "--position requires signed 32-bit integer coordinates")
                };
                options.position = Some((coordinate()?, coordinate()?));
            }
            "--pid" if options.pid.is_none() => {
                let pid = args
                    .next()
                    .ok_or("--pid requires a value")?
                    .parse::<i32>()
                    .map_err(|_| "Invalid PID")?;
                if pid <= 0 {
                    return Err("PID must be positive".into());
                }
                options.pid = Some(pid);
            }
            "--reference-stdin" if !options.reference_stdin => options.reference_stdin = true,
            "--world" if !world_seen => {
                options.world = args.next().ok_or("--world requires a value")?;
                world_seen = true;
                options.world_explicit = true;
            }
            "--api-base" if !base_seen => {
                options.api_base = args.next().ok_or("--api-base requires a value")?;
                base_seen = true;
                options.base_explicit = true;
            }
            _ => return Err(format!("Unknown or repeated argument: {arg}")),
        }
    }
    Ok(Some(options))
}

/// eframe 0.36.2's glow integration creates the root window hidden and maps it
/// only in EpiIntegration::post_rendering after the first frame. AppCreator runs
/// before that frame. Check that assumption at runtime, and complete checked X11
/// requests before returning the app. Never repair the identity after mapping.
#[cfg(target_os = "linux")]
fn prepare_x11_window(
    cc: &eframe::CreationContext<'_>,
    position: Option<(i32, i32)>,
) -> Result<(), SetupError> {
    let window = cc.winit_window().ok_or("Missing native window")?;
    let RawWindowHandle::Xlib(handle) = window.window_handle()?.as_raw() else {
        return Err("The window probe requires the winit X11/Xlib backend".into());
    };
    let id = u32::try_from(handle.window)?;
    let (connection, _) = x11rb::connect(None)?;
    let attributes = connection.get_window_attributes(id)?.reply()?;
    if attributes.map_state != MapState::UNMAPPED || attributes.override_redirect {
        return Err("Window must be unmapped and managed before X11 identity is set".into());
    }
    let atom = |name: &[u8]| -> Result<u32, SetupError> {
        Ok(connection.intern_atom(false, name)?.reply()?.atom)
    };
    let net_name = atom(b"_NET_WM_NAME")?;
    let utf8 = atom(b"UTF8_STRING")?;
    let net_type = atom(b"_NET_WM_WINDOW_TYPE")?;
    let normal = atom(b"_NET_WM_WINDOW_TYPE_NORMAL")?;
    connection
        .change_property8(
            PropMode::REPLACE,
            id,
            AtomEnum::WM_CLASS,
            AtomEnum::STRING,
            WM_CLASS,
        )?
        .check()?;
    connection
        .change_property8(
            PropMode::REPLACE,
            id,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            TITLE.as_bytes(),
        )?
        .check()?;
    connection
        .change_property8(PropMode::REPLACE, id, net_name, utf8, TITLE.as_bytes())?
        .check()?;
    connection
        .change_property32(PropMode::REPLACE, id, net_type, AtomEnum::ATOM, &[normal])?
        .check()?;
    for (property, property_type, expected) in [
        (
            u32::from(AtomEnum::WM_CLASS),
            u32::from(AtomEnum::STRING),
            WM_CLASS,
        ),
        (
            u32::from(AtomEnum::WM_NAME),
            u32::from(AtomEnum::STRING),
            TITLE.as_bytes(),
        ),
        (net_name, utf8, TITLE.as_bytes()),
    ] {
        let reply = connection
            .get_property(false, id, property, property_type, 0, 256)?
            .reply()?;
        if reply.type_ != property_type
            || reply.format != 8
            || reply.bytes_after != 0
            || reply.value != expected
        {
            return Err("Pre-map X11 window identity verification failed".into());
        }
    }
    let reply = connection
        .get_property(false, id, net_type, AtomEnum::ATOM, 0, 2)?
        .reply()?;
    if reply.type_ != u32::from(AtomEnum::ATOM)
        || reply.bytes_after != 0
        || reply.value32().map(|values| values.collect::<Vec<_>>()) != Some(vec![normal])
    {
        return Err("Pre-map X11 normal window type verification failed".into());
    }
    let attributes = connection.get_window_attributes(id)?.reply()?;
    if attributes.map_state != MapState::UNMAPPED || attributes.override_redirect {
        return Err("Window mapped or became unmanaged during initialization".into());
    }
    if let Some((x, y)) = position {
        window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
    }
    connection.flush()?;
    println!(
        "window=0x{id:x} backend=x11 identity=verified-before-map type=normal override_redirect=false"
    );
    Ok(())
}

fn desktop_backend() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows native"
    } else {
        "managed X11"
    }
}

#[cfg(target_os = "windows")]
fn prepare_windows_window(
    cc: &eframe::CreationContext<'_>,
    position: Option<(i32, i32)>,
) -> Result<(), SetupError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GWL_STYLE, GetWindowLongPtrW, WS_CHILD, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_OVERLAPPEDWINDOW,
    };
    let window = cc.winit_window().ok_or("Missing native window")?;
    let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
        return Err("Expected native Windows window".into());
    };
    let hwnd = handle.hwnd.get() as *mut std::ffi::c_void;
    // Inspect only our own window; normal decorations and resizing must remain enabled.
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    let extended = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    if style & WS_CHILD != 0
        || style & WS_OVERLAPPEDWINDOW != WS_OVERLAPPEDWINDOW
        || extended & (WS_EX_TOPMOST | WS_EX_TOOLWINDOW) != 0
    {
        return Err(
            "Windows companion must be a normal decorated resizable top-level window".into(),
        );
    }
    if let Some((x, y)) = position {
        window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
    }
    println!("backend=windows type=normal decorations=true resizable=true topmost=false");
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    WikiItems,
    Character,
    Broker,
    Map,
    Diagnostics,
}

struct Probe {
    started: Instant,
    smoke: Option<Duration>,
    controller: Controller,
    selected: Option<(i32, u64)>,
    name: String,
    world: String,
    base: String,
    scale: f32,
    tab: Tab,
    settings: bool,
    about: bool,
    show_log: bool,
    startup_scan: bool,
    preferences: Settings,
    preference_status: String,
    map: MapView,
    spawn_cache: SpawnCache,
    character: crate::character_view::CharacterView,
    wiki: crate::wiki_view::WikiView,
    faction_reference_id: Option<String>,
    feats: crate::feat_loader::Loader,
    broker: crate::broker_view::BrokerView,
    broker_owner: Option<(i32, u64, usize, u64)>,
    initialized: bool,
    detected_map_source_for: Option<(i32, u64)>,
}

impl Probe {
    fn new(options: Options, name: String) -> Self {
        let (preferences, warning) = settings::load();
        let restore_token = options.smoke.is_none();
        let mut app = Self::with_preferences(options, name, preferences, warning);
        if restore_token {
            app.wiki = crate::wiki_view::WikiView::restore();
        }
        app
    }

    fn with_preferences(
        options: Options,
        name: String,
        mut preferences: Settings,
        warning: Option<String>,
    ) -> Self {
        if options.world_explicit {
            preferences.world = options.world.clone();
        }
        if options.base_explicit {
            preferences.api_base = options.api_base.clone();
        }
        let mut map = MapView::new(preferences.map_catalog.clone());
        map.set_auto_import_source(if preferences.auto_import_maps {
            preferences.map_source.clone()
        } else {
            String::new()
        });
        if !preferences.map_catalog.is_empty() {
            map.load_saved_catalog();
        }
        let mut controller = Controller::new();
        controller.set_auto_reconnect(preferences.auto_reconnect);
        controller.set_ground_spawns_enabled(preferences.show_ground_spawns);
        if let Some(pid) = options.pid {
            controller.connect(pid);
        }
        if !name.is_empty() {
            controller.lookup_reference(&preferences.api_base, &name, &preferences.world);
        }
        let startup_scan = !name.is_empty() && options.pid.is_some();
        let mut character = crate::character_view::CharacterView::default();
        character.faction = preferences.feat_faction.clone();
        Self {
            started: Instant::now(),
            smoke: options.smoke,
            controller,
            selected: None,
            name,
            world: preferences.world.clone(),
            base: preferences.api_base.clone(),
            scale: preferences.ui_scale,
            tab: Tab::Character,
            broker: crate::broker_view::BrokerView::default(),
            broker_owner: None,
            settings: false,
            about: false,
            show_log: false,
            startup_scan,
            preferences,
            preference_status: warning.unwrap_or_default(),
            map,
            spawn_cache: SpawnCache::default(),
            character,
            wiki: crate::wiki_view::WikiView::default(),
            faction_reference_id: None,
            feats: crate::feat_loader::Loader::default(),
            initialized: false,
            detected_map_source_for: None,
        }
    }
}

impl Probe {
    fn save_feat_faction(&mut self) {
        self.preferences.feat_faction = self.character.faction.clone();
        if self.smoke.is_none() {
            self.preference_status = match Settings::save_feat_faction(&self.character.faction) {
                Ok(()) => "Faction filter saved.".into(),
                Err(error) => error,
            };
        }
    }
    fn save_ground_spawn_toggle(&mut self) {
        self.controller
            .set_ground_spawns_enabled(self.preferences.show_ground_spawns);
        self.preference_status = if self.smoke.is_some() {
            "Smoke mode: preferences are not saved.".into()
        } else {
            match Settings::save_ground_spawns(self.preferences.show_ground_spawns) {
                Ok(()) => "Ground spawn preference saved.".into(),
                Err(error) => error,
            }
        };
    }
    fn save_preferences(&mut self) {
        self.controller
            .set_ground_spawns_enabled(self.preferences.show_ground_spawns);
        self.preferences.api_base = self.base.clone();
        self.preferences.world = self.world.clone();
        self.preferences.ui_scale = self.scale;
        self.preferences.map_catalog = self.map.catalog_path.clone();
        if self.preferences.validate().is_ok() {
            self.map
                .set_auto_import_source(if self.preferences.auto_import_maps {
                    self.preferences.map_source.clone()
                } else {
                    String::new()
                });
        }
        self.preference_status = if self.smoke.is_some() {
            "Smoke mode: preferences are not saved.".into()
        } else {
            match self.preferences.save() {
                Ok(()) => "Preferences saved.".into(),
                Err(error) => error,
            }
        };
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        if self.smoke.is_none() {
            self.save_preferences();
            if self.preference_status != "Preferences saved." {
                eprintln!("{}", self.preference_status);
            }
        }
    }
}

fn map_observation(view: &crate::session::View) -> (Option<LocationSample>, MarkerGate<'static>) {
    let sample = view.native.as_ref().and_then(|native| {
        native.area_key.as_ref().map(|area| LocationSample {
            point: Point {
                x: native.position[0] as f64,
                y: native.position[1] as f64,
            },
            context: Context {
                zone: area.clone(),
                layout: "main".into(),
                floor: "overview".into(),
            },
            session_generation: view.location_generation,
            zone_generation: view.location_generation,
            observed_at: native.observed_at,
            valid: view.connected && !view.paused,
        })
    });
    let gate = MarkerGate {
        profile: Some(crate::native::PROFILE_ID),
        session_generation: view.location_generation,
        zone_generation: view.location_generation,
        paused: view.paused,
        confidence: LocationConfidence::Experimental,
    };
    (sample, gate)
}

fn sanitized_session_diagnostics(view: &crate::session::View) -> String {
    format!(
        "DCUO Companion {}\nBackend: {} / OpenGL\nConnection: {}\nPaused: {}\nHeader evidence available: {}\nAPI reference available: {}\nNative profile: experimental physics/resource chain v1\nFresh native sample available: {}\nLive character identity: unverified\nTrusted live location: unavailable\n",
        env!("CARGO_PKG_VERSION"),
        desktop_backend(),
        view.connected,
        view.paused,
        view.build.is_some(),
        view.api.is_some(),
        view.native.is_some()
    )
}

fn row(ui: &mut egui::Ui, label: &str, value: impl std::fmt::Display) {
    ui.label(egui::RichText::new(label).color(egui::Color32::from_rgb(150, 170, 190)));
    ui.label(value.to_string());
    ui.end_row();
}
fn optional<T: std::fmt::Display>(value: &Option<T>) -> String {
    value
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "Unavailable".into())
}

fn broker_control_pid(view: &crate::session::View) -> Option<i32> {
    if view.connected && !view.paused {
        view.selected_pid
    } else {
        None
    }
}

impl eframe::App for Probe {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.initialized {
            ctx.set_zoom_factor(self.scale);
            ctx.set_visuals(if self.preferences.dark_mode {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            self.initialized = true;
        }
        self.controller
            .set_feats_enabled(matches!(self.tab, Tab::Character));
        // One Broker reader and input lifecycle serve every companion tab.
        self.controller
            .set_broker_enabled(self.broker.needs_reads(self.tab == Tab::Broker));
        self.controller.poll();
        let view = self.controller.view();
        // Broker has its own observation age. Brief physics gaps must not
        // tear down its OCR/key listener; a changed session still clears it.
        if self.broker_owner.is_some_and(|(pid, ticks, _, _)| {
            view.build
                .as_ref()
                .is_none_or(|build| build.pid != pid || build.start_ticks != ticks)
        }) {
            self.broker.clear();
            self.broker_owner = None;
        }
        if !view.connected {
            self.broker.clear();
            self.broker_owner = None;
        } else if let Some(sample) = &view.native {
            let owner = view
                .build
                .as_ref()
                .zip(sample.feats_binding)
                .map(|(build, (entity, id))| (build.pid, build.start_ticks, entity, id));
            if owner.is_some() && self.broker_owner != owner {
                self.broker.clear();
                self.broker_owner = owner;
            }
            if owner.is_some()
                && let Some(snapshot) = &sample.broker
            {
                self.broker.observe(snapshot);
            }
        }
        self.broker
            .poll(ctx, broker_control_pid(view), self.tab == Tab::Broker);
        self.controller
            .set_broker_enabled(self.broker.needs_reads(self.tab == Tab::Broker));
        if let Some(reference) = self.controller.view().api.as_ref()
            && self.faction_reference_id.as_deref() != Some(reference.id.as_str())
        {
            self.faction_reference_id = Some(reference.id.clone());
            if let Some(faction) = reference.alignment.as_deref().and_then(|value| {
                if value.eq_ignore_ascii_case("Hero") {
                    Some("Hero")
                } else if value.eq_ignore_ascii_case("Villain") {
                    Some("Villain")
                } else {
                    None
                }
            }) {
                self.character.faction = faction.into();
                self.save_feat_faction();
            }
        }
        self.feats
            .select(self.controller.view().api.as_ref().map(|r| r.id.as_str()));
        self.feats.poll();
        self.spawn_cache.update(
            self.controller.view(),
            self.preferences.show_ground_spawns,
            Instant::now(),
        );
        if let Some(build) = self.controller.view().build.as_ref() {
            let identity = (build.pid, build.start_ticks);
            if self.detected_map_source_for != Some(identity) {
                self.detected_map_source_for = Some(identity);
                if self.preferences.map_source.is_empty()
                    && let Some(source) = &build.map_source
                {
                    self.preferences.map_source = source.to_string_lossy().into_owned();
                    self.map
                        .set_auto_import_source(if self.preferences.auto_import_maps {
                            self.preferences.map_source.clone()
                        } else {
                            String::new()
                        });
                }
            }
        }
        let (sample, gate) = map_observation(self.controller.view());
        self.map.update_current_context(sample.as_ref(), Some(gate));
        self.map.poll(ctx);
        if self.controller.view().connected
            && self.selected.is_none_or(|selected| {
                !self
                    .controller
                    .view()
                    .candidates
                    .iter()
                    .any(|candidate| (candidate.pid, candidate.start_ticks) == selected)
            })
        {
            self.selected = self
                .controller
                .view()
                .candidates
                .iter()
                .find(|choice| Some(choice.pid) == self.controller.view().selected_pid)
                .map(|choice| (choice.pid, choice.start_ticks));
        }
        if self.startup_scan && self.controller.view().connected && !self.controller.view().busy {
            self.controller.scan_name(&self.name);
            self.startup_scan = false;
        }
        if self
            .smoke
            .is_some_and(|limit| self.started.elapsed() >= limit)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let view = self.controller.view();
        ctx.request_repaint_after(repaint_delay(
            Duration::from_millis(self.preferences.poll_ms),
            view.connected,
            view.native.as_ref().map(|sample| sample.observed_at),
            Instant::now(),
        ));
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let view = self.controller.view().clone();
        self.draw(ui, &view);
    }
}

impl Probe {
    fn draw(&mut self, ui: &mut egui::Ui, view: &crate::session::View) {
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical().id_salt(match self.tab { Tab::WikiItems => "wiki-items-page", Tab::Broker => "broker-page", Tab::Map => "map-page", Tab::Character => "character-page", Tab::Diagnostics => "diagnostics-page" }).show(ui, |ui| {
                if self.tab == Tab::Diagnostics {
                    ui.heading(TITLE);
                    ui.label(egui::RichText::new("READ-ONLY VERIFICATION").color(egui::Color32::from_rgb(120, 195, 235)));
                }
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("process").selected_text(self.selected.map(|(pid, _)| format!("DCGAME.EXE / {pid}")).unwrap_or("Select game process".into())).show_ui(ui, |ui| {
                        for process in &view.candidates {
                            ui.selectable_value(&mut self.selected, Some((process.pid, process.start_ticks)), format!("PID {} / start {}", process.pid, process.start_ticks));
                        }
                    });
                    if ui.add_enabled(!view.busy, egui::Button::new("Refresh processes")).clicked() { self.controller.refresh_sessions(); }
                    if ui.add_enabled(self.selected.is_some() && !view.busy, egui::Button::new("Connect")).clicked() { let (pid, start) = self.selected.unwrap(); self.controller.connect_selection(pid, start); }
                    if ui.add_enabled(view.connected || view.busy || self.controller.is_recovering(), egui::Button::new("Disconnect")).clicked() { self.controller.disconnect(); self.startup_scan = false; }
                    if view.connected && ui.button(if view.paused { "Resume" } else { "Pause" }).clicked() {
                        if view.paused { self.controller.resume(); } else { self.controller.pause(); }
                    }
                });
                if self.tab != Tab::Map {
                    ui.label(&view.status);
                    if view.busy { ui.spinner(); }
                }
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    for (tab, label) in [(Tab::Character, "Character"), (Tab::Map, "Map"), (Tab::Broker, "Broker"), (Tab::WikiItems, "Wiki items"), (Tab::Diagnostics, "Diagnostics")] { ui.selectable_value(&mut self.tab, tab, label); }
                    ui.toggle_value(&mut self.show_log, "Log");
                    if ui.button("Settings").clicked() { self.settings = !self.settings; }
                    if ui.button("About").clicked() { self.about = !self.about; }
                    if ui.button("Quit").clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                });
                ui.add_space(4.0);
                match self.tab {
                    Tab::WikiItems => self.wiki.show(ui, view, &mut self.controller),
                    Tab::Character => {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Character");
                            ui.add(egui::TextEdit::singleline(&mut self.name).hint_text("Exact character name").char_limit(64).desired_width(200.0));
                            ui.label("World ID");
                            ui.add(egui::TextEdit::singleline(&mut self.world).desired_width(55.0).char_limit(10));
                            if ui.add_enabled(!view.busy && !self.name.trim().is_empty(), egui::Button::new("Look up character")).clicked() {
                                self.controller.lookup_reference(&self.base, &self.name, &self.world);
                            }
                        });
                        evidence_line(ui, &view.api_status, true);
                        let previous_faction = self.character.faction.clone();
                        if self.character.ui(ui, view.api.as_ref(), self.feats.progress.as_ref(), &self.feats.status, view.feats.as_deref()) {
                            self.feats.refresh();
                        }
                        if previous_faction != self.character.faction { self.save_feat_faction(); }
                    }
                    Tab::Broker => {
                        let status = if view.paused { "Paused - showing previously observed listings" } else { view.native.as_ref().map(|s| s.broker_status.as_str()).unwrap_or("Connect to the game and open its Broker") };
                        self.broker.ui(ui, status);
                    }
                    Tab::Map => {
                        let (sample, gate) = map_observation(view);
                        if ui.checkbox(&mut self.preferences.show_ground_spawns, "Ground spawns").on_hover_text("Keep last-seen ground spawn locations from this session until two complete scans miss them. Cached markers do not establish current availability.").changed() {
                            self.save_ground_spawn_toggle();
                        }
                        let enabled = self.preferences.show_ground_spawns;
                        self.map.ui(ui, sample.as_ref(), Some(gate), if enabled { self.spawn_cache.entries() } else { &[] });
                    }
                    Tab::Diagnostics => {
                        ui.heading("Process and build evidence");
                        egui::Grid::new("build").spacing([20.0, 8.0]).show(ui, |ui| {
                            row(ui, "Connection", if view.connected { "Connected" } else { "Disconnected" });
                            row(ui, "Selected PID", optional(&view.selected_pid));
                            if let Some(build) = &view.build {
                                row(ui, "Build PID", build.pid); row(ui, "Executable size", format!("{} bytes", build.executable_bytes));
                                row(ui, "PE machine", format!("0x{:04x}", build.machine)); row(ui, "Read-only header access", build.header_access);
                            }
                        });
                        if let Some(build) = &view.build { ui.label("Executable SHA-256"); ui.add(egui::Label::new(egui::RichText::new(&build.sha256).monospace()).wrap()); }
                        ui.label("A fingerprint and readable header do not establish a supported character or map profile.");
                        ui.separator(); ui.heading("Experimental native reader");
                        ui.label("Profile: build-specific experimental physics/resource chain v1");
                        ui.label(&view.native_status);
                        ui.label("Identity, resource semantics, map calibration and floor remain unverified.");
                        ui.separator(); ui.heading("Application");
                        ui.label(format!("Version {} / {} / OpenGL", env!("CARGO_PKG_VERSION"), desktop_backend()));
                        #[cfg(target_os = "linux")]
                        ui.label("WM_CLASS: dcuo-companion / DcuoCompanion");
                        #[cfg(target_os = "linux")]
                        ui.label("Normal managed window; exact identity checked before mapping. DWM controls placement and focus.");
                        #[cfg(target_os = "windows")]
                        ui.label("Normal Windows window; use Windows controls to move, resize and switch focus.");
                        ui.label(format!("Uptime: {:.0}s", self.started.elapsed().as_secs_f64()));
                        ui.separator(); ui.heading("Sanitized diagnostic preview");
                        let diagnostic = format!("{}{}", sanitized_session_diagnostics(view), self.map.sanitized_diagnostics(Some(crate::native::PROFILE_ID)));
                        ui.monospace(&diagnostic);
                        if ui.button("Copy sanitized diagnostics").clicked() { ui.ctx().copy_text(diagnostic); }
                    }
                }
                if self.settings {
                    ui.separator(); ui.heading("Settings");
                    ui.label("Preferences contain no character names, process IDs or location observations.");
                    if ui.add(egui::Slider::new(&mut self.scale, 0.75..=2.0).text("UI scale")).changed() { ui.ctx().set_zoom_factor(self.scale); }
                    if ui.checkbox(&mut self.preferences.dark_mode, "Dark theme").changed() { ui.ctx().set_visuals(if self.preferences.dark_mode { egui::Visuals::dark() } else { egui::Visuals::light() }); }
                    ui.label("API base URL"); ui.add(egui::TextEdit::singleline(&mut self.base).desired_width(f32::INFINITY).char_limit(2048));
                    ui.label("Reference world ID"); ui.add(egui::TextEdit::singleline(&mut self.world).char_limit(10));
                    ui.checkbox(&mut self.preferences.auto_import_maps, "Automatically load the current area's installed map");
                    ui.label("Installed map source (CookedPC directory)");
                    ui.add(egui::TextEdit::singleline(&mut self.preferences.map_source).desired_width(f32::INFINITY).char_limit(4096));
                    ui.small("Detected from the connected game when available. Apply preferences to change automatic loading.");
                    ui.label("Local map catalog"); ui.add(egui::TextEdit::singleline(&mut self.map.catalog_path).desired_width(f32::INFINITY).char_limit(4096));
                    ui.add(egui::Slider::new(&mut self.preferences.poll_ms, 100..=2000).text("Idle UI refresh interval (ms)"));
                    if ui.checkbox(&mut self.preferences.auto_reconnect, "Reconnect after the game restarts").changed() {
                        self.controller.set_auto_reconnect(self.preferences.auto_reconnect);
                    }
                    ui.small("Startup automatically connects when one supported game session is running. Reconnect checks the previous build after a lost connection. Disconnect stops automatic connection.");
                    if ui.button("Apply and save preferences").clicked() { self.save_preferences(); }
                    if ui.button("Reset preferences to defaults").clicked() {
                        self.preferences = Settings::default();
                        self.character.faction = self.preferences.feat_faction.clone();
                        self.base = self.preferences.api_base.clone();
                        self.world = self.preferences.world.clone();
                        self.scale = self.preferences.ui_scale;
                        self.map = MapView::new(String::new());
                        self.detected_map_source_for = None;
                        self.controller.set_auto_reconnect(false);
                        ui.ctx().set_zoom_factor(self.scale);
                        ui.ctx().set_visuals(egui::Visuals::dark());
                        self.save_preferences();
                    }
                    ui.small("Reset clears preferences; imported map files are kept.");
                    ui.label(&self.preference_status);
                }
                if self.about {
                    ui.separator(); ui.heading(format!("DCUO Companion {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Read-only Linux and Windows companion application. API reference and experimental memory evidence are independently labeled.");
                    ui.label("Map help: prepare a schema-version 1 local catalog and PNG/JPEG assets with matching dimensions and SHA-256 checksums. Load the catalog in Map; select a map/floor, drag to pan, scroll to zoom and use Fit to reset.");
                    ui.label("The build-specific experimental memory reader is available. Current-character identity, resource semantics, location context and map calibration remain unproven. Manual map browsing and calibration never establish a live location.");
                    ui.label("Connection help: startup finds and connects to one supported game session automatically. If several sessions are running, choose the game and connect. API lookup uses an exact character name and world; it is separately labeled reference data.");
                    ui.label("Use Tab and Shift+Tab to move between controls. Space or Enter activates focused controls.");
                }
            });
        });
        egui::Window::new("Log")
            .open(&mut self.show_log)
            .default_width(560.0)
            .vscroll(true)
            .show(ui.ctx(), |ui| {
                        ui.heading("Experimental memory resources");
                        ui.label("Read-only controlled-pawn chain observations. Current-character identity, resource meaning and this build-specific profile still require independent validation; these values are not API comparisons.");
                        ui.label(&view.native_status);
                        if let Some(sample) = &view.native {
                            ui.label(format!("Memory sample age: {:.2}s", sample.observed_at.elapsed().as_secs_f64()));
                            egui::Grid::new("experimental_resources").spacing([20.0, 8.0]).show(ui, |ui| {
                                row(ui, "Health (experimental)", format!("{:.0} / {:.0}", sample.health, sample.max_health));
                                row(ui, "Power (experimental)", format!("{:.0} / {:.0}", sample.power, sample.max_power));
                            });
                            let health = if sample.max_health > 0.0 { (sample.health / sample.max_health).clamp(0.0, 1.0) } else { 0.0 };
                            let power = if sample.max_power > 0.0 { (sample.power / sample.max_power).clamp(0.0, 1.0) } else { 0.0 };
                            ui.add(egui::ProgressBar::new(health).text("Experimental health"));
                            ui.add(egui::ProgressBar::new(power).text("Experimental power"));
                        } else { ui.label("No fresh experimental resource sample available."); }
                        ui.add_space(16.0); ui.separator(); ui.heading("Experimental name-byte evidence");
                        ui.label("A matching string can be cached or unrelated. It does not prove the current character.");
                        if ui.add_enabled(view.connected && !view.busy && !view.paused && !self.name.trim().is_empty(), egui::Button::new("Scan next bounded step")).clicked() { self.controller.scan_name(&self.name); }
                        if let Some(scan) = &view.scan {
                            ui.label(&scan.status);
                            let fraction = if scan.total_bytes > 0 { scan.bytes_scanned as f32 / scan.total_bytes as f32 } else { 0.0 };
                            ui.add(egui::ProgressBar::new(fraction.clamp(0.0, 1.0)).text(format!("{} / {} bytes", scan.bytes_scanned, scan.total_bytes)));
                            ui.label(format!("UTF-8 hits: {} / UTF-16 hits: {} / candidates: {}", scan.utf8_hits, scan.utf16_hits, scan.candidate_count));
                            if let Some(name) = &scan.observed_name { ui.label(format!("Observed string: {name} (unverified)")); }
                            if scan.done { ui.small("Bounded search complete. A supported character profile is still required."); }
                        }
                ui.label("Current reader status and experimental evidence");
                evidence_line(ui, &view.status, false);
                map_evidence_header(ui, view);
                ui.separator();
                let reader_status = view
                    .native
                    .as_ref()
                    .map(|n| n.ground_spawns_status.as_str())
                    .unwrap_or("Waiting for a fresh ground spawn sample");
                evidence_line(
                    ui,
                    format!(
                        "{} / 1024 cached last-seen locations / {}",
                        self.spawn_cache.entries().len(),
                        if self.preferences.show_ground_spawns {
                            reader_status
                        } else {
                            "Ground spawns off"
                        }
                    ),
                    false,
                );
            });
    }
}

pub fn run() -> eframe::Result {
    let options = match parse_options(std::env::args().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            print!("{HELP}");
            return Ok(());
        }
        Err(error) => {
            eprintln!("{error}\nUse --help for usage.");
            std::process::exit(2);
        }
    };
    let reference_name = if options.reference_stdin {
        let mut bytes = Vec::new();
        std::io::stdin()
            .lock()
            .take(258)
            .read_to_end(&mut bytes)
            .map_err(|error| eframe::Error::AppCreation(Box::new(error)))?;
        if bytes.len() > 257 {
            eprintln!("Reference input exceeds the bounded startup input size.");
            std::process::exit(2);
        }
        let name = String::from_utf8(bytes)
            .map_err(|error| eframe::Error::AppCreation(Box::new(error)))?;
        let name = name.trim().to_owned();
        if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
            eprintln!("Reference name must contain 1..64 characters without control characters.");
            std::process::exit(2);
        }
        name
    } else {
        String::new()
    };
    let viewport = egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_inner_size([640.0, 720.0])
        .with_min_inner_size([320.0, 260.0])
        .with_resizable(true)
        .with_transparent(false)
        .with_decorations(true);
    #[cfg(target_os = "linux")]
    let viewport = viewport
        .with_override_redirect(false)
        .with_window_type(egui::X11WindowType::Normal);
    let native = eframe::NativeOptions {
        viewport,
        #[cfg(target_os = "linux")]
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_x11();
        })),
        renderer: eframe::Renderer::Glow,
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        TITLE,
        native,
        Box::new(move |cc| {
            #[cfg(target_os = "linux")]
            prepare_x11_window(cc, options.position)?;
            #[cfg(target_os = "windows")]
            prepare_windows_window(cc, options.position)?;
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(Probe::new(options, reference_name)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }

    #[test]
    fn map_canvas_dominates_window_and_log_does_not_move_it() {
        let mut app = Probe::with_preferences(
            Options {
                smoke: Some(Duration::from_secs(1)),
                ..Options::default()
            },
            String::new(),
            Settings::default(),
            None,
        );
        app.tab = Tab::Map;
        for size in [egui::vec2(862.0, 1048.0), egui::vec2(1280.0, 720.0)] {
            let ctx = egui::Context::default();
            let mut baseline = None;
            for show_log in [false, false, true, true, false] {
                app.show_log = show_log;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        ..Default::default()
                    },
                    |ui| app.draw(ui, &crate::session::View::default()),
                );
                output.textures_delta.clear();
                let canvas = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.fill == egui::Color32::from_rgb(20, 25, 32) =>
                        {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .expect("map canvas drawn");
                assert!(
                    canvas.area() / (size.x * size.y) > 0.75,
                    "Canvas too small: {canvas:?} in {size:?}"
                );
                if let Some(expected) = baseline {
                    assert_eq!(canvas, expected);
                }
                baseline = Some(canvas);
            }
        }
    }

    #[test]
    fn ground_spawn_toggle_does_not_apply_pending_map_settings() {
        let mut app = Probe::with_preferences(
            Options {
                smoke: Some(Duration::from_secs(1)),
                ..Options::default()
            },
            String::new(),
            Settings::default(),
            None,
        );
        app.map.load();
        let before = app.map.sanitized_diagnostics(None);
        app.preferences.map_source = "/pending/source".into();
        app.preferences.show_ground_spawns = true;
        app.save_ground_spawn_toggle();
        assert_eq!(app.map.sanitized_diagnostics(None), before);
        assert_eq!(app.preferences.map_source, "/pending/source");
    }

    // Replay presentation only: no PID, name lookup, persisted preferences, or
    // process attachment. Backend generation/expiry tests live in session/maps.
    #[test]
    fn ordinary_launch_restores_faction_without_character_lookup() {
        let app = Probe::with_preferences(
            Options {
                smoke: Some(Duration::from_secs(1)),
                ..Options::default()
            },
            String::new(),
            Settings {
                feat_faction: "Villain".into(),
                ..Settings::default()
            },
            None,
        );
        assert_eq!(app.character.faction, "Villain");
        assert!(app.controller.view().api.is_none());
    }
    #[test]
    fn character_tab_keeps_connection_failure_visible_without_log() {
        let mut app = Probe::with_preferences(
            Options {
                smoke: Some(Duration::from_secs(1)),
                ..Options::default()
            },
            String::new(),
            Settings::default(),
            None,
        );
        let view = crate::session::View {
            status: "Read access denied for selected process".into(),
            ..Default::default()
        };
        let mut output =
            egui::Context::default().run_ui(egui::RawInput::default(), |ui| app.draw(ui, &view));
        assert!(!app.show_log);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::epaint::Shape::Text(text) if text.galley.text() == view.status)));
        output.textures_delta.clear();
    }

    #[test]
    fn offline_presentation_replay_at_supported_scales() {
        let mut app = Probe::with_preferences(
            Options {
                smoke: Some(Duration::from_secs(1)),
                ..Options::default()
            },
            String::new(),
            Settings::default(),
            None,
        );
        app.settings = true;
        app.about = true;
        let mut states = vec![crate::session::View::default()];
        let loading = crate::session::View {
            busy: true,
            status: "Synthetic loading".into(),
            ..Default::default()
        };
        states.push(loading);
        states.push(crate::session::View {
            connected: true,
            paused: true,
            status: "Synthetic paused".into(),
            ..Default::default()
        });
        states.push(crate::session::View {
            api_status: "Reference service unavailable".into(),
            native_status: "Sample expired".into(),
            ..Default::default()
        });
        let mut sample = crate::native::Sample::synthetic(1);
        sample.area_status = "Unsupported synthetic area".into();
        states.push(crate::session::View {
            connected: true,
            native: Some(sample),
            ..Default::default()
        });
        for scale in [0.75, 1.0, 2.0] {
            let ctx = egui::Context::default();
            ctx.set_zoom_factor(scale);
            for view in &states {
                for tab in [Tab::Character, Tab::Map, Tab::Broker, Tab::Diagnostics] {
                    app.tab = tab;
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(640.0 / scale, 480.0 / scale),
                            )),
                            ..Default::default()
                        },
                        |ui| app.draw(ui, view),
                    );
                    assert!(!output.shapes.is_empty());
                    output.textures_delta.clear();
                    assert!(!app.controller.view().connected);
                }
            }
        }
    }

    #[test]
    fn map_evidence_keeps_geometry_across_sample_loss_and_long_status() {
        let missing = crate::session::View::default();
        let mut sample = crate::native::Sample::synthetic(1);
        sample.area_status = "Long synthetic area detail ".repeat(30);
        let fresh = crate::session::View {
            native: Some(sample),
            native_status: "Long synthetic reader status\nwith another line ".repeat(30),
            ..Default::default()
        };
        for width in [240.0, 640.0, 1280.0] {
            let ctx = egui::Context::default();
            let mut baseline = None;
            for view in [&missing, &fresh, &missing, &fresh] {
                let mut bottom = 0.0;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 600.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        map_evidence_header(ui, view);
                        bottom = ui.cursor().top();
                    },
                );
                output.textures_delta.clear();
                if let Some(expected) = baseline {
                    assert_eq!(bottom, expected, "Header moved at width {width}");
                } else {
                    baseline = Some(bottom);
                }
            }
        }
    }

    #[test]
    fn exported_diagnostics_exclude_session_identifiers_and_errors() {
        let view = crate::session::View {
            selected_pid: Some(123456789),
            status: "PRIVATE status with URL and character".into(),
            api_status: "PRIVATE api error".into(),
            native_status: "PRIVATE coordinates".into(),
            ..Default::default()
        };
        let output = sanitized_session_diagnostics(&view);
        assert!(!output.contains("PRIVATE"));
        assert!(!output.contains("123456789"));
        assert!(output.contains("Live character identity: unverified"));
    }

    #[test]
    fn bounds_smoke_lifetime_and_accepts_negative_placement() {
        let options = parse_options(args(&["--smoke-seconds", "10", "--position", "-1920", "0"]))
            .unwrap()
            .unwrap();
        assert_eq!(options.smoke, Some(Duration::from_secs(10)));
        assert_eq!(options.position, Some((-1920, 0)));
        for invalid in ["0", "3601", "-1", "NaN"] {
            assert!(parse_options(args(&["--smoke-seconds", invalid])).is_err());
        }
    }

    #[test]
    fn live_repaint_never_waits_past_sample_expiry() {
        let now = Instant::now();
        let slow = Duration::from_secs(2);
        assert_eq!(repaint_delay(slow, false, None, now), slow);
        assert_eq!(
            repaint_delay(slow, true, None, now),
            Duration::from_millis(250)
        );
        assert_eq!(
            repaint_delay(slow, true, Some(now - Duration::from_millis(990)), now),
            Duration::from_millis(10)
        );
        assert_eq!(
            repaint_delay(slow, true, Some(now - Duration::from_secs(2)), now),
            Duration::ZERO
        );
    }
    #[test]
    fn accepts_explicit_session_and_bounded_reference_mode() {
        let options = parse_options(args(&[
            "--pid",
            "42",
            "--reference-stdin",
            "--world",
            "2",
            "--api-base",
            "https://example.invalid/api",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(options.pid, Some(42));
        assert!(options.reference_stdin);
        assert_eq!(options.world, "2");
        assert_eq!(options.api_base, "https://example.invalid/api");
        for values in [
            vec!["--pid", "0"],
            vec!["--pid", "-1"],
            vec!["--pid"],
            vec!["--world"],
            vec!["--api-base"],
            vec!["--reference-stdin", "--reference-stdin"],
            vec!["--world", "2", "--world", "3"],
        ] {
            assert!(parse_options(args(&values)).is_err());
        }
    }

    #[test]
    fn rejects_incomplete_repeated_and_unknown_arguments() {
        for values in [
            vec!["--position", "1"],
            vec!["--smoke-seconds"],
            vec!["--position", "1", "2", "--position", "3", "4"],
            vec!["--unknown"],
        ] {
            assert!(parse_options(args(&values)).is_err());
        }
        assert!(parse_options(args(&["--help"])).unwrap().is_none());
    }
}

#[cfg(test)]
mod broker_lifecycle_tests {
    use super::broker_control_pid;
    #[test]
    fn broker_control_is_independent_of_transient_physics_samples() {
        let mut view = crate::session::View {
            connected: true,
            selected_pid: Some(42),
            ..Default::default()
        };
        assert!(view.native.is_none());
        assert_eq!(broker_control_pid(&view), Some(42));
        view.paused = true;
        assert_eq!(broker_control_pid(&view), None);
        view.paused = false;
        view.connected = false;
        assert_eq!(broker_control_pid(&view), None);
    }
}
