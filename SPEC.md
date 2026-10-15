# Specification

Status: the managed application displays API references and build-gated
experimental live resources/position. On-demand local imports cover all 313 installed
main map overviews, with automatic area switching and private cache reuse. Gotham has a paired landmark observation;
the user confirmed matching map/marker movement in Gotham and MHDT0107. Offline hardening includes exact-build reconnect,
bounded API caching/retry, cancellable import and per-entry calibration metadata. Profile,
identity, quantitative indoor accuracy, floor handling and full phase acceptance
remain incomplete. See
[live map evidence](docs/live-map-preview.md).
Requirements below remain acceptance targets; no phase is fully complete.

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
- Provide a complete application shell with Character, Map, and Diagnostics tabs,
  connection controls, settings, help, and desktop integration.
- Display the current character's position on a 2D Map tab, covering open-world
  and indoor/instance maps in the first release. Automatically load installed
  main overviews as the area changes; retain explicit per-map calibration and
  floor-support status rather than claiming universal live alignment.
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
| Map catalog and projection | Zone/instance/floor matching, map assets, calibrated coordinates, coverage |
| Companion UI | Application shell, tabs, normal input, scaling, unavailable/stale states |
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

## Application shell and current-character workflow

The first usable milestone is a complete companion application with the following
surfaces. All tabs share one selected game session and one observation model;
switching tabs must not start duplicate readers or change the current character.

| Surface | Required behavior |
| --- | --- |
| Persistent header | Current character/world when known, selected game session, connection state, and Refresh sessions / Connect / Disconnect controls |
| Character tab (default) | Character identity, live resource values, proven stats, and per-field source/freshness/comparison states |
| Map tab | Current area and floor, 2D map, live position marker, zoom/pan, follow, recenter, and clear location/support status |
| Diagnostics tab | Build/profile compatibility, read health, last successful sample, API state, map/calibration version, actionable errors, and sanitized diagnostic export |
| Settings | API URL/world fallback, UI scale/theme, bounded polling preferences, reconnect preference, map asset location, and validated reset-to-defaults |
| Help / About / Quit | Setup and troubleshooting guidance, version/license/asset credits, and orderly shutdown |
| Persistent status bar | Separate memory and API status, last update age, loading/paused/stale indication, and a link to details |

Connection and lifecycle requirements:

- Launch into a usable shell even when the game, API, or map assets are absent.
  Discover candidate sessions; never attach to an arbitrary process when several
  match. Display enough identity to select one without dumping process memory.
  Selecting a process observes its active character; it does not change the game.
- Show explicit states for no game, session selection, checking build/access,
  connected/waiting for character, reading, loading/character switch, paused,
  access denied, unsupported build, read failure, and disconnected. Pair errors
  with Retry, session selection, or relevant help. API outages have independent
  status and must not disable otherwise valid local observations.
- Support pausing/resuming reads and disconnecting from the UI. Paused values
  must be labeled as such and must not keep a live map marker. On game restart,
  revalidate process identity and profile before reconnecting; do not reuse a PID
  or resolved addresses blindly. Disconnect cancels automatic reconnect until
  the user connects again.
- Clear old character values, portrait, comparisons, location, and marker on a
  session/character change. Reject delayed reader/API/asset results from the old
  session or zone generation. Loading and missing values never become zero.

The Character tab prioritizes name, world, power, movement, alignment, CR, skill
points, and current/maximum health and power as each field is proven. Resource
bars require compatible current and maximum values from the same character;
missing, stale, or invalid inputs cannot produce a live percentage. Show memory
readings separately from cached API values and comparison results. API identity
or an optional API portrait cannot establish which character is currently active
in memory. A portrait uses a placeholder when unavailable and is not a live game
render. Equipment, artifacts, and allies may appear as clearly sourced API
details; memory verification remains field-specific and is not implied.

Application polish is part of acceptance: consistent theme/spacing, readable
labels, visible keyboard focus, tab and keyboard navigation, status text as well
as color, scrollable/responsive layouts, and usable controls at tested UI scales.
Keep technical addresses and detailed errors in Diagnostics. Loading, empty,
partial, stale, and failure states must be designed for every tab. Save validated
UI preferences through the standard XDG locations and recover from malformed
configuration without crashing. Closing the window or choosing Quit stops
workers and saves preferences; the application must not keep reading invisibly.
Diagnostic export includes bounded error/compatibility metadata, with secrets,
character identifiers, precise locations, and raw memory contents excluded by
default. Preview the sanitized report before the user saves or shares it.
Provide an application icon, desktop launcher, setup help, and version/About
information as part of packaging. Normal DWM borders and controls remain in use.

## Map tab and live-location contract

First-release map scope includes both open-world and indoor/instance maps, as
requested. Maintain a coverage inventory identifying each supported area,
variant, floor/layer, asset, and calibration. Define the launch inventory during
Phase 0. The expanded user requirement is all instances with automatic live
area updates. Import all supported installed main overviews on demand; record
missing packages, unsupported formats, unproven layouts/floors and calibration
gaps explicitly. Universal live accuracy must not be inferred from import
coverage. A single outdoor demonstration does not complete the map feature.
Unproven areas remain visibly unsupported and instances cannot silently be moved
to a later feature phase.

### Location observations

- Discover the selected player's position, area/zone identifier, and any
  instance/layout/floor context needed to choose the correct map through bounded,
  read-only memory profiles. Prove that the coordinates belong to the controlled
  character rather than the camera, a target, or another entity. Record heading
  only if separately proven; otherwise use a directionless marker.
- Distinguish server/world identity from area/zone and instance identity. Do not
  assume Census world IDs identify maps, or assume axis order, units, origin,
  vertical axis, or global versus instance-local coordinates. Validate each on
  the actual game build using movement, landmarks, and area transitions.
- Bind the location sample to process/session, current character, zone generation,
  build/profile, timestamp, and validity. Recheck identity/zone around related
  reads and reject mixed snapshots. Reject non-finite/out-of-bounds coordinates,
  stale samples, ambiguous maps, and unsupported layouts; teleports and loading
  transitions need revalidation rather than a fabricated path between points.
- The inspected dcuobot-api character models expose no live position, zone, or
  floor fields. Treat these as memory observations without API corroboration;
  API availability must not gate a verified local marker. Do not assume another
  game's Census map endpoints apply to DCUO.

### Map assets, matching, and calibration

- Maintain a versioned map catalog separate from executable memory profiles.
  Each entry records proven zone/variant/layout matching keys, map dimensions,
  coordinate bounds, projection, floor/layer rules, asset version/checksum,
  provenance, and redistribution permission or local-only status. Bundle only
  assets with established permission; support documented local asset import
  where needed. Public visibility of an image does not establish permission to
  redistribute it. Do not commit proprietary game assets or personal snapshots.
- Calibrate the horizontal world coordinates into map-image coordinates, including
  origin, scale, rotation, and axis inversion as needed. Use enough independent
  landmarks to determine the transform (at least three non-collinear points for
  an affine transform), then validate at separate landmarks not used in the fit.
  Apply the same pan/zoom/resize transform to map and marker. Set accuracy and
  latency budgets from Phase 0 measurements before acceptance testing.
- Handle indoor maps by verified area/instance/layout and floor/layer, including
  overlapping floors and reused coordinates where present. Height alone is not
  proof of a floor unless validated for that map. Re-entering an instance must
  revalidate its mapping. Variable or ambiguous layouts are unsupported until a
  matching layout and transform are proven; never reuse a similar-looking map.
- Validate catalog entries and bound file size, image dimensions, texture memory,
  and asset loading. Missing/corrupt images or transforms produce a recoverable
  Map-tab error while Character and Diagnostics remain usable. Load assets off
  the UI thread and keep a bounded cache; no network image fetch per sample.

### Map interaction and failure behavior

- Automatically select the supported current area and floor, display their names,
  and draw a clearly visible player marker with last-update age. Include zoom
  buttons/wheel, drag-to-pan, Fit map, Follow player, and Recenter. Panning pauses
  follow until explicitly resumed. Keep controls accessible through the keyboard.
- Provide a floor/layer selector for multi-level maps and a supported-map browser.
  Clearly label manual browsing and offer Return to current location. A manually
  selected map/floor does not override location validity: show a live marker only
  when it matches the proven current context. Do not plot on an assumed floor.
- Remove the live marker immediately on character/session/zone transition and on
  invalidation, stale reads, or pause; reappear only after matching data and map
  are valid. An old map can remain for labeled browsing, never as a live location.
  Show distinct messages for waiting for location, loading, position unavailable,
  missing map asset, unsupported area/layout/floor, and calibration failure.
- Keep character observation running when another tab is selected. Reduce hidden
  map rendering work, keep the UI responsive, and resume from the latest valid
  sample. Never place an invalid marker at the origin or clamp it onto a map edge.
  Clicking the map changes only the companion view, never the game or movement.

Acceptance requires controlled routes and separate landmark checks in open-world
and indoor/instance maps, floor changes, instance exit/re-entry, character switch,
death/respawn, teleport/loading, and game restart. Synthetic fixtures must also
cover axis inversion, map transforms after zoom/resize, ambiguous floors, corrupt
assets, unsupported areas, stale data, and late results from the previous zone.
Record coverage and error/latency measurements per supported map. Map discovery,
assets, and calibration are still unproven; a decorative map or synthetic marker
does not satisfy the live-location milestone.

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
| Current health/power, cooldowns, combat events, live position/zone/floor | Not verifiable through the inspected API |

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
- Prove player position and map-selection context independently of character
  identity; invalidate location on zone/instance/floor transitions as required.
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
- Exact first-release map inventory across open-world and indoor/instance areas,
  asset sources, and independently proven map/floor selection and calibration.
- Priorities for optional character details beyond the core identity/live fields.
- Memory-access arrangement if normal process permissions deny reads.
- UI/backend choice following the feasibility experiment.
- Minimum usable window size, tested UI scales, field staleness thresholds, and
  measured memory/rendering/map accuracy and latency budgets.
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
- [DCUO Census collection index](https://census.daybreakgames.com/get/dcuo:v1/).

## Original nearby collectible slice (superseded by the ground spawn layer)

A user-requested map marker may show the currently exposed nearby blue
magnifying-glass interaction candidate when exact-build, reflected field,
object/world identity and finite position checks pass. Bind it to the same fresh
session/area observation and map projection as the player, but label it separately
as experimental. Missing/changed candidates hide the marker. This does not prove
all collectibles, collection availability, floors, or persistent spawn coverage.

## Exposed ground spawn layer

The Map tab provides a persisted Ground spawns toggle, off by default. When on,
bounded background enumeration discovers recognized live ground spawn actors
exposed in the current world, beyond the nearby interaction candidate. Each
marker requires current lifetime, world, identity and position validation and
the same map/context/freshness gates as the player. Turning off disables this
optional discovery and hides markers immediately. Discovery reports progress and
limits; never equate loaded objects with every spawn in an entire zone. Recognized
marker definitions include the installed magnifying-glass color variants and
question-mark/seasonal markers; ordinary damageable props are excluded.
No collected-state inference, game interaction or persistent location database.

The ground spawn layer also recognizes the observed exobyte definition/package
pair and distinguishes it with an orange diamond. Generic effect names from
other packages remain excluded. The same live actor and freshness gates apply.
