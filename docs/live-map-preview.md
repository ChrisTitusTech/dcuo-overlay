# Live map preview

The companion now has a working experimental map preview for the observed client
build. This is a vertical slice of Phases 1, 2, 3, 4 and 5, not completion of their
acceptance gates. Current-character identity and the multi-launch memory profile
remain unverified. API references retain their independent source labels.

## Implemented behavior

- `dcuo-companion` opens the normal managed X11 application. The old
  `dcuo-window-probe` entry point remains for existing smoke commands.
- One bounded read-only sampler follows the reflected local-player/controller/pawn
  chain. It requires the exact executable hash, PE metadata and expected reflected
  field schemas. Unsupported builds fail closed. Health, power and position are
  explicitly experimental, with age and unavailable states.
- Area selection uses visible loaded audio and designer streaming packages.
  Persistent parent-world names are not treated as the current indoor area.
  Only Gotham (`MGOT101`) and Old Gotham Subway (`MPYR0224`) are recognized.
- Map artwork is imported from the user's installed client into private local
  files. No proprietary images or package data are bundled or committed. The
  bounded importer verifies package structures, tile grids, decompression sizes,
  cache hashes and image checksums. See [format and provenance](local-map-import.md).
- The Map tab provides local browsing, live experimental markers, zoom, pan,
  fit and follow/recenter. Context, age, session changes and pause state gate the
  marker. Unknown areas have no marker. These are overview layers; separate floors
  and other instances are not yet supported.
- Settings, theme, scale, UI refresh interval and map catalog persist in XDG
  configuration. Corrupt preferences recover to defaults with an explanation.
  Automatic reconnect is not implemented and its control is disabled.
- API calls use a separate bounded worker, so a delayed reference request cannot
  stall local sampling. Shutdown joins the workers; HTTP has an eight-second
  bound. Sanitized diagnostics exclude names, positions, process IDs and URLs.

## Observed evidence

On Fedora 44, DWM-Titus 0.7.2, X11/Picom and GE-Proton 10-34, the managed companion
was displayed on HDMI-0 while the game ran under Gamescope on DP-0. The real window
retained normal type, stable class and `override_redirect=false`.

Local import decoded all 600 Gotham tiles and all six indoor tiles, assembled
consistent artwork and passed catalog/checksum loading. Both packages specify
100 world units per source pixel. Tile indices require transposition and reversed
row placement, with padding retained. Gotham is downsampled by two to bound GPU
and CPU image memory.

A paired live observation at the Old Gotham Subway entrance in Gotham placed the
companion marker at the same edge and neighboring buildings as the game's marker.
Zoom and Follow were exercised in the real companion window. The user then
confirmed that normal movement is tracked in the correct place. A live Pause
check removed the marker immediately. The game then changed to an unsupported
instance: resources recovered but the marker correctly remained unavailable. Warm native samples
with the outdoor streaming array took approximately 0.22-0.24 seconds; first-sample
schema discovery took about 1.3 seconds. The initial implementation consumed approximately 85% of one CPU core; this is
not acceptable as a polling baseline. After bounded scatter reads, warm samples in a different indoor area took
39-41 ms in the debug build and 9-10 ms in the optimized build. Outdoor and
extended-session comparison remains pending. The optimization target is warm samples
below 50 ms and average companion CPU below 10% of one core at four samples per
second, with fresh marker age below one second. These are provisional targets,
not passed acceptance or an extended-session performance bound. Indoor artwork matches the in-game layout;
indoor live-marker accuracy still needs an independently paired observation.

Personal screenshots, coordinates, memory observations and imported assets are
kept outside Git. No game memory writes, injection or gameplay automation are used.

## Automated checks

The integrated slice passed `cargo fmt --all -- --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`,
`cargo test --locked --workspace` (70 passed, one opt-in local import ignored),
`cargo build --locked --workspace`, the optimized workspace build, and
`cargo test --locked --no-default-features --lib` (58 passed).
`xvfb-run -a python3 scripts/check_x11.py` passed two normal-window launches and
clean exits. Rust 1.95 minimum-version check and strict all-target Clippy also
passed. The opt-in importer was separately exercised twice with installed
assets; private results are not fixtures. Independent review and remote CI are
recorded in the PR rather than inferred from these local checks.

## Remaining acceptance

- Multiple independent landmarks in both areas and indoor movement comparison.
- Indoor/outdoor switching, repeated instance entry, floor/teleport behavior and
  three fresh game launches, including unsupported-build rejection.
- Current-character identity binding and per-field semantic/API comparisons.
- Full monitor, fullscreen, compositor, keyboard and display-recovery matrix.
- Extended session, measured overhead, automatic reconnect and release packaging.
- Project license selection before release distribution. No release is published.

## Reproduce locally

```bash
cargo build --locked --workspace
cargo run --locked --bin dcuo-companion
```

Choose the game session and Connect. In Map, expand the installed-map importer,
confirm the local CookedPC directory, and import. Imported images remain private
local data. The marker is labeled experimental; a matching API name does not
promote it or any other memory field to verified.
