//! Local map catalogs and synthetic-testable projection. No live profile is supplied.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::OpenOptions,
    io::Read,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Component, Path},
    time::{Duration, Instant},
};

const CATALOG_LIMIT: usize = 1024 * 1024;
const ASSET_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub zone: String,
    pub layout: String,
    pub floor: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    OpenWorld,
    Instance,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapEntry {
    pub id: String,
    pub label: String,
    pub category: Category,
    pub context: Context,
    pub width: u32,
    pub height: u32,
    pub asset: String,
    pub sha256: String,
    pub provenance: String,
    pub credit: String,
    pub affine: [f64; 6],
    /// Minimum x, minimum y, maximum x, maximum y.
    pub world_bounds: [f64; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration: Option<Calibration>,
}
/// Versioned per-entry evidence. Counts describe fitting landmarks and separate
/// holdout landmarks; maximum error must be measured on the holdout set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Calibration {
    pub version: u32,
    /// Exact executable SHA-256 for the memory observation profile.
    pub profile: String,
    pub confidence: LocationConfidence,
    pub landmark_count: u32,
    pub independent_landmark_count: u32,
    pub max_error_pixels: Option<f64>,
    pub accuracy_budget_pixels: Option<f64>,
}
impl Calibration {
    fn validate(&self, width: u32, height: u32) -> Result<(), String> {
        if self.version != 1
            || self.profile.len() != 64
            || !self.profile.bytes().all(|byte| byte.is_ascii_hexdigit())
            || self.landmark_count > 128
            || self.independent_landmark_count > 128
        {
            return Err("Invalid calibration version, profile, or landmark count".into());
        }
        let image_diagonal = (width as f64).hypot(height as f64);
        if self
            .max_error_pixels
            .is_some_and(|error| !error.is_finite() || error < 0.0 || error > image_diagonal)
            || self.accuracy_budget_pixels.is_some_and(|budget| {
                !budget.is_finite() || budget <= 0.0 || budget >= image_diagonal
            })
        {
            return Err("Invalid calibration accuracy evidence or budget".into());
        }
        if self.max_error_pixels.is_some() && self.independent_landmark_count == 0 {
            return Err("Measured calibration error requires independent landmarks".into());
        }
        if self.confidence == LocationConfidence::Validated {
            let (Some(error), Some(budget)) = (self.max_error_pixels, self.accuracy_budget_pixels)
            else {
                return Err("Validated calibration requires measured independent error and an accuracy budget".into());
            };
            if self.landmark_count < 3 || self.independent_landmark_count == 0 || error > budget {
                return Err(
                    "Validated calibration lacks landmark evidence or exceeds its accuracy budget"
                        .into(),
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub entries: Vec<MapEntry>,
}

fn bounded_read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| format!("Cannot open local map file: {e}"))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Map file must be a regular file".into());
    }
    let mut data = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > limit {
        return Err("Map file exceeds size limit".into());
    }
    Ok(data)
}
fn confined_asset(entry: &MapEntry, dir: &Path) -> Result<std::path::PathBuf, String> {
    let path = Path::new(&entry.asset);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Asset must be a relative path without traversal".into());
    }
    if !matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("png" | "jpg" | "jpeg")
    ) {
        return Err("Asset must be PNG or JPEG".into());
    }
    let root = dir.canonicalize().map_err(|e| e.to_string())?;
    let resolved = root
        .join(path)
        .canonicalize()
        .map_err(|e| format!("Map asset unavailable: {e}"))?;
    if !resolved.starts_with(&root) {
        return Err("Asset escapes catalog directory".into());
    }
    Ok(resolved)
}
fn bounded_text(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)
}
fn valid_affine(a: &[f64; 6]) -> bool {
    let determinant = a[0] * a[4] - a[1] * a[3];
    a.iter().all(|v| v.is_finite()) && determinant.is_finite() && determinant.abs() > 1e-12
}
impl MapEntry {
    pub fn validate(&self) -> Result<(), String> {
        if [
            &self.id,
            &self.label,
            &self.context.zone,
            &self.context.layout,
            &self.context.floor,
            &self.asset,
            &self.provenance,
            &self.credit,
        ]
        .into_iter()
        .any(|s| !bounded_text(s))
        {
            return Err("Map text is empty, too long, or contains control characters".into());
        }
        if self.width == 0 || self.height == 0 || self.width > 8192 || self.height > 8192 {
            return Err("Invalid map dimensions".into());
        }
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid asset SHA-256".into());
        }
        if !valid_affine(&self.affine) {
            return Err("Invalid or degenerate map transform".into());
        }
        let [x0, y0, x1, y1] = self.world_bounds;
        if !self.world_bounds.iter().all(|v| v.is_finite()) || x0 >= x1 || y0 >= y1 {
            return Err("Invalid world bounds".into());
        }
        if let Some(calibration) = &self.calibration {
            calibration.validate(self.width, self.height)?;
        }
        Ok(())
    }
    /// Observation confidence is capped by this entry's calibration evidence.
    /// Legacy schema-1 entries remain usable only as experimental maps.
    pub fn effective_confidence(
        &self,
        requested: LocationConfidence,
        profile: Option<&str>,
    ) -> LocationConfidence {
        if requested == LocationConfidence::Unavailable || self.validate().is_err() {
            return LocationConfidence::Unavailable;
        }
        let evidence = match &self.calibration {
            None => LocationConfidence::Experimental,
            Some(calibration) if profile == Some(calibration.profile.as_str()) => {
                calibration.confidence
            }
            Some(_) => return LocationConfidence::Unavailable,
        };
        if evidence == LocationConfidence::Unavailable {
            LocationConfidence::Unavailable
        } else if evidence == LocationConfidence::Validated
            && requested == LocationConfidence::Validated
        {
            LocationConfidence::Validated
        } else {
            LocationConfidence::Experimental
        }
    }
    pub fn project(&self, p: Point) -> Point {
        project(self.affine, p)
    }
}
impl Catalog {
    pub fn matching(&self, context: &Context) -> Option<&MapEntry> {
        let mut matches = self.entries.iter().filter(|e| &e.context == context);
        let first = matches.next()?;
        if matches.next().is_some() {
            None
        } else {
            Some(first)
        }
    }
}
pub(crate) fn catalog_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}
pub fn load_catalog(path: &Path) -> Result<Catalog, String> {
    let data = bounded_read(path, CATALOG_LIMIT)?;
    let catalog: Catalog =
        serde_json::from_slice(&data).map_err(|e| format!("Invalid map catalog: {e}"))?;
    if catalog.schema_version != 1 || catalog.entries.len() > 128 {
        return Err("Unsupported catalog version or map count".into());
    }
    let mut contexts = HashSet::new();
    let mut ids = HashSet::new();
    for entry in &catalog.entries {
        entry.validate()?;
        if !contexts.insert(&entry.context) || !ids.insert(&entry.id) {
            return Err("Duplicate map identity or ambiguous context".into());
        }
        confined_asset(entry, catalog_directory(path))?;
    }
    Ok(catalog)
}
pub fn load_asset(entry: &MapEntry, catalog_dir: &Path) -> Result<Vec<u8>, String> {
    entry.validate()?;
    let path = confined_asset(entry, catalog_dir)?;
    let root = catalog_dir.canonicalize().map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    // Validate the opened object, not just a path that may have changed before open.
    let opened = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
        .map_err(|e| e.to_string())?;
    if !opened.starts_with(&root) || !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Opened asset escapes catalog directory or is not a file".into());
    }
    let mut bytes = Vec::new();
    file.take((ASSET_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > ASSET_LIMIT {
        return Err("Map asset exceeds size limit".into());
    }
    if format!("{:x}", Sha256::digest(&bytes)) != entry.sha256.to_ascii_lowercase() {
        return Err("Map asset checksum mismatch".into());
    }
    Ok(bytes)
}
pub fn project(a: [f64; 6], p: Point) -> Point {
    Point {
        x: a[0] * p.x + a[1] * p.y + a[2],
        y: a[3] * p.x + a[4] * p.y + a[5],
    }
}
/// Least-squares affine fit. Independent landmarks must validate the fitted result.
pub fn fit_affine(world: &[Point], pixels: &[Point]) -> Result<[f64; 6], String> {
    if world.len() != pixels.len() || world.len() < 3 || world.len() > 128 {
        return Err("Fit needs 3 to 128 paired landmarks".into());
    }
    if world
        .iter()
        .chain(pixels)
        .any(|p| !p.x.is_finite() || !p.y.is_finite())
    {
        return Err("Non-finite landmark".into());
    }
    // Center and scale the input to avoid an ill-conditioned translation column.
    let n = world.len() as f64;
    let cx = world.iter().map(|p| p.x / n).sum::<f64>();
    let cy = world.iter().map(|p| p.y / n).sum::<f64>();
    let scale = world
        .iter()
        .map(|p| (p.x - cx).abs().max((p.y - cy).abs()))
        .fold(0.0, f64::max);
    if !scale.is_finite() || scale == 0.0 {
        return Err("Degenerate landmarks".into());
    }
    let mut m = [[0.0; 5]; 3];
    for (w, p) in world.iter().zip(pixels) {
        let v = [(w.x - cx) / scale, (w.y - cy) / scale, 1.0];
        for r in 0..3 {
            for c in 0..3 {
                m[r][c] += v[r] * v[c];
            }
            m[r][3] += v[r] * p.x;
            m[r][4] += v[r] * p.y;
        }
    }
    for c in 0..3 {
        let pivot = (c..3)
            .max_by(|&a, &b| m[a][c].abs().total_cmp(&m[b][c].abs()))
            .unwrap();
        m.swap(c, pivot);
        if !m[c][c].is_finite() || m[c][c].abs() < 1e-10 {
            return Err("Collinear landmarks".into());
        }
        let d = m[c][c];
        for value in &mut m[c][c..] {
            *value /= d;
        }
        let pivot_row = m[c];
        for (r, row) in m.iter_mut().enumerate() {
            if r != c {
                let factor = row[c];
                for (value, pivot_value) in row[c..].iter_mut().zip(&pivot_row[c..]) {
                    *value -= factor * pivot_value;
                }
            }
        }
    }
    let a = [
        m[0][3] / scale,
        m[1][3] / scale,
        m[2][3] - m[0][3] * cx / scale - m[1][3] * cy / scale,
        m[0][4] / scale,
        m[1][4] / scale,
        m[2][4] - m[0][4] * cx / scale - m[1][4] * cy / scale,
    ];
    if !valid_affine(&a) {
        return Err("Fit produced a degenerate transform".into());
    }
    Ok(a)
}
#[derive(Debug, Clone)]
pub struct LocationSample {
    pub point: Point,
    pub context: Context,
    pub session_generation: u64,
    pub zone_generation: u64,
    pub observed_at: Instant,
    pub valid: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationConfidence {
    Unavailable,
    Experimental,
    Validated,
}

#[derive(Debug, Clone, Copy)]
pub struct MarkerGate<'a> {
    pub session_generation: u64,
    pub zone_generation: u64,
    pub paused: bool,
    pub confidence: LocationConfidence,
    pub profile: Option<&'a str>,
}
pub fn marker_position(
    entry: &MapEntry,
    sample: &LocationSample,
    gate: MarkerGate,
    now: Instant,
) -> Option<Point> {
    if entry.validate().is_err()
        || entry.effective_confidence(gate.confidence, gate.profile)
            == LocationConfidence::Unavailable
        || gate.paused
        || !sample.valid
        || sample.context != entry.context
        || sample.session_generation != gate.session_generation
        || sample.zone_generation != gate.zone_generation
        || now.checked_duration_since(sample.observed_at)? > Duration::from_secs(1)
    {
        return None;
    }
    let p = sample.point;
    let [x0, y0, x1, y1] = entry.world_bounds;
    if !p.x.is_finite() || !p.y.is_finite() || p.x < x0 || p.x > x1 || p.y < y0 || p.y > y1 {
        return None;
    }
    let pixel = entry.project(p);
    if !pixel.x.is_finite()
        || !pixel.y.is_finite()
        || pixel.x < 0.0
        || pixel.y < 0.0
        || pixel.x > entry.width as f64
        || pixel.y > entry.height as f64
    {
        return None;
    }
    Some(pixel)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry() -> MapEntry {
        MapEntry {
            id: "synthetic".into(),
            label: "Synthetic only".into(),
            category: Category::Instance,
            context: Context {
                zone: "test".into(),
                layout: "one".into(),
                floor: "ground".into(),
            },
            width: 100,
            height: 100,
            asset: "test.png".into(),
            sha256: format!("{:x}", Sha256::digest(b"synthetic")),
            provenance: "Synthetic test".into(),
            credit: "Test".into(),
            affine: [1., 0., 0., 0., 1., 0.],
            world_bounds: [0., 0., 100., 100.],
            calibration: None,
        }
    }
    fn calibration(confidence: LocationConfidence) -> Calibration {
        Calibration {
            version: 1,
            profile: "a".repeat(64),
            confidence,
            landmark_count: 3,
            independent_landmark_count: 2,
            max_error_pixels: Some(2.0),
            accuracy_budget_pixels: Some(5.0),
        }
    }
    #[test]
    fn legacy_entry_deserializes_and_cannot_claim_validated_confidence() {
        let json = serde_json::to_value(entry()).unwrap();
        assert!(json.get("calibration").is_none());
        let legacy: MapEntry = serde_json::from_value(json).unwrap();
        assert!(legacy.calibration.is_none());
        assert_eq!(
            legacy.effective_confidence(LocationConfidence::Validated, Some(&"a".repeat(64))),
            LocationConfidence::Experimental
        );
        assert_eq!(
            legacy.effective_confidence(LocationConfidence::Unavailable, None),
            LocationConfidence::Unavailable
        );
    }
    #[test]
    fn confidence_is_capped_by_both_entry_evidence_and_observation() {
        let profile = "a".repeat(64);
        let mut map = entry();
        for evidence in [
            LocationConfidence::Unavailable,
            LocationConfidence::Experimental,
            LocationConfidence::Validated,
        ] {
            map.calibration = Some(calibration(evidence));
            for observation in [
                LocationConfidence::Unavailable,
                LocationConfidence::Experimental,
                LocationConfidence::Validated,
            ] {
                let expected = if evidence == LocationConfidence::Unavailable
                    || observation == LocationConfidence::Unavailable
                {
                    LocationConfidence::Unavailable
                } else if evidence == LocationConfidence::Validated
                    && observation == LocationConfidence::Validated
                {
                    LocationConfidence::Validated
                } else {
                    LocationConfidence::Experimental
                };
                assert_eq!(
                    map.effective_confidence(observation, Some(&profile)),
                    expected
                );
            }
        }
        map.calibration = Some(Calibration {
            version: 1,
            profile: profile.clone(),
            confidence: LocationConfidence::Experimental,
            landmark_count: 0,
            independent_landmark_count: 0,
            max_error_pixels: None,
            accuracy_budget_pixels: None,
        });
        assert!(map.validate().is_ok());
        assert_eq!(
            map.effective_confidence(LocationConfidence::Validated, Some(&profile)),
            LocationConfidence::Experimental
        );
    }
    #[test]
    fn exact_profile_mismatch_or_missing_profile_hides_marker() {
        let now = Instant::now();
        let mut map = entry();
        map.calibration = Some(calibration(LocationConfidence::Validated));
        let sample = LocationSample {
            point: Point { x: 20.0, y: 30.0 },
            context: map.context.clone(),
            session_generation: 1,
            zone_generation: 2,
            observed_at: now,
            valid: true,
        };
        let profile = "a".repeat(64);
        let other_profile = "b".repeat(64);
        let gate = MarkerGate {
            session_generation: 1,
            zone_generation: 2,
            paused: false,
            confidence: LocationConfidence::Validated,
            profile: Some(&profile),
        };
        assert!(marker_position(&map, &sample, gate, now).is_some());
        for profile in [
            None,
            Some(other_profile.as_str()),
            Some(profile[..63].as_ref()),
        ] {
            assert_eq!(
                map.effective_confidence(LocationConfidence::Validated, profile),
                LocationConfidence::Unavailable
            );
            assert!(marker_position(&map, &sample, MarkerGate { profile, ..gate }, now).is_none());
        }
    }
    #[test]
    fn validated_calibration_requires_bounded_independent_accuracy_evidence() {
        let good = calibration(LocationConfidence::Validated);
        assert!(good.validate(100, 100).is_ok());
        for case in 0..13 {
            let mut bad = good.clone();
            match case {
                0 => bad.version = 2,
                1 => bad.profile = "not-a-sha".into(),
                2 => bad.landmark_count = 2,
                3 => bad.independent_landmark_count = 0,
                4 => bad.max_error_pixels = None,
                5 => bad.accuracy_budget_pixels = None,
                6 => bad.max_error_pixels = Some(f64::NAN),
                7 => bad.max_error_pixels = Some(f64::INFINITY),
                8 => bad.max_error_pixels = Some(-1.0),
                9 => bad.accuracy_budget_pixels = Some(0.0),
                10 => bad.max_error_pixels = Some(6.0),
                11 => bad.landmark_count = 129,
                _ => bad.independent_landmark_count = 129,
            }
            assert!(
                bad.validate(100, 100).is_err(),
                "invalid evidence case {case}"
            );
            let mut map = entry();
            map.calibration = Some(bad);
            assert_eq!(
                map.effective_confidence(LocationConfidence::Validated, Some(&good.profile)),
                LocationConfidence::Unavailable
            );
        }
        for budget in [-1.0, f64::NAN, f64::INFINITY, 1_000_000.0] {
            let mut bad = good.clone();
            bad.accuracy_budget_pixels = Some(budget);
            assert!(bad.validate(100, 100).is_err());
        }
    }
    #[test]
    fn catalog_parent_handles_bare_filenames_and_nested_paths() {
        assert_eq!(catalog_directory(Path::new("catalog.json")), Path::new("."));
        assert_eq!(
            catalog_directory(Path::new("./catalog.json")),
            Path::new(".")
        );
        assert_eq!(
            catalog_directory(Path::new("maps/catalog.json")),
            Path::new("maps")
        );
        assert_eq!(
            catalog_directory(Path::new("/maps/catalog.json")),
            Path::new("/maps")
        );
    }
    #[test]
    fn fits_inverted_rotated_landmarks() {
        let w = [
            Point { x: 0., y: 0. },
            Point { x: 10., y: 0. },
            Point { x: 0., y: 10. },
            Point { x: 10., y: 10. },
        ];
        let expected = [0., -2., 30., 3., 0., 5.];
        let p = w.map(|v| project(expected, v));
        let fit = fit_affine(&w, &p).unwrap();
        for (a, b) in fit.into_iter().zip(expected) {
            assert!((a - b).abs() < 1e-8);
        }
        let independent = project(fit, Point { x: 2., y: 4. });
        assert_eq!(independent, Point { x: 22., y: 11. });
    }
    #[test]
    fn rejects_collinear_and_nonfinite_fits() {
        let points = [
            Point { x: 0., y: 0. },
            Point { x: 1., y: 1. },
            Point { x: 2., y: 2. },
        ];
        assert!(fit_affine(&points, &points).is_err());
        let bad = [Point { x: f64::NAN, y: 0. }; 3];
        assert!(fit_affine(&bad, &bad).is_err());
    }
    #[test]
    fn marker_requires_current_proven_fresh_context() {
        let map = entry();
        let now = Instant::now();
        let mut sample = LocationSample {
            point: Point { x: 20., y: 30. },
            context: map.context.clone(),
            session_generation: 1,
            zone_generation: 2,
            observed_at: now,
            valid: true,
        };
        let gate = MarkerGate {
            session_generation: 1,
            zone_generation: 2,
            paused: false,
            confidence: LocationConfidence::Validated,
            profile: None,
        };
        assert_eq!(
            marker_position(&map, &sample, gate, now),
            Some(sample.point)
        );
        assert!(
            marker_position(
                &map,
                &sample,
                MarkerGate {
                    paused: true,
                    ..gate
                },
                now
            )
            .is_none()
        );
        assert!(
            marker_position(
                &map,
                &sample,
                MarkerGate {
                    confidence: LocationConfidence::Unavailable,
                    ..gate
                },
                now
            )
            .is_none()
        );
        assert!(
            marker_position(
                &map,
                &sample,
                MarkerGate {
                    zone_generation: 3,
                    ..gate
                },
                now
            )
            .is_none()
        );
        assert!(marker_position(&map, &sample, gate, now + Duration::from_millis(1001)).is_none());
        assert!(marker_position(&map, &sample, gate, now - Duration::from_millis(1)).is_none());
        sample.context.floor = "upper".into();
        assert!(marker_position(&map, &sample, gate, now).is_none());
        sample.context = map.context.clone();
        sample.point.x = 101.;
        assert!(marker_position(&map, &sample, gate, now).is_none());
    }
    #[test]
    fn rejects_bad_catalog_and_escaping_assets() {
        let root = std::env::temp_dir().join(format!(
            "dcuo-map-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("test.png"), b"synthetic").unwrap();
        let map = entry();
        let legacy = Catalog {
            schema_version: 1,
            entries: vec![map.clone()],
        };
        std::fs::write(
            root.join("legacy.json"),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        let loaded = load_catalog(&root.join("legacy.json")).unwrap();
        assert!(loaded.entries[0].calibration.is_none());
        assert_eq!(
            loaded.entries[0].effective_confidence(LocationConfidence::Validated, None),
            LocationConfidence::Experimental
        );
        assert_eq!(load_asset(&map, &root).unwrap(), b"synthetic");
        let mut bad = map.clone();
        bad.sha256 = "0".repeat(64);
        assert!(load_asset(&bad, &root).is_err());
        bad.asset = "../test.png".into();
        assert!(load_asset(&bad, &root).is_err());
        std::os::unix::fs::symlink("/etc/hosts", root.join("escape.png")).unwrap();
        bad.asset = "escape.png".into();
        assert!(load_asset(&bad, &root).is_err());
        let catalog = Catalog {
            schema_version: 1,
            entries: vec![map.clone(), map],
        };
        std::fs::write(
            root.join("catalog.json"),
            serde_json::to_vec(&catalog).unwrap(),
        )
        .unwrap();
        assert!(load_catalog(&root.join("catalog.json")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
