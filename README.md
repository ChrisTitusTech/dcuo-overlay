# DCUO Companion

A planned native Linux companion application written in Rust for DC Universe
Online running under Wine or Proton. It displays game information in a normal
managed window on another monitor, reads game memory, and cross-checks supported
character fields through [dcuobot-api](https://github.com/DCUOBot/dcuobot-api).

**Status: planning.** There is no runnable application yet. Memory layouts,
launcher compatibility, and two-monitor desktop behavior still need to be proven.

## First milestone

A resizable character panel with normal mouse and keyboard input, reliable
character detection, proven live memory fields, and per-field API verification
status. Unsupported or stale readings must be visible as such.

The initial target is DWM-Titus on X11. The main window must be managed
(`override_redirect=false`), with DWM handling placement, tags, borders, and focus.
Phase 0 must verify operation on a separate monitor while the game is active,
including fullscreen gameplay, focus switching, and display reconnection.

Later phases cover personal combat analytics, encounter history, timers, and
other quality-of-life features. These depend on establishing reliable sources.

## Project documents

- [Specification](SPEC.md): scope, architecture, data contracts, and constraints.
- [Roadmap](ROADMAP.md): implementation phases and acceptance criteria.
- [Tasks](TASKS.md): current status and actionable checklist.
- [Contributing](CONTRIBUTING.md): development and validation expectations.
- [Agent conventions](AGENTS.md): durable repository instructions.

## Data sources

Game memory provides live observations. The API provides character and reference
data that can corroborate selected fields. API matches do not certify an entire
memory layout. Current health, cooldowns, and combat events are not exposed by
the inspected API contract.

Optional combat-log support will be evaluated for event-based analytics. Each
feature must identify its source and distinguish measurements from estimates.

## References

- [DCUO-Assistant](https://github.com/265ada/DCUO-Assistant): feature inspiration.
- [dcuobot-api](https://github.com/DCUOBot/dcuobot-api): character and reference API.
- [Linux process memory reads](https://man7.org/linux/man-pages/man2/process_vm_readv.2.html).
- [egui](https://docs.rs/egui/latest/egui/): initial Rust UI candidate.

No reference-project code is included. Dependency and source licenses must be
checked before incorporating external code or assets.
