//! Local map browsing with one bounded, joined asset worker.
use crate::map_import::ImportCancellation;
use crate::maps::{self, Catalog, LocationSample, MapEntry, MarkerGate};
use eframe::egui;
use std::{
    collections::VecDeque,
    io::Cursor,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Instant,
};

enum Request {
    Catalog(u64, PathBuf),
    Import(u64, PathBuf, PathBuf),
    ImportArea(u64, PathBuf, PathBuf, String, ImportCancellation),
    Asset(u64, PathBuf, Box<MapEntry>),
}
enum Loaded {
    Catalog(Catalog, PathBuf),
    Image(egui::ColorImage),
    Imported(PathBuf),
    ImportedArea(String, PathBuf),
}
type Reply = (u64, Result<Loaded, String>);

#[derive(Clone, Debug, PartialEq, Eq)]
struct AutoContext {
    context: maps::Context,
    session: u64,
    zone: u64,
}
struct AutoMemo {
    area: String,
    path: Option<PathBuf>,
}
pub struct MapView {
    pub catalog_path: String,
    catalog: Option<Catalog>,
    directory: PathBuf,
    selected: usize,
    texture: Option<egui::TextureHandle>,
    generation: u64,
    pending: Option<Request>,
    tx: Option<mpsc::SyncSender<Request>>,
    rx: Option<mpsc::Receiver<Reply>>,
    worker: Option<thread::JoinHandle<()>>,
    cancellation: ImportCancellation,
    status: String,
    zoom: f32,
    pan: egui::Vec2,
    follow: bool,
    pairs: [[f64; 4]; 3],
    calibration: String,
    cookedpc: String,
    busy: bool,
    browse_other_maps: bool,
    details_open: bool,
    auto_import: bool,
    auto_context: Option<AutoContext>,
    auto_token: Option<ImportCancellation>,
    auto_inflight: Option<String>,
    auto_memo: VecDeque<AutoMemo>,
}
impl Default for MapView {
    fn default() -> Self {
        Self::new(String::new())
    }
}
impl MapView {
    pub fn new(catalog_path: String) -> Self {
        Self::new_with_importer(catalog_path, crate::map_import::import_maps_cancellable)
    }
    fn new_with_importer<F>(catalog_path: String, importer: F) -> Self
    where
        F: Fn(&Path, &Path, &ImportCancellation) -> Result<PathBuf, String> + Send + 'static,
    {
        let cancellation = ImportCancellation::default();
        let worker_cancellation = cancellation.clone();
        let (tx, requests) = mpsc::sync_channel(1);
        let (replies, rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            while let Ok(request) = requests.recv() {
                if worker_cancellation.is_cancelled() {
                    break;
                }
                let (generation, result) = match request {
                    Request::Catalog(g, path) => (
                        g,
                        maps::load_catalog(&path).map(|catalog| {
                            Loaded::Catalog(
                                catalog,
                                maps::catalog_directory(&path).into(),
                            )
                        }),
                    ),
                    Request::Asset(g, dir, entry) => (g, decode(&dir, &entry).map(Loaded::Image)),
                    Request::ImportArea(g, source, destination, area, token) => (
                        g,
                        crate::map_import::import_area_cancellable(&source, &destination, &area, &token)
                            .map(|path| Loaded::ImportedArea(area, path))
                            .map_err(|_| "Current-area map import failed or is unsupported. Check the installation, then use Retry current map.".into()),
                    ),
                    Request::Import(g, source, destination) => (
                        g,
                        importer(&source, &destination, &worker_cancellation)
                            .map(Loaded::Imported)
                            .map_err(|_| "Installed map import failed. Check the local CookedPC directory and writable data directory; unsupported or corrupt game packages are rejected.".into()),
                    ),
                };
                if worker_cancellation.is_cancelled() {
                    break;
                }
                if replies.send((generation, result)).is_err() {
                    break;
                }
            }
        });
        Self {
            catalog_path,
            catalog: None,
            directory: PathBuf::new(),
            selected: 0,
            texture: None,
            generation: 0,
            pending: None,
            tx: Some(tx),
            rx: Some(rx),
            worker: Some(worker),
            cancellation,
            status: "Load a local map catalog to browse maps.".into(),
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            follow: true,
            pairs: [[0.; 4]; 3],
            calibration: String::new(),
            cookedpc: std::env::var_os("HOME").map(PathBuf::from).filter(|path| path.is_absolute()).map(|path| path.join(".local/share/Steam/steamapps/common/DC Universe Online/UNREAL3/DCGAME/COOKEDPC").to_string_lossy().into_owned()).unwrap_or_default(),
            busy: false,
            browse_other_maps: false,
            details_open: false,
            auto_import: true,
            auto_context: None,
            auto_token: None,
            auto_inflight: None,
            auto_memo: VecDeque::new(),
        }
    }
    pub fn set_auto_import_source(&mut self, source: String) {
        if self.cookedpc != source {
            self.cancel_auto();
            self.auto_context = None;
            self.auto_memo.clear();
            self.cookedpc = source;
            self.auto_import = !self.cookedpc.is_empty() && !self.browse_other_maps;
            // A paused/disconnected view can retain a catalog after its live
            // context has expired. No source-dependent result may survive.
            self.catalog = None;
            self.texture = None;
            self.generation = self.generation.wrapping_add(1);
            self.pending = None;
            self.busy = false;
        }
        if self.cookedpc.is_empty() {
            self.cancel_auto();
            self.auto_import = false;
        }
    }
    fn cancel_auto(&mut self) {
        if let Some(token) = self.auto_token.take() {
            token.cancel();
        }
        if self.auto_inflight.take().is_some() {
            self.generation = self.generation.wrapping_add(1);
            self.pending = None;
            self.busy = false;
        }
    }
    fn remember_auto(&mut self, area: String, path: Option<PathBuf>) {
        self.auto_memo.retain(|memo| memo.area != area);
        if self.auto_memo.len() == 8 {
            self.auto_memo.pop_front();
        }
        self.auto_memo.push_back(AutoMemo { area, path });
    }
    pub fn update_current_context(
        &mut self,
        sample: Option<&LocationSample>,
        gate: Option<MarkerGate>,
    ) {
        if !self.auto_import || self.browse_other_maps || self.cookedpc.is_empty() {
            return;
        }
        let next = fresh_auto_context(sample, gate, Instant::now());
        if self.auto_context != next {
            self.cancel_auto();
            self.generation = self.generation.wrapping_add(1);
            self.pending = None;
            self.busy = false;
            // The image belongs to an area, not a particular memory sample.
            // Keep it through outages and same-area generation changes; the
            // independently gated marker still disappears immediately.
            if next.as_ref().is_some_and(|next| {
                self.catalog
                    .as_ref()
                    .and_then(|catalog| catalog.entries.get(self.selected))
                    .is_none_or(|entry| entry.context != next.context)
            }) {
                self.texture = None;
            }
            self.auto_context = next;
        }
        let Some(current) = self.auto_context.as_ref() else {
            return;
        };
        if self.busy {
            return;
        }
        let area = current.context.zone.clone();
        if self
            .auto_memo
            .iter()
            .any(|memo| memo.area == area && memo.path.is_none())
        {
            self.status = "Current-area map unavailable. Use Retry current map after correcting the installation.".into();
            return;
        }
        // Saved catalogs have not passed this launch's source fingerprint check.
        // Only imports memoized by this process may take the loaded-map shortcut.
        let source_checked = self.auto_memo.iter().any(|memo| {
            memo.area == area
                && memo
                    .path
                    .as_ref()
                    .is_some_and(|path| path == std::path::Path::new(&self.catalog_path))
        });
        if source_checked
            && let Some(index) = self.catalog.as_ref().and_then(|catalog| {
                catalog.matching(&current.context).and_then(|matching| {
                    catalog
                        .entries
                        .iter()
                        .position(|entry| std::ptr::eq(entry, matching))
                })
            })
        {
            if self.texture.is_none() || self.selected != index {
                self.selected = index;
                self.auto_inflight = Some(area);
                self.asset();
            }
            return;
        }
        if let Some(path) = self
            .auto_memo
            .iter()
            .find(|memo| memo.area == area)
            .and_then(|memo| memo.path.clone())
        {
            self.catalog_path = path.to_string_lossy().into_owned();
            self.auto_inflight = Some(area);
            self.load_internal();
            return;
        }
        let source = PathBuf::from(&self.cookedpc);
        let Some(destination) = import_destination().filter(|_| {
            source.is_absolute()
                && !self.cookedpc.contains("://")
                && !self.cookedpc.chars().any(char::is_control)
        }) else {
            self.remember_auto(area, None);
            self.status =
                "Configure a local installed-game directory before retrying the current map."
                    .into();
            return;
        };
        let token = ImportCancellation::default();
        self.auto_token = Some(token.clone());
        self.auto_inflight = Some(area.clone());
        self.generation = self.generation.wrapping_add(1);
        self.pending = Some(Request::ImportArea(
            self.generation,
            source,
            destination,
            area,
            token,
        ));
        self.texture = None;
        self.busy = true;
        self.status = "Loading current-area map from the local installation...".into();
    }
    fn browse(&mut self) {
        self.cancel_auto();
        self.browse_other_maps = true;
        self.auto_import = false;
        if self.texture.is_none()
            && self
                .catalog
                .as_ref()
                .is_some_and(|c| c.entries.get(self.selected).is_some())
        {
            self.asset();
        }
    }
    fn retry_current(&mut self) {
        if let Some(current) = &self.auto_context {
            self.auto_memo
                .retain(|memo| memo.area != current.context.zone);
        }
        self.cancel_auto();
        self.catalog = None;
        self.texture = None;
    }
    /// Export only fixed labels, enum values, numeric schema versions, and flags.
    /// Never export paths, map labels, catalog strings, error text, or coordinates.
    pub fn sanitized_diagnostics(&self, profile: Option<&str>) -> String {
        let entry = self
            .catalog
            .as_ref()
            .and_then(|c| c.entries.get(self.selected));
        let calibration = entry.and_then(|e| e.calibration.as_ref());
        let confidence = entry
            .map(|e| e.effective_confidence(maps::LocationConfidence::Validated, profile))
            .unwrap_or(maps::LocationConfidence::Unavailable);
        let compatible = calibration.map(|c| profile == Some(c.profile.as_str()));
        format!(
            "Map catalog present: {}; schema: {}; selected calibration present: {}; version: {}; effective confidence: {:?}; profile compatible: {:?}; loader busy: {}; texture loaded: {}",
            self.catalog.is_some(),
            self.catalog.as_ref().map_or(0, |c| c.schema_version),
            calibration.is_some(),
            calibration.map_or(0, |c| c.version),
            confidence,
            compatible,
            self.busy,
            self.texture.is_some()
        )
    }
    pub fn load_saved_catalog(&mut self) {
        self.load_internal();
    }
    pub fn load(&mut self) {
        self.cancel_auto();
        self.auto_import = false;
        self.load_internal();
    }
    fn load_internal(&mut self) {
        self.busy = true;
        self.generation = self.generation.wrapping_add(1);
        self.texture = None;
        self.catalog = None;
        self.pending = Some(Request::Catalog(
            self.generation,
            PathBuf::from(&self.catalog_path),
        ));
        self.status = "Loading catalog...".into();
    }
    fn asset(&mut self) {
        self.busy = true;
        self.generation = self.generation.wrapping_add(1);
        self.texture = None;
        self.pan = egui::Vec2::ZERO;
        if let Some(entry) = self
            .catalog
            .as_ref()
            .and_then(|c| c.entries.get(self.selected))
            .cloned()
        {
            self.pending = Some(Request::Asset(
                self.generation,
                self.directory.clone(),
                Box::new(entry),
            ));
            self.status = "Loading map...".into();
        }
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Some(Ok((generation, result))) = self.rx.as_ref().map(|rx| rx.try_recv()) {
            if generation != self.generation {
                continue;
            }
            self.busy = false;
            match result {
                Ok(Loaded::Catalog(catalog, directory)) => {
                    if self.auto_inflight.is_some()
                        && self
                            .auto_context
                            .as_ref()
                            .is_some_and(|current| catalog.matching(&current.context).is_none())
                    {
                        if let Some(area) = self.auto_inflight.take() {
                            self.remember_auto(area, None);
                        }
                        self.status = "Imported map does not match the current layout/floor. Use Retry current map after checking coverage.".into();
                        continue;
                    }
                    self.catalog = Some(catalog);
                    self.directory = directory;
                    self.selected = 0;
                    self.asset();
                    if self.catalog.as_ref().is_some_and(|c| c.entries.is_empty()) {
                        self.status = "Catalog contains no maps.".into();
                        self.busy = false;
                        if let Some(area) = self.auto_inflight.take() {
                            self.remember_auto(area, None);
                        }
                    }
                }
                Ok(Loaded::Image(image)) => {
                    self.texture =
                        Some(ctx.load_texture("local-map", image, egui::TextureOptions::LINEAR));
                    // A newly loaded map keeps the chosen scale and follows the
                    // player as soon as a valid matching location is available.
                    self.follow = true;
                    self.pan = egui::Vec2::ZERO;
                    self.status = "Map loaded.".into();
                    self.auto_inflight = None;
                    self.auto_token = None;
                }
                Ok(Loaded::ImportedArea(area, path)) => {
                    self.remember_auto(area, Some(path.clone()));
                    self.catalog_path = path.to_string_lossy().into_owned();
                    self.load_internal();
                }
                Ok(Loaded::Imported(path)) => {
                    self.catalog_path = path.to_string_lossy().into_owned();
                    self.load();
                }
                Err(error) => {
                    if let Some(area) = self.auto_inflight.take() {
                        self.remember_auto(area, None);
                    }
                    self.auto_token = None;
                    self.status = error;
                }
            }
        }
        if let Some(request) = self.pending.take()
            && let Some(tx) = &self.tx
        {
            match tx.try_send(request) {
                Err(mpsc::TrySendError::Full(request)) => self.pending = Some(request),
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    self.status = "Map loader unavailable.".into();
                    self.busy = false
                }
                Ok(()) => {}
            }
        }
    }
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        sample: Option<&LocationSample>,
        gate: Option<MarkerGate>,
        ground_spawns: &[crate::spawn_cache::CachedSpawn],
    ) {
        self.update_current_context(sample, gate);
        self.poll(ui.ctx());
        ui.horizontal(|ui| {
            ui.strong("Map");
            ui.toggle_value(&mut self.details_open, "Map details");
            if let Some(entry) = self
                .catalog
                .as_ref()
                .and_then(|c| c.entries.get(self.selected))
            {
                ui.add(egui::Label::new(&entry.label).truncate())
                    .on_hover_text(&entry.label);
            }
        });
        let mut details_open = self.details_open;
        egui::Window::new("Map details")
            .open(&mut details_open)
            .default_width(520.0)
            .vscroll(true)
            .show(ui.ctx(), |ui| self.details_ui(ui, sample, gate));
        self.details_open = details_open;
        let now = Instant::now();
        let current_index = self
            .catalog
            .as_ref()
            .and_then(|catalog| current_map_index(catalog, sample, gate, now));
        if !self.browse_other_maps
            && !self.busy
            && let Some(index) = current_index
            && index != self.selected
        {
            self.selected = index;
            self.asset();
        }
        let entry = self
            .catalog
            .as_ref()
            .and_then(|c| c.entries.get(self.selected));
        let marker = entry
            .filter(|_| self.texture.is_some())
            .and_then(|entry| projected_marker(entry, sample, gate, now));
        let ground_spawn_markers = entry
            .filter(|_| self.texture.is_some())
            .map(|entry| projected_ground_spawns(entry, ground_spawns, gate))
            .unwrap_or_default();
        let location_status = map_location_status(
            self.browse_other_maps,
            self.texture.is_some(),
            current_index == Some(self.selected),
            marker.is_some(),
        );
        ui.horizontal(|ui| {
            if ui.button("-").clicked() {
                self.zoom = (self.zoom / 1.25).max(0.25);
            }
            if ui.button("+").clicked() {
                self.zoom = (self.zoom * 1.25).min(16.0);
            }
            if ui.button("Fit").clicked() {
                self.zoom = 1.0;
                self.pan = egui::Vec2::ZERO;
                self.follow = false;
            }
            ui.add_enabled(
                marker.is_some(),
                egui::Checkbox::new(&mut self.follow, "Follow"),
            );
            if ui
                .add_enabled(marker.is_some(), egui::Button::new("Recenter"))
                .clicked()
            {
                self.pan = egui::Vec2::ZERO;
                self.follow = true;
            }
            ui.add(egui::Label::new(egui::RichText::new(location_status).small()).truncate())
                .on_hover_text(location_status);
        });
        // The enclosing scroll area can have unbounded content height.
        // Fill its visible viewport without reserving space for hidden details.
        let size = map_canvas_size(
            ui.available_width(),
            ui.clip_rect().bottom(),
            ui.cursor().top(),
        );
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::drag());
        ui.painter()
            .rect_filled(rect, 0.0, egui::Color32::from_rgb(20, 25, 32));
        if response.dragged() {
            self.pan += response.drag_delta();
            self.follow = false;
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            self.zoom = (self.zoom * (scroll * 0.002).exp()).clamp(0.25, 16.0);
        }
        if let Some(texture) = &self.texture {
            let image_size = texture.size_vec2();
            let scale = (rect.width() / image_size.x).min(rect.height() / image_size.y) * self.zoom;
            if self.follow
                && let Some(point) = marker
            {
                self.pan = (image_size * 0.5 - egui::vec2(point.x as f32, point.y as f32)) * scale;
            }
            let target = egui::Rect::from_center_size(rect.center() + self.pan, image_size * scale);
            let painter = ui.painter().with_clip_rect(rect);
            painter.image(
                texture.id(),
                target,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                egui::Color32::WHITE,
            );
            for (index, point) in ground_spawn_markers {
                let color = ground_spawn_color(ground_spawns[index].kind);
                let center = target.min + egui::vec2(point.x as f32, point.y as f32) * scale;
                let diamond = vec![
                    center + egui::vec2(0., -12.),
                    center + egui::vec2(12., 0.),
                    center + egui::vec2(0., 12.),
                    center + egui::vec2(-12., 0.),
                ];
                for stroke in [
                    egui::Stroke::new(5., egui::Color32::from_rgb(10, 20, 35)),
                    egui::Stroke::new(2., color),
                ] {
                    painter.add(egui::Shape::closed_line(diamond.clone(), stroke));
                }
                ui.interact(
                    egui::Rect::from_center_size(center, egui::vec2(28., 28.)).intersect(rect),
                    ui.id().with(("exposed-ground-spawn", index)),
                    egui::Sense::hover(),
                )
                .on_hover_text(format!("{} location last seen {}s ago. Removed after two complete scans miss it; availability is unknown.",
                    if ground_spawns[index].kind == crate::native::GroundSpawnKind::Exobyte { "Exo-material" } else { "Ground spawn" },
                    now.saturating_duration_since(ground_spawns[index].last_seen).as_secs()));
            }
            if let Some(point) = marker {
                painter.circle_filled(
                    target.min + egui::vec2(point.x as f32, point.y as f32) * scale,
                    6.,
                    egui::Color32::YELLOW,
                );
            }
        }
    }
    fn details_ui(
        &mut self,
        ui: &mut egui::Ui,
        sample: Option<&LocationSample>,
        gate: Option<MarkerGate>,
    ) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.catalog_path)
                    .hint_text("Local catalog.json")
                    .char_limit(4096),
            );
            if ui
                .add_enabled(!self.busy, egui::Button::new("Load"))
                .clicked()
            {
                self.load();
            }
        });
        ui.collapsing("Import starter maps (optional)", |ui| {
            ui.label("Current area loads installed maps automatically. This optional bulk import creates a four-map starter catalog for browsing. Marker alignment remains experimental; floor validation is incomplete.");
            ui.label("CookedPC directory");
            let mut source = self.cookedpc.clone();
            if ui.add(egui::TextEdit::singleline(&mut source).desired_width(f32::INFINITY).char_limit(4096)).changed() { self.set_auto_import_source(source); }
            if ui.add_enabled(!self.busy, egui::Button::new("Import starter catalog")).clicked() {
                self.cancel_auto(); self.auto_import = false;
                match import_destination() {
                    Some(destination) if !self.cookedpc.is_empty() && !self.cookedpc.contains("://") && !self.cookedpc.chars().any(char::is_control) => {
                        self.generation = self.generation.wrapping_add(1);
                        self.pending = Some(Request::Import(self.generation, PathBuf::from(&self.cookedpc), destination));
                        self.busy = true;
                        self.status = "Importing installed maps in the background...".into();
                    }
                    _ => self.status = "Use a local CookedPC directory and an available XDG data directory.".into(),
                }
            }
        });
        if self.busy {
            ui.spinner();
        }
        ui.horizontal(|ui| {
            if ui
                .selectable_label(!self.browse_other_maps && self.auto_import, "Current area")
                .clicked()
            {
                self.browse_other_maps = false;
                self.auto_import = true;
                self.update_current_context(sample, gate);
            }
            if ui
                .selectable_label(self.browse_other_maps, "Browse other maps")
                .clicked()
            {
                self.browse();
            }
            if ui
                .add_enabled(
                    self.auto_import && !self.busy && self.auto_context.is_some(),
                    egui::Button::new("Retry current map"),
                )
                .clicked()
            {
                self.retry_current();
                self.update_current_context(sample, gate);
            }
        });
        let previous = self.selected;
        if let Some(catalog) = &self.catalog {
            ui.add_enabled_ui(self.browse_other_maps && !self.busy, |ui| {
                egui::ComboBox::from_id_salt("map-floor")
                    .selected_text(
                        catalog
                            .entries
                            .get(self.selected)
                            .map(|e| e.label.as_str())
                            .unwrap_or("Select map / floor"),
                    )
                    .show_ui(ui, |ui| {
                        for (index, entry) in catalog.entries.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.selected,
                                index,
                                format!("{} / {}", entry.label, entry.context.floor),
                            );
                        }
                    });
            });
        }
        if self.browse_other_maps {
            let mut chosen = None;
            egui::ComboBox::from_id_salt("cached-area-maps")
                .selected_text("Previously imported areas")
                .show_ui(ui, |ui| {
                    for memo in &self.auto_memo {
                        if let Some(path) = &memo.path
                            && ui.selectable_label(false, &memo.area).clicked()
                        {
                            chosen = Some(path.clone());
                        }
                    }
                });
            if let Some(path) = chosen {
                self.catalog_path = path.to_string_lossy().into_owned();
                self.load();
            }
        }
        if previous != self.selected {
            self.asset();
        }
        let now = Instant::now();
        let entry = self
            .catalog
            .as_ref()
            .and_then(|c| c.entries.get(self.selected));
        let marker = entry
            .filter(|_| self.texture.is_some())
            .and_then(|entry| projected_marker(entry, sample, gate, now));
        let marker_status = if marker.is_some() {
            if entry.zip(gate).is_some_and(|(entry, g)| {
                entry.effective_confidence(g.confidence, g.profile)
                    == maps::LocationConfidence::Experimental
            }) {
                "Experimental live position - calibration under validation"
            } else {
                "Validated live location"
            }
        } else {
            "Browsing - live location unavailable"
        };
        ui.add(egui::Label::new(marker_status).truncate())
            .on_hover_text(marker_status);
        ui.add(egui::Label::new(&self.status).truncate())
            .on_hover_text(&self.status);
        ui.add(
            egui::Label::new(
                egui::RichText::new("Diamonds: Cached pickups / Orange: Exo-material")
                    .small()
                    .color(egui::Color32::from_rgb(180, 185, 195)),
            )
            .truncate(),
        )
        .on_hover_text("Last-seen locations persist during scanning and are removed after two complete scans miss them. Collection availability is unknown.");
        if let Some(entry) = entry {
            ui.small(format!(
                "Source: {} | Credit: {}",
                entry.provenance, entry.credit
            ));
        }
        ui.collapsing("Calibration experiment", |ui| {
            ui.label("Enter three independent world X/Y and image X/Y landmark pairs. A fit does not prove live coordinates or change this map.");
            egui::Grid::new("calibration-pairs").show(ui, |ui| { for label in ["World X", "World Y", "Image X", "Image Y"] { ui.label(label); } ui.end_row(); for pair in &mut self.pairs { for value in pair { ui.add(egui::DragValue::new(value)); } ui.end_row(); } });
            if ui.button("Fit affine transform").clicked() {
                let world: Vec<_> = self.pairs.iter().map(|p| maps::Point{x:p[0], y:p[1]}).collect();
                let pixels: Vec<_> = self.pairs.iter().map(|p| maps::Point{x:p[2], y:p[3]}).collect();
                self.calibration = match maps::fit_affine(&world, &pixels) { Ok(matrix) => format!("Matrix: {matrix:?}. Validate against additional independent landmarks, bounds, floor context and measured error before catalog use."), Err(error) => error };
            }
            ui.label(&self.calibration);
        });
    }
}
impl Drop for MapView {
    fn drop(&mut self) {
        self.cancel_auto();
        self.cancellation.cancel();
        self.pending.take();
        self.tx.take();
        self.rx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn map_canvas_size(width: f32, bottom: f32, top: f32) -> egui::Vec2 {
    egui::vec2(width.max(1.0), (bottom - top).max(1.0))
}

fn projected_marker(
    entry: &MapEntry,
    sample: Option<&LocationSample>,
    gate: Option<MarkerGate>,
    now: Instant,
) -> Option<maps::Point> {
    maps::marker_position(entry, sample?, gate?, now)
}

fn projected_ground_spawns(
    entry: &MapEntry,
    samples: &[crate::spawn_cache::CachedSpawn],
    gate: Option<MarkerGate>,
) -> Vec<(usize, maps::Point)> {
    let Some(gate) = gate else {
        return Vec::new();
    };
    samples
        .iter()
        .enumerate()
        .filter_map(|(index, sample)| {
            maps::recorded_marker_position(
                entry,
                sample.point,
                &sample.context,
                gate.confidence,
                gate.profile,
            )
            .map(|point| (index, point))
        })
        .collect()
}

fn ground_spawn_color(kind: crate::native::GroundSpawnKind) -> egui::Color32 {
    use crate::native::GroundSpawnKind;
    match kind {
        GroundSpawnKind::Blue => egui::Color32::from_rgb(80, 170, 255),
        GroundSpawnKind::Gold => egui::Color32::from_rgb(255, 195, 60),
        GroundSpawnKind::Green => egui::Color32::from_rgb(70, 220, 120),
        GroundSpawnKind::Grey => egui::Color32::from_rgb(180, 185, 195),
        GroundSpawnKind::Purple => egui::Color32::from_rgb(200, 110, 255),
        GroundSpawnKind::Exobyte => egui::Color32::from_rgb(255, 130, 45),
        GroundSpawnKind::Other => egui::Color32::from_rgb(235, 235, 235),
    }
}

fn fresh_auto_context(
    sample: Option<&LocationSample>,
    gate: Option<MarkerGate>,
    now: Instant,
) -> Option<AutoContext> {
    let sample = sample?;
    let gate = gate?;
    if !sample.valid
        || gate.paused
        || gate.confidence == maps::LocationConfidence::Unavailable
        || sample.session_generation != gate.session_generation
        || sample.zone_generation != gate.zone_generation
        || now.checked_duration_since(sample.observed_at)? > std::time::Duration::from_secs(1)
    {
        return None;
    }
    Some(AutoContext {
        context: sample.context.clone(),
        session: sample.session_generation,
        zone: sample.zone_generation,
    })
}

fn map_location_status(
    browsing: bool,
    has_texture: bool,
    current_map_matches: bool,
    has_marker: bool,
) -> &'static str {
    if browsing {
        "Manual browsing"
    } else if has_texture && !current_map_matches {
        "Last-known map - current area unavailable"
    } else if has_marker {
        "Live location"
    } else {
        "Location unavailable"
    }
}

fn current_map_index(
    catalog: &Catalog,
    sample: Option<&LocationSample>,
    gate: Option<MarkerGate>,
    now: Instant,
) -> Option<usize> {
    let sample = sample?;
    let gate = gate?;
    if !sample.valid
        || gate.paused
        || gate.confidence == maps::LocationConfidence::Unavailable
        || sample.session_generation != gate.session_generation
        || sample.zone_generation != gate.zone_generation
        || now.checked_duration_since(sample.observed_at)? > std::time::Duration::from_secs(1)
    {
        return None;
    }
    let matching = catalog.matching(&sample.context)?;
    catalog
        .entries
        .iter()
        .position(|entry| std::ptr::eq(entry, matching))
}

fn import_destination() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(".local/share"))
        })
        .map(|p| p.join("dcuo-companion/maps"))
}

fn decode(dir: &std::path::Path, entry: &MapEntry) -> Result<egui::ColorImage, String> {
    let bytes = maps::load_asset(entry, dir)?;
    decode_image(bytes, entry.width, entry.height)
}

fn decode_image(bytes: Vec<u8>, width: u32, height: u32) -> Result<egui::ColorImage, String> {
    if u64::from(width) * u64::from(height) * 4 > 128 * 1024 * 1024 {
        return Err("Decoded map exceeds the 128 MiB texture limit.".into());
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "Map image format is invalid.")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| "Map image cannot be decoded within resource limits.")?;
    if image.width() != width || image.height() != height {
        return Err("Map image dimensions do not match the catalog.".into());
    }
    let rgba = image.to_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [width as usize, height as usize],
        rgba.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_entry(zone: &str) -> MapEntry {
        MapEntry {
            id: zone.into(),
            label: zone.into(),
            category: maps::Category::OpenWorld,
            context: maps::Context {
                zone: zone.into(),
                layout: "layout".into(),
                floor: "floor".into(),
            },
            width: 10,
            height: 10,
            asset: "test.png".into(),
            sha256: "0".repeat(64),
            provenance: "Synthetic".into(),
            credit: "Synthetic".into(),
            affine: [1., 0., 0., 0., 1., 0.],
            world_bounds: [0., 0., 10., 10.],
            calibration: None,
        }
    }
    #[test]
    fn canvas_fills_remaining_viewport_without_footer_reservation() {
        assert_eq!(
            map_canvas_size(800.0, 720.0, 80.0),
            egui::vec2(800.0, 640.0)
        );
        assert_eq!(map_canvas_size(200.0, 100.0, 96.0), egui::vec2(200.0, 4.0));
        assert_eq!(map_canvas_size(0.0, 100.0, 120.0), egui::vec2(1.0, 1.0));
    }

    #[test]
    fn current_selection_requires_exact_fresh_unambiguous_session_context() {
        let now = Instant::now();
        let mut catalog = Catalog {
            schema_version: 1,
            entries: vec![test_entry("one"), test_entry("two")],
        };
        let mut sample = LocationSample {
            point: maps::Point { x: 1., y: 1. },
            context: catalog.entries[1].context.clone(),
            session_generation: 5,
            zone_generation: 9,
            observed_at: now,
            valid: true,
        };
        let gate = MarkerGate {
            session_generation: 5,
            zone_generation: 9,
            paused: false,
            confidence: maps::LocationConfidence::Experimental,
            profile: Some(crate::native::PROFILE_ID),
        };
        assert_eq!(
            current_map_index(&catalog, Some(&sample), Some(gate), now),
            Some(1)
        );
        for bad_gate in [
            MarkerGate {
                paused: true,
                ..gate
            },
            MarkerGate {
                session_generation: 6,
                ..gate
            },
            MarkerGate {
                zone_generation: 10,
                ..gate
            },
            MarkerGate {
                confidence: maps::LocationConfidence::Unavailable,
                ..gate
            },
        ] {
            assert_eq!(
                current_map_index(&catalog, Some(&sample), Some(bad_gate), now),
                None
            );
        }
        assert_eq!(
            current_map_index(
                &catalog,
                Some(&sample),
                Some(gate),
                now + std::time::Duration::from_secs(2)
            ),
            None
        );
        assert_eq!(current_map_index(&catalog, None, Some(gate), now), None);
        sample.context.floor = "unknown".into();
        assert_eq!(
            current_map_index(&catalog, Some(&sample), Some(gate), now),
            None
        );
        sample.context = catalog.entries[1].context.clone();
        catalog.entries.push(test_entry("two"));
        assert_eq!(
            current_map_index(&catalog, Some(&sample), Some(gate), now),
            None
        );
    }
    fn auto_sample(area: &str) -> (LocationSample, MarkerGate<'static>) {
        (
            LocationSample {
                point: maps::Point { x: 1., y: 1. },
                context: test_entry(area).context,
                session_generation: 1,
                zone_generation: 1,
                observed_at: Instant::now(),
                valid: true,
            },
            MarkerGate {
                session_generation: 1,
                zone_generation: 1,
                paused: false,
                confidence: maps::LocationConfidence::Experimental,
                profile: Some(crate::native::PROFILE_ID),
            },
        )
    }
    #[test]
    fn map_loads_preserve_zoom_and_enable_follow_only_for_current_results() {
        let mut view = MapView::default();
        assert!(view.follow);
        view.tx.take();
        let (sender, replies) = mpsc::sync_channel(2);
        view.rx = Some(replies);
        let ctx = egui::Context::default();
        for automatic in [true, false] {
            view.auto_import = automatic;
            view.catalog = Some(Catalog {
                schema_version: 1,
                entries: vec![test_entry("AREA1"), test_entry("AREA2")],
            });
            view.selected = usize::from(!automatic);
            view.zoom = 3.25;
            view.follow = false;
            view.asset();
            assert_eq!(view.zoom, 3.25);
            let image = || egui::ColorImage::filled([2, 2], egui::Color32::BLACK);
            sender
                .send((view.generation.wrapping_sub(1), Ok(Loaded::Image(image()))))
                .unwrap();
            view.poll(&ctx);
            assert!(!view.follow, "obsolete images must not change follow");
            // Zoom adjusted while loading is also retained.
            view.zoom = 4.0;
            sender
                .send((view.generation, Ok(Loaded::Image(image()))))
                .unwrap();
            view.poll(&ctx);
            assert_eq!(view.zoom, 4.0);
            assert!(view.follow);
            assert!(view.texture.is_some());
            view.follow = false;
            view.poll(&ctx);
            assert!(!view.follow, "ordinary polling must respect manual panning");
        }
    }

    #[test]
    fn automatic_area_changes_cancel_old_work_and_discard_late_completion() {
        let mut view = MapView::default();
        view.set_auto_import_source("/synthetic/absent-install".into());
        let (a, gate) = auto_sample("AREA1");
        let (b, _) = auto_sample("AREA2");
        view.update_current_context(Some(&a), Some(gate));
        let old_generation = view.generation;
        let old_token = view.auto_token.clone().unwrap();
        assert!(
            matches!(&view.pending, Some(Request::ImportArea(_,_,_,area,_)) if area == "AREA1")
        );
        view.update_current_context(Some(&b), Some(gate));
        assert!(old_token.is_cancelled());
        assert!(view.generation != old_generation);
        assert!(
            matches!(&view.pending, Some(Request::ImportArea(_,_,_,area,_)) if area == "AREA2")
        );
        let (sender, replies) = mpsc::sync_channel(1);
        view.rx = Some(replies);
        view.tx.take();
        sender
            .send((
                old_generation,
                Ok(Loaded::ImportedArea(
                    "AREA1".into(),
                    PathBuf::from("/private/late/catalog.json"),
                )),
            ))
            .unwrap();
        view.poll(&egui::Context::default());
        assert!(view.catalog_path.is_empty());
        assert!(view.auto_memo.is_empty());
        let token = view.auto_token.clone().unwrap();
        view.update_current_context(None, Some(gate));
        assert!(token.is_cancelled());
        assert!(view.auto_context.is_none());
        assert!(view.pending.is_none());
        assert!(!view.busy);
    }
    #[test]
    fn automatic_failure_is_memoized_until_explicit_retry_and_browsing_does_not_override() {
        let mut view = MapView::default();
        view.set_auto_import_source("/synthetic/absent-install".into());
        let (a, gate) = auto_sample("AREA1");
        view.update_current_context(Some(&a), Some(gate));
        let (sender, replies) = mpsc::sync_channel(1);
        view.rx = Some(replies);
        view.tx.take();
        sender
            .send((view.generation, Err("Synthetic import unavailable".into())))
            .unwrap();
        view.pending = None;
        view.poll(&egui::Context::default());
        let generation = view.generation;
        for _ in 0..5 {
            view.update_current_context(Some(&a), Some(gate));
        }
        assert_eq!(view.generation, generation);
        assert!(view.pending.is_none());
        assert!(!view.busy);
        view.retry_current();
        view.update_current_context(Some(&a), Some(gate));
        assert!(view.busy);
        assert!(view.pending.is_some());
        view.cancel_auto();
        view.auto_import = false;
        view.browse_other_maps = true;
        view.update_current_context(Some(&auto_sample("AREA2").0), Some(gate));
        assert!(view.pending.is_none());
        assert!(!view.busy);
        for index in 0..20 {
            view.remember_auto(format!("AREA{index}"), None);
        }
        assert_eq!(view.auto_memo.len(), 8);
    }
    #[test]
    fn fresh_current_catalog_reloads_texture_after_outage_without_reimport() {
        let mut view = MapView::default();
        view.catalog_path = "/synthetic/checked/catalog.json".into();
        view.remember_auto("AREA1".into(), Some(PathBuf::from(&view.catalog_path)));
        let (a, gate) = auto_sample("AREA1");
        view.catalog = Some(Catalog {
            schema_version: 1,
            entries: vec![test_entry("AREA1")],
        });
        view.update_current_context(Some(&a), Some(gate));
        assert!(matches!(view.pending, Some(Request::Asset(..))));
        view.update_current_context(None, Some(gate));
        assert!(view.pending.is_none());
        view.update_current_context(Some(&a), Some(gate));
        assert!(matches!(view.pending, Some(Request::Asset(..))));
        let generation = view.generation;
        let mut stale = a;
        stale.observed_at = Instant::now() - std::time::Duration::from_secs(2);
        view.update_current_context(Some(&stale), Some(gate));
        assert!(view.pending.is_none());
        assert!(view.generation != generation);
    }
    #[test]
    fn empty_source_disables_imports_without_failure_memo_or_retries() {
        let mut view = MapView::default();
        let (a, gate) = auto_sample("AREA1");
        view.set_auto_import_source(String::new());
        for _ in 0..3 {
            view.update_current_context(Some(&a), Some(gate));
        }
        assert!(!view.busy);
        assert!(view.pending.is_none());
        assert!(view.auto_memo.is_empty());
    }
    #[test]
    fn changing_source_invalidates_same_area_assets_and_preserves_explicit_browsing() {
        let mut view = MapView::default();
        view.set_auto_import_source("/synthetic/source-a".into());
        let (a, gate) = auto_sample("AREA1");
        view.update_current_context(Some(&a), Some(gate));
        let old_token = view.auto_token.clone().unwrap();
        view.catalog = Some(Catalog {
            schema_version: 1,
            entries: vec![test_entry("AREA1")],
        });
        view.texture = Some(egui::Context::default().load_texture(
            "synthetic",
            egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]),
            egui::TextureOptions::LINEAR,
        ));
        view.remember_auto(
            "AREA1".into(),
            Some(PathBuf::from("/synthetic/old-catalog")),
        );
        view.set_auto_import_source("/synthetic/source-b".into());
        assert!(old_token.is_cancelled());
        assert!(view.catalog.is_none());
        assert!(view.texture.is_none());
        assert!(view.auto_memo.is_empty());
        view.update_current_context(Some(&a), Some(gate));
        assert!(
            matches!(&view.pending, Some(Request::ImportArea(_, source, _, area, _)) if source == &PathBuf::from("/synthetic/source-b") && area == "AREA1")
        );
        view.cancel_auto();
        view.browse_other_maps = true;
        view.auto_import = false;
        view.set_auto_import_source("/synthetic/source-c".into());
        view.update_current_context(Some(&a), Some(gate));
        assert!(view.browse_other_maps);
        assert!(!view.auto_import);
        assert!(view.pending.is_none());
    }
    #[test]
    fn source_change_after_context_loss_reimports_instead_of_reusing_old_catalog() {
        let mut view = MapView::default();
        view.set_auto_import_source("/synthetic/source-a".into());
        let (sample, gate) = auto_sample("AREA1");
        view.update_current_context(Some(&sample), Some(gate));
        view.cancel_auto();
        view.catalog = Some(Catalog {
            schema_version: 1,
            entries: vec![test_entry("AREA1")],
        });
        view.update_current_context(None, Some(gate));
        assert!(view.auto_context.is_none());
        assert!(view.catalog.is_some());
        view.set_auto_import_source("/synthetic/source-b".into());
        assert!(view.catalog.is_none());
        view.update_current_context(Some(&sample), Some(gate));
        assert!(matches!(&view.pending,
            Some(Request::ImportArea(_, source, _, area, _))
            if source == &PathBuf::from("/synthetic/source-b") && area == "AREA1"));
    }
    #[test]
    fn browse_recovers_selected_asset_after_outage_and_inflight_cancellation() {
        for inflight in [false, true] {
            let mut view = MapView::default();
            view.set_auto_import_source("/synthetic/source".into());
            let (sample, gate) = auto_sample("AREA1");
            view.update_current_context(Some(&sample), Some(gate));
            view.cancel_auto();
            view.catalog = Some(Catalog {
                schema_version: 1,
                entries: vec![test_entry("AREA1")],
            });
            view.update_current_context(None, Some(gate));
            if inflight {
                view.auto_inflight = Some("AREA1".into());
                view.asset();
            }
            let old_generation = view.generation;
            view.browse();
            assert!(view.browse_other_maps);
            assert!(!view.auto_import);
            assert!(view.busy);
            assert!(view.generation > old_generation);
            assert!(
                matches!(&view.pending, Some(Request::Asset(_, _, entry)) if entry.context.zone == "AREA1")
            );
        }
    }
    #[test]
    fn saved_automatic_catalog_must_pass_source_check_once_per_launch() {
        let mut view = MapView::default();
        view.set_auto_import_source("/synthetic/source".into());
        view.catalog_path = "/synthetic/saved/catalog.json".into();
        view.catalog = Some(Catalog {
            schema_version: 1,
            entries: vec![test_entry("AREA1")],
        });
        let (sample, gate) = auto_sample("AREA1");
        view.update_current_context(Some(&sample), Some(gate));
        assert!(
            matches!(&view.pending, Some(Request::ImportArea(_, _, _, area, _)) if area == "AREA1")
        );
        view.cancel_auto();
        view.remember_auto("AREA1".into(), Some(PathBuf::from(&view.catalog_path)));
        view.update_current_context(Some(&sample), Some(gate));
        assert!(
            matches!(&view.pending, Some(Request::Asset(_, _, entry)) if entry.context.zone == "AREA1")
        );
    }
    #[test]
    fn transient_location_loss_keeps_texture_and_view_but_hides_marker() {
        let mut view = MapView::default();
        view.set_auto_import_source("/synthetic/source".into());
        view.catalog_path = "/synthetic/checked/catalog.json".into();
        view.remember_auto("AREA1".into(), Some(PathBuf::from(&view.catalog_path)));
        view.catalog = Some(Catalog {
            schema_version: 1,
            entries: vec![test_entry("AREA1")],
        });
        let ctx = egui::Context::default();
        let texture = ctx.load_texture(
            "stable",
            egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]),
            egui::TextureOptions::LINEAR,
        );
        let id = texture.id();
        view.texture = Some(texture);
        view.zoom = 2.0;
        view.pan = egui::vec2(13., 17.);
        let (mut sample, mut gate) = auto_sample("AREA1");
        for generation in 1..=20 {
            sample.session_generation = generation;
            sample.zone_generation = generation;
            gate.session_generation = generation;
            gate.zone_generation = generation;
            view.update_current_context(Some(&sample), Some(gate));
            assert!(view.pending.is_none());
            view.update_current_context(None, Some(gate));
            assert!(
                current_map_index(
                    view.catalog.as_ref().unwrap(),
                    None,
                    Some(gate),
                    Instant::now()
                )
                .is_none()
            );
            assert_eq!(
                map_location_status(false, view.texture.is_some(), false, false),
                "Last-known map - current area unavailable"
            );
            assert_eq!(view.texture.as_ref().unwrap().id(), id);
            assert_eq!(view.zoom, 2.0);
            assert_eq!(view.pan, egui::vec2(13., 17.));
            assert!(view.pending.is_none());
        }
        sample.context.zone = "AREA2".into();
        view.update_current_context(Some(&sample), Some(gate));
        assert!(view.texture.is_none());
        assert!(matches!(view.pending, Some(Request::ImportArea(..))));
    }
    #[test]
    fn map_status_distinguishes_retained_current_and_browsed_maps() {
        assert_eq!(
            map_location_status(false, true, false, false),
            "Last-known map - current area unavailable"
        );
        assert_eq!(
            map_location_status(false, true, true, false),
            "Location unavailable"
        );
        assert_eq!(
            map_location_status(false, true, true, true),
            "Live location"
        );
        assert_eq!(
            map_location_status(false, false, false, false),
            "Location unavailable"
        );
        assert_eq!(
            map_location_status(true, true, false, false),
            "Manual browsing"
        );
    }
    #[test]
    fn collectible_projection_uses_current_context_and_hides_unusable_samples() {
        let entry = test_entry("AREA1");
        let (mut sample, gate) = auto_sample("AREA1");
        let now = sample.observed_at;
        sample.point = maps::Point { x: 3., y: 4. };
        assert_eq!(
            projected_marker(&entry, Some(&sample), Some(gate), now),
            Some(entry.project(sample.point))
        );
        assert!(projected_marker(&entry, None, Some(gate), now).is_none());
        assert!(projected_marker(&entry, Some(&sample), None, now).is_none());
        for bad_gate in [
            MarkerGate {
                paused: true,
                ..gate
            },
            MarkerGate {
                session_generation: 2,
                ..gate
            },
            MarkerGate {
                zone_generation: 2,
                ..gate
            },
            MarkerGate {
                confidence: maps::LocationConfidence::Unavailable,
                ..gate
            },
        ] {
            assert!(projected_marker(&entry, Some(&sample), Some(bad_gate), now).is_none());
        }
        assert!(
            projected_marker(
                &entry,
                Some(&sample),
                Some(gate),
                now + std::time::Duration::from_secs(2)
            )
            .is_none()
        );
        sample.context.zone = "AREA2".into();
        assert!(projected_marker(&entry, Some(&sample), Some(gate), now).is_none());
        sample.context = entry.context.clone();
        sample.point.x = 11.;
        assert!(projected_marker(&entry, Some(&sample), Some(gate), now).is_none());
        sample.point.x = 3.;
        sample.valid = false;
        assert!(projected_marker(&entry, Some(&sample), Some(gate), now).is_none());
        sample.valid = true;
        let mut incompatible = entry;
        incompatible.calibration = Some(maps::Calibration {
            version: 1,
            profile: "different-profile".into(),
            confidence: maps::LocationConfidence::Experimental,
            landmark_count: 0,
            independent_landmark_count: 0,
            max_error_pixels: None,
            accuracy_budget_pixels: None,
        });
        assert!(projected_marker(&incompatible, Some(&sample), Some(gate), now).is_none());
    }
    #[test]
    fn cached_spawn_projection_retains_old_points_but_checks_map_and_profile() {
        let entry = test_entry("AREA1");
        let (sample, gate) = auto_sample("AREA1");
        let mut samples = vec![
            crate::spawn_cache::CachedSpawn {
                point: maps::Point { x: 2., y: 3. },
                context: sample.context.clone(),
                kind: crate::native::GroundSpawnKind::Blue,
                last_seen: sample.observed_at - std::time::Duration::from_secs(120),
            };
            4
        ];
        samples[1].context.zone = "AREA2".into();
        samples[2].point.x = f64::NAN;
        samples[3].point.x = 11.;
        assert_eq!(
            projected_ground_spawns(&entry, &samples, Some(gate)),
            vec![(0, entry.project(samples[0].point))]
        );
        assert_eq!(
            projected_ground_spawns(
                &entry,
                &samples,
                Some(MarkerGate {
                    paused: true,
                    session_generation: 42,
                    ..gate
                })
            ),
            vec![(0, entry.project(samples[0].point))]
        );
        assert!(projected_ground_spawns(&entry, &samples, None).is_empty());
        assert!(
            projected_ground_spawns(
                &entry,
                &samples,
                Some(MarkerGate {
                    confidence: maps::LocationConfidence::Unavailable,
                    ..gate
                })
            )
            .is_empty()
        );
        let mut incompatible = entry;
        incompatible.calibration = Some(maps::Calibration {
            version: 1,
            profile: "different-profile".into(),
            confidence: maps::LocationConfidence::Experimental,
            landmark_count: 0,
            independent_landmark_count: 0,
            max_error_pixels: None,
            accuracy_budget_pixels: None,
        });
        assert!(projected_ground_spawns(&incompatible, &samples, Some(gate)).is_empty());
    }
    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([20, 30, 40, 255]));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }
    #[test]
    fn decodes_synthetic_image_and_rejects_wrong_dimensions_corruption_and_limits() {
        let image = decode_image(png(4, 3), 4, 3).unwrap();
        assert_eq!(image.size, [4, 3]);
        assert!(decode_image(png(4, 3), 3, 4).is_err());
        assert!(decode_image(vec![0; 32], 4, 3).is_err());
        assert!(decode_image(png(8193, 1), 8193, 1).is_err());
        assert!(
            decode_image(Vec::new(), 8192, 8192)
                .unwrap_err()
                .contains("texture limit")
        );
    }
    #[test]
    fn diagnostics_exclude_catalog_strings_paths_errors_and_coordinates() {
        let mut view = MapView::default();
        let mut entry = test_entry("secret-map-label");
        entry.provenance = "private game evidence".into();
        entry.calibration = Some(maps::Calibration {
            version: 1,
            profile: crate::native::PROFILE_ID.into(),
            confidence: maps::LocationConfidence::Experimental,
            landmark_count: 0,
            independent_landmark_count: 0,
            max_error_pixels: None,
            accuracy_budget_pixels: None,
        });
        view.catalog = Some(Catalog {
            schema_version: 1,
            entries: vec![entry],
        });
        view.catalog_path = "/private/map-path".into();
        view.status = "private error with coordinates 1234,5678".into();
        let diagnostic = view.sanitized_diagnostics(Some(crate::native::PROFILE_ID));
        assert!(diagnostic.contains("effective confidence: Experimental"));
        assert!(diagnostic.contains("profile compatible: Some(true)"));
        for private in [
            "secret-map-label",
            "private",
            "1234",
            "5678",
            crate::native::PROFILE_ID,
        ] {
            assert!(!diagnostic.contains(private));
        }
        let incompatible = view.sanitized_diagnostics(Some("different-profile"));
        assert!(incompatible.contains("effective confidence: Unavailable"));
        assert!(incompatible.contains("profile compatible: Some(false)"));
    }
    #[test]
    fn shutdown_cancels_in_progress_import_before_joining() {
        let (started, start) = mpsc::channel();
        let (close, closing) = mpsc::channel();
        let (done, completion) = mpsc::channel();
        thread::spawn(move || {
            let view = MapView::new_with_importer(String::new(), move |_, _, cancellation| {
                started.send(()).unwrap();
                while !cancellation.is_cancelled() {
                    thread::park_timeout(std::time::Duration::from_millis(5));
                }
                Err("Map import cancelled".into())
            });
            view.tx
                .as_ref()
                .unwrap()
                .send(Request::Import(1, PathBuf::new(), PathBuf::new()))
                .unwrap();
            closing.recv().unwrap();
            drop(view);
            done.send(()).unwrap();
        });
        start
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        close.send(()).unwrap();
        completion
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
    }
    #[test]
    fn shutdown_joins_when_result_channel_is_full() {
        let (done, completion) = mpsc::channel();
        thread::spawn(move || {
            let view = MapView::default();
            for generation in 1..=2 {
                view.tx
                    .as_ref()
                    .unwrap()
                    .send(Request::Catalog(generation, PathBuf::new()))
                    .unwrap();
            }
            drop(view);
            done.send(()).unwrap();
        });
        completion
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
    }
}
