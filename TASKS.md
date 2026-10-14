# Tasks

## Current status

Phase 0 implementation is underway. Runnable Rust memory and window probes exist;
bounded live executable-header reads and initial managed-window checks passed on
the development machine. There is no supported character/location profile yet.
The initial desktop target is Fedora/X11 with DWM-Titus. The application is a
normal managed companion window on another monitor from the game. This replaces
the earlier overlay design. The full live desktop acceptance matrix is pending.
The first release includes a complete Character/Map application shell and live
location on supported open-world and indoor/instance maps. Position readers,
map identities, assets, floor selection, and calibration are all unproven.
See [Phase 0 evidence and next experiments](docs/phase-0.md).

## Planning

- [x] Inspect the existing API contract and cache behavior.
- [x] Review DCUO-Assistant for feature inspiration.
- [x] Document scope, source limitations, architecture, and phased acceptance criteria.
- [x] Specify the managed X11 companion-window contract and two-monitor acceptance checks.
- [x] Define application shell, current-character workflow, and Map-tab requirements.
- [x] Include both open-world and indoor/instance maps in first-release scope.
- [x] Define explicit `gpt-6.1-sol` worker assignments, ownership, and integration gates.
- [ ] Resolve remaining desktop/launcher details and initial feature priorities.
- [ ] Select a project license before application code is distributed.

## Worker coordination

Use the phase assignments in ROADMAP.md and worker rules in AGENTS.md. For each
delegated assignment, add a record here containing phase, worker/model, bounded
deliverable, prerequisites, owned files, shared interfaces, required checks, and
stop point. At completion, record changed files, check results, unresolved limits,
and lead review/integration evidence. Keep implementation tasks pending until their
exit criteria pass; planning and worker review do not demonstrate live capability.

Completed assignment: `review_worker_plan`, explicitly spawned on `gpt-6.1-sol`,
reviewed the phase split and coordination risks. Its scope was read-only planning,
with no owned implementation files or further delegation. It recommended separate
memory, map, UI, API, and review work after shared contracts are agreed, one live
operator, and lead-owned integration. The lead incorporated those recommendations.
A final read-only worker review found no actionable consistency issues in the
three changed planning documents. No implementation worker ran during that review.

## Phase 0

Implementation assignments (2026-10-13):

- `memory_probe`, `gpt-6.1-sol`: owns `src/process.rs`, `src/memory.rs`, and
  `src/bin/probe.rs`. Implement bounded process discovery, executable identity,
  read-only reads, and metadata-only probe output with synthetic tests. Shared
  contract: independent library modules exposed by lead-owned `src/lib.rs`, no
  supported character profiles yet. Requires the lead's Cargo scaffold. Stop at
  a tested probe; no live game reads, scanning, desktop changes, or commits.
- `window_probe`, `gpt-6.1-sol`: owns `src/bin/window_probe.rs`. Implement a small
  egui/X11 managed-window feasibility probe using the lead's dependency choices.
  No dependency on memory observations; show explicit unsupported status, ordinary
  input and resize controls, and optional bounded smoke-test lifetime. Verify
  compilation/API behavior, but leave live launching to the lead. Stop at the
  prototype; no full application, shared-file edits, or commits.
- `map_feasibility`, `gpt-6.1-sol`: owns `docs/map-feasibility.md`. Research map
  source/provenance and a bounded outdoor/indoor calibration experiment using
  public primary sources. No source assumptions imply permission or live support.
  Requires SPEC.md's location contract; no application interfaces to change.
  Return source links, unresolved requirements, and proposed experiments. Stop
  before asset acquisition, game interaction, implementation, or commits.

The lead owns Cargo files, library wiring, integration, live desktop/process
experiments, documentation/status, final format/lint/test/build gates, and commits.
The lead also owns `scripts/check_x11.py` for bounded startup/relaunch checks.
The memory assignment includes `search-name`, a bounded stdin-based exact-byte
discovery experiment, with synthetic matching tests and no live profile claims.

- Follow-up `map_feasibility` review, still `gpt-6.1-sol`: read-only review of
  the completed process/memory/CLI implementation and lead smoke checker. Check
  bounds, identity ambiguity, error handling, sensitive output, and unmet gates.
  No file ownership, edits, live reads, commits, or further delegation. Return
  actionable findings for lead resolution before committing.

Assignment results: all three implementation/research workers completed their
bounded deliverables. The memory worker fixed the live Wine mapping assumption
and the reviewer's stale-mapping finding, with regression tests. The window worker
fixed frame filling after lead visual inspection. The lead integrated the probes,
resolved the all-target lint warning, passed 14 tests plus format/lint/build, and
ran the live header, bounded search, keyboard, and window checks recorded in
docs/phase-0.md. The reviewer confirmed its finding resolved. All live profile
and map capability claims remain unsupported.

- [x] Build bounded process/memory and managed-X11 feasibility prototypes with synthetic tests.
- [ ] Record running DWM-Titus revision, monitor layout/scaling, companion monitor/tag, and game display mode.
- [x] Record launcher, Wine/Proton version, executable build, and architecture.
- [x] Identify the candidate game process and establish a bounded read-only header-access path.
- [x] Verify exact pre-map X11 identity, normal type, and override redirect off over two launches.
- [ ] Demonstrate a known character value across three launches.
- [ ] Prove player position and map/instance/floor context in an outdoor area and an indoor instance.
- [ ] Inventory first-release areas/layers and identify usable assets, provenance, and distribution status.
- [ ] Calibrate prototype maps against independent landmarks and set accuracy/latency budgets.
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
- [ ] Build shared session controls/status, Character/Map/Diagnostics tabs, and Settings/Help/About/Quit.
- [ ] Define versioned location/map-catalog schemas and bounded asset handling.
- [ ] Implement replay fixtures and error-path tests.
- [ ] Validate keyboard navigation, all tab states, config recovery, and worker shutdown offline.
- [ ] Add passing build, formatting, lint, and test CI.

## Phase 2

- [ ] Discover and validate identity fields.
- [ ] Investigate CR, skill points, maximum/current health, and power independently.
- [ ] Discover player position, zone/instance/layout/floor context, and optional heading independently.
- [ ] Implement profile fingerprints, bounded decoding, and invalidation.
- [ ] Validate transitions, partial reads, ambiguous signatures, and PID reuse handling.
- [ ] Validate location across outdoor/indoor routes, floor changes, teleports, and repeated instance entry.
- [ ] Reject mixed/late session or zone observations and unsupported location contexts.
- [ ] Record field-level live evidence and regression fixtures.

## Phase 3

- [ ] Implement typed character/reference API client and exact identity matching.
- [ ] Implement comparison states, freshness handling, caching, and retry limits.
- [ ] Evaluate timestamp, identifier, and equipment-slot contract gaps.
- [ ] Pass mismatch, ambiguity, stale-data, outage, and rate-limit tests.
- [ ] Verify API failure leaves local Character/Map readings usable and old responses cannot change current identity.
- [ ] Record live API cross-check evidence.

## Phase 4A: Application shell and Character

- [ ] Implement the proven character panel and data-status display.
- [ ] Finish session selection, connect/disconnect, pause/resume, reconnect, and error recovery.
- [ ] Add responsive layout, adjustable UI scaling, and saved UI preferences.
- [ ] Complete icon/theme, settings, diagnostics/export, help/About, and clean close behavior.
- [ ] Validate no-game/loading/stale/error states, keyboard-only use, and current-character resource bars.
- [ ] Preserve normal DWM management and repeat Phase 0 checks with the real panel.

## Phase 4B: Map and integrated acceptance

- [ ] Implement catalog matching, bounded asset cache, calibrated projection, and zone/layout/floor selection.
- [ ] Implement live marker, update age, zoom/pan, fit, follow/recenter, and labeled map/floor browsing.
- [ ] Complete declared first-release map coverage across open-world and indoor/instance areas.
- [ ] Validate marker accuracy at independent landmarks and measured update latency per supported map/layer.
- [ ] Test stale/paused/mixed samples, zone changes, wrong-floor browsing, and missing/corrupt maps.
- [ ] Prove Character/Map share one session and tab changes neither duplicate readers nor leak old results.
- [ ] Validate full-session use and recovery on the target desktop.
- [ ] Measure overhead with Map visible/hidden and API offline against Phase 0 budgets.
- [ ] Capture sanitized visual acceptance evidence for all tabs and lifecycle/error states.

## Phase 5: First-release packaging

- [ ] Complete the supported environment matrix, map coverage, and extended-session tests.
- [ ] Package desktop entry/icon, help/About/credits, XDG storage, and diagnostic export.
- [ ] Document DWM-Titus monitor/tag setup, display recovery, and configuration rollback.
- [ ] Document map sources/permissions, local asset installation, supported areas, and calibration versions.
- [ ] Validate desktop launch/close, install/removal, package/profile/map rollback, and unsupported updates.
- [ ] Complete release checks; publish only with explicit authorization.

## Phase 6: Later optional features

- [ ] Select initial combat and quality-of-life features.
- [ ] Prove event sources and replay correctness before presenting analytics.
- [ ] Implement selected features in small increments with relevant evidence.
- [ ] Record deferred features and their missing data requirements.

## Evidence log

Implementation validation is recorded below and in docs/phase-0.md. Add dated entries per phase with commit,
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

### 2026-10-13: Full application and live-map planning review

- Expanded the shell to Character/Map/Diagnostics tabs with shared session
  controls, settings, lifecycle states, accessibility, help, and desktop packaging.
- The user selected open-world and indoor/instance maps for the first release.
  Added independent location discovery, asset provenance, map/floor matching,
  calibration, coverage, and live acceptance requirements. All remain unproven.
- Inspected dcuobot-api at `060908d`: CharacterResponse and CensusCharacter have
  no live position/zone/floor fields. The official DCUO Census collection index
  was also reviewed; it does not establish a live-location or map-asset source.
- Moved first-release packaging ahead of optional combat analytics. Updated the
  plan only; no application, map assets, memory reads, or desktop changes were made.
- Documentation validation passed for whitespace, ASCII, code fences, relative
  links, and scope/phase consistency across all six documents. The added Census
  reference was reachable. Rust and live game/map checks were not run because
  the repository still has no application; those gates remain pending.

### 2026-10-13: Development worker planning review

- Used a `gpt-6.1-sol` subagent for a read-only review of parallel work and phase
  dependencies. Added explicit model selection and phase-specific assignments,
  with up to three extra workers when useful and within runtime limits.
- Defined file ownership, prerequisites, worker handoff evidence, single-operator
  live experiments, and lead-owned shared interfaces, integration, and commits.
  Preserved the managed two-monitor window and both first-release map categories.
- Reviewed the official subagent model-selection guidance linked in ROADMAP.md.
  No global settings, application code, game state, or desktop configuration changed.
  Phase 0 remains the next implementation step; all live feasibility gates remain
  pending.
- Documentation checks passed for whitespace, ASCII, code fences, local links,
  explicit worker model, and retained phases. The worker's final consistency
  review found no actionable concerns. Rust build/lint/test and live acceptance
  checks remain pending because there is no application package or prototype.

### 2026-10-13: First Phase 0 prototypes

- Added a locked Rust package, bounded read-only memory/process primitives,
  metadata inspection and experimental name-search CLI, a managed X11 window
  prototype, and a two-launch smoke checker. No profiles or API integration exist.
- Ordinary user access read and matched the live 64-byte DCGAME.EXE header.
  Exact-name searches stopped at their time bounds without matches; active
  character identity, all character fields, and location remain unverified.
- Two live launches passed the X11 property contract on HDMI-0 while the fullscreen
  Gamescope host occupied DP-0. Keyboard tab/input and background-focus checks
  passed. The full DWM/game/compositor/recreation matrix remains pending.
- Format, strict all-target Clippy, 14 tests, workspace build, optimized memory
  build, and fixed-header access passed. Independent review's stale-mapping
  issue was fixed and confirmed. See docs/phase-0.md for commands and limits.
- Map research found no advertised DCUO map collection in the inspected Census
  index. Asset permissions, coordinates/context, inventory and calibration remain
  unresolved for both required map categories; see docs/map-feasibility.md.
- No game actions, desktop policy changes, publication, or personal-data commits
  were performed. Phase 0 remains incomplete; continue bounded discovery and the
  live acceptance matrix before selecting the production backend or profiles.
