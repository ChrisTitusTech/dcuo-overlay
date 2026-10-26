//! Platform file operations shared by bounded local asset readers.
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io,
    path::{Path, PathBuf},
};

pub(crate) fn read_options(no_follow: bool) -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | if no_follow { libc::O_NOFOLLOW } else { 0 });
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Opening the reparse point itself allows rejecting it before any read.
        if no_follow {
            options.custom_flags(
                windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT,
            );
        }
    }
    options
}
pub(crate) fn create_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options
}
pub(crate) fn is_reparse(meta: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}
pub(crate) fn regular(file: &File) -> io::Result<bool> {
    let meta = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        if unsafe { windows_sys::Win32::Storage::FileSystem::GetFileType(file.as_raw_handle()) }
            != windows_sys::Win32::Storage::FileSystem::FILE_TYPE_DISK
        {
            return Ok(false);
        }
    }
    Ok(meta.is_file() && !is_reparse(&meta))
}
pub(crate) fn opened_path(file: &File) -> io::Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
        let mut buf = vec![0u16; 32768];
        let len = unsafe {
            GetFinalPathNameByHandleW(file.as_raw_handle(), buf.as_mut_ptr(), buf.len() as u32, 0)
        } as usize;
        if len == 0 {
            return Err(io::Error::last_os_error());
        }
        if len >= buf.len() {
            return Err(io::Error::other("Opened path exceeds limit"));
        }
        Ok(PathBuf::from(std::ffi::OsString::from_wide(&buf[..len])))
    }
}
pub(crate) fn create_directory(path: &Path) -> io::Result<()> {
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let mut builder = builder;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}
#[cfg(feature = "window")]
pub(crate) fn private_owned(meta: &Metadata, parent: &Metadata, directory: bool) -> bool {
    if is_reparse(meta) {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        meta.uid() == parent.uid()
            && meta.permissions().mode() & 0o777 == if directory { 0o700 } else { 0o600 }
    }
    // Windows files inherit the user profile directory ACL. Unix mode/uid bits
    // have no equivalent in Metadata; reparse points are still rejected above.
    #[cfg(not(unix))]
    {
        let _ = (parent, directory);
        true
    }
}
pub(crate) fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::rename(source, destination)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let src: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let dst: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        if unsafe {
            MoveFileExW(
                src.as_ptr(),
                dst.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
pub(crate) fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_overwrites_existing_file_and_confines_open_handle() {
        let dir = std::env::temp_dir().join(format!(
            "dcuo-storage-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        create_directory(&dir).unwrap();
        let destination = dir.join("settings.json");
        let source = dir.join("temporary");
        fs::write(&destination, b"old").unwrap();
        fs::write(&source, b"new").unwrap();
        replace(&source, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"new");
        assert!(!source.exists());
        let file = read_options(true).open(&destination).unwrap();
        assert!(regular(&file).unwrap());
        assert!(
            opened_path(&file)
                .unwrap()
                .starts_with(dir.canonicalize().unwrap())
        );
        drop(file);
        fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn reparse_attributes_are_rejected_without_following() {
        use std::os::windows::fs::symlink_file;
        let dir = std::env::temp_dir().join(format!("dcuo-reparse-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("target");
        let link = dir.join("link");
        fs::write(&target, b"secret").unwrap();
        // Symlink creation needs Developer Mode or a privilege on Windows.
        match symlink_file(&target, &link) {
            Ok(()) => {
                assert!(is_reparse(&fs::symlink_metadata(&link).unwrap()));
                let file = read_options(true).open(&link).unwrap();
                assert!(!regular(&file).unwrap());
                drop(file);
            }
            Err(e) if e.raw_os_error() == Some(1314) => {}
            Err(e) => panic!("Cannot create test symlink: {e}"),
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
