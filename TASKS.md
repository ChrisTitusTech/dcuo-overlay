# Tasks

## Current status

Planning foundation prepared. No Rust implementation, memory profile, or live
compatibility result exists yet. Next implementation work is Phase 0.

## Planning

- [x] Inspect the existing API contract and cache behavior.
- [x] Review DCUO-Assistant for feature inspiration.
- [x] Document scope, source limitations, architecture, and phased acceptance criteria.
- [ ] Resolve desktop/launcher details and initial feature priorities.
- [ ] Select a project license before application code is distributed.

## Phase 0

- [ ] Record distribution, compositor, monitor scaling, and game display mode.
- [ ] Record launcher, Wine/Proton version, executable build, and architecture.
- [ ] Identify the correct process and establish a bounded read-only access path.
- [ ] Demonstrate a known character value across three launches.
- [ ] Demonstrate transparency, stacking, click-through, edit mode, and focus behavior.
- [ ] Record findings, performance baselines/budgets, backend decision, and blockers.

## Phase 1

- [ ] Scaffold Rust modules and install/prove required development tooling.
- [ ] Implement configuration, diagnostics, observation types, and lifecycle states.
- [ ] Implement replay fixtures and error-path tests.
- [ ] Add passing build, formatting, lint, and test CI.

## Phase 2

- [ ] Discover and validate identity fields.
- [ ] Investigate CR, skill points, maximum/current health, and power independently.
- [ ] Implement profile fingerprints, bounded decoding, and invalidation.
- [ ] Validate transitions, partial reads, ambiguous signatures, and PID reuse handling.
- [ ] Record field-level live evidence and regression fixtures.

## Phase 3

- [ ] Implement typed character/reference API client and exact identity matching.
- [ ] Implement comparison states, freshness handling, caching, and retry limits.
- [ ] Evaluate timestamp, identifier, and equipment-slot contract gaps.
- [ ] Pass mismatch, ambiguity, stale-data, outage, and rate-limit tests.
- [ ] Record live API cross-check evidence.

## Phase 4

- [ ] Implement the proven character panel and data-status display.
- [ ] Add saved placement, scaling, opacity, hide/show, and edit controls.
- [ ] Validate full-session use and recovery on the target desktop.
- [ ] Measure overhead against the agreed Phase 0 budgets.

## Phase 5

- [ ] Select initial combat and quality-of-life features.
- [ ] Prove event sources and replay correctness before presenting analytics.
- [ ] Implement selected features in small increments with relevant evidence.
- [ ] Record deferred features and their missing data requirements.

## Phase 6

- [ ] Complete the supported environment matrix and extended-session tests.
- [ ] Package and document setup, local storage, and diagnostics.
- [ ] Validate installation, removal, rollback, and unsupported-build behavior.
- [ ] Complete release checks; publish only with explicit authorization.

## Evidence log

No implementation validation has run. Add dated entries per phase with commit,
environment, checks, observed results, and unresolved limitations. Do not mark
tasks complete solely because code was written.
