# Phased roadmap

All implementation phases are pending. Each phase produces a reviewable commit
and evidence against its exit criteria. Discovery results may narrow supported
features; record that explicitly rather than marking unproven features complete.

## Phase 0: Linux and Wine feasibility

Record the target Fedora/X11 machine, running DWM-Titus build, Picom version and
configuration, launcher, runtime, display mode, and game build. Identify the
correct process and mapped executable. Prototype a bounded read of a known
character value and a transparent unmanaged overlay following the SPEC.md
DWM-Titus contract. Determine a workable process-access arrangement.

Create the overlay with `override_redirect=true` before mapping and stable
class/instance/title properties. Prove the narrow DWM-Titus `alwaysontop=1` rule,
empty input region, reachable edit toggle, and application-owned placement and
visibility. Test Picom fullscreen unredirection with this overlay; record any
required configuration change and its rollback before selecting the backend.

Exit: repeatable reading across three game launches and demonstrated overlay
behavior on the target desktop. Record focus, stacking, scaling, and fullscreen
limitations. Select the initial backend only from this evidence, with the
following overlay checks:

- Locate the panel using `xwininfo -root -tree` or a diagnostic window ID; a
  click-through panel cannot reliably be selected by clicking it. Run
  `xwininfo -id 0xWINDOW_ID` and require `Override Redirect State: yes` on initial
  display, hide/show, edit entry/exit, and every recreated window.
- Run `xprop -id 0xWINDOW_ID WM_CLASS WM_NAME _NET_WM_NAME _NET_WM_STATE` and verify
  the stable rule identity. Inspect `_NET_WM_STATE` on the game window separately
  and record DWM mode and geometry: neither appearance nor the fullscreen atom
  alone distinguishes all DWM-Titus fake-fullscreen variants from real fullscreen.
- Verify no tiling, DWM border, layout displacement, or focus-cycle entry for the
  overlay. Test stacking in windowed, borderless, fake-fullscreen, and real
  fullscreen modes, including refocus/restacking and rule reload. Check both
  before and after the configured Picom unredirection delay.
- Exercise pointer clicks, scrolling, and keyboard input over the panel in play
  mode; the game must retain them. Enter edit mode from outside the panel, move
  and resize it, then exit/cancel without trapped input or focus theft.
- Switch tags and applications, minimize/restore the game, move it between
  monitors, change resolution/scaling, and restart it. Require correct hiding,
  reappearance, placement, and session/window rebinding without stale overlays.

Automate X11 attribute/input-region and visibility-state regression checks where
possible. A nested X server or synthetic window test does not replace live
DWM-Titus, Picom, and Wine/Proton acceptance evidence.

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

Deliver the character panel using only proven fields and the Phase 0 unmanaged
X11 backend. Add placement persistence, scaling, opacity, hide/show, and
click-through/edit modes while retaining the DWM-Titus contract. Display source
and unavailable/stale status without overwhelming the main panel.

Exit: a full play session passes the Phase 0 overlay checks with the real panel
and demonstrates character switching, reattachment, and recovery. Record CPU,
memory, update latency, and game frame-time impact against a baseline on the
target machine. Set acceptance budgets from Phase 0 measurements before this
validation, not after seeing results.

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
Include the verified DWM-Titus overlay rule, identity inspection commands,
supported fullscreen modes, and any proven Picom exception with rollback.

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
