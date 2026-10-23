# Tasks

## Current status

Phase 0 acceptance remains open. The managed companion displays exact API
references plus build-gated experimental native resources, position and streaming
area. Automatic local-only import covers all 313 installed main map overviews,
with background loading on fresh area changes and reusable private caching.
The user confirmed matching map/marker movement in Gotham and MHDT0107.
Quantitative accuracy, other indoor routes/floors, multi-launch profile and full
desktop acceptance remain pending. No phase is declared complete.
See [live map evidence](docs/live-map-preview.md). PR #1 through PR #3 are merged. The completed offline
slice added optional restart recovery, API cache/backoff, cancellable imports, per-map
calibration evidence, preference reset, and user-local install/rollback. See
[offline hardening evidence](docs/offline-hardening.md).

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

### Offline recovery and local packaging (2026-10-14)

- The game has been closed by the user. Do not relaunch or perform live reads
  until a new game session is available. No live exit transition is claimed: the
  originally connected window was already closed when the process exit was checked.
- `native_reader`, `gpt-6.1-sol`, owns `src/session.rs` only: implement opt-in
  reconnect after unexpected process loss, for the exact previously attached
  executable fingerprint and one unambiguous fresh candidate. Manual Disconnect
  cancels recovery, pause never reconnects, generation/cancellation remain intact,
  and recovery retries are bounded/backed off. Expose a setter; lead wires UI.
  Use deterministic synthetic event/candidate tests; no game launch, live reads,
  settings or application edits. Stop at tested session module.
- `map_package`, `gpt-6.1-sol`, owns `scripts/install_local.py`,
  `scripts/test_install_local.py`, and `docs/local-install.md`: reversible
  user-local install/upgrade/uninstall and rollback of this app's binary/desktop/
  icon, preserving unrelated files and user maps/settings. Use private ownership
  metadata and hashes; reject symlink/traversal and unknown occupied destinations.
  Tests use temporary prefixes and synthetic binary/assets. Include optional
  monitor placement and explicit source/build commands. No actual user installation
  or Git operations; lead owns target-machine validation.
- `map_ui`, `gpt-6.1-sol`, performs a read-only offline-readiness audit across
  SPEC.md, ROADMAP.md, TASKS.md and current code. Identify concrete first-release
  work still possible without the game, distinguishing required implementation
  from live validation and deferred optional features. No file edits or nested
  workers; return prioritized file-level findings and achievable offline gates.
- Follow-up `map_ui` owns `src/api.rs` for a bounded reference client with typed
  sanitized failures, exact-key cache and original receipt timestamps, rate-limit
  cooldowns honoring Retry-After, and capped transient retries within an eight-
  second request budget. Synthetic local HTTP/clock tests only. Lead adds the
  httpdate dependency and wires the client into the existing API worker.
- Follow-up `native_reader` owns `src/maps.rs` for structured per-entry calibration
  version/profile/confidence/error evidence and effective marker-confidence
  gating. Preserve schema-1 compatibility as experimental-only; incompatible
  profiles cannot place markers. Lead adapts importer/UI/tests to the agreed
  types. No live profiles are promoted and no assets are rewritten by workers.
- Follow-up `map_package` owns `src/map_import.rs` and `src/map_view.rs`: add
  cooperative cancellation to background imports/catalog work and shutdown, clean
  incomplete outputs, and synthetic cancellation tests. Coordinate calibration
  constructor changes with the lead; no game launch or private asset commits.
- Lead owns UI settings, documentation/status, CI integration and Git. All checks
  and independent review precede the next authorized PR/merge; no release upload.

### Map foundation implementation (2026-10-13)

- `map_foundation`, `gpt-6.1-sol`: owns `src/maps.rs`; implement bounded local map
  catalog schema, affine fit/project, exact context matching and stale/session
  marker gating with synthetic tests. Prerequisites: SPEC map contract and shared
  types agreed below. No live proof or bundled assets. Stop at tested module.
- `settings_foundation`, `gpt-6.1-sol`: owns `src/settings.rs` and `assets/` desktop
  entry/icon only; validated XDG settings load/save and sanitized diagnostics
  helpers with corruption/error tests. No UI wiring or manifests. Stop at module
  plus installable source assets, no desktop installation.
- `map_ui`, `gpt-6.1-sol`: owns `src/map_view.rs` and `src/bin/window_probe.rs`;
  integrates the tested catalog/settings modules, background asset loading, map
  browsing controls and calibration UI. No live profile claims; synthetic UI
  checks and clean shutdown required. Follow-up wires experimental native readings
  and background local-map import, keeping marker approval with the lead. No other
  files or shared types changed.
- `native_reader`, `gpt-6.1-sol`: owns `src/native.rs`; implement a fingerprint-
  gated experimental local-player sampler from lead-observed reflection facts.
  Validate object/field metadata and double-read ownership around samples; expose
  experimental position/resources separately from supported map validity. Only
  synthetic tests, no worker live reads or promotion to a supported profile.
- Follow-up `native_reader` owns `src/session.rs` for lead-approved experimental
  sample integration: one background reader, 250 ms bounded cadence, pause/loss
  invalidation, stale samples and independent status. A follow-up resolves only
  the two observed area keys from visible streaming audio/designer packages,
  retaining them as experimental context and rejecting mixed transitions. Preserve exact API source
  boundaries and test generation/owner changes. No UI/Git/live actions.
- Follow-up `native_reader` owns `src/session.rs` for a bounded independent API
  worker so slow reference requests do not interrupt local sampling. Preserve
  generation checks, command accounting, cancellation and joined shutdown; test
  responsiveness against a deliberately delayed local synthetic HTTP server.
- `map_package`, `gpt-6.1-sol`: owns `docs/local-map-import.md` only; inspect
  installed map-package metadata and public decoder specifications to establish
  a bounded local-only import route. No live memory, asset copies in Git, game
  execution, redistribution, shared code edits or nested workers. Report exact
  package format, dimensions/coordinate metadata, existing tools and limitations.
  Follow-up adds `src/map_import.rs`: bounded local installed-package import for
  only the two test areas using independently implemented metadata parsing and
  LZO/BC1 decode, private output assets and synthetic corruption tests. No game
  assets in Git; imported calibrations remain experimental until verified.
- Follow-up `map_ui` owns `src/map_view.rs`: automatic exact-context selection
  while following, an explicit Browse/current-area mode, larger usable map canvas,
  and regressions for stale/unavailable context. Experimental markers must never
  be labeled verified. Lead owns application wiring and live acceptance.
- Follow-up `native_reader` owns `src/memory.rs` and `src/native.rs`: reduce
  measured polling overhead with bounded scatter reads, preserving process
  identity, readable-range, partial-read and sample-generation checks. Synthetic
  self-process tests only; lead measures real sampling cost. No session/UI edits.
- Lead owns public module wiring, dependency selection, CI,
  live discovery, documentation and final validation. No worker live reads,
  commits or nested workers. Research source offsets failed the current build;
  no profile is enabled by those candidates.

### Continued implementation and publication (2026-10-13)

The user authorized publishing/merging completed progress and continuing toward a
working map without routine questions. Existing live acceptance gates remain.

- `location_research`, `gpt-6.1-sol`: read-only Phase 0/2 research of primary
  sources and public reader structures for controlled-player location/context.
  Prerequisites: current SPEC and map feasibility evidence. No owned source files
  or shared-contract changes. Return sources, bounded experiment proposals and
  limitations; stop before live reads, edits or nested workers.
- `map_sources`, `gpt-6.1-sol`: read-only Phase 0 map-source/calibration research
  for Gotham and one instance, including a local asset import route. Same plan
  prerequisites; no file ownership. Return verified sources, coordinate evidence
  and limits; stop before asset downloads, live actions, edits or nested workers.
- The lead owns live discovery, integration and publication. A built-in Codex
  review covers all progress against the remote main branch before the initial PR.


### Completed verification slice (2026-10-13)

The user requests continuation until actual application/character information can
be checked. Integrate process/build/access diagnostics and explicitly sourced API
reference fields with a bounded, resumable memory-name experiment. This does not
promote a name-string occurrence to current-character identity, nor bypass live
profile/map gates. Independent shell/API work proceeds while discovery continues.

- `discovery_v2`, `gpt-6.1-sol`: owns `src/discovery.rs`, `src/memory.rs`, and
  `src/bin/probe.rs`; efficient bounded reads and resumable name search, synthetic
  boundary/cancellation tests. Uses existing process identity/fingerprint types.
  No live reads, personal fixtures, shared manifests, commits, or nested workers.
- `reference_client`, `gpt-6.1-sol`: owns `src/api.rs`; typed bounded dcuobot-api
  reference lookup with exact name/world matching, optional field handling, and
  synthetic response tests. Reads API source but does not modify it. No personal
  response fixtures, shared manifests, commits, or nested workers.
- `verification_ui`, `gpt-6.1-sol`: owns `src/bin/window_probe.rs`; usable session
  controls, real diagnostics, clearly separated memory evidence/API reference,
  application About/settings and honest Map state, against the lead's controller
  interface. No live launch, shared manifests, commits, or nested workers.

The lead owns `src/session.rs`, shared API contracts, Cargo/lib wiring, live tests,
sanitized documentation, integration checks, and commits. Workers stop at their
bounded components; the lead integrates and validates the user-verifiable slice.
Follow-up `reference_client` review is read-only across session/discovery/UI
integration: check cancellation, stale results, queue bounds, source labeling,
and shutdown. No file ownership or live actions; report issues to the lead.

Assignment results: all three workers completed the owned files above. The lead
integrated the controller and UI, passed 34 tests plus format/lint/build, and
verified the populated Character and Diagnostics tabs on the live desktop. The
reviewer identified pause/lookup status and process-selection identity issues;
the lead fixed both with regression tests and the reviewer confirmed resolution.
The user confirmed the displayed API character details match the game. Supported
active-character memory fields and all location/map gates remain pending.

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

- [x] Scaffold Rust modules and install/prove required development tooling.
- [ ] Implement configuration, diagnostics, observation types, and lifecycle states.
- [x] Build shared session controls/status, Character/Map/Diagnostics tabs, and Settings/Help/About/Quit.
- [x] Define versioned location/map-catalog schemas and bounded asset handling.
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

### 2026-10-13: User-verifiable character and diagnostics slice

- Integrated the exact API reference client, bounded resumable name discovery,
  single session worker, process controls, and populated application tabs.
- Live read-only discovery found 16 unique UTF-16 name occurrences after
  14,446,592 bytes, stopping at the candidate cap. This does not establish
  active-character ownership or verify memory-backed stats.
- The user confirmed the displayed API name, power, combat rating and skill
  points match the character sheet. The Diagnostics tab shows actual process,
  executable fingerprint/header evidence and application/backend information.
- Format, strict all-target Clippy, 34 tests and workspace build passed. The
  optimized probe completed the bounded live search. Two nested-X startup/exit
  checks passed, and live window properties retained the managed X11 contract.
- Independent review findings were fixed, regression-tested and confirmed.
  Personal observations/captures remain outside committed files. The populated
  window is left open for user verification. See docs/verification-slice.md.
- Current-character profiles, live resources, location, calibrated maps and the
  complete desktop acceptance matrix remain unproven; no phase is declared done.

### 2026-10-13: Experimental live-map slice

- PR #1 merged the character verification slice as `d46416b`. User authorization
  covers subsequent validated progress PRs and merges, without release publication.
- Added build-gated read-only local-pawn resources/position, independently scheduled
  API requests, persistent settings, map catalog/projection and a bounded importer.
- Two real private imports passed: 600 Gotham tiles and six Old Gotham Subway
  tiles, with catalog/image checksums and private output permissions. No game
  artwork, personal observations or memory dumps are staged.
- Gotham streaming context and one paired in-game marker landmark matched.
  The real companion displayed the live preview, zoom and Follow on HDMI-0.
  Indoor marker, movement and multi-launch validation remain pending.
- See docs/live-map-preview.md for demonstrated behavior and remaining gates.
  Format, strict all-target lint, 65 workspace tests, 53 non-window tests, build
  and two nested-X startup/shutdown checks passed. One local-asset test is opt-in
  and was separately run twice. Independent review is in progress.

- The user confirmed the live Gotham marker follows movement in the correct place.

- Independent Codex review found three actionable defects: name-search progress
  reset on native errors, bare catalog paths, and timestamps assigned after
  validation. Fixed all three with relevant regression checks. The supplemental
  CodeRabbit invocation could not start; it is not a required repository gate.
- Added bounded scatter reads to reduce polling cost while preserving fresh
  mapping/PID checks and all-or-nothing partial-read handling. Warm optimized
  samples in the currently observed unsupported instance took 9-10 ms; a renewed
  outdoor performance comparison remains pending. Updated local gates pass
  70 workspace tests and 58 non-window tests, format and strict all-target lint.

- Second independent Codex review found a repaint-timing gap at slow UI settings.
  Live refresh is now capped at 250 ms and scheduled no later than the current
  sample expiry; the regression test passes. The optimized companion measured
  7.8% of one CPU core over ten seconds in the current instance. Pause/Resume
  restored resources; the unsupported-area marker remained correctly hidden.

- User explicitly set the stopping point: continue unattended until all useful
  work possible without a running game is exhausted. Do not relaunch the game;
  stop at the remaining live-only checks and report them precisely.

## Offline hardening integration evidence (2026-10-14)

All three follow-up workers completed their recorded ownership scopes. The lead
integrated persistent API receipt/source state, opt-in reconnect settings, reset,
calibration profile gating and sanitized diagnostics. Installer commit `00df5fb`
passed all 12 isolated tests. Workspace, minimum-toolchain and actual offline
window/import/install measurements are recorded in
[offline hardening](docs/offline-hardening.md). Independent review found a stale
process selector after recovery; the lead fixed it with PID-reuse regression
coverage. Full phase checkboxes remain pending live acceptance. No game was
launched, no live snapshots/assets were committed, and no release was published.

## Instance coverage continuation (2026-10-14)

The user relaunched the game and requests instance maps. The lead observed a
unique loaded audio/designer context for MHDT0107; the prior unsupported instance
was MPYR0242. Both have locally installed UI map packages. No name/coordinates or
artwork will be committed. Existing calibration remains experimental.

- `map_package`, gpt-6.1-sol, owns src/map_import.rs only: inspect local metadata
  and extend bounded import for MHDT0107 and MPYR0242 alongside the original two
  maps, requiring supported package/grid structure and synthetic regressions.
  No live reads, application control, Git, or nested workers. Report metadata and
  constructor changes; lead validates private imports and live alignment.
- Lead owns native area allowlist, application integration, live read-only
  validation, documentation, review and authorized publication.

Instance continuation result: the worker extended private import to all four maps,
including the single-tile MHDT0107 grid and the MPYR0242 main overview. The lead
verified the restarted build, wired exact streaming contexts and installed the
expanded private catalog. The user explicitly confirmed that the current indoor
map and marker match and track movement. This is qualitative acceptance for
MHDT0107, not validated calibration, all-instance coverage or floor proof.
Workspace tests: 96 passed (one local-asset opt-in test separately passed), 75
non-window tests passed; format, strict Clippy and builds passed.

## Automatic installed-instance coverage (expanded user requirement)

The user requires ALL instances with live updates. The four-map extension alone
is insufficient. The installed UI/MAPS tree has 313 main map packages. Implement
bounded per-area import on demand and explicit unsupported-format diagnostics;
do not claim universal live calibration or floor proof from package inventory.

- `map_package`, gpt-6.1-sol, owns src/map_import.rs: inventory all 313 installed
  main packages offline, report metadata/texture format coverage, generalize a
  bounded per-area cancellable importer for standard main overviews, and provide
  safe reusable private output caching. Keep four-map API compatibility. Report
  concrete unsupported cases and shared API before integration. No live reads.
- `native_reader`, gpt-6.1-sol, owns src/native.rs only: replace four-area gate
  with strictly validated unique visible audio/designer area identity, preserve
  build/ownership/generation/ambiguity checks. Unknown contexts remain experimental
  and cannot draw a marker without a matching imported catalog. Synthetic tests.
- `map_ui`, gpt-6.1-sol, owns src/map_view.rs: auto-load/import the current area
  through the existing bounded worker, generation-safe transitions, bounded
  per-area caching, no duplicate jobs/retry loops, retain manual Browse mode.
  Coordinate importer API; expose source configuration setter to lead. Synthetic
  switch/failure/stale-result tests. No live game reads or desktop control.
- Lead owns settings/application integration, scope/evidence documents, live
  verification and Git/review/publication. Workers do not spawn nested agents.


Automatic coverage integration evidence (2026-10-14): all three workers completed
and the lead inspected their changes. All 313 installed main overviews passed
actual decode, catalog/image validation and stable cache reuse (no residual
staging directories). Lead review caught a scale-four resizing defect; the fix
has a pixel-level regression and the full 313-area check was rerun successfully.
Starting with empty catalog/source, the release auto-detected the installation
and loaded MADV155 in the background; Map displayed its overview and fresh
experimental marker. No game input was sent. User-local installation upgraded.

Local gates: format, strict workspace/all-target Clippy (Rust 1.96 and 1.95),
111 workspace tests, 79 no-default library tests, workspace/release builds,
12 installer tests, two nested-X managed-window launches and documentation links
passed. Three local-asset tests are opt-in; all were separately exercised against
private local files (initial four-map import, metadata inventory, and corrected
313-area actual imports). Area switching is covered synthetically; all-instance
live alignment, auxiliary floors and the full desktop/profile matrix remain open.
Independent review found stale source-dependent catalog reuse after pausing and
changing the map source. Fixed unconditional source invalidation and added a
context-loss regression; 111 workspace tests and strict lint/builds pass.
A second review found Browse could remain blank after context loss or cancelling
an automatic asset load. Browse now reloads its selected asset; both paths have
regression coverage. A third review found saved automatic catalogs bypassed
source fingerprint checks on startup. Automatic catalog reuse now requires a
successful per-launch source check; a regression covers first-load and checked
reuse. Final independent Codex review found no actionable regressions.
All 111 enabled workspace tests, format and strict Clippy pass. The reviewer did
not run private local-asset/live checks; lead evidence above covers those.
Authorized PR publication and remote checks follow.

## Map blinking correction (2026-10-14)

User reports repeated map refresh/blinking. Lead owns map_view.rs: decouple static
map texture lifetime from transient location validity, retain marker gating and
cancel stale jobs. `map_ui` (gpt-6.1-sol) owns application.rs only: stabilize the
Map evidence header geometry across missing/fresh samples; regression for layout
if practical. No game input or live reads by worker. Lead owns live validation,
local gates, review and the already-authorized PR/merge workflow.

Blinking fix evidence: transient sample loss and same-area generation changes
now retain the static map texture, zoom and pan, while marker freshness remains
independently gated. A positively different area still clears the old texture
and cancels obsolete work. Header and map status rows keep stable height across
missing/fresh samples; full clipped status text is available on hover.

113 workspace tests, 79 no-default library tests, strict Clippy on Rust 1.96/1.95,
format and workspace/release builds passed. Regression coverage exercises twenty
sample-loss/recovery generations without texture reload or pan/zoom reset, actual
area changes, and header geometry at three widths. On the installed X11 build,
Pause retained the Gotham map while removing its marker; Resume restored it.
Twelve live frames differed from the paused map region by at most 0.016%, confined
to the marker, without blanking or displacement. Captures remain private.
Independent Codex review found no actionable regressions; supplemental CodeRabbit
reported zero findings. The reviewer did not repeat live checks; lead evidence
above covers the installed application. Authorized PR/CI/merge follow.

## Nearby collectible observation (2026-10-14)

User requests marking the ground investigation/interactive spawn if exposed.
Bounded read-only discovery found the controller's potentialInteractable refers
to a nearby DCServerPhysicsObject whose DamageObj definition is the blue
magnifying-glass asset. User confirmed the visible spawn is blue. This authorizes
an experimental nearby blue collectible marker, not global collectible discovery,
interaction automation, persistence, or collected/uncollected classification.

- `collectible_reader`, gpt-6.1-sol, owns src/native.rs: expose optional
  Collectible { position: [f32; 3] } on Sample, validate exact build/reflection,
  candidate identity/world/location/definition before and after reads, restrict
  to observed blue asset. Errors omit collectible without breaking player reads.
  No live reads, Git, nested workers; synthetic validation and handoff.
- `collectible_ui`, gpt-6.1-sol, owns src/map_view.rs: extra optional location
  argument on MapView::ui; project through existing freshness/context/profile
  gates and draw a distinct blue diamond ring with legend/tooltip. Preserve
  map stability and yellow player marker, including overlapping points. Tests.
- Lead owns application wiring, scope/evidence, the single live operator,
  integrated gates/review and already-authorized publication.

Collectible integration evidence: read-only discovery identified the controller's
nearby potential interaction candidate, its blue magnifying-glass definition,
and an independent world position. User confirmed the spawn is blue. The installed
application now displays a blue diamond ring at that live object's projected
position alongside the yellow player marker. No game inputs were sent.

Reader gates include the exact existing executable, reflected pointer/location/
world fields, class generations, hidden/deleted flags, same-world and bounded
proximity checks, and matching relevant identity/position data before/after.
Optional collectible work has a 250ms deadline and leaves player-read budget
headroom. One live probe succeeded; a later changing name table was correctly
rejected rather than publishing a mixed sample. The normal installed application
subsequently displayed fresh player and collectible markers together. This is
nearby candidate evidence, not zone-wide discovery or uncollected-state proof.

117 workspace tests, 82 no-default library tests, format, strict all-target
Clippy and workspace/release builds passed. Added regressions cover lifetime/
world/proximity/finite-value rejection, unchanged relevant snapshots despite
unrelated actor updates, and collectible projection/freshness/area/profile gates.
Private probes/captures remain ignored. The user explicitly confirmed that the
blue diamond matches the nearby spawn. Independent review precedes publication.
Independent Codex review found no actionable regressions. A supplemental review
suggested hiding the legend when no collectible is visible; retained the static
legend intentionally because it describes marker meaning, not presence, and
conditional rows would reintroduce the map displacement just fixed.

## Exposed ground spawn layer (2026-10-14)

User requests a toggle for all ground spawns exposed by the client.
- ground_reader, gpt-6.1-sol, owns src/native.rs: bounded incremental enumeration,
  lifetime/context checks, opt-in reader configuration, synthetic tests.
- ground_ui, gpt-6.1-sol, owns src/map_view.rs: multiple colored markers, projection
  gates, stable legend/geometry and synthetic tests.
- ground_session, gpt-6.1-sol, owns src/session.rs: propagate toggle and suppress
  delayed optional samples across off/on transitions without resetting the map.
- Lead owns application/settings, shared contract, live read-only checks, docs,
  full gates and authorized review/PR/merge. Workers do not perform live reads,
  desktop actions, Git operations or spawn additional workers.

Acceptance pending: saved toggle, no optional enumeration while off, multiple
current-world markers, bounded scanning, stale/hidden/unloaded rejection,
unchanged static map on toggles and full local/independent review gates.

Ground spawn integration evidence: the installed application displayed two live
blue diamonds, including an object outside the former interaction radius.
Toggling off immediately removed both while retaining player position, map
texture, zoom and pan. The map-region image difference was confined to the
51x36-pixel marker region. The preference persisted to the private settings file and remained enabled after
restarting the installed application.
No game input was sent, and private observations/captures remain ignored.

120 workspace tests and 84 no-default library tests passed, along with format,
strict Clippy on Rust 1.96/1.95 and workspace/release builds. Three unchanged
local-asset tests remain opt-in; their earlier passing evidence is retained.
Tests cover multiple independent projection gates, late optional samples across
off/on transitions, preference round trips and bounded recognized definitions.
The scan processes at most 4096 slots per step under a 100ms optional deadline,
with 250000 object and 256 candidate bounds. Candidates are freshly revalidated;
ordinary object allocations do not discard discovery progress. Unloaded objects,
unrecognized definitions and collection availability remain unsupported; other
colors have package evidence and synthetic coverage but no live acceptance.
Independent review and authorized publication follow.

Independent review identified candidate-refresh starvation, a scan stall on
invalid class metadata, and a toggle that applied unrelated pending settings.
Fixed rotating refresh with reserved discovery time, per-object failure progress
and isolated preference persistence. Added regressions for scheduling, interrupted
scan progress, saved-setting preservation and pending map settings. Updated local
gates passed: 124 workspace and 87 headless tests, strict Clippy on both toolchains,
format, whitespace and builds. Both nested-X managed startup/relaunch checks passed.
The installed application was upgraded again; repeat review is in progress.

Final independent Codex review found no actionable regressions after the fixes.
The reviewer repeated workspace tests, format, strict Clippy and diff checks;
live evidence remains the lead's installed-application checks above.

## Exobyte ground spawn follow-up

PR #7 merged the ground spawn layer as 6e96861 after clean independent review and
CI. The user now identifies an exobyte spawn beside the character. Read-only
inspection binds the nearby interaction actor to FXSM103_DAMAGEABLEOBJECT in
DFXSM103, with matching FXSM103 mesh. Extend the layer using the exact pair and
a distinct orange marker; retain world, visibility, identity and freshness checks.
The lead owns this small dependent reader/UI change and live verification.
No additional worker is needed; independent review remains required.

Exobyte validation: the installed application displayed seven exposed objects,
with five orange exobyte diamonds and two blue collectible diamonds. An orange
marker coincides with the nearby potential interaction actor and player position.
No game actions were sent. The user supplied the spawn-type identification;
independent user confirmation of the new orange marker has not been requested.
125 workspace and 88 headless tests, format, strict Clippy on Rust 1.96/1.95,
workspace and release builds pass. The new classifier test rejects the generic
definition name under any package other than the observed package.

Independent Codex review found no actionable regressions; its repeated workspace
tests, format, strict Clippy and diff checks passed. Authorized PR/CI/merge follow.

## Cached spawn locations

User reports markers disappear while scanning and requests cached locations.
- spawn_cache worker, gpt-6.1-sol: owns new src/spawn_cache.rs and application.rs,
  bounded session/area cache and application integration, synthetic lifecycle tests.
- Lead: native stable ownership comparison, maps/map_view cached projection and
  labels, module wiring, live verification, review, commits and authorized PR/merge.
No worker live reads or Git; no nested workers. Cache is in-memory, last-seen only.

User refinement: remove a cached location only after two consecutive complete
scans fail to detect it. Partial/interrupted scans do not count; a new detection
resets the miss count. Native worker owns completed-scan evidence in native.rs;
cache worker owns expiry tests and integration.

Cache implementation evidence: completed-scan snapshots are bounded and only
published after sample validation. Failed chunks retry without losing prior scan
progress; same-area streaming changes preserve traversal. The private live probe
completed four certified scans despite concurrent client allocations/name-table
changes. No game inputs were sent. Snapshots use shared ownership to avoid
repeated large copies in the session queue.

137 workspace and 100 headless tests passed, plus format, strict Clippy on Rust
1.96/1.95 and workspace/release builds. New tests cover retained partial/missing
observations, exactly two distinct missed scans, reappearance, repeated snapshots,
unknown-area gaps, paused/disabled views, session/context resets, cache bounds,
scan rollback/publication, and cached projection with unchanged live-player gates.
The local application is upgraded; live presentation and independent review follow.

Independent review found that a sweep completed after the sample-start timestamp
could be skipped indefinitely when each sample completes a whole scan. Fixed the
gate to compare completion with the current time and added the exact timestamp
ordering regression. Updated validation passes 138 workspace and 101 headless
tests, strict lint on both toolchains and builds. The installed window retains
the managed X11 contract and has the final build. The running client now reports
an ambiguous area; live cached-marker presentation/removal cannot be confirmed
in that state. Synthetic cache/projection tests cover it; the four completed live
reader scans above validate actual scan completion. No new user acceptance claimed.

The client subsequently resolved MLOC136. The installed map retained seven
cached markers during an actual missing-sample/name-table-change event and Pause.
Toggling the layer off/on while paused restored all seven without scanning; the
map-region pixel difference before/after the toggle cycle was exactly zero.
Reads were resumed. This resolves the earlier live-presentation limitation.

A second review found bounds could allow skipped actors to certify absence.
Both discovery bounds now mark the pass uncertain; a regression proves later
observations cannot make that limited pass authoritative. Updated tests: 139
workspace and 102 headless, with format, both strict lints and builds passing.

Final independent Codex review covered staged, unstaged and untracked files and
found no actionable regressions. Lead live evidence is recorded above.

## Map-focused presentation

The Map tab now reserves its remaining viewport for the canvas. Log is hidden by
default and contains reader status, experimental evidence and scan progress.
Map details is hidden by default and contains import/catalog setup, browsing,
retry, provenance, marker validation and calibration. Essential navigation and
ground-spawn toggle remain visible; no memory or cache semantics changed.

Validation: 141 workspace tests passed (three opt-in private asset tests remain
ignored), 102 headless tests passed, formatting, strict Clippy on Rust 1.96/1.95,
workspace/release builds and two managed-X11 startup/shutdown checks passed.
The application regression requires over 75% canvas area at 862x1048 and 1280x720
and identical canvas geometry with Log open/closed. Live DWM observation at
862x1048 shows approximately 86% canvas, the current map and player/spawn
markers. Log opens without shifting the map. Private screenshots remain under
ignored local-data; no game assets or personal observations are published.

Independent Codex review found no actionable regressions. A supplemental review
suggested preventing inherited scroll offsets; each tab now has its own scroll
state, so long Character/Diagnostics pages cannot shift the Map viewport. The
installed app was also checked with Map details open and closed. Both detail
windows preserve map geometry; user acceptance is not claimed for this UI change.

Final independent Codex review after scroll-state isolation found no actionable
regressions. The supplemental suggestion to restore experimental wording on the
main map is not applied: the user's explicit request is to hide that evidence;
confidence remains visible in Map details and the compact label reports live
availability, not verification. All 12 installer tests also passed.

## Ground-spawn false-positive cleanup

User-reported orange clutter exposed an unsupported assumption: the shared
FXSM103 effect definition does not establish an exo-material node. Bounded live
inventory found 33 matching effect actors and one explicit green collectible in
the current area. No distinguishing replicated mesh name was exposed. Discovery
now rejects shared effects and unverified event variants, accepting exact blue,
green and gold collectible definitions only with the expected Package outer.
Exo detection is unavailable until a specific discriminator is validated; the
user has been asked for a real nearby exo node for paired evidence. Existing cache
expiry is unchanged; restarting the installed app clears old false locations.

Validation: 140 workspace tests passed (3 opt-in private-asset tests ignored),
101 headless tests passed, format, strict lint on Rust 1.96/1.95, workspace and
release builds passed. The classifier regression covers the previously accepted
shared effect and rejects wrong-package and unproven variant definitions.

Independent review caught a renamed rejection message missing from the known
absence classifier. Fixed that mapping and covered it in sweep-completion tests,
so a rejected non-package actor cannot prevent two-scan expiry. All gates rerun.
The installed app's live map retained its background after the client changed
areas; the reader reports ambiguous streaming context and correctly hides live
markers. A retained positive pickup cannot be revalidated in that state. No new
user acceptance claimed; private captures/inventory remain ignored locally.

Final independent Codex review found no actionable regressions. The corrected
build is installed and running on Map; no false orange markers are retained.

## Repair current area without audio stream

Live bounded inspection identifies a current MGOT103_ALT_US world with sole
visible MGOT103_DESIGNERDATA and no audio signal, explaining the unresolved area.
Added an explicit alias guarded by exact WorldInfo/Level/World/Package ancestry,
observed name instances, generations, terminal ownership, resnapshot and designer
corroboration. Unknown aliases and audio conflicts retain rejection. Historical
cache tolerates temporarily unavailable chain evidence, while known identity
changes invalidate it. Last-known map status now makes retained backgrounds clear.

Validation: 144 workspace tests passed, 3 opt-in private-asset tests ignored;
104 headless tests passed. Format, strict lint on Rust 1.96/1.95, workspace and
release builds passed. New regressions cover aliases, ambiguity, wrong ancestor
contracts, fallback generation/cache gaps and retained-map status. Area context
is boxed to keep worker messages compact. The corrected build is installed.

Live installed validation: the app automatically loaded MGOT103, displayed
"Live location" and rendered the yellow player marker on the imported map.
The stale MLOC136 background is no longer shown. The actual managed X11 window
remains responsive; private screenshot is kept under ignored local-data. This is
lead-observed rendering/area evidence, not new user alignment acceptance.

Independent Codex review completed with no actionable regressions.

## Preserve zoom and follow on map load

All asset loads retain the in-session zoom instead of resetting manual map
selections to 1x. Successful current-generation images enable Follow and reset
pan for centering on the next valid player position. Initial Follow defaults on;
Fit and dragging still disable Follow explicitly, and ordinary polling does not
re-enable it. Obsolete completions cannot alter the view.

Validation: 145 workspace tests passed (3 opt-in private-asset tests ignored),
104 headless tests passed, formatting, strict Clippy on Rust 1.96/1.95 and
workspace/release builds passed. A loader-result regression covers automatic and
manual selections, zoom adjustments during loading, stale results, and manual
Follow overrides after load. The updated application is installed.

Live installed validation on Fedora/DWM: increased zoom three steps, disabled
Follow, and used Retry current map. The reloaded map retained the enlarged scale,
Follow was checked again, and the live player marker was centered. Private
before/after captures remain under ignored local-data; no game input was used.

Independent Codex review completed without actionable findings.

## Restore exo-material using owned crafting effect

User provided a confirmed nearby exo-material node. Bounded lead inspection
resolved actor AllComponents -> owned ParticleSystemComponent -> ParticleSystem
FXGEN109_FX_BAUBLE_CRAFTING -> Package DFXGEN109. Both asset instances are zero;
package outer is null. The reflected array inner is ComponentProperty, dimension
1, element size 8, outer equal to AllComponents and property class ActorComponent.
A current-area inventory found two actors with this owned effect, including the
confirmed nearby node. Generic FXSM103 alone remains excluded.

Implemented bounded component inspection with schema, owner, template/package,
instance, class generation and membership checks, repeated before publication.
Stable missing effects count as ordinary exclusions; read errors, oversized
arrays or changing evidence cannot certify a complete absence scan. Tooltip
calls orange markers Exo-material. Existing two-scan cache rules remain intact.

Validation: 148 workspace tests pass (3 opt-in private-asset tests ignored),
107 headless tests pass, formatting, strict lint on Rust 1.96/1.95 and workspace/
release builds pass. Tests reject wrong owners/assets/packages/instances and
cover array bounds, proof mutations and known-absence classification.

Live installed validation shows an orange exo-material diamond beside the yellow
player marker, plus a second matching node. A release-built bounded reader check
completed two authoritative sweeps, each containing two exo nodes. An initial
unoptimized diagnostic could not complete within the optional scan budget; this
was a diagnostic-build artifact, not evidence of a release scan-completion defect.
Independent Codex review found no actionable defects. Private captures and
inspection output remain ignored locally. Quantitative all-node acceptance is
not claimed from this one confirmed example.

Scan performance investigation (no performance changes): a release reader at a
250 ms start cadence completed sweeps at 10.47 s and 20.25 s (9.78 s for the
second sweep), averaging approximately 98-100 ms per sample. Each timing sweep
reported one exo node; counts differ from the earlier live capture as client
objects change. The 4096-slot step cap and repeated process identity/mapping
parses are the main improvement candidates. Bounded scatter proof validation
and allocation-light metadata parsing should be evaluated before increasing
work per step; preserve deadlines, identity checks and uncertainty semantics.

## Faster player discovery and metadata parsing

User requests faster player detection. Bounded worker player_detection_review
(gpt-6.1-sol) owns process.rs allocation reductions and discovery.rs substring
matching/parity tests; lead owns integration, dependencies, live measurements,
validation, documentation and publication. No worker live reads or nested workers.

Replaced byte-window name matching with memchr substring search, preserving
cross-chunk matches, overlaps, encoding order, deduplication and the occurrence
cap. Process stat parsing selects its needed field without collecting all fields;
map parsing uses a fixed five-field array instead of allocating per mapping.
All identity, mapping, cancellation, ownership and area validation gates remain.
The initial name search still finds string occurrences, not proven player identity.

Acceptance: parity with original matching across synthetic chunk sizes and edge
cases; parser boundary/error compatibility; full local gates, release timing,
installed live sampling and independent review. An ownership scatter experiment
was removed after it failed to improve the live baseline. Polling cadence remains
250 ms; no claim of faster screen refresh or relaxed checks.

Evidence: 151 workspace tests pass (3 opt-in private-asset checks ignored), 110
headless tests pass, strict Clippy on Rust 1.96/1.95, formatting and workspace/
release builds pass. All 12 installer tests and both nested-X managed startup/
relaunch checks pass. A final paired release test with ground spawns off produced
39/39 valid warmed samples for each build: median 10.76 ms before, 9.47 ms after;
first samples 338.24/319.28 ms. Earlier timings varied with live context, so no
larger speedup or universal latency guarantee is claimed. The 250 ms polling
cadence is unchanged. Private benchmark/probe data stays outside Git.
Installed validation: fresh controlled-pawn resource samples displayed, and the
optimized name search completed its full 64 MiB bounded batch without error
(no name occurrences in that batch). The app was returned to Map afterward.
This does not claim automatic character-name detection or identity proof.
Lead reproduced the synthetic substring benchmark (eight 64 MiB scans): old
window matching 550.98 ms, optimized finder 13.72 ms, both eight hits. This
approximately 40x local matching improvement excludes remote reads, metadata
checks and decoding; it is not an end-to-end detection-speed claim. Independent
Codex review found no actionable regressions in the final implementation.

## Character solo-feat checklist

User requests all solo On Duty feats ordered by CR, feat-point rewards (clarified,
not allocated skill points), and completed feats hidden with an unhide option.
Workers: solo_feat_catalog (catalog/source research), feat_api_research (completion
client), player_detection_review (model/view), all gpt-6.1-sol. Lead owns wiring,
background lifecycle, dependency/contracts, live operator, review and publication.

Acceptance includes lowest-CR first, level-30 entries first, completed toggle,
faction filtering where known, unknown status for missing IDs/data, cancellation
and stale-result rejection on character changes, full bounded Census pagination,
and visible catalog gaps. Public sources currently expose incomplete feat/ID
coverage; comprehensive current-game acceptance remains blocked until gaps are
resolved. No personal snapshots or raw research are included in published data.

Source audit delivered 40 current Normal-entry solo missions, 171 documented
mission-feat entries and 25 public/archived exact-ID mappings with provenance.
Nekron's Lair and Summit of Sorrow have no published feat lists in retrieved
sources; all other mission coverage remains partial. The public ToonData catalog
and bounded metadata research did not resolve remaining IDs. Unmapped feats stay
unknown and visible. Removed unrelated exploration/open-world feats and reconciled
mission/faction aliases rather than treating string matches as sufficient proof.

Implementation uses one bounded background completion worker, complete Census
pagination, cancellation and generation-based stale-result rejection. Refresh or
character changes clear previous completion evidence; errors never turn unknown
into incomplete. Character details remain accessible, while experimental resource
and name-scan information moved to Log. No game memory or map behavior changed.

Validation: 164 workspace tests pass (3 opt-in private-asset checks ignored),
121 headless tests pass, formatting, strict Clippy on Rust 1.96/1.95 and workspace/
release builds pass. All 12 installer tests and both managed-X11 startup/relaunch
checks pass. The installed app fetched the selected character and a complete
Census completion set. Searching a known completed Catwoman feat showed no row
with the default filter; enabling Show completed displayed its completed label
and 10-point reward. Restored hidden-completed/default search afterward. Initial
API timeout recovered on explicit retry; unavailable completion remained unknown.
Private captures and personal responses remain ignored. Comprehensive all-feat
coverage is explicitly not achieved; the remaining blocker is missing reliable
public feat definitions/IDs, not a hidden claim of incompletion.

Independent review caught hidden connection errors on Character after the layout
change. Restored connection progress/failure feedback beside the controls and
added a rendered-text regression. Updated gates pass: 165 workspace tests, 121
headless tests (unchanged core), both strict lints and workspace/release builds.
Final independent review follows the fix; source coverage remains partial.
Final independent Codex review found no actionable regressions after the feedback
fix. Lead-installed validation confirms the final build loads completion data and
retains the default hidden-completed filter; review did not independently verify
live Census data or source accuracy. The published milestone is the usable partial
checklist, not acceptance of the requested exhaustive all-solo feat catalog.

## Live feat completion fallback (2026-10-16)

Implemented read-only native feat completion and client definition decoding for
the exact supported build. Live discovery established the native manager entity
matches the controlled pawn. The native completed-ID set exactly matched a fresh
Census result. Two user-supplied positive/negative examples independently matched
the decoded title, reward and character-completion value. Personal observations,
addresses, archive bytes and probes remain ignored local data.

The bounded parser validates indexed trees, owner identity, serialized IDs, the
archive type, linked-block lengths and stable rereads. Unknown serialized variants
are omitted, never inferred incomplete. UI joins only unique exact title/reward
matches or consistent sourced IDs and keeps live current-character completion
separate from API references. Account completion does not set character completion.
Live reads run serially on the existing session worker while Character is visible;
Map does not schedule periodic feat scans. Source freshness and catalog coverage
remain explicit. Remaining catalog gaps and ambiguous matches are not resolved.

The first full live scan exceeded its two-second limit; consolidated cache-node
reads retained masked structural rechecks and reduced the measured scan to about
1.4 seconds on this workstation. The live fallback resolved both user examples.
Synthetic codec, malformed-data, mutation, source-separation and filtering checks
are implemented. Final application/UI validation, complete gates and independent
review are being completed before publication.

Final local gates pass: 178 workspace tests (3 opt-in private-asset checks ignored),
133 headless tests, strict Clippy on Rust 1.96 and 1.95, formatting, workspace and
release builds, 12 installer checks and both managed-X11 smoke launches. Installed
live validation shows the user-confirmed incomplete example and hides the completed
example by default; Show completed reveals its completed state and point reward.
Restored default filters after inspection. The session still has unresolved
catalog entries; the application reports them explicitly. Independent review is
pending; no complete-catalog or all-build acceptance claim is made.

Independent review found synchronous feat scans could expire the enclosing
one-second physics sample. Fixed with cooperative fresh physics reads during the
optional scan and a new physics read afterward, using the same session worker.
No old coordinates are retimestamped. A live scan took 1.56 seconds and emitted
six progress samples, with a maximum observed gap of 0.36 seconds and a final
physics age below 4 ms. Added a session regression for fresh progress preserving
location generation. Full affected gates and independent review are repeated.

After the timing fix, 179 workspace and 134 headless tests pass, along with both
strict lint versions, formatting, builds and both X11 smoke launches. The final
independent Codex review found no actionable regressions across tracked changes
and new modules. Lead live validation covers installed behavior; the reviewer did
not independently operate the game. The installed final build retains the verified
states and default filtering across refreshes.

### Feat cache follow-up (2026-10-16)

Added a bounded cache owned by the existing PID/build-bound native reader. It
retains decoded definitions and proven character completions across tab switches.
Confirmed completions never downgrade for the same owner. Unchanged completed
rows and serialized definition payloads are not reread; active rows and cheap
owner/archive/tree signatures remain validated. New completion trees are read and
unioned with proven facts. Unsupported definition IDs are remembered to avoid
repeated failed decoding. Cache changes publish only after stable rechecks; owner
changes rebuild it and a shrinking completed tree fails closed. No native pointer
or personal completion data is persisted across reconnects/app restarts.

The gpt-6.1-sol parser worker added six synthetic cache regressions covering hot
read avoidance, new completions, monotonic status, source/owner invalidation and
failed-refresh rollback. Lead integrated the cache and measured the same live
character at 1.52 seconds cold and 0.59 seconds warm (61% lower elapsed time),
with identical catalog statuses and all confirmed completions retained across a
tab toggle. Both user-confirmed example states remained correct.

Local gates: 185 workspace tests pass (3 opt-in private asset checks ignored),
140 headless tests, strict Clippy on Rust 1.96 and 1.95, formatting, workspace and
release builds, and 12 installer checks. Managed-X11 smoke, installed UI and
independent review are being finalized before updating PR #17.

Both managed-X11 smoke launches pass. Installed live UI retains the expected
24 completed / 106 incomplete / 38 unknown catalog status counts and default
filtering. Independent Codex review completed with no actionable regressions.

## Stable character checklist (2026-10-16)

User observed the checklist alternating between live and API counts during
refreshes. Retained feat knowledge now lives independently of expiring physics
samples. Same-owner successful snapshots merge observations and preserve confirmed
completions and omitted rows; transient errors and missing snapshots leave the
list intact. Known native owner changes and explicit session invalidation clear
it. The UI uses retained knowledge with its last successful read age.

The selected cached API faction filters both completion sources consistently,
explicitly labeled as an API reference rather than verified live identity.
Same-character reference refresh keeps existing details while pending/failed.
API feat refresh likewise retains previous results and unions confirmed IDs;
selection changes clear those API results and reject stale replies.

Worker player_detection_review (gpt-6.1-sol) owned faction filtering and API
completion retention with regression tests. Lead owned session/view integration.
Local gates: 192 workspace tests pass (3 opt-in private-asset tests ignored),
147 headless tests, formatting, strict Clippy on Rust 1.96/1.95 and debug/release
builds. Final installed UI checks and independent review follow before publication.

Installed live evidence retained 106 incomplete / 24 completed / 11 unknown with
the cached Villain faction across refreshes, including a rejected mutable memory
snapshot. Removed the conditional scan-error row that shifted the list; read age
remains visible. All 12 installer checks and both X11 smoke launches pass.

Independent review identified a native entity-binding change that can happen
without changing Unreal ownership objects, plus hidden API refresh failure status.
Propagated the independently checked native binding so retained knowledge clears
before replacement scans, added a regression, and made API status occupy a stable
line whether details are retained or not. Repeating affected gates and review.

Re-review found the full physics ownership comparison also includes ordinary
streaming-context changes. Feat retention now uses character object/class identity
plus the independently read native entity binding; map ownership rules remain
unchanged. Expanded the gap regression with a streaming change. All affected
gates pass: 193 workspace / 148 headless tests, both strict lint versions, format
and builds. Final independent re-review follows.

Final independent Codex review found no actionable regressions. The installed
final binary matches the release build; lead live UI validation shows the stable
Villain-filtered list and read-age/status lines. Private screenshots retained
locally. The independent reviewer did not operate the game.

## Resolve remaining completion unknowns (2026-10-16)

Investigated the 11 unknown catalog entries visible for the connected Villain.
Four failed exact matching due to trailing whitespace in client titles; two had
identical title/reward pairs in solos and duos, distinguished by exact mission
requirements. Three catalog mismatches were a conflicting public Fun Times ID,
the Constantine Shmonstantine spelling, and the At Arm's Length reward (10).
Two Gotham Sewers entries are explicitly Hero-only in published mission sources;
corrected faction metadata rather than inventing completion. Minimal correction
provenance is in docs/feat-catalog-corrections.md; personal snapshots remain local.

Worker player_detection_review (gpt-6.1-sol) implemented trimmed exact matching and
unique exact-description disambiguation with synthetic ambiguity, reward and ID
regressions. Lead reconciled catalog data and performed serial read-only live
validation: 26 completed / 113 incomplete / 0 unknown applicable catalog rows,
with cold and cached snapshots agreeing. The current snapshot reflects new
completions since earlier observations; no statuses are hardcoded.

195 workspace tests pass (3 opt-in private-asset checks ignored), 150 headless
tests, both strict lint versions, formatting, debug/release builds and 12 installer
checks. Installed build and managed-X11 smoke validation are finishing before
independent review/publication. This resolves current applicable catalog unknowns,
not missing mission catalogs, other characters/factions or unsupported builds.

Both managed-X11 smoke launches and installer checks pass. Final independent
Codex review found no actionable regressions. Lead verified the actual live
reader/model results above; the reviewer did not independently verify live data
or external sources. The updated application is installed. Window capture on the
current monitor returned blank, so no new rendered-UI screenshot acceptance is
claimed; this change alters catalog/matching only, with live model and existing
view tests providing evidence.

## Persist faction on ordinary launch (2026-10-16)

User still saw Hero feats after connecting through the normal launcher. Installed
bytes already matched the prior build, but two app copies were open. The ordinary
launcher supplied no API reference, and connect also discarded any previously
loaded reference, removing the faction filter.

Added an explicit saved All/Hero/Villain filter. Newly observed API character IDs
seed the filter from their alignment; users can override it, and ordinary launches
restore it without saving personal character data. Connecting preserves the
independent API reference while clearing native session knowledge. Faction remains
a labeled selection, not proof of live identity. Save writes only this immediate
preference and preserves other pending/saved settings.

Worker player_detection_review (gpt-6.1-sol) owned model/view filtering and tests;
lead owned settings, API lifecycle, integration and installed restart validation.
199 workspace tests pass (3 opt-in private-asset checks ignored), 153 headless
tests, formatting, both strict lint versions, builds and installer checks. Closed
both old app copies, upgraded the local install, confirmed API lookup saves Villain,
and relaunched with process connection only for normal-launch validation.

Installed ordinary process-only launch restored Villain with no API reference and
showed 113 incomplete / 26 completed / 0 unknown. Captured private UI evidence and
confirmed only one app copy remains. Independent review found the existing reset
preferences handler did not reset the new visible selector; synchronized it and
reran affected tests, lint and builds before re-review.

Final independent Codex review found no actionable regressions. Final updated
binary installed and process-only launch restored; all required local gates pass.

## Broker tab (2026-10-16, implemented and locally validated)

- [x] Clarify comparison target: existing broker listings.
- [x] Verify public API collection discovery and sibling API routes: no verified
  Broker listings/prices; equipped item references are not market data.
- [x] Pair live My Wares screenshot with bounded cache read: all ten visible prices
  and seller ownership match. Search comparison matches seven visible results,
  including the owned listing and six competing listings for the same item ID.
- [x] Complete bounded parser and mutation/corruption regressions.
- [x] Resolve item names without personal hardcoded fixtures.
- [x] Add session-bound retention, comparison model and Broker tab.
- [x] Verify installed UI, lifecycle, tests/lint/build and independent review.

Evidence remains private in ignored local-data. Quantity semantics, durable query
mode and whole-market coverage remain unproven. The active response cache is shared
by My Wares and Browse; ownership must not be inferred from current query alone.

Installed live UI retained all ten owned listings while switching to the searched
item and showed the matching competing total and difference. Read ages continue
to advance rather than resetting on cache reads. The first response after attach
is treated as a baseline; only a later user-driven query populates the session.
The initial full gates passed (216 workspace, 163 headless, both strict Clippy
versions, builds, 12 installer tests, managed-X11 startup/relaunch). Independent
review identified late-loaded names discarded for repeated tokens; added bounded
name-only enrichment preserving timestamps with an expiry/mismatch regression.
Affected gates and independent re-review are in progress.

The late-name regression is fixed without renewing prices or reviving expired
rows. Final workspace tests: 217 passed, 3 private-asset checks ignored; both strict
Clippy versions and release build pass. Independent Codex re-review found no
actionable bugs. Final binary is installed; the already-open validated Broker
window retains its session observations and uses the final cache fix next launch.
Publication and remote CI are tracked in PR #21.

## Broker Update prices (2026-10-16, active)

- [x] User confirmed normal Broker controls and a button-triggered workflow.
- [x] Bounded sequential queue, correlation, cancellation and timeout tests.
- [ ] Process-bound supported-layout desktop adapter.
- [ ] Update prices, progress, Cancel and partial-price result integration.
- [ ] Live full-queue validation, gates, independent review and publication.

The queue, button/progress/Cancel and desktop adapter are integrated. User further
requested disabling Usable by me; preparation now recognizes checked/off pixels,
only toggles a positively checked box and verifies off before searches.
Independent initial review identified pointer interference and an old Browse
response baseline. Added exact pointer guards plus direct XTEST button pairs and
a post-preparation response baseline. 232 workspace tests and both strict Clippy
versions pass; headless tests, builds, installer checks and managed-X11 startup
pass. Live button validation and re-review remain pending.

Live prototype loaded the ten owned listings and queued seven distinct item groups.
The first run stopped at zero searches because Usable by me remained checked after
the synthetic click; no price scan acceptance is claimed. Cursor capture also
prevented reliable operator access to the companion. User then explicitly switched
to injection. Halted the prototype application; normal-controls work remains local
and unaccepted. Offline research must establish ABI/thread/argument/lifetime
contracts before the injection path can be implemented or tested.

Offline injection investigation completed without loading code or writing game
memory. The previously identified searchAuctions address is a branch inside the
GFx dispatcher, not a callable entry point. Static analysis mapped request fields
and found a smaller searchItem path, but query enum meanings, native GFx ownership,
UI-thread scheduling, helper loading and shutdown are not proven. A Windows Rust
helper toolchain is also not installed. Injection remains blocked on these
contracts; no automatic price update is accepted or ready for publication.
Transport-independent queue committed as 80140bb with five synthetic tests passing.
The failed desktop prototype remains local for reference, not release acceptance.

Restored the installed companion from merged commit ea56b27 after a successful
release build. The unsupported automatic-input prototype is no longer installed.

## Broker price hotkey (in progress)

Implemented selection of observed competing item/quantity groups, editable price,
one-shot 60-second arming and cancellation. Keyboard worker observes Ctrl+Alt+P
without pointer movement or a global grab, waits for release, validates process
and focus, then types digits without Enter. Full validation and live field
acceptance remain pending. Automatic search and injection remain unfinished.

Hotkey validation: 229 workspace tests pass (four opt-in checks ignored), 168
headless tests, both strict Clippy versions (1.96/1.95), workspace/release builds,
12 installer tests and two managed-X11 startup/relaunch checks pass. The opt-in
Xvfb typing check was run separately and passed exact digit press/release events
with no mouse/Enter events. Review found effective locked/latched modifiers could
change digit output; fixed with effective modifier validation and verified held
Shift/released Caps Lock rejection on the private X server. Re-review found no
remaining actionable issues. Installed the updated build and relaunched the
companion. Live replacement in the game's price widget is still unverified; no
listing is submitted by this feature and no release acceptance is claimed yet.

## Broker search then price fill (in progress)

User superseded direct price entry: first hotkey searches the selected name from
Browse; fresh response supplies lowest matching total minus $1; second hotkey
fills that amount on My Wares, without Enter or a second subtraction. Selection
now includes owned items without requiring a competing observation beforehand.
No cached response may complete a new search. Full gates/review/live acceptance
are pending. The earlier direct-price flow is superseded.

Validation: 232 workspace tests pass (four opt-in checks ignored); all six
keyboard tests pass when run on a private Xvfb server, including exact search
selection/text/Return and price selection/digits without Return. Independent review
found an earlier in-flight response could satisfy the price wait. Added a fresh
matched baseline handshake before Return using the existing reader, followed by
exact next-token/post-completion matching and a regression. Final review found no
further actionable findings. Final build/install checks are in progress; actual
in-game search, clipboard and price-widget behavior remain unverified.

Final strict Clippy checks pass on Rust 1.96 and 1.95; workspace and release
builds pass. Both managed-X11 startup/relaunch checks pass. Reused unchanged
headless and installer evidence from the preceding hotkey implementation. Updated
local install and relaunched the single companion. Live game acceptance remains
pending, so no PR publication/merge acceptance is claimed for this workflow.

## Automatic selected item (in progress)

Removed manual arm/selection controls. Native research did not establish selected
item fields, so the supported-layout fallback uses explicit screen evidence.
Offline screenshot recognition matched the prior selection; three live OCR reads
matched the user-changed selection. Personal names/screenshots remain ignored.
Two consecutive names are required; a selection change clears prepared prices.
Neither current hotkey action sends Return. Initial native Broker context is now
published for correlation, while BrokerView suppresses baseline listings.

Review fixes cover cancelled unfinished name workers, OCR restart, consecutive
selection evidence, fresh memory and filter-token correlation. Market rows must
have a single unambiguous item/quantity and quantity candidate one; selected
inventory quantity remains unverified and is explicitly a single-item workflow.
Installed Tesseract/English OCR runtime on this host. Live name recognition passes;
full automatic paste/search/price field acceptance remains pending.

Final validation: 240 workspace tests pass (four opt-in checks ignored), 168
headless tests, both strict Clippy versions, workspace/release builds, focused
Broker regressions and all six isolated Xvfb keyboard tests pass. Managed-X11
startup and relaunch both show normal windows. Reused unchanged installer tests.
Final independent review found no further actionable issues after filter token
floor and OCR recovery fixes. The shared filter position is verified in both
supported Broker panels. Updated the local install and relaunched the companion.
Live recognition of the user's changed item succeeded on three consecutive reads.
End-to-end field pasting/search/price verification remains a user live acceptance
gate; no automatic transaction or injection occurred.

## Hotkey readiness repair

User reported no hotkey response. Live thread inspection showed Broker OCR and
keyboard workers disappearing/restarting during ordinary sampling. Application
lifecycle incorrectly tied Broker controls to transient one-second physics
observations; it now retains them across missing physics/binding samples, clears
on a proven owner or build/session change, and gates input with Broker's own
five-second freshness (OCR remains three seconds). Pause/disconnect/tab exit
still cancel. Added a regression for missing-physics routing.

The workstation's Num Lock was on, which the old modifier guard rejected. Worker
hotkey_failure_review (gpt-6.1-sol) added dedicated-mapping Num Lock support while
retaining all held-key and other effective-modifier checks. Seven isolated Xvfb
keyboard tests pass, including mixed-case text and digits with Num Lock on and
ambiguous modifier rejection. Added Listening/Detected events for visible feedback.
All 242 workspace tests pass (four opt-in checks ignored), strict Clippy on both
toolchains and release build pass. Independent review found no blocking regression.
Installed and relaunched the fix; physical game hotkey acceptance is still pending.
A bounded diagnostic observed only Ctrl/Alt/P states, with no unrelated keystrokes
recorded. No physical chord was captured during that diagnostic window.

## October 20 client update and universal Broker hotkey (active)

User authorizes restoring map reads after the game update and keeping the Broker
hotkey available across companion tabs. Lead owns build-profile investigation,
read-only live evidence, integration, documentation, local commits and validation.
Worker universal_broker_hotkey (gpt-6.1-sol) owns src/application.rs and
src/broker_view.rs only: move Broker polling/control lifecycle out of tab rendering,
keep one existing reader/listener across tabs, preserve pause/disconnect/session,
freshness and game-focus guards, and add focused synthetic regressions. Existing
session/native interfaces stay unchanged; request shared changes through lead.
Worker must not operate the live desktop/game or commit. Deliver reviewed changes
and test evidence, then stop. Lead runs integrated checks after worker completion.

Worker ownership extension: src/broker_price.rs is also assigned to
universal_broker_hotkey because its private tick currently runs inside UI drawing.
Expose a background poll entry point for existing control state; preserve action
contracts and keep presentation separate. Lead does not edit these three files.

User refinement: outside Broker tab run only the shortcut listener; activate
screen/listing work on a press and return idle after bounded work. Supersedes
always-on monitoring above. Worker also owns src/broker_hotkey.rs for detection-only
and already-triggered guarded input paths. A queued press expires promptly; pending
manual search response remains bounded by its existing 120-second limit. Keep
prepared price without background OCR/listing reads until the next press.

Worker update_review (gpt-6.1-sol), read-only independent review: inspect the final
map-profile/object-table changes and then the on-demand hotkey changes once lead
signals they are ready. No file ownership or live reads/desktop operation. Check
exact-build rejection, bounded memory, tab/session lifecycle, idle-work guarantees,
input freshness and failure paths; report actionable findings with file/line
references. Stop after review; no edits, commits, or nested workers.

Completed local implementation and installation (2026-10-20): map-profile commit
`dedaeda` restores the exact updated build after read-only schema, area, position
and Broker-binding validation. Expanded only reserved object-table capacity;
populated-entry and scan budgets stay bounded. MHQS104 map import passed with the
new profile. Installed UI then automatically selected MPYR0242 after the user's
area change and displayed the live player marker plus ground-spawn markers.

On-demand Broker controls use a detection-only listener outside Broker. Released
press timestamps expire after five seconds and are consumed once; OCR/listing
reads start only for an active request or pending manual search, then stop. A
prepared price remains idle until another press. Session/owner/pause/freshness,
focus and per-stroke input guards remain. No automatic transaction was added.
Worker focused checks passed; independent update_review found no actionable
issues, including the final timestamp/replay changes.

Final gates: 247 workspace tests passed (4 opt-in checks ignored), 169 headless
tests passed, formatting and strict all-target Clippy passed on default Rust and
Rust 1.95.0, workspace/release builds passed, and all 12 installer tests passed.
All seven keyboard tests passed separately on private Xvfb, including the opt-in
input-event check. The first concurrent X11 smoke run lost its test display;
a separate rerun passed both normal-window startup/relaunch checks. Updated the
local install and relaunched one connected companion. Actual installed off-tab
thread inspection showed a shortcut listener and no OCR monitor. Private UI
capture confirms the restored map. End-to-end physical Broker shortcut/search/
price-field acceptance is still unverified; no publication or release performed.

## October 22 connection and reported update repair (active)

User requests updated offsets and automatic process detection/connection. Lead
owns read-only live build/profile diagnosis, integration, status, checks and commits.
Worker auto_connect (gpt-6.1-sol) owns src/session.rs, src/settings.rs and
src/application.rs: default startup discovery/connection for exactly one candidate,
bounded waiting when absent, ambiguity/build rejection and explicit disconnect
cancellation, using existing process/native interfaces. Add lifecycle regressions;
no desktop/game operations, commits or nested workers. Stop with focused evidence.

Worker connection_review (gpt-6.1-sol) independently reviews the automatic
connection diff and lifecycle regressions. Read-only ownership; no live game or
desktop operations, edits, commits or nested workers. Check startup queue ordering,
unsupported/ambiguous selection, cancellation, stale events and retry behavior;
report actionable findings and stop. Lead resolves findings and owns final gates.

Live diagnosis: executable remains the exact October 20 profile. Map/resources
and user movement resolve with existing offsets, but the feat archive vtable was
still the prior build's 0x1d94a68. Verified the current type-tree binding and PE
entries at 0x1d94a78; corrected this field only. Cold/warm live reads now decode
4,778 definitions. Added a wrong-type cache preservation regression. Worker
connection_review also reviews this change using lead evidence, without live reads.

Startup review found automatic attachment cancelling a pending API reference.
Worker fixed it by retaining the initial generation/token and pending work for
first automatic attachment only; explicit session changes still invalidate. A
synthetic delayed-reference regression passes. Integrated gates are in progress.

Final validation: 254 workspace tests passed (4 existing opt-in checks ignored),
176 headless tests passed, strict all-target Clippy passed on Rust 1.96 and 1.95,
formatting, workspace/release builds and 12 installer checks passed. Both managed
X11 startup/relaunch checks passed. Independent connection_review found no further
actionable findings after the API fix and feat offset review.

Updated the local installation and launched without a PID argument. The installed
app automatically selected the running game and displayed live feat completion
with zero unknown applicable entries. Only one companion remains running. Private
UI captures stay ignored. No new full map-calibration, Broker hotkey, transaction
or release/publication acceptance is claimed.
