# DCUO Companion

A native Linux companion application in development, written in Rust for DC Universe
Online running under Wine or Proton. It displays game information in a normal
managed window on another monitor, reads game memory, and cross-checks supported
character fields through [dcuobot-api](https://github.com/DCUOBot/dcuobot-api).
Its Map tab previews experimental live position on locally imported open-world
and indoor maps.

**Status: experimental live map preview.** The managed application displays API
character references, experimental memory resources/position, and automatic maps
from the local game installation. All 313 installed main overviews pass import
and cache validation; this is not live calibration of every instance. The user confirmed matching map/marker movement in Gotham and the
MHDT0107 indoor area. Current-character identity, quantitative indoor accuracy, multi-launch
profiles and full release acceptance remain incomplete. See
[live map evidence](docs/live-map-preview.md).

The Character tab includes a solo On Duty feat checklist. Look up an exact
character name/world to load completion data, then use Refresh feats when needed.
Missions are ordered by Normal-entry CR (level-30 missions first); each documented
feat shows its reward points. Completed feats are hidden unless Show completed is
checked. Search matches mission names, feat names and requirements. Character
details and experimental Log remain available.

**Catalog coverage is partial:** 40 missions, 171 documented feat entries and 25
sourced completion-ID mappings. Unknown completion remains visible. Nekron's Lair
and Summit of Sorrow currently lack documented feat lists. Census retrieval time
does not establish in-game freshness, and the selected API character is not
automatically verified against the controlled player. Sources and coverage are
available under each mission's Details. No personal snapshots are bundled.

## Run the application

Use Linux/X11 and Rust 1.95 or newer (validated with 1.96.0):

```sh
cargo run --locked --bin dcuo-companion
cargo run --locked --no-default-features --bin dcuo-probe -- self-test
cargo run --locked --no-default-features --bin dcuo-probe -- list
```

In the window, refresh/select the game process and Connect to inspect its build
and read access. Enter an exact character name/world and choose Look up reference
for API data. Scan next bounded step searches the selected process for matching
name bytes, separately labeled as unverified string evidence. Diagnostics shows
the actual process/build, application version and backend. Pause, Disconnect,
Settings, About and Quit are available. Current area automatically imports the matching installed map in the background,
even while another tab is open, and switches when fresh area evidence changes.
The game installation is detected on connection; Settings allows a CookedPC
path override and disabling automatic imports. Browse preserves manual selection.
Missing maps or ambiguous area evidence hide the marker; Retry current map
retries a failed import. Multi-floor selection is not yet proven. See
[local import details](docs/local-map-import.md).
DWM owns placement, tags, borders, and focus; use its controls to choose a monitor.

Optional reconnect recovers only one fresh session with the same executable build;
manual Disconnect cancels recovery. API references retain their original receipt
time through a five-minute local cache and honor service cooldowns. Closing the
application cancels incomplete map imports. Preferences can be reset without
deleting imported maps. See [local install and rollback](docs/local-install.md)
and [offline validation and remaining live checks](docs/offline-hardening.md).

See [development instructions](CONTRIBUTING.md) for explicit-PID inspection,
bounded name-search experiments, X11 checks, and build requirements.

## First milestone

A complete application with Character, Map, and Diagnostics tabs, shared session
controls/status, Settings, Help/About, and desktop integration. The Character tab
shows the active character, proven memory fields, and per-field API verification.
The Map tab adds a live position marker, area/floor display, zoom/pan, and
follow/recenter. First-release coverage includes both open-world and indoor/
instance maps, with supported areas listed explicitly. Unknown maps, stale
positions, and unproven fields remain visibly unavailable.

The initial target is DWM-Titus on X11. The main window must be managed
(`override_redirect=false`), with DWM handling placement, tags, borders, and focus.
Phase 0 must verify operation on a separate monitor while the game is active,
including fullscreen gameplay, focus switching, and display reconnection.

First-release packaging precedes optional personal combat analytics, encounter
history, timers, and other quality-of-life features. These depend on establishing
reliable sources.

## Project documents

- [Specification](SPEC.md): scope, architecture, data contracts, and constraints.
- [Roadmap](ROADMAP.md): implementation phases and acceptance criteria.
- [Tasks](TASKS.md): current status and actionable checklist.
- [Contributing](CONTRIBUTING.md): development and validation expectations.
- [Agent conventions](AGENTS.md): durable repository instructions.

## Data sources

Game memory provides live observations. The API provides character and reference
data that can corroborate selected fields. API matches do not certify an entire
memory layout. Current health, cooldowns, combat events, and live position/zone/
floor are not exposed by the inspected API contract. Live maps need independently
proven memory observations and correctly matched, calibrated map assets.

Optional combat-log support will be evaluated for event-based analytics. Each
feature must identify its source and distinguish measurements from estimates.

## References

- [DCUO-Assistant](https://github.com/265ada/DCUO-Assistant): feature inspiration.
- [dcuobot-api](https://github.com/DCUOBot/dcuobot-api): character and reference API.
- [Linux process memory reads](https://man7.org/linux/man-pages/man2/process_vm_readv.2.html).
- [egui](https://docs.rs/egui/latest/egui/): initial Rust UI candidate.

No reference-project code is included. Dependency and source licenses must be
checked before incorporating external code or assets.

On the Map tab, enable **Ground spawns** to show recognized spawn objects exposed
by the running client. The switch is saved and defaults to off. Discovery runs
in bounded background steps; the status shows progress. Colored diamonds identify
recognized spawn visuals, while the yellow circle remains your character.
Unloaded spawns and collected/uncollected status are unavailable.

Orange diamonds mark recognized exobyte spawns whenever the Ground spawns layer
is enabled and the client exposes a valid live object.

Spawn locations stay cached during partial scans and temporary read gaps. A
location disappears after two consecutive complete scans miss it; seeing it again
resets that count. These are last-seen locations, not a guarantee that a spawn is
available. The cache clears on a confirmed area/character/session change and is
not saved to disk. Turning the layer off hides cached locations without erasing
them in the same context.
