# Offline hardening

The user closed the game after confirming that the Gotham marker follows the
character correctly. Continue first-release work without starting the game;
stop at checks that need live observations. This does not complete a phase or
promote the native profile or map calibration.

## Planned offline acceptance budgets

These are engineering targets set before this slice's measurements, not claims
about game performance. Measure a release build on the current workstation.

- Idle companion, Character and Map tabs: below 10% of one CPU core averaged
  across ten seconds after startup; resident memory below 256 MiB.
- Local map import: below 30 seconds and 512 MiB peak resident memory for the
  currently supported Gotham and Old Gotham Subway packages.
- Import cancellation: below two seconds after the next bounded tile operation;
  remove an incomplete import directory and never publish a partial catalog.
- API lookup: one eight-second total operation budget, at most three attempts;
  preserve original cache receipt times, honor service cooldowns, and keep the
  native reader independent. A blocking transport operation can use its remaining
  timeout before observing cancellation.
- UI: no game required for startup, all three tabs, settings, and help; nested X11
  exercises the declared 640x480 minimum size and 0.75, 1.0, and 2.0 scales.
  Scrolling is expected at the minimum size. Presentation replay uses synthetic
  states only and does not certify real game transitions.

Actual location accuracy, end-to-end latency, outdoor polling cost and game frame
impact need a running game. No threshold here replaces their live acceptance.

## Remaining live gates

- Repeat the exact-build reader across three fresh launches; prove active
  character identity and the semantics of each resource separately.
- Check restart recovery against a real game exit/restart and ambiguous sessions.
- Validate indoor placement, independent landmarks, map/floor ambiguity,
  transitions, teleports, death/respawn, and character switching.
- Verify extended gameplay overhead and the complete DWM/game display-mode,
  monitor-disconnection, focus, and keyboard acceptance matrix.
- Choose a project license before distributing an application release.

Additional map coverage and Phase 6 analytics remain outside this accepted
experimental two-map inventory. No optional analytics were selected.

## Recorded offline evidence

- Workspace: 95 tests passed, including replacement of the process selector
  after PID reuse. The separate non-window suite passed 75 tests. Strict Clippy passed with both
  Rust 1.96 and the declared Rust 1.95 minimum. Debug and release builds passed.
- Installer: all 12 temporary-prefix tests passed. The previous manually installed
  binary/launcher/icon were checked exactly before ownership adoption. Actual
  user-local upgrade and two rollback swaps restored the expected binary hashes.
  Settings and maps remained in place. Actual release-binary install/removal in
  an isolated local prefix preserved an unrelated map sentinel. The launcher passes desktop-file validation
  with its existing nonfatal multiple-category hint. The updated menu launcher
  opened a normal managed window on the real DWM desktop; its Quit control
  closed it cleanly.
- Two release-window startup/relaunch cycles passed exact WM_CLASS/title, normal
  window type and override_redirect=false checks in nested X11.
- Synthetic presentation replay rendered no-game, loading, paused, API failure,
  expired and unsupported-area states across all tabs and all three UI scales.
  Session/map tests separately cover stale generations, corruption, missing assets,
  mismatched maps and calibration profiles. Replay is not live acceptance.
- Actual 640x480 windows at scales 0.75/1/2 accepted Tab/Space navigation to Map
  and keyboard navigation to Quit exited cleanly. Private screenshots were inspected; scrolling is required
  at larger scales. This is not a full keyboard-only/DWM acceptance claim.
- Release idle samples with a loaded local Gotham map, ten seconds per tab:

  | UI scale | Character CPU | Map CPU | Peak sampled RSS |
  | --- | --- | --- | --- |
  | 0.75 | 2.3% | 2.7% | 144.3 MiB |
  | 1.0 | 2.6% | 2.9% | 138.3 MiB |
  | 2.0 | 2.8% | 2.8% | 138.6 MiB |

  These use nested X11/software rendering and percent of one CPU core. An empty
  catalog used 98.6-99.1 MiB and 2.3-2.9% Character CPU. No game was running.
- Actual release import of 600 Gotham tiles and six Subway tiles passed image
  checksums, metadata and private output checks. Direct test execution took
  0.33 seconds and 45,996 KiB peak RSS on this warm filesystem. Compiler memory
  is excluded; an initial timing around Cargo included compilation and is not an
  import measurement. Synthetic cancellation tests prove incomplete output cleanup
  and worker shutdown; no running game was used.
- Independent Codex review found a stale process-selector identity after recovery.
  The fix replaces the exited candidate with the recovered PID/start time before
  presentation, including PID reuse. The follow-up review found no actionable regressions.

Reproduce the GUI/idle observations after a release build:

```sh
LIBGL_ALWAYS_SOFTWARE=1 xvfb-run -a python3 scripts/check_x11.py --binary target/release/dcuo-companion
xvfb-run -a python3 scripts/check_offline_ui.py
# Optional local-only artwork and private screenshots:
xvfb-run -a python3 scripts/check_offline_ui.py --catalog /absolute/private/catalog.json --captures captures/offline-check
```

The UI script never attaches to a process or requests character information.
ImageMagick's `import` is needed only for optional screenshots. Neither game
artwork nor personal observations are committed.
