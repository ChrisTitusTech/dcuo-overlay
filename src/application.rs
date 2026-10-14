//! Managed X11 companion with explicitly sourced verification evidence.

use crate::{
    api::DEFAULT_BASE_URL,
    map_view::MapView,
    maps::{Context, LocationConfidence, LocationSample, MarkerGate, Point},
    session::Controller,
    settings::{self, Settings},
};
use std::io::Read;
use std::time::{Duration, Instant};

use eframe::egui;
use winit::platform::x11::EventLoopBuilderExtX11;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, MapState, PropMode};
use x11rb::wrapper::ConnectionExt as _;

const TITLE: &str = "DCUO Companion";
const WM_CLASS: &[u8] = b"dcuo-companion\0DcuoCompanion\0";
type SetupError = Box<dyn std::error::Error + Send + Sync>;
const HELP: &str = "DCUO Companion - managed X11 verification window
Usage: dcuo-companion [--smoke-seconds SECONDS] [--position X Y]
       [--pid PID] [--reference-stdin] [--world ID] [--api-base URL]

  --smoke-seconds SECONDS  Close after 1..3600 seconds.
  --position X Y          Initial physical-pixel placement hint for DWM.
  --pid PID               Connect to an explicit positive process ID.
  --reference-stdin       Read an exact reference name from stdin (64 characters).
  --world ID              Reference world ID (saved preference, otherwise 2).
  --api-base URL          Reference service (saved preference, otherwise https://dcuo.bot/api).
  --help                  Show help.
";

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

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Character,
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
    startup_scan: bool,
    preferences: Settings,
    preference_status: String,
    map: MapView,
    initialized: bool,
}

impl Probe {
    fn new(options: Options, name: String) -> Self {
        let (mut preferences, warning) = settings::load();
        if options.world_explicit {
            preferences.world = options.world.clone();
        }
        if options.base_explicit {
            preferences.api_base = options.api_base.clone();
        }
        let mut map = MapView::new(preferences.map_catalog.clone());
        if !preferences.map_catalog.is_empty() {
            map.load();
        }
        let mut controller = Controller::new();
        if let Some(pid) = options.pid {
            controller.connect(pid);
        }
        if !name.is_empty() {
            controller.lookup_reference(&preferences.api_base, &name, &preferences.world);
        }
        let startup_scan = !name.is_empty() && options.pid.is_some();
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
            settings: false,
            about: false,
            startup_scan,
            preferences,
            preference_status: warning.unwrap_or_default(),
            map,
            initialized: false,
        }
    }
}

impl Probe {
    fn save_preferences(&mut self) {
        self.preferences.api_base = self.base.clone();
        self.preferences.world = self.world.clone();
        self.preferences.ui_scale = self.scale;
        self.preferences.map_catalog = self.map.catalog_path.clone();
        self.preferences.auto_reconnect = false;
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
        self.map.poll(ctx);
        self.controller.poll();
        if self.selected.is_none() && self.controller.view().connected {
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
        ctx.request_repaint_after(Duration::from_millis(self.preferences.poll_ms));
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let view = self.controller.view().clone();
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading(TITLE);
                ui.label(egui::RichText::new("READ-ONLY VERIFICATION").color(egui::Color32::from_rgb(120, 195, 235)));
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("process").selected_text(self.selected.map(|(pid, _)| format!("DCGAME.EXE / {pid}")).unwrap_or("Select game process".into())).show_ui(ui, |ui| {
                        for process in &view.candidates {
                            ui.selectable_value(&mut self.selected, Some((process.pid, process.start_ticks)), format!("PID {} / start {}", process.pid, process.start_ticks));
                        }
                    });
                    if ui.add_enabled(!view.busy, egui::Button::new("Refresh processes")).clicked() { self.controller.refresh_sessions(); }
                    if ui.add_enabled(self.selected.is_some() && !view.busy, egui::Button::new("Connect")).clicked() { let (pid, start) = self.selected.unwrap(); self.controller.connect_selection(pid, start); }
                    if ui.add_enabled(view.connected || view.busy, egui::Button::new("Disconnect")).clicked() { self.controller.disconnect(); self.startup_scan = false; }
                    if view.connected && ui.button(if view.paused { "Resume" } else { "Pause" }).clicked() {
                        if view.paused { self.controller.resume(); } else { self.controller.pause(); }
                    }
                });
                ui.label(&view.status);
                if view.busy { ui.spinner(); }
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    for (tab, label) in [(Tab::Character, "Character"), (Tab::Map, "Map"), (Tab::Diagnostics, "Diagnostics")] { ui.selectable_value(&mut self.tab, tab, label); }
                    if ui.button("Settings").clicked() { self.settings = !self.settings; }
                    if ui.button("About").clicked() { self.about = !self.about; }
                    if ui.button("Quit").clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                });
                ui.add_space(12.0);
                match self.tab {
                    Tab::Character => {
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
                        ui.separator();
                        ui.heading("API character reference");
                        ui.label("An exact-name lookup is a reference, not memory-verified current-character identity.");
                        ui.add(egui::TextEdit::singleline(&mut self.name).hint_text("Exact character name").char_limit(64).desired_width(f32::INFINITY));
                        ui.horizontal_wrapped(|ui| {
                            ui.label("World ID"); ui.add(egui::TextEdit::singleline(&mut self.world).desired_width(70.0).char_limit(10));
                            if ui.add_enabled(!view.busy && !self.name.trim().is_empty(), egui::Button::new("Look up reference")).clicked() { self.controller.lookup_reference(&self.base, &self.name, &self.world); }
                        });
                        ui.label(&view.api_status);
                        if let Some(fetched) = view.api_fetched { ui.label(format!("Received {:.0}s ago / server cache freshness unknown", fetched.elapsed().as_secs_f64())); }
                        if let Some(reference) = &view.api {
                            egui::Grid::new("reference_stats").spacing([20.0, 8.0]).show(ui, |ui| {
                                row(ui, "Reference name", &reference.name); row(ui, "Character ID", &reference.id);
                                row(ui, "World ID", &reference.world_id);
                                row(ui, "Power", optional(&reference.power)); row(ui, "Movement", optional(&reference.movement));
                                row(ui, "Alignment", optional(&reference.alignment)); row(ui, "Combat rating", optional(&reference.combat_rating));
                                row(ui, "Skill points", optional(&reference.skill_points)); row(ui, "Health (reference maximum)", optional(&reference.health));
                                row(ui, "Power (reference maximum)", optional(&reference.power_max)); row(ui, "League", optional(&reference.league));
                            });
                            ui.small("Every field above comes from the API. Live health, power, location and character identity remain unverified.");
                        }
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
                    }
                    Tab::Map => {
                        ui.heading("Experimental location evidence");
                        ui.label(&view.native_status);
                        if let Some(sample) = &view.native {
                            ui.label(format!("Memory sample age: {:.2}s", sample.observed_at.elapsed().as_secs_f64()));
                            ui.label(format!("Position (experimental): {:.1}, {:.1}, {:.1}", sample.position[0], sample.position[1], sample.position[2]));
                            ui.label(format!("Streaming area evidence: {}", sample.area_key.as_deref().unwrap_or("Unavailable / ambiguous")));
                            ui.small(&sample.area_status);
                            ui.small("Position and streaming evidence do not establish map calibration, floor or current-character identity. The experimental marker is a preview, not a fully validated profile.");
                        } else { ui.label("No fresh experimental position sample available."); }
                        ui.separator();
                        let sample = view.native.as_ref().and_then(|n| n.area_key.as_ref().map(|area| LocationSample {
                            point: Point { x: n.position[0] as f64, y: n.position[1] as f64 },
                            context: Context { zone: area.clone(), layout: "main".into(), floor: "overview".into() },
                            session_generation: view.location_generation,
                            zone_generation: view.location_generation,
                            observed_at: n.observed_at,
                            valid: view.connected && !view.paused,
                        }));
                        self.map.ui(ui, sample.as_ref(), Some(MarkerGate {
                            session_generation: view.location_generation,
                            zone_generation: view.location_generation,
                            paused: view.paused,
                            confidence: LocationConfidence::Experimental,
                        }));
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
                        ui.label(format!("Version {} / X11 / OpenGL", env!("CARGO_PKG_VERSION")));
                        ui.label("WM_CLASS: dcuo-companion / DcuoCompanion");
                        ui.label("Normal managed window; exact identity checked before mapping. DWM controls placement and focus.");
                        ui.label(format!("Uptime: {:.0}s", self.started.elapsed().as_secs_f64()));
                        ui.separator(); ui.heading("Sanitized diagnostic preview");
                        let diagnostic = format!("DCUO Companion {}\nBackend: managed X11 / OpenGL\nConnection: {}\nPaused: {}\nHeader evidence available: {}\nAPI reference available: {}\nNative profile: experimental physics/resource chain v1\nFresh native sample available: {}\nLive character identity: unverified\nTrusted live location: unavailable\n", env!("CARGO_PKG_VERSION"), view.connected, view.paused, view.build.is_some(), view.api.is_some(), view.native.is_some());
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
                    ui.label("Local map catalog"); ui.add(egui::TextEdit::singleline(&mut self.map.catalog_path).desired_width(f32::INFINITY).char_limit(4096));
                    ui.add(egui::Slider::new(&mut self.preferences.poll_ms, 100..=2000).text("UI refresh interval (ms)"));
                    ui.add_enabled(false, egui::Checkbox::new(&mut self.preferences.auto_reconnect, "Automatic reconnect - unavailable"));
                    if ui.button("Apply and save preferences").clicked() { self.save_preferences(); }
                    ui.label(&self.preference_status);
                }
                if self.about {
                    ui.separator(); ui.heading(format!("DCUO Companion {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Read-only Linux/X11 companion feasibility application. API reference and experimental memory evidence are independently labeled.");
                    ui.label("Map help: prepare a schema-version 1 local catalog and PNG/JPEG assets with matching dimensions and SHA-256 checksums. Load the catalog in Map; select a map/floor, drag to pan, scroll to zoom and use Fit to reset.");
                    ui.label("The build-specific experimental memory reader is available. Current-character identity, resource semantics, location context and map calibration remain unproven. Manual map browsing and calibration never establish a live location.");
                    ui.label("Connection help: refresh processes, choose the game and connect. API lookup uses an exact character name and world; it is separately labeled reference data.");
                    ui.label("Use Tab and Shift+Tab to move between controls. Space or Enter activates focused controls.");
                }
            });
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
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([640.0, 720.0])
            .with_min_inner_size([320.0, 260.0])
            .with_resizable(true)
            .with_transparent(false)
            .with_decorations(true)
            .with_override_redirect(false)
            .with_window_type(egui::X11WindowType::Normal),
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
            prepare_x11_window(cc, options.position)?;
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
