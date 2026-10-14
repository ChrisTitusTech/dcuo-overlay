use dcuo_companion::{memory, process};
use std::io::{self, Read};
fn search_name(pid: i32, batches: usize) -> Result<(), Box<dyn std::error::Error>> {
    if !(1..=128).contains(&batches) {
        return Err("batches must be 1 to 128".into());
    }
    let mut input = String::new();
    io::stdin()
        .take(259)
        .read_to_string(&mut input)
        .map_err(|_| "stdin must be bounded UTF8")?;
    if input.len() > 258 {
        return Err("stdin reference exceeds bound".into());
    }
    let name = input.strip_suffix('\n').unwrap_or(&input);
    let name = name.strip_suffix('\r').unwrap_or(name);
    let mut search = dcuo_companion::discovery::NameSearch::new(pid, name)?;
    let cancel = std::sync::atomic::AtomicBool::new(false);
    for batch in 1..=batches {
        let p = search.step(&cancel)?;
        println!(
            "batch={batch} bytes_scanned={} total_bytes={} utf8_hits={} utf16le_hits={} candidate_count={} done={} status={}",
            p.bytes_scanned,
            p.total_bytes,
            p.utf8_hits,
            p.utf16_hits,
            p.candidate_count,
            p.done,
            p.status
        );
        if p.done {
            break;
        }
    }
    println!(
        "experimental string occurrences only; active-character identity unverified; no supported character/location profile"
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
        ["search-name", "--pid", pid] => search_name(pid.parse()?, 1)?,
        ["search-name", "--pid", pid, "--batches", batches] => search_name(pid.parse()?, batches.parse()?)?,
        _ => return Err("usage: dcuo-probe list | inspect --pid PID | search-name --pid PID [--batches 1..128] (reference on stdin) | self-test".into()),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("probe: {e}");
        std::process::exit(1);
    }
}
