# DCUO Companion

A native Linux companion application in development, written in Rust for DC Universe
Online running under Wine or Proton. It displays game information in a normal
managed window on another monitor, reads game memory, and cross-checks supported
character fields through [dcuobot-api](https://github.com/DCUOBot/dcuobot-api).
Its Map tab will show the current character's live location on supported
open-world and indoor/instance maps.

**Status: a runnable verification slice.** The managed window displays real
process/build information, exact API character-reference values, and bounded
memory-name search evidence. The user confirmed the displayed character details
match the game. There are no proven active-character/location profiles or usable
calibrated maps yet. See [verification evidence](docs/verification-slice.md).

## Run the prototypes

Use Linux/X11 and Rust 1.95 or newer (validated with 1.96.0):

```sh
cargo run --locked --bin dcuo-window-probe
cargo run --locked --no-default-features --bin dcuo-probe -- self-test
cargo run --locked --no-default-features --bin dcuo-probe -- list
```

In the window, refresh/select the game process and Connect to inspect its build
and read access. Enter an exact character name/world and choose Look up reference
for API data. Scan next bounded step searches the selected process for matching
name bytes, separately labeled as unverified string evidence. Diagnostics shows
the actual process/build, application version and backend. Pause, Disconnect,
Settings, About and Quit are available. Map remains explicitly unsupported.
DWM owns placement, tags, borders, and focus; use its controls to choose a monitor.

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
