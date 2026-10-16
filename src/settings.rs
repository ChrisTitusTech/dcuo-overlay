//! Validated, non-personal preferences stored in the standard XDG config directory.
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_BYTES: u64 = 64 * 1024;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub api_base: String,
    pub world: String,
    pub feat_faction: String,
    pub ui_scale: f32,
    pub dark_mode: bool,
    pub map_catalog: String,
    pub map_source: String,
    pub auto_import_maps: bool,
    pub auto_reconnect: bool,
    pub show_ground_spawns: bool,
    pub poll_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            api_base: crate::api::DEFAULT_BASE_URL.into(),
            world: "2".into(),
            feat_faction: String::new(),
            ui_scale: 1.0,
            dark_mode: true,
            map_catalog: String::new(),
            map_source: String::new(),
            auto_import_maps: true,
            auto_reconnect: false,
            show_ground_spawns: false,
            poll_ms: 250,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.feat_faction.as_str(), "" | "Hero" | "Villain") {
            return Err("Choose All, Hero or Villain for the feat faction filter.".into());
        }
        if self.api_base.len() > 2048 || self.api_base.chars().any(char::is_control) {
            return Err("API URL exceeds its length limit or contains control characters.".into());
        }
        let url = url::Url::parse(&self.api_base)
            .map_err(|_| "API URL must be a valid HTTP or HTTPS URL.".to_owned())?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(
                "API URL must use HTTP(S) without credentials, a query, or a fragment.".into(),
            );
        }
        if !matches!(self.world.as_str(), "2" | "4" | "10" | "11" | "5001") {
            return Err("Select a supported world ID.".into());
        }
        if !self.ui_scale.is_finite() || !(0.75..=2.0).contains(&self.ui_scale) {
            return Err("UI scale must be between 0.75 and 2.0.".into());
        }
        if !(100..=2000).contains(&self.poll_ms) {
            return Err("Polling interval must be between 100 and 2000 milliseconds.".into());
        }
        if self.map_catalog.len() > 4096 || self.map_catalog.chars().any(char::is_control) {
            return Err(
                "Map catalog path exceeds its length limit or contains control characters.".into(),
            );
        }
        if self.map_catalog.contains("://") {
            return Err("Map catalog must be a local file path.".into());
        }
        if self.map_source.len() > 4096
            || self.map_source.chars().any(char::is_control)
            || self.map_source.contains("://")
        {
            return Err("Installed map source must be a bounded local directory path.".into());
        }
        Ok(())
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config_path().ok_or("The configuration directory is unavailable.")?;
        self.save_path(&path)
    }

    /// Persist only this immediate Map control, leaving unapplied settings alone.
    pub fn save_ground_spawns(enabled: bool) -> Result<(), String> {
        let path = config_path().ok_or("The configuration directory is unavailable.")?;
        Self::save_ground_spawns_path(&path, enabled)
    }

    fn save_ground_spawns_path(path: &Path, enabled: bool) -> Result<(), String> {
        let (mut saved, warning) = load_path(path);
        if let Some(warning) = warning {
            return Err(warning);
        }
        saved.show_ground_spawns = enabled;
        saved.save_path(path)
    }

    /// Persist only the immediate Character faction filter.
    pub fn save_feat_faction(faction: &str) -> Result<(), String> {
        let path = config_path().ok_or("The configuration directory is unavailable.")?;
        Self::save_feat_faction_path(&path, faction)
    }
    fn save_feat_faction_path(path: &Path, faction: &str) -> Result<(), String> {
        let (mut saved, warning) = load_path(path);
        if let Some(warning) = warning {
            return Err(warning);
        }
        saved.feat_faction = faction.into();
        saved.save_path(path)
    }

    fn save_path(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|_| "Settings could not be serialized.".to_owned())?;
        let parent = path
            .parent()
            .ok_or("The settings path has no parent directory.")?;
        ensure_directory(parent)
            .map_err(|_| "The configuration directory is unavailable or unsafe.".to_owned())?;
        reject_non_regular(path)
            .map_err(|_| "The settings destination is not a regular file.".to_owned())?;
        let mut created = None;
        for _ in 0..32 {
            let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let temp = parent.join(format!(".settings-{}-{id}.tmp", std::process::id()));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temp)
            {
                Ok(file) => {
                    created = Some((temp, file));
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err("A temporary settings file could not be created.".into()),
            }
        }
        let (temp, mut file) =
            created.ok_or("A unique temporary settings file could not be created.")?;
        let result = (|| {
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            reject_non_regular(path)?;
            // Rename replaces the directory entry and never follows a target symlink.
            fs::rename(&temp, path)?;
            fs::File::open(parent)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
            return Err("Settings could not be saved atomically.".into());
        }
        Ok(())
    }
}

pub fn load() -> (Settings, Option<String>) {
    match config_path() {
        Some(path) => load_path(&path),
        None => (
            Settings::default(),
            Some("The configuration directory is unavailable; defaults are in use.".into()),
        ),
    }
}

fn config_path() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let base = xdg.or_else(|| {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|path| path.join(".config"))
    })?;
    Some(base.join("dcuo-companion/settings.json"))
}

fn load_path(path: &Path) -> (Settings, Option<String>) {
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Settings::default(), None);
        }
        Err(_) => return defaults_warning(),
    };
    if !file
        .metadata()
        .is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_BYTES)
    {
        return defaults_warning();
    }
    let mut bytes = Vec::new();
    if (&mut file)
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_BYTES
    {
        return defaults_warning();
    }
    match serde_json::from_slice::<Settings>(&bytes) {
        Ok(settings) if settings.validate().is_ok() => (settings, None),
        _ => defaults_warning(),
    }
}

fn defaults_warning() -> (Settings, Option<String>) {
    (
        Settings::default(),
        Some("Settings could not be read or validated; defaults are in use.".into()),
    )
}

fn reject_non_regular(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(()),
        Ok(_) => Err(std::io::Error::other("not a regular file")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn ensure_directory(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                ensure_directory(parent)?;
            }
            Ok(())
        }
        Ok(_) => Err(std::io::Error::other("unsafe directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                ensure_directory(parent)?;
            }
            match fs::create_dir(path) {
                Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o700)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    ensure_directory(path)
                }
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "dcuo-settings-test-{}-{}",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> PathBuf {
            self.0.join("config/settings.json")
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn round_trip_private_file_and_updates() {
        let dir = TempDir::new();
        let mut settings = Settings::default();
        settings.save_path(&dir.path()).unwrap();
        settings.poll_ms = 100;
        settings.show_ground_spawns = true;
        settings.ui_scale = 2.0;
        settings.save_path(&dir.path()).unwrap();
        assert_eq!(load_path(&dir.path()), (settings, None));
        assert_eq!(
            fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::read_dir(dir.path().parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn faction_filter_round_trip_preserves_unrelated_settings_and_rejects_invalid() {
        let dir = TempDir::new();
        let saved = Settings {
            map_source: "/saved/source".into(),
            ..Settings::default()
        };
        saved.save_path(&dir.path()).unwrap();
        Settings::save_feat_faction_path(&dir.path(), "Villain").unwrap();
        let (loaded, warning) = load_path(&dir.path());
        assert!(warning.is_none());
        assert_eq!(loaded.feat_faction, "Villain");
        assert_eq!(loaded.map_source, saved.map_source);
        assert!(Settings::save_feat_faction_path(&dir.path(), "invalid").is_err());
        assert_eq!(load_path(&dir.path()).0.feat_faction, "Villain");
        Settings::save_feat_faction_path(&dir.path(), "").unwrap();
        assert!(load_path(&dir.path()).0.feat_faction.is_empty());
    }
    #[test]
    fn map_toggle_preserves_other_saved_preferences() {
        let dir = TempDir::new();
        let saved = Settings {
            map_source: "/saved/source".into(),
            auto_import_maps: false,
            ..Settings::default()
        };
        saved.save_path(&dir.path()).unwrap();
        Settings::save_ground_spawns_path(&dir.path(), true).unwrap();
        let (actual, warning) = load_path(&dir.path());
        assert!(warning.is_none());
        assert_eq!(
            actual,
            Settings {
                show_ground_spawns: true,
                ..saved
            }
        );
        fs::write(dir.path(), b"invalid").unwrap();
        assert!(Settings::save_ground_spawns_path(&dir.path(), false).is_err());
        assert_eq!(fs::read(dir.path()).unwrap(), b"invalid");
    }

    #[test]
    fn missing_and_invalid_configuration_recover() {
        let dir = TempDir::new();
        assert_eq!(load_path(&dir.path()), (Settings::default(), None));
        fs::create_dir(dir.path().parent().unwrap()).unwrap();
        for bytes in [
            b"invalid-secret-value".to_vec(),
            b"{\"poll_ms\":0}".to_vec(),
            b"{\"character_name\":\"private\"}".to_vec(),
            vec![b' '; MAX_BYTES as usize + 1],
        ] {
            fs::write(dir.path(), bytes).unwrap();
            let (settings, warning) = load_path(&dir.path());
            assert_eq!(settings, Settings::default());
            assert_eq!(
                warning.as_deref(),
                Some("Settings could not be read or validated; defaults are in use.")
            );
        }
        fs::write(dir.path(), b"{}").unwrap();
        assert_eq!(load_path(&dir.path()), (Settings::default(), None));
    }

    #[test]
    fn rejects_unsafe_urls_and_out_of_bounds_values() {
        for api_base in [
            "ftp://example.com",
            "https://secret@example.com",
            "https://example.com?a=secret",
            "https://example.com#secret",
            "https://example.com\n",
            "relative",
        ] {
            assert!(
                Settings {
                    api_base: api_base.into(),
                    ..Settings::default()
                }
                .validate()
                .is_err()
            );
        }
        for world in ["", "1", "3", "private"] {
            assert!(
                Settings {
                    world: world.into(),
                    ..Settings::default()
                }
                .validate()
                .is_err()
            );
        }
        for ui_scale in [f32::NAN, f32::INFINITY, 0.74, 2.01] {
            assert!(
                Settings {
                    ui_scale,
                    ..Settings::default()
                }
                .validate()
                .is_err()
            );
        }
        for poll_ms in [0, 99, 2001, u64::MAX] {
            assert!(
                Settings {
                    poll_ms,
                    ..Settings::default()
                }
                .validate()
                .is_err()
            );
        }
        for map_catalog in [
            "https://example.com/map".to_string(),
            "bad\0path".to_string(),
            "x".repeat(4097),
        ] {
            assert!(
                Settings {
                    map_catalog,
                    ..Settings::default()
                }
                .validate()
                .is_err()
            );
        }
    }

    #[test]
    fn automatic_map_preferences_validate_without_overwriting_saved_settings() {
        let dir = TempDir::new();
        let settings = Settings {
            map_source: "/games/UNREAL3/DCGAME/COOKEDPC".into(),
            ..Default::default()
        };
        settings.save_path(&dir.path()).unwrap();
        assert_eq!(load_path(&dir.path()).0, settings);
        for source in [
            "https://example.invalid/maps".into(),
            "bad\0path".into(),
            "x".repeat(4097),
        ] {
            let invalid = Settings {
                map_source: source,
                ..settings.clone()
            };
            assert!(invalid.save_path(&dir.path()).is_err());
            assert_eq!(load_path(&dir.path()).0, settings);
        }
        assert!(Settings::default().auto_import_maps);
    }

    #[test]
    fn invalid_save_preserves_existing_preferences() {
        let dir = TempDir::new();
        Settings::default().save_path(&dir.path()).unwrap();
        let before = fs::read(dir.path()).unwrap();
        let invalid = Settings {
            poll_ms: 1,
            ..Settings::default()
        };
        assert!(invalid.save_path(&dir.path()).is_err());
        assert_eq!(fs::read(dir.path()).unwrap(), before);
        assert_eq!(
            fs::read_dir(dir.path().parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn concurrent_saves_publish_complete_files_and_clean_temporary_files() {
        let dir = TempDir::new();
        Settings::default().save_path(&dir.path()).unwrap();
        std::thread::scope(|scope| {
            for poll_ms in 100..108 {
                let path = dir.path();
                scope.spawn(move || {
                    Settings {
                        poll_ms,
                        ..Settings::default()
                    }
                    .save_path(&path)
                    .unwrap();
                    let (settings, warning) = load_path(&path);
                    assert!(warning.is_none());
                    assert!((100..108).contains(&settings.poll_ms));
                });
            }
        });
        assert_eq!(
            fs::read_dir(dir.path().parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn symlink_destinations_and_directories_are_rejected() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new();
        let target = dir.0.join("original");
        fs::write(&target, b"keep").unwrap();
        fs::create_dir(dir.path().parent().unwrap()).unwrap();
        symlink(&target, dir.path()).unwrap();
        assert!(Settings::default().save_path(&dir.path()).is_err());
        assert!(load_path(&dir.path()).1.is_some());
        assert_eq!(fs::read(&target).unwrap(), b"keep");
        let linked_dir = dir.0.join("link");
        symlink(dir.path().parent().unwrap(), &linked_dir).unwrap();
        assert!(
            Settings::default()
                .save_path(&linked_dir.join("other.json"))
                .is_err()
        );
        assert_eq!(
            fs::read_dir(dir.path().parent().unwrap()).unwrap().count(),
            1
        );
    }
}
