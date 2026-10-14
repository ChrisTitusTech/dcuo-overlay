# Tasks

## Current status

Planning foundation prepared. No Rust implementation, memory profile, or live
compatibility result exists yet. Next implementation work is Phase 0.
The initial desktop target is Fedora/X11 with DWM-Titus. The application is a
normal managed companion window on another monitor from the game. This replaces
the earlier overlay design and remains unverified at runtime.

## Planning

- [x] Inspect the existing API contract and cache behavior.
- [x] Review DCUO-Assistant for feature inspiration.
- [x] Document scope, source limitations, architecture, and phased acceptance criteria.
- [x] Specify the managed X11 companion-window contract and two-monitor acceptance checks.
- [ ] Resolve remaining desktop/launcher details and initial feature priorities.
- [ ] Select a project license before application code is distributed.

## Phase 0

- [ ] Record running DWM-Titus revision, monitor layout/scaling, companion monitor/tag, and game display mode.
- [ ] Record launcher, Wine/Proton version, executable build, and architecture.
- [ ] Identify the correct process and establish a bounded read-only access path.
- [ ] Demonstrate a known character value across three launches.
- [ ] Prove `override_redirect=false`, normal window type, and DWM management across recreation.
- [ ] Verify stable X11 identities and monitor/tag placement, including relaunch.
- [ ] Prove ordinary input, resizing/tiling, focus cycling, and no focus theft from updates.
- [ ] Validate visible updates on the other monitor during windowed and fullscreen gameplay.
- [ ] Validate independent window visibility and session status across game focus/exit/restart.
- [ ] Validate scaling, monitor-disconnection recovery, and rendering with/without Picom.
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
- [ ] Add responsive layout, adjustable UI scaling, and saved UI preferences.
- [ ] Preserve normal DWM management and repeat Phase 0 checks with the real panel.
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
- [ ] Document DWM-Titus monitor/tag setup, display recovery, and configuration rollback.
- [ ] Validate installation, removal, rollback, and unsupported-build behavior.
- [ ] Complete release checks; publish only with explicit authorization.

## Evidence log

No implementation validation has run. Add dated entries per phase with commit,
environment, checks, observed results, and unresolved limitations. Do not mark
tasks complete solely because code was written.

### 2026-10-13: DWM-Titus overlay planning review (superseded)

Historical evidence for commit `c2a3457`. The overlay requirements below were
superseded by the managed companion-window decision recorded in the next entry.

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

### 2026-10-13: Managed companion-window plan

- Replaced the overlay scope with a normal DWM-managed X11 window on a separate
  monitor, at the user's request. DWM owns window placement, tags, and focus.
- Removed active transparency, click-through/edit-mode, fullscreen stacking, and
  game-relative visibility requirements. Added independent session-status display,
  normal input, two-monitor operation, and monitor-disconnection recovery checks.
- The actual companion monitor/tag and startup rule remain Phase 0 setup choices.
  No application code or desktop configuration changed; live validation is pending.
- Documentation checks passed for whitespace, ASCII, code fences, and relative
  links. Reviewed all six repository documents for consistent scope; obsolete
  requirements remain only as explicitly superseded history or exclusions.
  Rust build/lint/test gates remain inapplicable until a package exists.
