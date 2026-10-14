//! Local map browsing with one bounded, joined asset worker.
use crate::map_import::ImportCancellation;
use crate::maps::{self, Catalog, LocationSample, MapEntry, MarkerGate};
use eframe::egui;
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Instant,
};

enum Request {
    Catalog(u64, PathBuf),
    Import(u64, PathBuf, PathBuf),
    Asset(u64, PathBuf, Box<MapEntry>),
}
enum Loaded {
    Catalog(Catalog, PathBuf),
    Image(egui::ColorImage),
    Imported(PathBuf),
}
type Reply = (u64, Result<Loaded, String>);

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
            follow: false,
            pairs: [[0.; 4]; 3],
            calibration: String::new(),
            cookedpc: std::env::var_os("HOME").map(PathBuf::from).filter(|path| path.is_absolute()).map(|path| path.join(".local/share/Steam/steamapps/common/DC Universe Online/UNREAL3/DCGAME/COOKEDPC").to_string_lossy().into_owned()).unwrap_or_default(),
            busy: false,
            browse_other_maps: false,
        }
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
    pub fn load(&mut self) {
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
    fn asset(&mut self, preserve_view: bool) {
        self.busy = true;
        self.generation = self.generation.wrapping_add(1);
        self.texture = None;
        if !preserve_view {
            self.zoom = 1.0;
            self.follow = false;
        }
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
                    self.catalog = Some(catalog);
                    self.directory = directory;
                    self.selected = 0;
                    self.asset(false);
                    if self.catalog.as_ref().is_some_and(|c| c.entries.is_empty()) {
                        self.status = "Catalog contains no maps.".into();
                        self.busy = false;
                    }
                }
                Ok(Loaded::Image(image)) => {
                    self.texture =
                        Some(ctx.load_texture("local-map", image, egui::TextureOptions::LINEAR));
                    self.status = "Map loaded.".into();
                }
                Ok(Loaded::Imported(path)) => {
                    self.catalog_path = path.to_string_lossy().into_owned();
                    self.load();
                }
                Err(error) => self.status = error,
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
    ) {
        self.poll(ui.ctx());
        ui.heading("Map browser");
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
        ui.collapsing("Import local installed game maps", |ui| {
            ui.label("Import two installed-game area maps for private local use. Supported area alignment can show an experimental marker; calibration, floor and transition validation remain incomplete.");
            ui.label("CookedPC directory");
            ui.add(egui::TextEdit::singleline(&mut self.cookedpc).desired_width(f32::INFINITY).char_limit(4096));
            if ui.add_enabled(!self.busy, egui::Button::new("Import installed game maps")).clicked() {
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
            ui.selectable_value(&mut self.browse_other_maps, false, "Current area");
            ui.selectable_value(&mut self.browse_other_maps, true, "Browse other maps");
        });
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
            self.asset(true);
        }
        if !self.browse_other_maps && current_index.is_none() {
            ui.small("Current area unavailable: waiting for fresh, unambiguous session and area evidence.");
        }
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
        if previous != self.selected {
            self.asset(false);
        }
        let entry = self
            .catalog
            .as_ref()
            .and_then(|c| c.entries.get(self.selected));
        let marker = entry.filter(|_| self.texture.is_some()).and_then(|entry| {
            sample.and_then(|sample| {
                gate.and_then(|gate| maps::marker_position(entry, sample, gate, now))
            })
        });
        ui.label(if marker.is_some() {
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
        });
        ui.label(&self.status);
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
        });
        // ScrollArea has unbounded content height; use its visible clipping bounds.
        // Leave room for source credits and the collapsed calibration footer.
        let visible_height = (ui.clip_rect().bottom() - ui.cursor().top() - 90.0).max(160.0);
        let size = egui::vec2(ui.available_width().max(1.0), visible_height);
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
            if let Some(point) = marker {
                painter.circle_filled(
                    target.min + egui::vec2(point.x as f32, point.y as f32) * scale,
                    6.,
                    egui::Color32::YELLOW,
                );
            }
        }
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
        self.cancellation.cancel();
        self.pending.take();
        self.tx.take();
        self.rx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
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
