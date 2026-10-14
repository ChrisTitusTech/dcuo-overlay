//! Bounded Phase 0 window experiment. No game reader or API client is started.

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
const HELP: &str = "DCUO Companion - Phase 0 managed X11 window probe
Usage: dcuo-window-probe [--smoke-seconds SECONDS] [--position X Y]

  --smoke-seconds SECONDS  Close automatically after 1..3600 seconds.
  --position X Y          Initial physical-pixel placement hint for DWM.
  --help                  Print this help without opening a window.

The probe opens a normal opaque X11 window with ordinary input and resizing.
It reads no game memory and makes no API requests. DWM owns placement and focus.
";

#[derive(Default)]
struct Options {
    smoke: Option<Duration>,
    position: Option<(i32, i32)>,
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<Option<Options>, String> {
    let mut args = args.into_iter();
    let mut options = Options::default();
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
    paints: u64,
    clicks: u64,
    input: String,
    scale: f32,
    tab: Tab,
}

impl Probe {
    fn new(smoke: Option<Duration>) -> Self {
        Self {
            started: Instant::now(),
            smoke,
            paints: 0,
            clicks: 0,
            input: String::new(),
            scale: 1.0,
            tab: Tab::Character,
        }
    }
}

impl eframe::App for Probe {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self
            .smoke
            .is_some_and(|limit| self.started.elapsed() >= limit)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Periodic repaint requests do not issue focus, raise, or geometry commands.
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.paints = self.paints.saturating_add(1);
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            // Fill the DWM tile even when the probe content needs less height.
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading(TITLE);
                ui.label(egui::RichText::new("PHASE 0  /  Window feasibility").color(egui::Color32::from_rgb(130, 190, 240)));
                ui.label("Game: disconnected | Memory: unavailable | API: not connected");
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    for (tab, label) in [(Tab::Character, "Character"), (Tab::Map, "Map"), (Tab::Diagnostics, "Diagnostics")] {
                        ui.selectable_value(&mut self.tab, tab, label);
                    }
                });
                ui.add_space(12.0);
                match self.tab {
                    Tab::Character => {
                        ui.heading("Waiting for proven character data");
                        ui.label("Character profiles are unsupported in this prototype.");
                        egui::Grid::new("unavailable_fields").spacing([24.0, 8.0]).show(ui, |ui| {
                            for label in ["Character / world", "Power / movement", "CR / skill points", "Health / power"] {
                                ui.label(label); ui.label("Unavailable"); ui.end_row();
                            }
                        });
                    }
                    Tab::Map => {
                        ui.heading("Location unavailable");
                        ui.label("Area, layout, floor, coordinates, assets, and calibration are unproven.");
                        ui.label("No live marker is displayed. Outdoor and indoor support require separate validation.");
                    }
                    Tab::Diagnostics => {
                        ui.heading("Desktop experiment");
                        ui.label("Backend: X11 / OpenGL (glow)");
                        ui.label("Identity checked before mapping: dcuo-companion / DcuoCompanion");
                        ui.label("Normal managed window; DWM controls tags, placement, borders and focus.");
                        ui.label("Live desktop, game-focus, scaling, Picom, and monitor recovery checks are pending.");
                    }
                }
                ui.add_space(12.0);
                ui.separator();
                ui.heading("Input and rendering checks");
                ui.label("Type here, use Tab to move focus, scroll, and resize through DWM.");
                ui.add(egui::TextEdit::singleline(&mut self.input).hint_text("Keyboard input check").char_limit(256).desired_width(f32::INFINITY));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Pointer check").clicked() { self.clicks = self.clicks.saturating_add(1); }
                    ui.label(format!("Clicks: {}", self.clicks));
                    if ui.button("Quit").clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                });
                if ui.add(egui::Slider::new(&mut self.scale, 0.75..=2.0).text("UI scale")).changed() {
                    ui.ctx().set_zoom_factor(self.scale);
                }
                ui.add_space(8.0);
                ui.monospace(format!("Rendered frames: {} | Running: {:.1}s", self.paints, self.started.elapsed().as_secs_f64()));
                if let Some(limit) = self.smoke {
                    ui.label(format!("Smoke test closes after {} seconds.", limit.as_secs()));
                }
                ui.label("This counter measures UI rendering only. No game data is read.");
            });
        });
    }
}

fn main() -> eframe::Result {
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
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([540.0, 460.0])
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
            Ok(Box::new(Probe::new(options.smoke)))
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
