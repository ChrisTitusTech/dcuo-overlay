# Contributing

This repository currently contains a plan, not a runnable Rust application.
Start with the Phase 0 experiments in ROADMAP.md. Read SPEC.md and TASKS.md before
implementation and record any scope changes explicitly.

## Development setup

Phase 1 will select and record a supported Rust toolchain, system dependencies,
and the actual application run command. Do not assume every Wine launcher or
Wayland compositor is supported. Live validation needs access to the intended
Linux desktop and a running DCUO session.

## Validation

For documentation changes, check links, scope consistency, and whitespace.
Once the Rust package exists, the baseline checks will be:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

These commands are planned gates and do not run against the current docs-only
repository. Add feature-specific checks as needed. UI and memory changes also
need the applicable live acceptance evidence from ROADMAP.md.

Report failed or skipped gates and their blockers. Do not use synthetic fixtures
as proof that live memory layouts or desktop overlay behavior work.

## Changes and publication

Keep changes small, preserve unrelated work, and update task status only after
acceptance criteria pass. Review external licenses before reusing code or assets.
Never publish private snapshots or raw process dumps. Pushes, pull requests, and
releases require explicit authorization from the repository owner.
