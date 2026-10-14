# Phased roadmap

All implementation phases are pending. Each phase produces a reviewable commit
and evidence against its exit criteria. Discovery results may narrow supported
features; record that explicitly rather than marking unproven features complete.

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

Exit: repeatable reading across three game launches and demonstrated companion
behavior on the target two-monitor desktop. Select the initial backend only from
this evidence, with the following checks:

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

If either experiment fails, document the specific blocker and the next bounded
experiment. Do not build the full UI around an unproven backend.

## Phase 1: Rust foundation

Create the Rust package, module boundaries, configuration, diagnostics, typed
observations, lifecycle states, and replay mode. Add CI for the actual package.
Install required development tools and prove build, lint, and tests work.

Exit: fixtures drive state without the game; malformed input, absent processes,
character switches, and network failures do not crash or leak old session data.

## Phase 2: Stable memory observations

Discover identity first, then investigate CR, skill points, maximum/current
health, and power. Build executable-specific profiles with bounded reads and
explicit unsupported-build behavior. Validate through controlled in-game changes.

Exit: every supported field has evidence across launches and transitions, plus
decoder regression fixtures. Invalid profiles disable their readers. Record
undiscovered fields as unsupported rather than implying complete coverage.

## Phase 3: API cross-verification

Implement exact identity matching, normalized field comparisons, cached reference
data, independent refresh schedules, bounded retries, and visible freshness.
Evaluate API contract gaps and implement only necessary additive changes in the
API repository when that work is authorized.

Exit: tests cover correct/wrong identities, short-name ambiguity, stale equipment,
unknown freshness, outages, and rate limiting. A live comparison records actual
API responses and controlled observations without publishing personal snapshots.

## Phase 4: First usable companion window

Deliver the character panel using only proven fields and the Phase 0 managed X11
backend. Add responsive layout, adjustable UI scaling, and saved UI preferences
while respecting DWM placement and input. Display source and unavailable/stale
status without overwhelming the main panel.

Exit: a full play session passes the Phase 0 companion checks with the real panel
and demonstrates character switching, reattachment, and recovery. Record CPU,
memory, update latency, and game frame-time impact against a baseline on the
target machine. Set acceptance budgets from Phase 0 measurements before this
validation, not after seeing results.

Milestone: a usable memory-backed companion window on another monitor with API
cross-verification.

## Phase 5: Combat and quality-of-life features

Confirm priorities before selecting features. Evaluate combat logs as a source
for personal analytics, with memory supplying proven live context. Add personal
DPS, ability breakdowns, crit rate, fixed-duration parses, history, and personal
bests incrementally. Evaluate LFG reminders, Nth Metal tracking, and a chat-length
helper separately according to available sources.

Exit: recorded encounters reproduce expected totals and duration, source overlap
does not double count events, and log rotation/session changes are handled.
Estimates remain labeled. Group metrics and other unproven sources remain deferred.

## Phase 6: Hardening and packaging

Test the supported launcher/desktop matrix, extended sessions, scaling,
outages, restarts, and unsupported game updates. Package the application with
setup instructions, diagnostics, local data locations, and profile controls.
Include verified DWM-Titus monitor/tag setup, identity inspection commands,
monitor-disconnection recovery, and any setup rule with rollback.

Exit: build, formatting, lint, tests, and documented live smoke tests pass. Verify
install/uninstall and rollback to the previous package/profile. Disable affected
readers on unsupported builds while preserving independent features.

## Validation and rollback throughout

- Record commands, outcomes, fixture versions, environment, and observed limits.
- Use small synthetic or sanitized fixtures instead of process-memory dumps.
- Run relevant checks after changes; retain unchanged passing evidence.
- Keep new readers/features individually disableable.
- Version configuration/history schemas and preserve user data on rollback.
- Publish releases only when explicitly authorized; a passing CI run is not a
  substitute for live companion-window and memory validation.
