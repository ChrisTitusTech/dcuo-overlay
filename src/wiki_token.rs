//! Bounded private credential persistence, separate from shareable preferences.
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
const LIMIT: u64 = 4096;
const ERROR: &str = "The saved wiki token could not be accessed safely.";
static NEXT: AtomicU64 = AtomicU64::new(0);
static IO: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) struct Store {
    path: Option<PathBuf>,
}
impl Default for Store {
    fn default() -> Self {
        #[cfg(windows)]
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute());
        #[cfg(not(windows))]
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .map(|p| p.join(".config"))
            });
        Self {
            path: base.map(|p| p.join("dcuo-companion/wiki-credentials/token")),
        }
    }
}
impl Store {
    /// The supplied file must be inside a dedicated private directory.
    #[cfg(test)]
    pub(crate) fn at(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }
    pub(crate) fn load(&self) -> Result<Option<String>, String> {
        let _guard = IO.lock().unwrap_or_else(|e| e.into_inner());
        let path = self.path.as_ref().ok_or(ERROR)?;
        let parent = path.parent().ok_or(ERROR)?;
        if !directory(parent, false)? {
            return Ok(None);
        }
        validate_file(path)?;
        let file = match crate::storage::read_options(true).open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ERROR.into()),
        };
        let meta = file.metadata().map_err(|_| ERROR)?;
        if !crate::storage::regular(&file).map_err(|_| ERROR)?
            || !private(&meta, false)
            || meta.len() > LIMIT
        {
            return Err(ERROR.into());
        }
        let mut bytes = Vec::new();
        file.take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ERROR)?;
        if bytes.len() as u64 > LIMIT {
            return Err(ERROR.into());
        }
        let token = decode(&bytes)?;
        if !crate::wiki_items::is_hex64(&token) {
            return Err(ERROR.into());
        }
        Ok(Some(token))
    }
    pub(crate) fn save(&self, token: &str) -> Result<(), String> {
        let _guard = IO.lock().unwrap_or_else(|e| e.into_inner());
        if !crate::wiki_items::is_hex64(token) {
            return Err("Wiki tokens must contain exactly 64 hexadecimal characters.".into());
        }
        let bytes = encode(token)?;
        let path = self.path.as_ref().ok_or(ERROR)?;
        let parent = path.parent().ok_or(ERROR)?;
        directory(parent, true)?;
        validate_file(path)?;
        let mut created = None;
        for _ in 0..32 {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let temp = parent.join(format!(".wiki-token-{}-{id}.tmp", std::process::id()));
            match crate::storage::create_options().open(&temp) {
                Ok(file) => {
                    created = Some((temp, file));
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(ERROR.into()),
            }
        }
        let (temp, mut file) = created.ok_or(ERROR)?;
        let result = (|| -> Result<(), String> {
            file.write_all(&bytes).map_err(|_| ERROR)?;
            file.sync_all().map_err(|_| ERROR)?;
            drop(file);
            directory(parent, false)?;
            validate_file(path)?;
            crate::storage::replace(&temp, path).map_err(|_| ERROR)?;
            crate::storage::sync_directory(parent).map_err(|_| ERROR)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
    pub(crate) fn clear(&self) -> Result<(), String> {
        let _guard = IO.lock().unwrap_or_else(|e| e.into_inner());
        let path = self.path.as_ref().ok_or(ERROR)?;
        let parent = path.parent().ok_or(ERROR)?;
        if !directory(parent, false)? {
            return Ok(());
        }
        validate_file(path)?;
        match fs::remove_file(path) {
            Ok(()) => crate::storage::sync_directory(parent).map_err(|_| ERROR.into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(ERROR.into()),
        }
    }
}
fn private(meta: &fs::Metadata, dir: bool) -> bool {
    if crate::storage::is_reparse(meta) {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // SAFETY: geteuid takes no arguments and returns this process's effective uid.
        meta.uid() == unsafe { libc::geteuid() }
            && meta.permissions().mode() & 0o777 == if dir { 0o700 } else { 0o600 }
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        true
    }
}
fn ancestors(path: &Path, create: bool) -> Result<bool, String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
        && !ancestors(parent, create)?
    {
        return Ok(false);
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !crate::storage::is_reparse(&meta) => Ok(true),
        Ok(_) => Err(ERROR.into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !create {
                return Ok(false);
            }
            match crate::storage::create_directory(path) {
                Ok(()) => Ok(true),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => ancestors(path, false),
                Err(_) => Err(ERROR.into()),
            }
        }
        Err(_) => Err(ERROR.into()),
    }
}
fn directory(path: &Path, create: bool) -> Result<bool, String> {
    if !path.is_absolute() {
        return Err(ERROR.into());
    }
    if !ancestors(path, create)? {
        return Ok(false);
    }
    if !private(&fs::symlink_metadata(path).map_err(|_| ERROR)?, true) {
        return Err(ERROR.into());
    }
    Ok(true)
}
fn validate_file(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && private(&meta, false) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(ERROR.into()),
    }
}
#[cfg(not(windows))]
fn encode(token: &str) -> Result<Vec<u8>, String> {
    Ok(token.as_bytes().to_vec())
}
#[cfg(not(windows))]
fn decode(bytes: &[u8]) -> Result<String, String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| ERROR.into())
}
#[cfg(windows)]
fn crypt(bytes: &[u8], protect: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: input is valid for its stated length; DPAPI initializes output on success.
    let ok = unsafe {
        if protect {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err(ERROR.into());
    }
    let valid = output.cbData as u64 <= LIMIT && !output.pbData.is_null();
    // SAFETY: successful DPAPI output is a readable LocalAlloc allocation.
    let result = if valid {
        Ok(unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec())
    } else {
        Err(ERROR.into())
    };
    // SAFETY: DPAPI requires callers to release this allocation with LocalFree.
    unsafe {
        LocalFree(output.pbData.cast());
    }
    result
}
#[cfg(windows)]
fn encode(token: &str) -> Result<Vec<u8>, String> {
    crypt(token.as_bytes(), true)
}
#[cfg(windows)]
fn decode(bytes: &[u8]) -> Result<String, String> {
    String::from_utf8(crypt(bytes, false)?).map_err(|_| ERROR.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("wiki-token-test-{}-{id}", std::process::id()));
            crate::storage::create_directory(&path).unwrap();
            Self(path)
        }
        fn store(&self) -> Store {
            Store::at(self.0.join("private/token"))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn persistence_reload_update_and_delete() {
        let fixture = Fixture::new();
        let store = fixture.store();
        assert_eq!(store.load().unwrap(), None);
        let token = "a".repeat(64);
        store.save(&token).unwrap();
        assert_eq!(fixture.store().load().unwrap(), Some(token));
        let updated = "b".repeat(64);
        store.save(&updated).unwrap();
        assert_eq!(fixture.store().load().unwrap(), Some(updated));
        store.clear().unwrap();
        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
    }
    #[test]
    fn malformed_and_oversize_are_rejected() {
        let fixture = Fixture::new();
        let store = fixture.store();
        assert!(store.save("invalid").is_err());
        store.save(&"a".repeat(64)).unwrap();
        let path = store.path.as_ref().unwrap();
        for bytes in [b"bad".to_vec(), vec![b'a'; LIMIT as usize + 1]] {
            fs::write(path, bytes).unwrap();
            assert!(store.load().is_err());
        }
        store.clear().unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn missing_ancestors_and_shared_config_parent_are_supported() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::new();
        let config = fixture.0.join("config");
        fs::create_dir(&config).unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o755)).unwrap();
        let store = Store::at(config.join("companion/wiki-credentials/token"));
        store.save(&"a".repeat(64)).unwrap();
        assert_eq!(store.load().unwrap(), Some("a".repeat(64)));
        assert_eq!(
            fs::metadata(config).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
    #[cfg(unix)]
    #[test]
    fn permissions_and_symlinks_are_rejected() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let fixture = Fixture::new();
        let store = fixture.store();
        store.save(&"a".repeat(64)).unwrap();
        let path = store.path.as_ref().unwrap();
        let parent = path.parent().unwrap();
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(parent).unwrap().permissions().mode() & 0o777,
            0o700
        );
        fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.load().is_err());
        assert!(store.save(&"b".repeat(64)).is_err());
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(store.load().is_err());
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_file(path).unwrap();
        let target = fixture.0.join("target");
        fs::write(&target, b"untouched").unwrap();
        symlink(&target, path).unwrap();
        assert!(store.load().is_err());
        assert!(store.save(&"b".repeat(64)).is_err());
        assert!(store.clear().is_err());
        assert_eq!(fs::read(&target).unwrap(), b"untouched");
        let linked = fixture.0.join("linked");
        symlink(parent, &linked).unwrap();
        assert!(Store::at(linked.join("token")).load().is_err());
    }
}
