# Specification

Status: proposed implementation scope; no runtime capabilities verified.

## Objective

Build a native Linux Rust companion application that displays useful DCUO
information in a managed window on a separate monitor from the game running under
Wine/Proton. Read live game state externally and use dcuobot-api to corroborate
fields that have equivalent API representations.

## Initial scope

- Discover and select the Linux process hosting the intended DCUO session.
- Identify its executable build and supported memory profile.
- Read character identity and incrementally proven live fields.
- Cross-check identity and comparable profile fields against the API.
- Display an interactive, resizable DWM-managed X11 window on another monitor.
- Handle loading, character switching, game exit, restart, and API outages.

Memory access is read-only. Injection, game-memory writes, gameplay automation,
and automatic system security changes are outside the proposed scope.

## Architecture

Start with one Rust application and separately testable modules. Split into
workspace crates only when boundaries or testing needs justify it.

| Module | Responsibility |
| --- | --- |
| Process discovery | Session selection, process identity, executable mappings, lifecycle |
| Memory reader | Bounded external reads, access errors, partial-read handling |
| Memory profiles | Build fingerprints, signatures, pointer resolution, field decoding |
| API client | Typed character/reference responses, caching, retries, rate limiting |
| Validation | Per-field comparison, freshness, identity binding, provenance |
| State and events | Timestamped observations, lifecycle transitions, replay fixtures |
| Companion UI | Panels, normal window input, scaling, visible unavailable/stale states |
| Local storage | Configuration, selected history, schema versioning |

Initial technology candidates are process_vm_readv for reads and egui for UI.
The initial desktop target is native X11 under DWM-Titus with the game and
companion window on different monitors. Choose the window/rendering integration
after it proves the managed-window contract below. Wayland and other window
managers are deferred compatibility work. Picom may be present in the desktop
session, but the application must not require it for transparency or stacking.

Keep memory polling and network work off the UI thread. Bound queues and buffers.
Measure polling and rendering costs before setting production update intervals.

## DWM-Titus companion-window contract

Create the main companion window as a normal managed X11 top-level window with
`override_redirect = false` and `_NET_WM_WINDOW_TYPE_NORMAL`. Preserve that behavior
when the main window is recreated. DWM controls its borders, tiling/floating,
focus, tags, placement, and resizing. Do not embed it into or modify the Wine
game window.

Use stable identity before mapping: `WM_CLASS` instance `dcuo-companion`, class
`DcuoCompanion`, and `WM_NAME`/`_NET_WM_NAME` title `DCUO Companion`. Keep character
names and changing status inside the panel so DWM rules remain stable. Ordinary
egui/winit window integration is a candidate; verify the actual X11 attributes
and DWM behavior before selecting the backend.

- Monitor placement: keep the main window on a user-selected monitor separate
  from the game. Use normal DWM monitor/tag controls and, for repeatable startup,
  document a narrowly matched per-user rule in
  `${XDG_CONFIG_HOME:-$HOME/.config}/dwm-titus/window-rules.toml`. Select the monitor
  index and tag from the actual desktop layout in Phase 0; do not hard-code a
  second-monitor index or continually override DWM placement. Recheck the mapping
  after display changes. If that monitor is unavailable, keep the window reachable
  on an available monitor and allow normal DWM controls to relocate it.
- Input and focus: accept normal pointer, scroll, and keyboard input. The user
  changes focus through DWM normally; data refreshes and game lifecycle events
  must not activate or raise the companion window. Record startup focus behavior
  and test focus switching while the game is fullscreen.
- Visibility and lifecycle: follow the companion window's own DWM tag visibility.
  Keep displaying updates while the game has focus on the other monitor. Do not
  automatically hide or close the companion when the game loses focus, changes
  tags, minimizes, or exits. Show disconnected, stale, or unavailable states as
  appropriate and recover on game restart without leaking old character data.
- Layout and rendering: use an opaque, readable interface with adjustable UI
  scaling and a layout that handles ordinary DWM tiling and resizing. Save UI
  preferences while leaving window geometry and workspace policy to DWM. Verify
  rendering with and without Picom and recovery after monitor/resolution changes.

On-game drawing, click-through/edit modes, alpha transparency, always-on-top
rules, fullscreen stacking exceptions, and game-relative positioning are outside
the revised scope. The primary acceptance setup is two monitors: game on one,
managed companion on the other. Validate windowed and fullscreen gameplay,
ordinary window management, focus changes, and display reconnection in Phase 0.

## Observation contract

Each field observation records its value, source, observation time, session
identity, executable/profile version, and validity. API comparisons additionally
record retrieval time and source freshness when known.

Represent read validity separately from API comparison:

- Read validity: valid, stale, unavailable, or invalid.
- API comparison: matched, different, freshness unknown, unavailable, or not comparable.

A successful identity comparison does not validate unrelated fields. An API
outage does not automatically invalidate a locally validated memory observation.
Never substitute zero for missing data or silently retain a previous character's
values after a switch. Clear comparisons when session identity changes.

## API integration

Use a configurable API base URL. The inspected service exposes:

- GET /v1/census/characters?name=...&worldId=...
- GET /v1/data/power-types
- GET /v1/data/movement-modes
- GET /v1/data/alignments
- GET /v1/data/artifacts
- GET /v1/data/allies

Preserve string identifiers and URL-encode lookup parameters. Require an exact
returned identity match: the current service can wildcard short names and select
the first result. Permit explicit world selection until automatic detection is
proven. Do not assume client object IDs equal Census IDs without evidence.

| Fields | Comparison rules |
| --- | --- |
| Name, world, character ID | Exact identity checks where equivalent IDs are proven |
| Power, movement, alignment | Explicit normalization through reference data |
| CR, skill points, equipment | Compare with freshness caveats and controlled changes |
| Maximum health/power, core stats | Compare only under documented comparable conditions |
| Current health/power, cooldowns, combat events | Not verifiable through the inspected API |

The inspected API caches character lookups for five minutes and applies a
60-request/minute per-client limit. Upstream freshness can be unknown. Refresh
on identity changes and bounded schedules, not on every memory sample. Honor
Retry-After and use bounded backoff for network/upstream failures.

Current artifact responses expose identity but not ranks. Investigate equipment
slot semantics before trusting equipment comparisons. Missing timestamp, raw-ID,
or slot fields may justify additive changes in dcuobot-api; those changes belong
in that repository with its own validation.

## Memory profile requirements

- Determine process architecture and actual executable mappings at runtime.
- Key supported profiles by executable fingerprint and profile version.
- Resolve addresses relative to validated mappings; avoid fixed absolute addresses.
- Bound pointer traversal, string lengths, read sizes, and scanning work.
- Reject ambiguous signatures and unsupported builds.
- Check ranges, encoding, plausible values, and consistency across observations.
- Handle partial reads and torn snapshots; external reads are not atomic.
- Prevent PID reuse from binding old addresses to a different process.
- Test character switches, zone changes, death/respawn, and process restart.
- Preserve small sanitized fixtures; do not publish full process dumps.

Access failures must report the actual permission problem. Do not automatically
change ptrace policy, assign broad capabilities, or run the whole UI as root.
Any required access arrangement is a Phase 0 design decision.

## Later features

Personal DPS, per-ability damage, critical-hit rate, timed parses, fight history,
and personal bests are candidates after reliable event acquisition is proven.
Combat-log support may complement memory observations. Deduplicate overlapping
sources and handle log rotation, truncation, and session boundaries.

Group metrics, cooldowns, buffs, and automatic Nth Metal tracking require separate
source discovery. Do not infer DPS from sparse health samples or label damage
ticks as confirmed ability casts. Manual timers must remain labeled manual.

## Open decisions

- Exact DWM-Titus revision, game display mode, and chosen companion monitor/tag
  on the initial Fedora/X11 target.
- Launcher, Wine/Proton version, game build, architecture, and install location.
- First-release feature priorities beyond the character panel.
- Memory-access arrangement if normal process permissions deny reads.
- UI/backend choice following the feasibility experiment.
- Distribution format and project license before shipping application code.

## References

- [DCUO-Assistant release notes](https://github.com/265ada/DCUO-Assistant/blob/master/RELEASE_NOTES_v1.2.0.md).
- [API source](https://github.com/DCUOBot/dcuobot-api).
- [process_vm_readv](https://man7.org/linux/man-pages/man2/process_vm_readv.2.html).
- [Yama access restrictions](https://docs.kernel.org/admin-guide/LSM/Yama.html).
- [Window behavior](https://docs.rs/winit/latest/winit/window/struct.Window.html).
- [DWM-Titus configuration and window rules](https://dwm.christitus.com/configuration.html).
- [winit X11 window attributes](https://docs.rs/winit/0.30.13/winit/platform/x11/trait.WindowAttributesExtX11.html).
- [Xlib window attributes](https://www.x.org/releases/X11R7.5/doc/libX11/libX11.html).
