# Contributing

This repository contains runnable Phase 0 feasibility prototypes.
Continue the Phase 0 experiments in ROADMAP.md. Read SPEC.md and TASKS.md before
implementation and record any scope changes explicitly.

## Development setup

The prototypes require Linux and Rust 1.95 or newer because of eframe 0.36.2.
Rust/Cargo 1.96.0, rustfmt, and Clippy were validated on Fedora 44. The window uses
X11 and OpenGL; a working X11 display, graphics driver, and native linker are
required. The development host already had the necessary system libraries and
tools, so no package installation or permission changes were needed.

Build the window and memory probes with `cargo build --locked --workspace`.
For only the memory probe, use
`cargo build --locked --no-default-features --bin dcuo-probe`.
The lockfile fixes dependency versions; no project license is selected yet and
the package is marked `publish = false`.

Do not assume every Wine launcher or
Wayland compositor is supported. Live validation needs access to the intended
Linux desktop, two monitors for the primary companion-window setup, and a running
DCUO session.

## Validation

For documentation changes, check links, scope consistency, and whitespace.
The baseline checks are:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
```

These commands pass for the current prototypes. Add feature-specific checks as
needed. UI and memory changes also
need the applicable live acceptance evidence from ROADMAP.md.
Location/map work also needs controlled outdoor and indoor/instance routes,
independent calibration landmarks, floor/zone transition checks, and documented
asset provenance. Synthetic markers and images are useful fixtures, not proof
of live map coverage. UI changes need visual checks of every affected tab's
loading, empty, stale, and error states with sanitized data.

Report failed or skipped gates and their blockers. Do not use synthetic fixtures
as proof that live memory layouts or managed companion-window behavior work.

## Bounded experiments

```sh
target/debug/dcuo-probe self-test
target/debug/dcuo-probe list
target/debug/dcuo-probe inspect --pid PID
target/debug/dcuo-window-probe --smoke-seconds 15 --position 80 80
python3 scripts/check_x11.py --position 80 80
```

Replace `PID` with an explicitly selected candidate from `list`. Discovery can
skip inaccessible or racing processes; an empty list is not proof the game is
absent. `inspect` fingerprints the actual mapped game executable and compares a
fixed 64-byte remote DOS header with that file. It prints metadata, never a memory
dump. This establishes access only; all game builds remain unsupported for
character/location observations until a profile is independently proven.

`--position` is a physical-pixel startup hint, not a persistent DWM placement rule.
Choose coordinates from the actual desktop; DWM may override them. The X11 check
requires Python 3, `xdotool`, `xprop`, and `xwininfo`. It opens and closes two probe
windows, checks their attributes, and verifies bounded shutdown. It does not test
fullscreen gameplay, internal window recreation, or monitor recovery. On a nested
X server it provides synthetic window evidence only.

For a bounded discovery experiment:

```sh
cargo build --locked --release --no-default-features --bin dcuo-probe
target/release/dcuo-probe search-name --pid PID
```

Enter the known character name on stdin, then EOF (Enter followed by Ctrl-D in a
terminal). Do not put personal references in committed scripts or fixtures. The
probe checks UTF-8 and UTF-16LE occurrences in readable/writable mappings, with
4 KiB reads, a 64 MiB total remote-read cap, a 16-match cap, and a soft five-second
scan deadline checked between reads. Executable fingerprinting happens before
that deadline. Output contains counts and a stop reason, not the reference,
addresses, or raw data. A hit can be a cached string, and a bounded miss is
inconclusive. Read errors end the scan and process identity changes invalidate
its result. No search result enables a memory profile or changes the UI.

Keep local captures under ignored `captures/` and other private evidence under
`local-data/`. Do not commit personal screenshots, raw dumps, or game binaries.
See [Phase 0 evidence](docs/phase-0.md) and [map research](docs/map-feasibility.md)
for the next bounded experiments and unfulfilled live gates.

## Changes and publication

Keep changes small, preserve unrelated work, and update task status only after
acceptance criteria pass. Review external licenses before reusing code or assets.
Never publish private snapshots or raw process dumps. Pushes, pull requests, and
releases require explicit authorization from the repository owner.
