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
  A unique bounded area key with matching designer evidence is required; the
  former two-area allowlist is removed.
- Map artwork is imported from the user's installed client into private local
  files. No proprietary images or package data are bundled or committed. The
  bounded importer verifies package structures, tile grids, decompression sizes,
  cache hashes and image checksums. See [format and provenance](local-map-import.md).
- The Map tab provides local browsing, live experimental markers, zoom, pan,
  fit and follow/recenter. Context, age, session changes and pause state gate the
  marker. UI repaint timing respects sample expiry even with slow idle refresh
  preferences. Current area automatically loads installed main overviews as the
  area changes, even while another tab is open. All 313 installed overviews pass
  import checks. Missing maps and ambiguous context have no marker. Separate
  floors and universal live alignment remain unvalidated.
- Settings, theme, scale, UI refresh interval and map catalog persist in XDG
  configuration. Corrupt preferences recover to defaults with an explanation.
  Optional reconnect is restricted to a unique session of the same exact build.
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
39-41 ms in the debug build and 9-10 ms in the optimized build. The optimized application used 7.8% of one CPU core in a ten-second check in
that instance. Pause/Resume cleared and restored resources. Outdoor and
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
`cargo test --locked --workspace` (71 passed, one opt-in local import ignored),
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

## Restarted session and indoor acceptance (2026-10-14)

After the user relaunched the game, the exact supported executable build passed
read-only attachment and fresh controlled-pawn sampling. The current visible
streaming context was MHDT0107 with matching audio/designer signals. Its installed
map is a single-tile indoor floor plan. The companion selected that entry, showed
a fresh experimental marker within the floor plan, and the user explicitly
confirmed: "Yes, the map and marker match" in response to the movement check.

The expanded private catalog also includes MPYR0242's six-tile main overview.
Its live alignment, auxiliary layouts and floor handling remain unvalidated.
The catalog retains experimental calibration confidence for both new entries;
qualitative user confirmation does not replace independent landmark/error budgets.
No game inputs, memory writes, personal snapshots or proprietary assets were
committed. Screenshots and imported artwork remain private.


## Automatic loading acceptance (2026-10-14)

With saved catalog/source cleared, the new release connected to the existing
supported game process, detected its CookedPC installation, and imported MADV155
in the background while Character was selected. Opening Map showed its 2048 x
2304 overview and a fresh experimental marker. This area was outside the former
four-map set and required no manual import or code change. Its precise alignment
has not been independently measured. The earlier MHDT0107 confirmation remains
qualitative evidence for that indoor map, not all instances.

All 313 installed main overviews passed corrected import/cache validation.
Synthetic tests cover rapid area changes, cancellation, stale/late results,
manual browsing, source changes, retry and large-grid downsampling. Real loading
transitions, auxiliary floor selection and quantitative calibration across every
instance remain live acceptance work. Private screenshots were inspected locally
and are intentionally excluded from publication.

The installed release was also checked through companion Pause -> Browse ->
Current area -> Resume. Browse restored the overview while paused without a
marker; resuming restored fresh live tracking. No game input was involved.
