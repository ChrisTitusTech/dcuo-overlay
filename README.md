# DCUO Overlay

A planned native Linux overlay written in Rust for DC Universe Online running
under Wine or Proton. The overlay will read game memory and cross-check supported
character fields through [dcuobot-api](https://github.com/DCUOBot/dcuobot-api).

**Status: planning.** There is no runnable application yet. Memory layouts,
launcher compatibility, and desktop overlay behavior still need to be proven.

## First milestone

A transparent, movable character panel with click-through mode, reliable
character detection, proven live memory fields, and per-field API verification
status. Unsupported or stale readings must be visible as such.

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
