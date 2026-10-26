//! Process identity and bounded mapped-PE metadata discovery.
#[cfg(windows)]
#[path = "process_windows.rs"]
mod windows;
#[cfg(target_os = "linux")]
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};
#[cfg(windows)]
pub(crate) use windows::windows_read;
#[cfg(windows)]
pub use windows::{candidate, discover, executable_metadata, identity, mappings};

#[cfg(target_os = "linux")]
use std::{
    fs,
    io::{Seek, SeekFrom},
};

pub const MAX_PROC_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub pid: i32,
    /// Linux start ticks or Windows process creation FILETIME, scoped to PID.
    pub start_ticks: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub start: usize,
    pub end: usize,
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
    pub offset: u64,
    pub device: String,
    pub inode: u64,
    pub path: Option<PathBuf>,
}
#[derive(Debug)]
pub struct Candidate {
    pub identity: Identity,
    pub mapping: Mapping,
}
#[derive(Debug)]
pub struct PeMetadata {
    pub machine: u16,
    pub timestamp: u32,
    pub optional_magic: u16,
    pub size: u64,
    pub sha256: String,
    dos_header: [u8; 64],
}
impl PeMetadata {
    pub fn matches_dos_header(&self, header: &[u8]) -> bool {
        header == self.dos_header
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
pub fn bounded_text(path: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_PROC_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PROC_BYTES {
        return Err(invalid("proc metadata exceeds bound"));
    }
    String::from_utf8(bytes).map_err(|_| invalid("non-UTF8 proc metadata"))
}
pub fn parse_stat(pid: i32, text: &str) -> io::Result<Identity> {
    let open = text.find('(').ok_or_else(|| invalid("missing stat comm"))?;
    if text[..open].trim().parse::<i32>().ok() != Some(pid) {
        return Err(invalid("stat PID mismatch"));
    }
    let close = text
        .rfind(')')
        .ok_or_else(|| invalid("missing stat comm terminator"))?;
    if close <= open {
        return Err(invalid("invalid stat comm delimiters"));
    }
    let start_ticks = text[close + 1..]
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| invalid("short stat"))?
        .parse()
        .map_err(|_| invalid("invalid starttime"))?;
    if pid <= 0 {
        return Err(invalid("invalid PID"));
    }
    Ok(Identity { pid, start_ticks })
}
#[cfg(target_os = "linux")]
pub fn identity(pid: i32) -> io::Result<Identity> {
    parse_stat(pid, &bounded_text(Path::new(&format!("/proc/{pid}/stat")))?)
}
pub fn parse_maps(text: &str) -> io::Result<Vec<Mapping>> {
    text.lines()
        .map(|line| {
            // Keep paths containing spaces intact after the five fixed fields.
            let mut rest = line;
            let mut fields = [""; 5];
            for field in &mut fields {
                rest = rest.trim_start();
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                if end == 0 {
                    return Err(invalid("short maps line"));
                }
                *field = &rest[..end];
                rest = &rest[end..];
            }
            let (start, end) = fields[0]
                .split_once('-')
                .ok_or_else(|| invalid("bad map range"))?;
            let start = usize::from_str_radix(start, 16).map_err(|_| invalid("bad map start"))?;
            let end = usize::from_str_radix(end, 16).map_err(|_| invalid("bad map end"))?;
            let perms = fields[1].as_bytes();
            if start >= end
                || perms.len() != 4
                || !matches!(perms[0], b'r' | b'-')
                || !matches!(perms[1], b'w' | b'-')
                || !matches!(perms[2], b'x' | b'-')
                || !matches!(perms[3], b'p' | b's')
            {
                return Err(invalid("invalid map permissions/range"));
            }
            let offset =
                u64::from_str_radix(fields[2], 16).map_err(|_| invalid("bad map offset"))?;
            let (major, minor) = fields[3]
                .split_once(':')
                .ok_or_else(|| invalid("bad map device"))?;
            u32::from_str_radix(major, 16).map_err(|_| invalid("bad map device"))?;
            u32::from_str_radix(minor, 16).map_err(|_| invalid("bad map device"))?;
            let inode = fields[4].parse().map_err(|_| invalid("bad map inode"))?;
            let path = rest.trim_start();
            Ok(Mapping {
                start,
                end,
                readable: perms[0] == b'r',
                writable: perms[1] == b'w',
                executable: perms[2] == b'x',
                offset,
                device: fields[3].into(),
                inode,
                path: (!path.is_empty()).then(|| PathBuf::from(path)),
            })
        })
        .collect()
}
#[cfg(target_os = "linux")]
pub fn mappings(pid: i32) -> io::Result<Vec<Mapping>> {
    parse_maps(&bounded_text(Path::new(&format!("/proc/{pid}/maps")))?)
}
#[cfg(target_os = "linux")]
pub fn candidate(pid: i32) -> io::Result<Candidate> {
    let before = identity(pid)?;
    let comm = bounded_text(Path::new(&format!("/proc/{pid}/comm")))?;
    let maps = mappings(pid)?;
    let mapping = select_game_mapping(&comm, &maps)?;
    if before != identity(pid)? {
        return Err(invalid("process identity changed"));
    }
    Ok(Candidate {
        identity: before,
        mapping,
    })
}
#[cfg(any(target_os = "linux", test))]
fn select_game_mapping(comm: &str, maps: &[Mapping]) -> io::Result<Mapping> {
    if !comm.trim_end().eq_ignore_ascii_case("DCGAME.EXE") {
        return Err(invalid("process comm is not DCGAME.EXE"));
    }
    let matches: Vec<_> = maps
        .iter()
        .filter(|m| {
            m.offset == 0
                && m.readable
                && m.inode != 0
                && m.path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("DCGAME.EXE"))
        })
        .collect();
    if matches.len() != 1 {
        return Err(invalid("missing or ambiguous mapped DCGAME.EXE"));
    }
    // Wine can map the DOS header from the file and executable PE sections
    // anonymously. The exact named header is a candidate, not a profile proof.
    Ok(matches[0].clone())
}
#[cfg(target_os = "linux")]
pub fn discover() -> io::Result<Vec<Candidate>> {
    let mut candidates = Vec::new();
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        if let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<i32>().ok())
            && let Ok(c) = candidate(pid)
        {
            candidates.push(c);
        }
    }
    candidates.sort_by_key(|c| c.identity.pid);
    Ok(candidates)
}
pub fn parse_pe(header: &[u8]) -> io::Result<(u16, u32, u16)> {
    if header.len() < 64 || &header[..2] != b"MZ" {
        return Err(invalid("not a DOS/PE header"));
    }
    let offset = u32::from_le_bytes(header[60..64].try_into().unwrap()) as usize;
    let end = offset
        .checked_add(26)
        .ok_or_else(|| invalid("PE offset overflow"))?;
    if end > header.len() || &header[offset..offset + 4] != b"PE\0\0" {
        return Err(invalid("missing bounded PE header"));
    }
    let machine = u16::from_le_bytes(header[offset + 4..offset + 6].try_into().unwrap());
    let stamp = u32::from_le_bytes(header[offset + 8..offset + 12].try_into().unwrap());
    let optional_size = u16::from_le_bytes(header[offset + 20..offset + 22].try_into().unwrap());
    let magic = u16::from_le_bytes(header[offset + 24..offset + 26].try_into().unwrap());
    if optional_size < 2 || !matches!((machine, magic), (0x14c, 0x10b) | (0x8664, 0x20b)) {
        return Err(invalid("unsupported PE architecture/header"));
    }
    Ok((machine, stamp, magic))
}
#[cfg(target_os = "linux")]
pub fn executable_metadata(candidate: &Candidate) -> io::Result<PeMetadata> {
    use std::os::unix::fs::MetadataExt;
    revalidate_candidate(candidate)?;
    let map = &candidate.mapping;
    // map_files resolves the exact mapped inode; namespace path fallback must match device/inode.
    let exact = format!(
        "/proc/{}/map_files/{:x}-{:x}",
        candidate.identity.pid, map.start, map.end
    );
    let mut file = match File::open(exact) {
        Ok(f) => f,
        Err(_) => {
            let path = map
                .path
                .as_ref()
                .ok_or_else(|| invalid("missing executable path"))?;
            File::open(
                PathBuf::from(format!("/proc/{}/root", candidate.identity.pid)).join(
                    path.strip_prefix("/")
                        .map_err(|_| invalid("nonabsolute executable path"))?,
                ),
            )?
        }
    };
    let before = file.metadata()?;
    let device = format!(
        "{:02x}:{:02x}",
        libc::major(before.dev()),
        libc::minor(before.dev())
    );
    if before.ino() != map.inode
        || device != map.device
        || !before.is_file()
        || before.len() > MAX_EXECUTABLE_BYTES
    {
        return Err(invalid("mapped executable identity or size mismatch"));
    }
    let mut header = vec![0; (before.len().min(65536)) as usize];
    file.read_exact(&mut header)?;
    let (machine, timestamp, optional_magic) = parse_pe(&header)?;
    let dos_header = header[..64].try_into().unwrap();
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
            return Err(invalid("executable grew beyond bound"));
        }
        hash.update(&buffer[..n]);
    }
    let after = file.metadata()?;
    if total != before.len()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
        || identity(candidate.identity.pid)? != candidate.identity
    {
        return Err(invalid("executable/process changed during fingerprint"));
    }
    revalidate_candidate(candidate)?;
    Ok(PeMetadata {
        machine,
        timestamp,
        optional_magic,
        size: total,
        sha256: format!("{:x}", hash.finalize()),
        dos_header,
    })
}
fn ensure_same_candidate(expected: &Candidate, current: &Candidate) -> io::Result<()> {
    if expected.identity != current.identity || expected.mapping != current.mapping {
        return Err(invalid("process or mapped game header changed"));
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn revalidate_candidate(expected: &Candidate) -> io::Result<()> {
    // candidate() checks the comm, unique current header and identity around maps.
    // Compare every mapping attribute, including permissions, before accepting
    // a pathname fallback or returning a completed fingerprint.
    let current = candidate(expected.identity.pid)?;
    ensure_same_candidate(expected, &current)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_candidate_mapping_is_rejected() {
        let mapping = parse_maps("10000-11000 r--p 0 08:01 42 /synthetic/DCGAME.EXE")
            .unwrap()
            .remove(0);
        let expected = Candidate {
            identity: Identity {
                pid: 7,
                start_ticks: 42,
            },
            mapping,
        };
        let mut current = Candidate {
            identity: expected.identity.clone(),
            mapping: expected.mapping.clone(),
        };
        assert!(ensure_same_candidate(&expected, &current).is_ok());
        current.mapping.start += 4096;
        assert!(ensure_same_candidate(&expected, &current).is_err());
        current.mapping = expected.mapping.clone();
        current.mapping.end += 4096;
        assert!(ensure_same_candidate(&expected, &current).is_err());
        current.mapping = expected.mapping.clone();
        current.mapping.inode += 1;
        assert!(ensure_same_candidate(&expected, &current).is_err());
        current.mapping = expected.mapping.clone();
        current.mapping.offset = 4096;
        assert!(ensure_same_candidate(&expected, &current).is_err());
        current.mapping = expected.mapping.clone();
        current.mapping.path = Some(PathBuf::from("/other/DCGAME.EXE"));
        assert!(ensure_same_candidate(&expected, &current).is_err());
        current.mapping = expected.mapping.clone();
        current.mapping.device = "08:02".into();
        assert!(ensure_same_candidate(&expected, &current).is_err());
        current.mapping = expected.mapping.clone();
        current.mapping.readable = false;
        assert!(ensure_same_candidate(&expected, &current).is_err());
        current.mapping = expected.mapping.clone();
        current.identity.start_ticks += 1;
        assert!(ensure_same_candidate(&expected, &current).is_err());
    }
    #[test]
    fn wine_header_with_anonymous_code_is_candidate() {
        let maps = parse_maps(
            "10000-11000 r--p 0 08:01 42 /synthetic/DCGAME.EXE\n11000-20000 r-xp 0 00:00 0",
        )
        .unwrap();
        let selected = select_game_mapping("DCGAME.EXE\n", &maps).unwrap();
        assert_eq!(selected.start, 0x10000);
        assert!(!selected.executable);
        assert!(select_game_mapping("wine-preloader\n", &maps).is_err());
        let mut ambiguous = maps.clone();
        ambiguous.push(maps[0].clone());
        assert!(select_game_mapping("DCGAME.EXE\n", &ambiguous).is_err());
        assert!(select_game_mapping("DCGAME.EXE\n", &maps[1..]).is_err());
    }
    #[test]
    fn dos_header_match_requires_all_bytes() {
        let mut header = [0u8; 64];
        header[..2].copy_from_slice(b"MZ");
        let meta = PeMetadata {
            machine: 0x8664,
            timestamp: 0,
            optional_magic: 0x20b,
            size: 128,
            sha256: String::new(),
            dos_header: header,
        };
        assert!(meta.matches_dos_header(&header));
        header[12] = 1;
        assert!(!meta.matches_dos_header(&header));
        assert!(!meta.matches_dos_header(b"MZ"));
    }
    #[test]
    fn stat_with_parentheses() {
        let mut fields = vec!["0"; 20];
        fields[0] = "S";
        fields[19] = "123";
        assert_eq!(
            parse_stat(7, &format!("7 (a ) b) {}", fields.join(" ")))
                .unwrap()
                .start_ticks,
            123
        );
        assert!(parse_stat(8, "7 (a) S").is_err());
        assert!(parse_stat(7, "7 (a) S").is_err());
    }
    #[test]
    fn stat_starttime_boundary_and_trailing_fields() {
        let mut fields = ["0"; 20];
        fields[0] = "S";
        fields[19] = "18446744073709551615";
        let stat = format!("7 (name) {} ignored trailing fields", fields.join("\t"));
        assert_eq!(parse_stat(7, &stat).unwrap().start_ticks, u64::MAX);
        assert_eq!(
            parse_stat(7, &format!("7 (name) {}", fields[..19].join(" ")))
                .unwrap_err()
                .to_string(),
            "short stat"
        );
        fields[19] = "18446744073709551616";
        assert_eq!(
            parse_stat(7, &format!("7 (name) {}", fields.join(" ")))
                .unwrap_err()
                .to_string(),
            "invalid starttime"
        );
    }
    #[test]
    fn maps_fixed_fields_preserve_path_and_missing_field_errors() {
        for line in ["10-20", "10-20 r--p", "10-20 r--p 0", "10-20 r--p 0 00:00"] {
            assert_eq!(parse_maps(line).unwrap_err().to_string(), "short maps line");
        }
        let maps = parse_maps(
            "\t10-20\trw-p  0\t00:00 0\t \n20-30 r--s 10 08:01 42   /tmp/a  b (deleted)",
        )
        .unwrap();
        assert!(maps[0].path.is_none());
        assert!(maps[0].readable && maps[0].writable && !maps[0].executable);
        assert_eq!(
            maps[1].path.as_deref(),
            Some(Path::new("/tmp/a  b (deleted)"))
        );
        assert_eq!(maps[1].offset, 0x10);
        assert_eq!(maps[1].inode, 42);
    }
    #[test]
    fn malformed_maps() {
        assert!(parse_maps("20-10 rw-p 0 00:00 0").is_err());
        assert!(parse_maps("10-20 rw-q 0 00:00 0").is_err());
        assert!(parse_maps("10-20 r--p 0 bad 0").is_err());
        let m = parse_maps("10-20 r-xp 0 08:01 42 /tmp/with space/DCGAME.EXE").unwrap();
        assert_eq!(
            m[0].path.as_deref(),
            Some(Path::new("/tmp/with space/DCGAME.EXE"))
        );
    }
    #[test]
    fn malformed_pe() {
        assert!(parse_pe(&[0; 64]).is_err());
        let mut h = vec![0; 128];
        h[..2].copy_from_slice(b"MZ");
        h[60..64].copy_from_slice(&64u32.to_le_bytes());
        h[64..68].copy_from_slice(b"PE\0\0");
        h[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
        h[84..86].copy_from_slice(&2u16.to_le_bytes());
        h[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        assert_eq!(parse_pe(&h).unwrap().0, 0x8664);
        h[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_pe(&h).is_err());
    }
}
