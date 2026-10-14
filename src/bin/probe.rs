use dcuo_companion::{memory, process};
use std::{
    io::{self, Read},
    time::{Duration, Instant},
};
const SEARCH_BYTES: usize = 64 * 1024 * 1024;
const SEARCH_HITS: usize = 16;
const SEARCH_TIME: Duration = Duration::from_secs(5);
fn patterns(input: &str) -> Result<[Vec<u8>; 2], Box<dyn std::error::Error>> {
    let name = input.strip_suffix('\n').unwrap_or(input);
    let name = name.strip_suffix('\r').unwrap_or(name);
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(
            "stdin reference must contain 1 to 64 non-control characters on one line".into(),
        );
    }
    Ok([
        name.as_bytes().to_vec(),
        name.encode_utf16().flat_map(u16::to_le_bytes).collect(),
    ])
}
fn count_matches(bytes: &[u8], pattern: &[u8], carry: usize, limit: usize) -> usize {
    bytes
        .windows(pattern.len())
        .enumerate()
        .filter(|(offset, window)| offset + pattern.len() > carry && *window == pattern)
        .take(limit)
        .count()
}
fn search_name(pid: i32) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin()
        .take(259)
        .read_to_string(&mut input)
        .map_err(|_| "stdin must be bounded UTF8")?;
    if input.len() > 258 {
        return Err("stdin reference exceeds bound".into());
    }
    let needles = patterns(&input)?;
    drop(input);
    let candidate = process::candidate(pid)?;
    let meta = process::executable_metadata(&candidate)?;
    let header = memory::read_exact(&candidate.identity, candidate.mapping.start, 64)?;
    if !meta.matches_dos_header(&header) {
        return Err("mapped DOS header differs from fingerprinted executable".into());
    }
    let maps = process::mappings(pid)?;
    let started = Instant::now();
    // Count the fixed access-validation header toward the total remote-read cap.
    let mut read_bytes = header.len();
    let mut hits = [0usize; 2];
    let mut stop = "readable-writable scope exhausted";
    let overlap = needles.iter().map(Vec::len).max().unwrap() - 1;
    'scan: for map in maps.iter().filter(|m| m.readable && m.writable) {
        let mut address = map.start;
        let mut carry = Vec::new();
        while address < map.end {
            if started.elapsed() >= SEARCH_TIME {
                stop = "soft time limit";
                break 'scan;
            }
            if read_bytes >= SEARCH_BYTES {
                stop = "byte limit";
                break 'scan;
            }
            let len = memory::MAX_READ_BYTES
                .min(map.end - address)
                .min(SEARCH_BYTES - read_bytes);
            let chunk = match memory::read_exact(&candidate.identity, address, len) {
                Ok(chunk) => chunk,
                Err(_) => {
                    stop = "read error or process changed; incomplete";
                    break 'scan;
                }
            };
            read_bytes += chunk.len();
            let carry_len = carry.len();
            carry.extend_from_slice(&chunk);
            drop(chunk);
            for (index, needle) in needles.iter().enumerate() {
                let remaining = SEARCH_HITS - hits.iter().sum::<usize>();
                hits[index] += count_matches(&carry, needle, carry_len, remaining);
            }
            if hits.iter().sum::<usize>() >= SEARCH_HITS {
                stop = "match limit";
                break 'scan;
            }
            let keep = overlap.min(carry.len());
            carry.drain(..carry.len() - keep);
            address += len;
        }
    }
    if process::identity(pid).ok().as_ref() != Some(&candidate.identity) {
        return Err("process changed; search result discarded".into());
    }
    println!(
        "bytes_read={read_bytes} utf8_hits={} utf16le_hits={} stop={stop}",
        hits[0], hits[1]
    );
    println!(
        "experimental exact-byte occurrences only; coverage bounded; identity ambiguous/unverified; no supported profile or verified character/location"
    );
    Ok(())
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["list"] => {
            let candidates = process::discover()?;
            println!("DCGAME.EXE candidates: {}", candidates.len());
            for c in candidates {
                println!(
                    "pid={} start_ticks={} mapped_game_header_candidate=yes",
                    c.identity.pid, c.identity.start_ticks
                );
            }
            println!("Select an explicit PID to inspect. No supported game profiles.");
        }
        ["inspect", "--pid", pid] => {
            let pid: i32 = pid.parse()?;
            if pid <= 0 {
                return Err("PID must be positive".into());
            }
            let c = process::candidate(pid)?;
            let meta = process::executable_metadata(&c)?;
            println!(
                "pid={} start_ticks={} size={} sha256={} PE_machine=0x{:04x} PE_timestamp={} optional_magic=0x{:04x}",
                pid,
                c.identity.start_ticks,
                meta.size,
                meta.sha256,
                meta.machine,
                meta.timestamp,
                meta.optional_magic
            );
            // Fixed DOS header read only. It establishes access, never field validity.
            let header = memory::read_exact(&c.identity, c.mapping.start, 64)?;
            if !meta.matches_dos_header(&header) {
                return Err("mapped DOS header differs from fingerprinted executable".into());
            }
            println!(
                "bounded_header_access=ok file_header_match=yes bytes=64; unsupported build: no memory profile; character/location unavailable"
            );
        }
        ["self-test"] => {
            memory::self_test()?;
            println!("synthetic self-owned process read passed; game access remains unverified");
        }
        ["search-name", "--pid", pid] => search_name(pid.parse()?)?,
        _ => return Err("usage: dcuo-probe list | inspect --pid PID | search-name --pid PID (reference on stdin) | self-test".into()),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("probe: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_bounds() {
        assert!(patterns("").is_err());
        assert!(patterns(&"a".repeat(65)).is_err());
        assert!(patterns("a\nb").is_err());
        assert_eq!(patterns("Test\n").unwrap()[1], b"T\0e\0s\0t\0");
    }
    #[test]
    fn overlap_and_hit_cap() {
        assert_eq!(count_matches(b"abcXYZabc", b"abc", 3, 16), 1);
        assert_eq!(count_matches(b"abXYZ", b"XYZ", 4, 16), 1);
        assert_eq!(count_matches(b"aaaaa", b"aa", 0, 2), 2);
        assert_eq!(count_matches(b"abc", b"abcdef", 0, 16), 0);
    }
}
