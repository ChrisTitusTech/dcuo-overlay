# Phased roadmap

All implementation phases are pending. Each phase produces a reviewable commit
and evidence against its exit criteria. Discovery results may narrow supported
features; record that explicitly rather than marking unproven features complete.

## Phase 0: Linux and Wine feasibility

Record the target machine, compositor, launcher, runtime, display mode, and game
build. Identify the correct process and mapped executable. Prototype a bounded
read of a known character value and a transparent overlay with click-through and
an edit toggle. Determine a workable process-access arrangement.

Exit: repeatable reading across three game launches and demonstrated overlay
behavior on the target desktop. Record focus, stacking, scaling, and fullscreen
limitations. Select the initial backend only from this evidence.

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

## Phase 4: First usable overlay

Deliver the character panel using only proven fields. Add placement persistence,
scaling, opacity, hide/show, and click-through/edit modes. Display source and
unavailable/stale status without overwhelming the main panel.

Exit: a full play session demonstrates focus behavior, character switching,
reattachment, and recovery. Record CPU, memory, update latency, and game frame-time
impact against a baseline on the target machine. Set acceptance budgets from
Phase 0 measurements before this validation, not after seeing results.

Milestone: a usable memory-backed character overlay with API cross-verification.

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

Test the supported launcher/compositor matrix, extended sessions, scaling,
outages, restarts, and unsupported game updates. Package the application with
setup instructions, diagnostics, local data locations, and profile controls.

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
  substitute for live overlay and memory validation.
