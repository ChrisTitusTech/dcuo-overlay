# Tasks

## Current status

Planning foundation prepared. No Rust implementation, memory profile, or live
compatibility result exists yet. Next implementation work is Phase 0.
The initial desktop target is Fedora/X11 with DWM-Titus and Picom; the overlay
must be unmanaged. This is a requirement, not a verified runtime capability.

## Planning

- [x] Inspect the existing API contract and cache behavior.
- [x] Review DCUO-Assistant for feature inspiration.
- [x] Document scope, source limitations, architecture, and phased acceptance criteria.
- [x] Specify the unmanaged X11/DWM-Titus contract and its acceptance checks.
- [ ] Resolve remaining desktop/launcher details and initial feature priorities.
- [ ] Select a project license before application code is distributed.

## Phase 0

- [ ] Record running DWM-Titus/Picom revisions, configuration, monitors/scaling, and game display mode.
- [ ] Record launcher, Wine/Proton version, executable build, and architecture.
- [ ] Identify the correct process and establish a bounded read-only access path.
- [ ] Demonstrate a known character value across three launches.
- [ ] Prove `override_redirect=true` before first map and after edit/hide/show/recreation.
- [ ] Verify stable X11 identities and the narrow DWM-Titus fullscreen stacking rule.
- [ ] Prove alpha transparency with Picom, including fullscreen unredirection behavior.
- [ ] Prove click-through, no focus theft, and a reachable edit toggle with input recovery.
- [ ] Validate windowed, borderless, fake-fullscreen, and real fullscreen stacking.
- [ ] Validate app-owned placement/visibility across tags, focus, monitors, and game restart.
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
- [ ] Preserve the unmanaged X11 contract and repeat Phase 0 checks with the real panel.
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
- [ ] Document verified DWM-Titus/Picom setup, supported modes, and configuration rollback.
- [ ] Validate installation, removal, rollback, and unsupported-build behavior.
- [ ] Complete release checks; publish only with explicit authorization.

## Evidence log

No implementation validation has run. Add dated entries per phase with commit,
environment, checks, observed results, and unresolved limitations. Do not mark
tasks complete solely because code was written.

### 2026-10-13: DWM-Titus planning review

- Read-only inspection found Fedora 44, `XDG_SESSION_TYPE=x11`, a running `dwm`
  process, and `picom --backend glx`. The installed `dwm -v` reports `dwm-0.7.2`;
  the exact running build and unmanaged stacking capability remain to be proven.
- Existing DWM-Titus rules place `steam_app_24200` on tag 4, monitor 0, and include
  a WFHelper unmanaged-overlay stacking rule. The DCUO overlay needs its own
  identity/rule and visibility tracking; it cannot inherit the game's rules.
- The inspected Picom configuration enables fullscreen unredirection with a
  500 ms delay. DCUO transparency and stacking across that transition are untested.
- Reviewed the upstream DWM-Titus overlay documentation and winit 0.30.13 X11
  creation API linked in SPEC.md. Updated planning only; no desktop configuration
  or application code changed. All live Phase 0 acceptance checks remain pending.
- Documentation validation: whitespace, ASCII, code fences, relative links, and
  the four added external references passed. Scope/acceptance consistency was
  reviewed across SPEC.md, ROADMAP.md, TASKS.md, and README.md. Rust build/lint/test
  gates are not applicable until a package exists; live checks need the prototype.
