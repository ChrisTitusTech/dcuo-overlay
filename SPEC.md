# Specification

Status: proposed implementation scope; no runtime capabilities verified.

## Objective

Build a native Linux Rust application that displays useful DCUO information over
the game running under Wine/Proton. Read live game state externally and use
dcuobot-api to corroborate fields that have equivalent API representations.

## Initial scope

- Discover and select the Linux process hosting the intended DCUO session.
- Identify its executable build and supported memory profile.
- Read character identity and incrementally proven live fields.
- Cross-check identity and comparable profile fields against the API.
- Display an unmanaged X11 panel on DWM-Titus with transparency, saved placement,
  and click-through/edit modes.
- Handle loading, character switching, game exit, restart, and API outages.

Memory access is read-only. Injection, game-memory writes, gameplay automation,
and automatic system security changes are outside the proposed scope.

## Architecture

Start with one Rust application and separately testable modules. Split into
workspace crates only when boundaries or testing needs justify it.

| Module | Responsibility |
| --- | --- |
| Process discovery | Session selection, process identity, executable mappings, lifecycle |
| Memory reader | Bounded external reads, access errors, partial-read handling |
| Memory profiles | Build fingerprints, signatures, pointer resolution, field decoding |
| API client | Typed character/reference responses, caching, retries, rate limiting |
| Validation | Per-field comparison, freshness, identity binding, provenance |
| State and events | Timestamped observations, lifecycle transitions, replay fixtures |
| Overlay | Panels, edit mode, display integration, visible unavailable/stale states |
| Local storage | Configuration, selected history, schema versioning |

Initial technology candidates are process_vm_readv for reads and egui for UI.
The initial desktop target is native X11 under DWM-Titus with Picom. Choose the
window/rendering integration after it proves the unmanaged-window contract below.
Wayland and other window managers are deferred compatibility work; do not silently
select a different backend or fall back to a managed overlay.

Keep memory polling and network work off the UI thread. Bound queues and buffers.
Measure polling and rendering costs before setting production update intervals.

## DWM-Titus overlay contract

The game-facing overlay must be a separate, root-child X11 window created with
`override_redirect = true` before its first map. Keep it unmanaged through
hide/show, edit-mode changes, and window recreation. Borderless styling, floating
rules, and `_NET_WM_STATE_ABOVE` are not substitutes for this X11 attribute.
Do not embed the overlay into or modify the Wine game window.

The backend must expose window creation before mapping. For a winit integration,
evaluate `WindowAttributesExtX11::with_override_redirect(true)` with an explicitly
selected X11 event loop; verify the actual window attributes in Phase 0. Any egui
host must preserve this creation path, including recreated viewports. Reject a
host that only offers generic always-on-top or decoration settings.

Use stable identity before mapping: `WM_CLASS` instance `dcuo-overlay`, class
`DcuoOverlay`, and `WM_NAME`/`_NET_WM_NAME` title `DCUO Overlay`. Keep character names
and changing status inside the panel. Separate settings windows, if introduced,
must use a different title and must not match the overlay rule.

DWM-Titus documents a fullscreen stacking exception for unmanaged windows with
an explicit matching `alwaysontop=1` rule. Plan this entry inside the existing
`rules = [...]` array in
`${XDG_CONFIG_HOME:-$HOME/.config}/dwm-titus/window-rules.toml`:

```toml
{ class="DcuoOverlay", instance="dcuo-overlay", title="DCUO Overlay", alwaysontop=1 },
```

Matching is case-sensitive substring matching; the last matching rule wins.
Verify the identities and rule ordering on the target installation. This rule
controls stacking only; the application must already create an unmanaged window.
Confirm the running DWM build supports the exception, including live rule reload.
Record a missing capability or fullscreen failure as a blocker. Do not claim
fullscreen support from an above hint or a successful windowed-mode test.

The application owns the behavior DWM does not provide for this surface:

- Input: default to click-through using an empty X11 input region (Shape/XFixes
  or a verified backend equivalent), independently of visual transparency.
  Mapping, refreshing, and raising the panel must not take keyboard focus or
  grab game input. If click-through cannot be established, keep the panel hidden
  and report the failure through diagnostics or a separate control surface.
- Editing: provide a control reachable while the panel is click-through, such
  as a configurable shortcut or local control command. Enable pointer input only
  for explicit editing, implement movement/resizing without DWM move/resize
  requests, and restore click-through on exit/cancel. Any keyboard focus must be
  explicitly requested and released without stealing focus after a user switches
  to another application. The overlay remains unmanaged in edit mode.
- Placement and visibility: bind to the selected game window, track its geometry
  and monitor, and restore/clamp saved placement after resolution or monitor
  changes. Hide when the game is minimized, on another tag, exited, or no longer
  active, except during explicit overlay editing. DWM tag/monitor rules for the
  game do not manage the overlay. Do not rely on mapped state alone to determine
  whether a DWM client is visible on the current tag.
- Rendering: use an alpha-capable visual and transparent framebuffer. Verify
  Picom composition, fullscreen unredirection, opacity, shadows, and blur on the
  actual setup. Missing composition must produce a clear limitation rather than
  an opaque panel over the game. Any necessary compositor exception must be
  narrow, documented with rollback, and measured for frame-time impact.

Phase 0 must prove this contract before the full UI backend is selected. Include
windowed, borderless, DWM fake fullscreen, and real fullscreen cases (record both
game `_NET_WM_STATE` and DWM mode), with click-through, edit mode, tag/focus changes,
and hide/show transitions. Record unsupported modes explicitly. Avoid continuous
raise/focus loops to mask stacking failures.

## Observation contract

Each field observation records its value, source, observation time, session
identity, executable/profile version, and validity. API comparisons additionally
record retrieval time and source freshness when known.

Represent read validity separately from API comparison:

- Read validity: valid, stale, unavailable, or invalid.
- API comparison: matched, different, freshness unknown, unavailable, or not comparable.

A successful identity comparison does not validate unrelated fields. An API
outage does not automatically invalidate a locally validated memory observation.
Never substitute zero for missing data or silently retain a previous character's
values after a switch. Clear comparisons when session identity changes.

## API integration

Use a configurable API base URL. The inspected service exposes:

- GET /v1/census/characters?name=...&worldId=...
- GET /v1/data/power-types
- GET /v1/data/movement-modes
- GET /v1/data/alignments
- GET /v1/data/artifacts
- GET /v1/data/allies

Preserve string identifiers and URL-encode lookup parameters. Require an exact
returned identity match: the current service can wildcard short names and select
the first result. Permit explicit world selection until automatic detection is
proven. Do not assume client object IDs equal Census IDs without evidence.

| Fields | Comparison rules |
| --- | --- |
| Name, world, character ID | Exact identity checks where equivalent IDs are proven |
| Power, movement, alignment | Explicit normalization through reference data |
| CR, skill points, equipment | Compare with freshness caveats and controlled changes |
| Maximum health/power, core stats | Compare only under documented comparable conditions |
| Current health/power, cooldowns, combat events | Not verifiable through the inspected API |

The inspected API caches character lookups for five minutes and applies a
60-request/minute per-client limit. Upstream freshness can be unknown. Refresh
on identity changes and bounded schedules, not on every memory sample. Honor
Retry-After and use bounded backoff for network/upstream failures.

Current artifact responses expose identity but not ranks. Investigate equipment
slot semantics before trusting equipment comparisons. Missing timestamp, raw-ID,
or slot fields may justify additive changes in dcuobot-api; those changes belong
in that repository with its own validation.

## Memory profile requirements

- Determine process architecture and actual executable mappings at runtime.
- Key supported profiles by executable fingerprint and profile version.
- Resolve addresses relative to validated mappings; avoid fixed absolute addresses.
- Bound pointer traversal, string lengths, read sizes, and scanning work.
- Reject ambiguous signatures and unsupported builds.
- Check ranges, encoding, plausible values, and consistency across observations.
- Handle partial reads and torn snapshots; external reads are not atomic.
- Prevent PID reuse from binding old addresses to a different process.
- Test character switches, zone changes, death/respawn, and process restart.
- Preserve small sanitized fixtures; do not publish full process dumps.

Access failures must report the actual permission problem. Do not automatically
change ptrace policy, assign broad capabilities, or run the whole UI as root.
Any required access arrangement is a Phase 0 design decision.

## Later features

Personal DPS, per-ability damage, critical-hit rate, timed parses, fight history,
and personal bests are candidates after reliable event acquisition is proven.
Combat-log support may complement memory observations. Deduplicate overlapping
sources and handle log rotation, truncation, and session boundaries.

Group metrics, cooldowns, buffs, and automatic Nth Metal tracking require separate
source discovery. Do not infer DPS from sparse health samples or label damage
ticks as confirmed ability casts. Manual timers must remain labeled manual.

## Open decisions

- Exact DWM-Titus/Picom revisions, game display mode, and monitor arrangement on
  the initial Fedora/X11 target.
- Launcher, Wine/Proton version, game build, architecture, and install location.
- First-release feature priorities beyond the character panel.
- Memory-access arrangement if normal process permissions deny reads.
- UI/backend choice following the feasibility experiment.
- Distribution format and project license before shipping application code.

## References

- [DCUO-Assistant release notes](https://github.com/265ada/DCUO-Assistant/blob/master/RELEASE_NOTES_v1.2.0.md).
- [API source](https://github.com/DCUOBot/dcuobot-api).
- [process_vm_readv](https://man7.org/linux/man-pages/man2/process_vm_readv.2.html).
- [Yama access restrictions](https://docs.kernel.org/admin-guide/LSM/Yama.html).
- [Window behavior](https://docs.rs/winit/latest/winit/window/struct.Window.html).
- [DWM-Titus overlay rules](https://dwm.christitus.com/configuration.html#overlays-above-other-applications).
- [winit X11 window attributes](https://docs.rs/winit/0.30.13/winit/platform/x11/trait.WindowAttributesExtX11.html).
- [Xlib window attributes](https://www.x.org/releases/X11R7.5/doc/libX11/libX11.html).
- [X11 input shapes](https://www.x.org/releases/X11R7.7/doc/xextproto/shape.html).
