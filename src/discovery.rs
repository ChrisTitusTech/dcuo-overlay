//! Resumable bounded string-occurrence discovery, never a character profile.
use crate::{
    memory,
    process::{self, Candidate, Mapping},
};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
const BATCH: u64 = 64 * 1024 * 1024;
const LIFETIME: u64 = 2 * 1024 * 1024 * 1024;
const CHUNK: usize = 65536;
const CANDIDATES: usize = 16;
#[derive(Clone, Debug)]
pub struct Progress {
    pub bytes_scanned: u64,
    pub total_bytes: u64,
    pub utf8_hits: usize,
    pub utf16_hits: usize,
    pub candidate_count: usize,
    pub done: bool,
    pub status: String,
    pub observed_name: Option<String>,
}
pub struct NameSearch {
    candidate: Candidate,
    maps: Vec<Mapping>,
    index: usize,
    address: usize,
    carry: Vec<u8>,
    patterns: [Vec<u8>; 2],
    addresses: Vec<usize>,
    progress: Progress,
}
fn eligible_maps(pid: i32) -> Result<Vec<Mapping>, String> {
    let raw = process::bounded_text(Path::new(&format!("/proc/{pid}/maps")))
        .map_err(|e| e.to_string())?;
    let maps = process::parse_maps(&raw).map_err(|e| e.to_string())?;
    Ok(maps
        .into_iter()
        .zip(raw.lines())
        .filter_map(|(map, line)| {
            let private = line.split_whitespace().nth(1) == Some("rw-p");
            (private
                && map.inode == 0
                && map.device == "00:00"
                && map.path.as_deref().is_none_or(|p| p == Path::new("[heap]")))
            .then_some(map)
        })
        .collect())
}
fn patterns(name: &str) -> Result<[Vec<u8>; 2], String> {
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err("reference must contain 1 to 64 non-control characters".into());
    }
    Ok([
        name.as_bytes().to_vec(),
        name.encode_utf16().flat_map(u16::to_le_bytes).collect(),
    ])
}
impl NameSearch {
    pub fn new(pid: i32, name: &str) -> Result<Self, String> {
        let patterns = patterns(name)?;
        let candidate = process::candidate(pid).map_err(|e| e.to_string())?;
        let meta = process::executable_metadata(&candidate).map_err(|e| e.to_string())?;
        let header = memory::read_exact(&candidate.identity, candidate.mapping.start, 64)
            .map_err(|e| e.to_string())?;
        if !meta.matches_dos_header(&header) {
            return Err("mapped header differs from executable fingerprint".into());
        }
        let maps = eligible_maps(pid)?;
        let total_bytes = maps
            .iter()
            .map(|m| (m.end - m.start) as u64)
            .sum::<u64>()
            .min(LIFETIME);
        let address = maps.first().map_or(0, |m| m.start);
        Ok(Self {
            candidate,
            maps,
            index: 0,
            address,
            carry: Vec::new(),
            patterns,
            addresses: Vec::new(),
            progress: Progress {
                bytes_scanned: 0,
                total_bytes,
                utf8_hits: 0,
                utf16_hits: 0,
                candidate_count: 0,
                done: false,
                status: "Ready for bounded string-occurrence search; no supported profile".into(),
                observed_name: None,
            },
        })
    }
    pub fn progress(&self) -> Progress {
        self.progress.clone()
    }
    fn validate(&self) -> Result<(), String> {
        let current = process::candidate(self.candidate.identity.pid).map_err(|e| e.to_string())?;
        if current.identity != self.candidate.identity || current.mapping != self.candidate.mapping
        {
            return Err("process or executable mapping changed".into());
        }
        // A changed eligible scope invalidates the cursor instead of retargeting.
        if eligible_maps(current.identity.pid)? != self.maps {
            return Err("search mappings changed; restart required".into());
        }
        Ok(())
    }
    fn invalidate(&mut self, status: &str) {
        self.carry.clear();
        self.addresses.clear();
        self.index = self.maps.len();
        self.address = 0;
        self.progress.utf8_hits = 0;
        self.progress.utf16_hits = 0;
        self.progress.candidate_count = 0;
        self.progress.observed_name = None;
        self.progress.done = true;
        self.progress.status = status.into();
    }
    pub fn step(&mut self, cancel: &AtomicBool) -> Result<Progress, String> {
        if self.progress.done {
            return Ok(self.progress());
        }
        let result = self.scan(cancel);
        if let Err(e) = &result {
            self.invalidate(&format!("Search invalidated: {e}"));
        }
        result.map(|()| self.progress())
    }
    fn scan(&mut self, cancel: &AtomicBool) -> Result<(), String> {
        if cancel.load(Ordering::Relaxed) {
            self.invalidate("Search cancelled; occurrences discarded");
            return Ok(());
        }
        self.validate()?;
        let start = Instant::now();
        let mut read = 0u64;
        let mut revalidate_at = 1024 * 1024;
        while self.index < self.maps.len()
            && self.progress.bytes_scanned < self.progress.total_bytes
        {
            if cancel.load(Ordering::Relaxed) {
                self.invalidate("Search cancelled; occurrences discarded");
                return Ok(());
            }
            if read >= BATCH || start.elapsed() >= Duration::from_secs(1) {
                break;
            }
            if read >= revalidate_at {
                self.validate()?;
                revalidate_at = read + 1024 * 1024;
            }
            let map = &self.maps[self.index];
            let len = CHUNK
                .min(map.end - self.address)
                .min((BATCH - read) as usize)
                .min((self.progress.total_bytes - self.progress.bytes_scanned) as usize);
            let chunk = memory::read_discovery(&self.candidate.identity, map, self.address, len)
                .map_err(|e| e.to_string())?;
            self.consume(&chunk);
            self.address += len;
            read += len as u64;
            self.progress.bytes_scanned += len as u64;
            if self.address == self.maps[self.index].end {
                self.index += 1;
                self.carry.clear();
                self.address = self.maps.get(self.index).map_or(0, |m| m.start);
            }
            if self.addresses.len() == CANDIDATES {
                break;
            }
        }
        self.validate()?;
        self.progress.candidate_count = self.addresses.len();
        self.progress.done = self.addresses.len() == CANDIDATES
            || self.index == self.maps.len()
            || self.progress.bytes_scanned == self.progress.total_bytes;
        self.progress.status = if self.addresses.len() == CANDIDATES {
            "String occurrence limit reached; identity unverified"
        } else if self.progress.done {
            "Bounded scope complete; string occurrences do not prove active character"
        } else {
            "Batch complete; cursor retained; string occurrences only"
        }
        .into();
        Ok(())
    }
    fn consume(&mut self, chunk: &[u8]) {
        let carry_len = self.carry.len();
        self.carry.extend_from_slice(chunk);
        let base = self.address - carry_len;
        for (encoding, needle) in self.patterns.iter().enumerate() {
            for (offset, window) in self.carry.windows(needle.len()).enumerate() {
                if offset + needle.len() <= carry_len
                    || window != needle
                    || self.addresses.len() >= CANDIDATES
                {
                    continue;
                }
                let address = base + offset;
                if self.addresses.contains(&address) {
                    continue;
                }
                self.addresses.push(address);
                if encoding == 0 {
                    self.progress.utf8_hits += 1;
                } else {
                    self.progress.utf16_hits += 1;
                }
                // Decode the actual matched remote bytes, never just echo reference input.
                self.progress.observed_name = if encoding == 0 {
                    String::from_utf8(window.to_vec()).ok()
                } else {
                    String::from_utf16(
                        &window
                            .chunks_exact(2)
                            .map(|p| u16::from_le_bytes([p[0], p[1]]))
                            .collect::<Vec<_>>(),
                    )
                    .ok()
                };
            }
        }
        let keep = (self.patterns.iter().map(Vec::len).max().unwrap() - 1).min(self.carry.len());
        self.carry.drain(..self.carry.len() - keep);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn synthetic(name: &str) -> NameSearch {
        NameSearch {
            candidate: Candidate {
                identity: process::Identity {
                    pid: 1,
                    start_ticks: 1,
                },
                mapping: process::parse_maps("1000-2000 rw-p 0 00:00 0")
                    .unwrap()
                    .remove(0),
            },
            maps: Vec::new(),
            index: 0,
            address: 4096,
            carry: Vec::new(),
            patterns: patterns(name).unwrap(),
            addresses: Vec::new(),
            progress: Progress {
                bytes_scanned: 0,
                total_bytes: 0,
                utf8_hits: 0,
                utf16_hits: 0,
                candidate_count: 0,
                done: false,
                status: String::new(),
                observed_name: None,
            },
        }
    }
    #[test]
    fn overlap_unique_and_decoded() {
        let mut s = synthetic("Alpha");
        s.consume(b"xxAl");
        s.address += 4;
        s.consume(b"phaxxAlpha");
        assert_eq!(s.progress.utf8_hits, 2);
        assert_eq!(s.addresses, vec![4098, 4105]);
        assert_eq!(s.progress.observed_name.as_deref(), Some("Alpha"));
    }
    #[test]
    fn retained_overlap_does_not_rescan_completed_matches() {
        let mut s = synthetic("Alpha");
        s.consume(b"Alpha");
        s.address += 5;
        s.consume(b"xx");
        assert_eq!(s.progress.utf8_hits, 1);
        assert_eq!(s.addresses, vec![4096]);
        s.address += 2;
        s.consume(b"Alpha");
        assert_eq!(s.addresses, vec![4096, 4103]);
    }
    #[test]
    fn cap_and_cancellation_clear_observations() {
        let mut s = synthetic("aa");
        s.consume(&[b'a'; 100]);
        assert_eq!(s.addresses.len(), CANDIDATES);
        let progress = s.step(&AtomicBool::new(true)).unwrap();
        assert!(progress.done);
        assert_eq!(progress.utf8_hits, 0);
        assert!(progress.observed_name.is_none());
        assert_eq!(s.address, 0);
    }
    #[test]
    fn invalidation_discards_results() {
        let mut s = synthetic("Alpha");
        s.consume(b"Alpha");
        s.invalidate("changed");
        assert!(s.addresses.is_empty());
        assert!(s.progress.observed_name.is_none());
    }
    #[test]
    fn utf16_split_chunk_decodes_actual_occurrence() {
        let mut s = synthetic("Omega");
        let encoded: Vec<u8> = "Omega".encode_utf16().flat_map(u16::to_le_bytes).collect();
        s.consume(&encoded[..3]);
        s.address += 3;
        s.consume(&encoded[3..]);
        assert_eq!(s.progress.utf16_hits, 1);
        assert_eq!(s.progress.utf8_hits, 0);
        assert_eq!(s.progress.observed_name.as_deref(), Some("Omega"));
    }
    #[test]
    fn reference_bounds() {
        assert!(patterns("").is_err());
        assert!(patterns("a\nb").is_err());
        assert!(patterns(&"a".repeat(65)).is_err());
    }
}
