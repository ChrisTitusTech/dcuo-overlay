//! Native Windows read/query-only process backend. No debug privilege changes.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    mem::{size_of, zeroed},
    os::windows::{ffi::OsStringExt, io::AsRawHandle},
};
use std::{
    fs,
    io::{Seek, SeekFrom},
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{
        Diagnostics::{Debug::ReadProcessMemory, ToolHelp::*},
        Memory::*,
        Threading::*,
    },
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn open(pid: i32, read: bool) -> io::Result<Handle> {
    if pid <= 0 {
        return Err(invalid("invalid PID"));
    }
    // SAFETY: no borrowed pointers; request only query and optional read rights.
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_INFORMATION | if read { PROCESS_VM_READ } else { 0 },
            0,
            pid as u32,
        )
    };
    if handle.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(Handle(handle))
    }
}
fn handle_identity(handle: &Handle, pid: i32) -> io::Result<Identity> {
    let (mut created, mut exited, mut kernel, mut user) =
        unsafe { (zeroed(), zeroed(), zeroed(), zeroed()) };
    // SAFETY: handle is owned and all FILETIME outputs are valid.
    if unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut exit = 0;
    if unsafe { GetExitCodeProcess(handle.0, &mut exit) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if exit != STILL_ACTIVE as u32 {
        return Err(invalid("process exited"));
    }
    Ok(Identity {
        pid,
        start_ticks: (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime),
    })
}
pub fn identity(pid: i32) -> io::Result<Identity> {
    handle_identity(&open(pid, false)?, pid)
}
fn image_path(handle: &Handle) -> io::Result<PathBuf> {
    let mut buffer = vec![0u16; 32768];
    let mut len = buffer.len() as u32;
    // SAFETY: bounded writable UTF-16 buffer and valid process handle.
    if unsafe { QueryFullProcessImageNameW(handle.0, 0, buffer.as_mut_ptr(), &mut len) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..len as usize])))
}
fn protection(protect: u32) -> (bool, bool, bool) {
    if protect & (PAGE_GUARD | PAGE_NOACCESS) != 0 {
        return (false, false, false);
    }
    let base = protect & 0xff;
    (
        matches!(
            base,
            PAGE_READONLY
                | PAGE_READWRITE
                | PAGE_WRITECOPY
                | PAGE_EXECUTE_READ
                | PAGE_EXECUTE_READWRITE
                | PAGE_EXECUTE_WRITECOPY
        ),
        matches!(
            base,
            PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
        ),
        matches!(
            base,
            PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
        ),
    )
}
fn query_maps(handle: &Handle) -> io::Result<Vec<Mapping>> {
    let mut address = 0usize;
    let mut maps = Vec::new();
    // Bound metadata work even if another thread continuously mutates mappings.
    for _ in 0..131072 {
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { zeroed() };
        // SAFETY: query never dereferences the remote address; output has exact ABI size.
        let count = unsafe {
            VirtualQueryEx(
                handle.0,
                address as _,
                &mut info,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if count == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                return Ok(maps);
            }
            return Err(error);
        }
        let start = info.BaseAddress as usize;
        let end = start
            .checked_add(info.RegionSize)
            .ok_or_else(|| invalid("mapping range overflow"))?;
        if end <= address {
            return Err(invalid("non-progressing mapping query"));
        }
        if info.State == MEM_COMMIT {
            let (readable, writable, executable) = protection(info.Protect);
            maps.push(Mapping {
                start,
                end,
                readable,
                writable,
                executable,
                offset: (start - info.AllocationBase as usize) as u64,
                device: if info.Type == MEM_PRIVATE {
                    "windows-private"
                } else {
                    "windows-mapped"
                }
                .into(),
                inode: 0,
                path: None,
            });
        }
        address = end;
    }
    Err(invalid("mapping count exceeds bound"))
}
pub fn mappings(pid: i32) -> io::Result<Vec<Mapping>> {
    let handle = open(pid, false)?;
    let before = handle_identity(&handle, pid)?;
    let maps = query_maps(&handle)?;
    if handle_identity(&handle, pid)? != before || identity(pid)? != before {
        return Err(invalid("process identity changed"));
    }
    Ok(maps)
}
fn main_mapping(pid: i32, path: &Path, maps: &[Mapping]) -> io::Result<Mapping> {
    let snapshot =
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid as u32) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = Handle(snapshot);
    let mut module: MODULEENTRY32W = unsafe { zeroed() };
    module.dwSize = size_of::<MODULEENTRY32W>() as u32;
    if unsafe { Module32FirstW(snapshot.0, &mut module) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut matches = Vec::new();
    loop {
        let len = module
            .szExePath
            .iter()
            .position(|v| *v == 0)
            .unwrap_or(module.szExePath.len());
        let module_path = PathBuf::from(OsString::from_wide(&module.szExePath[..len]));
        if module_path
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&path.as_os_str().to_string_lossy())
            && let Some(mapping) = maps
                .iter()
                .find(|m| m.start == module.modBaseAddr as usize && m.offset == 0 && m.readable)
        {
            let mut mapping = mapping.clone();
            mapping.path = Some(path.to_owned());
            matches.push(mapping);
        }
        if unsafe { Module32NextW(snapshot.0, &mut module) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                return Err(error);
            }
            break;
        }
    }
    if matches.len() != 1 {
        return Err(invalid("missing or ambiguous main image mapping"));
    }
    Ok(matches.remove(0))
}
pub fn candidate(pid: i32) -> io::Result<Candidate> {
    let handle = open(pid, false)?;
    let before = handle_identity(&handle, pid)?;
    let path = image_path(&handle)?;
    if !path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("DCGAME.EXE"))
    {
        return Err(invalid("process image is not DCGAME.EXE"));
    }
    let mapping = main_mapping(pid, &path, &query_maps(&handle)?)?;
    if handle_identity(&handle, pid)? != before || identity(pid)? != before {
        return Err(invalid("process identity changed"));
    }
    Ok(Candidate {
        identity: before,
        mapping,
    })
}
pub fn discover() -> io::Result<Vec<Candidate>> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = Handle(snapshot);
    let mut entry: PROCESSENTRY32W = unsafe { zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut candidates = Vec::new();
    loop {
        let len = entry
            .szExeFile
            .iter()
            .position(|v| *v == 0)
            .unwrap_or(entry.szExeFile.len());
        if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case("DCGAME.EXE")
            && let Ok(c) = candidate(entry.th32ProcessID as i32)
        {
            candidates.push(c);
        }
        if unsafe { Process32NextW(snapshot.0, &mut entry) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                return Err(error);
            }
            break;
        }
    }
    candidates.sort_by_key(|c| c.identity.pid);
    Ok(candidates)
}
fn file_identity(file: &File) -> io::Result<(u32, u64, u64, u64, u64)> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let value = |hi, lo| (u64::from(hi) << 32) | u64::from(lo);
    Ok((
        info.dwVolumeSerialNumber,
        value(info.nFileIndexHigh, info.nFileIndexLow),
        value(info.nFileSizeHigh, info.nFileSizeLow),
        value(
            info.ftLastWriteTime.dwHighDateTime,
            info.ftLastWriteTime.dwLowDateTime,
        ),
        value(
            info.ftCreationTime.dwHighDateTime,
            info.ftCreationTime.dwLowDateTime,
        ),
    ))
}
fn mapped_path(path: &[u16]) -> io::Result<PathBuf> {
    let dos_prefix: Vec<u16> = r"\??\".encode_utf16().collect();
    let device_prefix: Vec<u16> = r"\Device\".encode_utf16().collect();
    if path.iter().any(|c| *c == 0 || *c == u16::from(b'/')) {
        return Err(invalid("invalid mapped image path"));
    }
    let valid_parts = |tail: &[u16]| {
        tail.split(|c| *c == u16::from(b'\\'))
            .all(|part| !part.is_empty() && part != [46] && part != [46, 46])
    };
    let wide: Vec<u16> = if path.starts_with(&dos_prefix) {
        let tail = &path[dos_prefix.len()..];
        let drive_root = tail.len() > 3
            && tail[0] <= 127
            && (tail[0] as u8).is_ascii_alphabetic()
            && tail[1] == 58
            && tail[2] == 92;
        let unc_root = tail.starts_with(&[85, 78, 67, 92]) && tail.split(|c| *c == 92).count() >= 4;
        if !(drive_root || unc_root) || !valid_parts(tail) {
            return Err(invalid("ambiguous mapped DOS path"));
        }
        r"\\?\"
            .encode_utf16()
            .chain(path[dos_prefix.len()..].iter().copied())
            .collect()
    } else if path.starts_with(&device_prefix) {
        let tail = &path[device_prefix.len()..];
        if !tail.contains(&92) || !valid_parts(tail) {
            return Err(invalid("ambiguous mapped device path"));
        }
        r"\\?\GLOBALROOT"
            .encode_utf16()
            .chain(path.iter().copied())
            .collect()
    } else {
        return Err(invalid("unsupported mapped image path namespace"));
    };
    Ok(PathBuf::from(OsString::from_wide(&wide)))
}
fn verify_mapped_path(expected: &Candidate, file: &File) -> io::Result<()> {
    use windows_sys::Win32::System::ProcessStatus::K32GetMappedFileNameW;
    let handle = open(expected.identity.pid, true)?;
    if handle_identity(&handle, expected.identity.pid)? != expected.identity {
        return Err(invalid("process identity changed"));
    }
    let mut mapped = vec![0u16; 32768];
    // SAFETY: the query receives a valid bounded output buffer and owned handle.
    let mapped_len = unsafe {
        K32GetMappedFileNameW(
            handle.0,
            expected.mapping.start as _,
            mapped.as_mut_ptr(),
            mapped.len() as u32,
        )
    } as usize;
    if mapped_len == 0 {
        return Err(io::Error::other(format!(
            "mapped file path query: {}",
            io::Error::last_os_error()
        )));
    }
    if mapped_len >= mapped.len() {
        return Err(invalid("mapped image path exceeds bound"));
    }
    let path = mapped_path(&mapped[..mapped_len])?;
    // Open the kernel-reported mapped image rather than comparing path aliases.
    // Deny writes/deletion and require the same volume/file ID and stable metadata.
    use std::os::windows::fs::OpenOptionsExt;
    let mapped_file = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)?;
    if file_identity(file)? != file_identity(&mapped_file)? {
        return Err(invalid("mapped image file identity mismatch"));
    }

    Ok(())
}
pub fn executable_metadata(expected: &Candidate) -> io::Result<PeMetadata> {
    ensure_same_candidate(expected, &candidate(expected.identity.pid)?)?;
    let path = expected
        .mapping
        .path
        .as_ref()
        .ok_or_else(|| invalid("missing image path"))?;
    // Deny writers/deletion while hashing, including path replacement.
    use std::os::windows::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)?;
    verify_mapped_path(expected, &file)?;
    let before = file_identity(&file)?;
    if !file.metadata()?.is_file() || before.2 > MAX_EXECUTABLE_BYTES {
        return Err(invalid("invalid executable size/type"));
    }
    let mut header = vec![0; before.2.min(65536) as usize];
    file.read_exact(&mut header)?;
    let (machine, timestamp, optional_magic) = parse_pe(&header)?;
    let dos_header = header[..64].try_into().unwrap();
    let mut live_header = [0u8; 64];
    if windows_read(&expected.identity, expected.mapping.start, &mut live_header)?
        != live_header.len()
        || live_header != dos_header
    {
        return Err(invalid("mapped executable header mismatch"));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_EXECUTABLE_BYTES {
            return Err(invalid("executable exceeds bound"));
        }
        hash.update(&buffer[..n]);
    }
    let reopened = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)?;
    if total != before.2 || file_identity(&file)? != before || file_identity(&reopened)? != before {
        return Err(invalid("executable changed during fingerprint"));
    }
    ensure_same_candidate(expected, &candidate(expected.identity.pid)?)?;
    verify_mapped_path(expected, &file)?;
    Ok(PeMetadata {
        machine,
        timestamp,
        optional_magic,
        size: total,
        sha256: format!("{:x}", hash.finalize()),
        dos_header,
    })
}
pub(crate) fn windows_read(
    expected: &Identity,
    address: usize,
    bytes: &mut [u8],
) -> io::Result<usize> {
    let handle = open(expected.pid, true)?;
    if handle_identity(&handle, expected.pid)? != *expected {
        return Err(invalid("process identity changed"));
    }
    let mut received = 0;
    // SAFETY: the destination is an owned bounded slice; Windows validates the
    // foreign address. Only PROCESS_VM_READ was requested, never write rights.
    let success = unsafe {
        ReadProcessMemory(
            handle.0,
            address as _,
            bytes.as_mut_ptr().cast(),
            bytes.len(),
            &mut received,
        )
    };
    let error = io::Error::last_os_error();
    if handle_identity(&handle, expected.pid)? != *expected || identity(expected.pid)? != *expected
    {
        return Err(invalid("process identity changed"));
    }
    if success == 0 && (received == 0 || received == bytes.len()) {
        Err(error)
    } else {
        Ok(received)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn self_identity_and_mapping() {
        let pid = std::process::id() as i32;
        let id = identity(pid).unwrap();
        assert_eq!(identity(pid).unwrap(), id);
        assert_ne!(id.start_ticks, 0);
        let fixture = [1u8, 2, 3];
        let addr = fixture.as_ptr() as usize;
        assert!(
            mappings(pid)
                .unwrap()
                .iter()
                .any(|m| m.readable && m.start <= addr && addr + fixture.len() <= m.end)
        );
        let mut bytes = [0; 3];
        assert_eq!(windows_read(&id, addr, &mut bytes).unwrap(), 3);
        assert_eq!(bytes, fixture);
        let mut reused = id;
        reused.start_ticks += 1;
        assert!(windows_read(&reused, addr, &mut bytes).is_err());
    }
    #[test]
    fn mapped_path_accepts_only_known_nt_namespaces() {
        for (source, expected) in [
            (r"\??\C:\Games\DCGAME.EXE", r"\\?\C:\Games\DCGAME.EXE"),
            (
                r"\??\UNC\server\share\DCGAME.EXE",
                r"\\?\UNC\server\share\DCGAME.EXE",
            ),
            (
                r"\Device\HarddiskVolume3\Games\DCGAME.EXE",
                r"\\?\GLOBALROOT\Device\HarddiskVolume3\Games\DCGAME.EXE",
            ),
        ] {
            assert_eq!(
                mapped_path(&source.encode_utf16().collect::<Vec<_>>()).unwrap(),
                PathBuf::from(expected)
            );
        }
        for source in [
            "",
            r"C:\Games\DCGAME.EXE",
            r"\\server\share\DCGAME.EXE",
            r"\Unknown\DCGAME.EXE",
            r"\DeviceOther\DCGAME.EXE",
            r"\??\relative\DCGAME.EXE",
            r"\??\C:relative\DCGAME.EXE",
            r"\??\C:\games\..\DCGAME.EXE",
            r"\??\UNC\server",
            r"\Device\",
        ] {
            assert!(mapped_path(&source.encode_utf16().collect::<Vec<_>>()).is_err());
        }
    }
    #[test]
    fn self_image_file_and_mapped_path_match() {
        let pid = std::process::id() as i32;
        let handle = open(pid, false).unwrap();
        let path = image_path(&handle).unwrap();
        let mapping = main_mapping(pid, &path, &query_maps(&handle).unwrap()).unwrap();
        let candidate = Candidate {
            identity: identity(pid).unwrap(),
            mapping,
        };
        let file = File::open(&path).unwrap();
        verify_mapped_path(&candidate, &file).unwrap();
        let reopened = File::open(&path).unwrap();
        assert_eq!(
            file_identity(&file).unwrap(),
            file_identity(&reopened).unwrap()
        );
        assert!(file_identity(&file).unwrap().2 > 0);
        let temporary = std::env::temp_dir().join(format!(
            "dcuo-reader-other-file-{}-{}",
            pid,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let different = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&temporary)
            .unwrap();
        let rejected = verify_mapped_path(&candidate, &different).is_err();
        drop(different);
        fs::remove_file(temporary).unwrap();
        assert!(rejected, "a different backing file must be rejected");
        let mut wrong = candidate;
        wrong.mapping.start = 1;
        assert!(verify_mapped_path(&wrong, &file).is_err());
    }
    #[test]
    fn guard_and_execute_only_are_unreadable() {
        assert_eq!(
            protection(PAGE_READWRITE | PAGE_GUARD),
            (false, false, false)
        );
        assert_eq!(protection(PAGE_NOACCESS), (false, false, false));
        assert_eq!(protection(PAGE_EXECUTE), (false, false, true));
        assert_eq!(protection(PAGE_EXECUTE_READ), (true, false, true));
    }
}
