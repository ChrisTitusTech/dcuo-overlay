# Phased roadmap

Phase 0 is underway; no phase has met all exit criteria. See
[current evidence and blockers](docs/phase-0.md). Each phase produces a reviewable commit
and evidence against its exit criteria. Discovery results may narrow supported
features; record that explicitly rather than marking unproven features complete.

The user requested continuation to a verifiable application. A bounded vertical
slice now integrates session/build diagnostics, exact API reference lookup and
experimental memory-name search. This exercises parts of Phases 1 and 3 while
Phase 0 live profile/map discovery remains open; it does not certify those phases.
See [user verification and remaining gates](docs/verification-slice.md).
The merged map slice adds experimental native resources/position, a local map importer,
Gotham live preview, independent API work and persistent application settings.
See [live map evidence and remaining gates](docs/live-map-preview.md).
Offline hardening adds bounded API cache/retry, opt-in exact-build reconnect,
cancellable import, calibration provenance and reversible local packaging.
[Offline evidence](docs/offline-hardening.md) keeps remaining live gates explicit.
The resumed live slice adds automatic on-demand imports for installed areas
(all 313 main overviews decode and cache successfully), including indoor maps. The user confirmed MHDT0107
map/marker movement; other indoor routes, quantitative accuracy and floors remain
open. See the updated [live map evidence](docs/live-map-preview.md).

## Development worker execution

Use extra `gpt-6.1-sol` workers when a phase has independent work that benefits
from parallel execution or a separate review. Explicitly select that model when
spawning; report unavailability rather than silently substituting another model.
Use no more than three extra workers alongside the lead, and respect lower
runtime limits. This is a ceiling, not a quota: small edits and work waiting on
the same dependency remain serial. The lead schedules all workers, including
reusing an existing worker for a follow-up, with no nested delegation.

Before spawning, record the assignment in TASKS.md: authorized phase, deliverable,
prerequisites, owned files, shared interfaces, required checks, and stop point.
The lead creates shared scaffolding and agrees observation/session/map/API
contracts before dependent implementation. Workers own disjoint files and request
changes to shared contracts through the lead. The lead owns shared manifests and
lockfiles, module wiring, integration, plan/status updates, and Git operations.
Workers return changed files, check commands/results, and unresolved limitations.
Concurrent checks must not modify the same generated files or output directories.

| Phase | Suitable worker assignments | Prerequisites and lead-owned work |
| --- | --- | --- |
| 0: Feasibility | Managed X11 prototype; bounded reader prototype/offline decoder review; map source, coverage, and calibration research | Coordinate prototype interfaces and files. One operator performs live reads, routes, restarts, and desktop experiments; workers analyze recorded sanitized evidence. Select the backend only after live evidence. |
| 1: Foundation | Configuration/diagnostics; replay fixtures and error-path tests; application shell/tab presentation | Lead creates the Rust package and shared types first, and owns module wiring and CI. Shell work can use labeled fixtures while discovery continues. |
| 2: Memory | Character-field decoders; location/context decoders; independent profile and regression review | Establish identity/session contracts first. Live discovery and profile promotion remain coordinated; each field needs its own evidence. |
| 3: API | Transport/cache/retry; normalization/comparison; independent stale-response and failure-path tests | Agree typed responses, exact identity matching, and session binding first. Lead integrates refresh scheduling and live comparison. Changes in dcuobot-api require their own authorized scope. |
| 4A/4B: Application | Character presentation; map catalog/projection/view; settings/diagnostics/accessibility | Assign separate modules after session and map contracts stabilize. Lead integrates shared controls, cancellation, session invalidation, and background lifecycle before full live acceptance. |
| 5: Hardening | Packaging/help; map coverage and asset-permission audit; independent diagnostics/regression review | Lead runs integrated release gates and live smoke tests, records remaining gaps, and handles any explicitly authorized publication. |
| 6: Optional features | Separately selected analytics or quality-of-life features and replay tests | Confirm feature priorities and event contracts first. Lead owns shared ingestion, deduplication, and session boundaries. |

Choose only the assignments needed for the current phase, and divide tests from
implementation only when their contracts and files are independent. A worker may
review a coherent completed change without editing it. The lead inspects all
results, resolves conflicts, runs relevant integrated checks, and commits only
accepted changes. Record worker evidence and remaining blockers in TASKS.md;
a worker's success report does not replace phase exit criteria.

Do not run competing live discovery or acceptance experiments against the same
game session. Game interaction and desktop/setup changes retain their existing
authorization requirements. Parallel fixture-based UI/API work cannot establish
live memory or map feasibility, and no worker may advance past a plan-only stop
point. Development workers are distinct from runtime application workers: the
application still shares one session model and must not duplicate readers.

Model selection follows the [OpenAI subagent guidance](https://learn.chatgpt.com/docs/agent-configuration/subagents).
Use explicit selection for this project; no global model-setting change is needed.

## Phase 0: Linux and Wine feasibility

Record the target Fedora/X11 machine, running DWM-Titus build, monitor layout and
scaling, chosen companion monitor/tag, launcher, runtime, display mode, and game
build. Identify the correct process and mapped executable. Prototype a bounded
read of a known character value and a normal managed companion window on another
monitor following the SPEC.md contract. Determine a workable process-access
arrangement.

Create the main window with `override_redirect=false`, normal window type, and
stable class/instance/title properties. Prove DWM placement and ordinary window
interaction, then document an optional monitor/tag rule for repeatable startup.
Record any required setup change and its rollback before selecting the backend.

Investigate map feasibility alongside the bounded identity experiment, with
parallel offline analysis and a single scheduled live operator as described
above, without building the full map UI. Prove controlled-player position and
enough zone/instance/floor context to distinguish one open-world area and one indoor
instance, including exit/re-entry. Identify usable map assets and calibrate an
initial transform against independent landmarks. Record the broader first-release
coverage inventory for both categories, asset provenance, unresolved readers,
and preliminary accuracy/update-latency budgets. A pair of prototype maps is
feasibility evidence, not a claim that the whole inventory is supported.

Exit: repeatable reading across three game launches and demonstrated companion
behavior on the target two-monitor desktop. Select the initial backend only from
this evidence. Record a map feasibility result and remaining coverage gaps, with
the following window checks:

- Locate the main window with `xwininfo` or `xwininfo -root -tree`. Run
  `xwininfo -id 0xWINDOW_ID` and require `Override Redirect State: no` on initial
  display, reopening, and any main-window recreation.
- Run `xprop -id 0xWINDOW_ID WM_CLASS WM_NAME _NET_WM_NAME _NET_WM_WINDOW_TYPE _NET_WM_STATE`
  and verify the stable identity and normal window type. Confirm DWM manages
  tiling/floating, borders, resize, monitor/tag movement, and focus cycling.
- Place the game and companion on different monitors. Verify placement on
  relaunch and keep live updates visible while the game has focus in windowed,
  borderless, fake-fullscreen, and real fullscreen modes. Record any game pause,
  minimization, or focus-switching limitation instead of assuming simultaneous
  keyboard focus. Refreshes must not steal focus or change stacking.
- Exercise normal pointer, scroll, and keyboard input in the companion, then
  return to the game. Switch tags, minimize/restore or exit the game, and restart
  it. The companion follows its own DWM visibility and reports session state
  without automatically hiding or leaking previous-character values.
- Resize/tile the companion, change resolution/scaling, and disconnect/reconnect
  its monitor. Require readable layout and a reachable fallback window. Verify
  rendering with and without Picom, without special compositing rules.

Automate X11 attribute and UI/session-state regression checks where useful. A
nested X server or synthetic window test does not replace live DWM-Titus and
Wine/Proton acceptance evidence on the two-monitor setup.

If a memory, window, or map experiment fails, document the specific blocker and
the next bounded experiment. Do not build the full UI around an unproven backend
or claim live maps from placeholders. Independent shell/character work may use
fixtures while map discovery continues, but the map milestone remains blocked
until live location and usable calibrated assets are demonstrated.

## Phase 1: Rust foundation

Create the Rust package, module boundaries, configuration, diagnostics, typed
observations, lifecycle states, and replay mode. Add CI for the actual package.
Install required development tools and prove build, lint, and tests work.
Add the managed application shell with Character, Map, and Diagnostics tabs,
shared session header/status, connection controls, Settings, and Help/About/Quit.
Use clearly labeled synthetic replay data for offline UI work. Define versioned
location observations and map-catalog schemas with bounded asset validation.

Exit: fixtures drive state without the game; malformed input, absent processes,
character switches, and network failures do not crash or leak old session data.
Every tab has loading/empty/error states, keyboard navigation, and one shared
session model. Tab changes do not spawn duplicate readers. Configuration errors
are recoverable, and closing the application cancels background work.

## Phase 2: Stable memory observations

Discover identity first, then investigate CR, skill points, maximum/current
health, and power. Discover controlled-player position and the zone/instance/
layout/floor context needed by the map inventory. Build executable-specific
profiles with bounded reads and explicit unsupported-build behavior. Validate
through controlled in-game changes, routes, and separate landmark observations.
Treat heading as optional until independently proven.

Exit: every supported field has evidence across launches and transitions, plus
decoder regression fixtures. Invalid profiles disable their readers. Record
undiscovered fields as unsupported rather than implying complete coverage.
Location evidence must distinguish player from camera/target coordinates and
cover open-world and indoor/instance transitions, vertical/floor changes,
teleports, death/respawn, and repeated instance entry. Reject mixed or delayed
samples across session/zone changes. Identity success does not certify location.

## Phase 3: API cross-verification

Implement exact identity matching, normalized field comparisons, cached reference
data, independent refresh schedules, bounded retries, and visible freshness.
Evaluate API contract gaps and implement only necessary additive changes in the
API repository when that work is authorized.

Exit: tests cover correct/wrong identities, short-name ambiguity, stale equipment,
unknown freshness, outages, and rate limiting. A live comparison records actual
API responses and controlled observations without publishing personal snapshots.
The current-character view remains useful with the API offline; verified local
position updates continue independently. Optional API portraits/details must
never bind a previous or merely searched character to the active memory session.

## Phase 4: First usable companion window

### 4A: Application shell and current character

Integrate the Phase 1 shell with the proven memory readers and API comparisons.
Deliver current-character identity, live resources, and supported stats with
explicit per-field status. Finish session selection, connect/disconnect, pause/
resume, reconnect behavior, settings, diagnostics, help, and orderly shutdown.
Add responsive layout, adjustable UI scaling, a consistent theme, application
icon, and saved preferences while respecting DWM placement and input.

Exit: no-game, ambiguous-session, access-denied, unsupported-build, loading,
character-switch, partial-read, API-offline, and restart scenarios have useful
UI states and recovery actions. Keyboard-only use and all tabs work at the
declared minimum window size and tested UI scales. No old-character values or
asynchronous results appear in a new session.

### 4B: Live Map tab for both map categories

Implement the catalog, asset loading/cache, verified world-to-map transforms,
zone/layout/floor selection, and marker from the same session model as Character.
Add zoom/pan, fit, follow/recenter, floor selection, and labeled map browsing.
Support the declared first-release inventory across open-world and indoor/
instance maps; publish coverage and unresolved areas without implying all maps.

Exit: live controlled routes and independent landmarks meet the agreed error and
latency budgets on every supported map/layer. Test floor overlap, instance
exit/re-entry, zone loading, teleport, death/respawn, character switch, and game
restart. Stale/paused/invalid/ambiguous location hides the live marker; wrong-map
browsing cannot display it as current. Missing/corrupt assets and API outages do
not break Character. Synthetic tests cover projection/zoom/resize, invalid
coordinates, old-zone results, and catalog/asset limits. A fixture-only map cannot
complete this stage.

Combined exit: a full play session passes the Phase 0 companion checks with both
tabs and demonstrates character switching, reattachment, and recovery. Measure
CPU, memory, location-update latency, and game frame-time impact against the
Phase 0 budgets, including Map visible/hidden and API offline. Record visual
acceptance evidence for ordinary, empty, loading, stale, and error states using
synthetic or sanitized data. Set budgets before validation, not after results.

Milestone: a complete managed companion application on another monitor showing
the current character and its live location on supported open-world and indoor/
instance maps, with independent per-field API cross-verification.

## Phase 5: First-release hardening and packaging

Package the Character/Map application before optional combat features. Test the
supported launcher/desktop matrix, extended sessions, scaling, outages, restarts,
unsupported game updates, map coverage, and monitor-disconnection recovery.
Provide a desktop entry and icon, version/About/credits, setup and troubleshooting
help, XDG data/config/cache locations, diagnostic export, and profile/map controls.
Document DWM-Titus monitor/tag setup and any setup rule with rollback, plus map
asset installation, source permissions, versions, calibration, and supported areas.

Exit: build, formatting, lint, tests, and documented live smoke tests pass for
both Character and Map. Verify desktop launch/close, install/uninstall, malformed
configuration recovery, and rollback of package/profile/map-catalog versions.
Review bundled asset permissions and diagnostic redaction. Disable affected
readers/maps on unsupported updates while preserving independently valid features.
Publishing the first release still requires explicit authorization.

## Phase 6: Later combat and quality-of-life features

These optional features do not block the Character/Map first release. Confirm
priorities before selecting features. Evaluate combat logs as a source
for personal analytics, with memory supplying proven live context. Add personal
DPS, ability breakdowns, crit rate, fixed-duration parses, history, and personal
bests incrementally. Evaluate LFG reminders, Nth Metal tracking, and a chat-length
helper separately according to available sources.

Exit: recorded encounters reproduce expected totals and duration, source overlap
does not double count events, and log rotation/session changes are handled.
Estimates remain labeled. Group metrics and other unproven sources remain deferred.

## Validation and rollback throughout

- Record commands, outcomes, fixture versions, environment, and observed limits.
- Use small synthetic or sanitized fixtures instead of process-memory dumps.
- Run relevant checks after changes; retain unchanged passing evidence.
- Keep new readers/features individually disableable.
- Version configuration/history schemas and preserve user data on rollback.
- Version map catalogs/assets/calibrations independently and keep per-area
  acceptance evidence. Never substitute a synthetic marker for live validation.
- Publish releases only when explicitly authorized; a passing CI run is not a
  substitute for live companion-window and memory validation.
