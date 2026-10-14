# Phase 0 implementation evidence

Date: 2026-10-13. Phase 0 remains in progress. These are feasibility prototypes,
not a working current-character or live-map application.

## Environment observed

| Item | Observation |
| --- | --- |
| Desktop | Fedora 44, X11, DWM-Titus reporting dwm-0.7.2, Picom running |
| Running DWM binary | SHA-256 `1ef634cc30242b7151578dde116fa166d045615e2b8d727189c63d8626085598`; source revision not established |
| Displays | DP-0 primary: 2560x1440 at (0,1080); HDMI-0: 1920x1080 at (0,0), both approximately 60 Hz during inspection |
| Game host window | Gamescope, fullscreen on DP-0; outer window 2560x1440 at (0,1080) |
| Launcher/runtime | Steam installation, GE-Proton10-34, wine64-preloader hosting DCGAME.EXE |
| Game executable | WIN64/DCGAME.EXE, PE AMD64 (`0x8664`), PE32+ (`0x020b`), 35,066,880 bytes, PE timestamp 1790857590 |
| Executable SHA-256 | `a5a14396497bd05336e0267d14a1c3442fec5c21b4c28b8d298fa0a6e0105cf3` |
| Access | Existing `ptrace_scope=0`; ordinary user read succeeded. No policy/capability/root changes made |
| Toolchain | Rust/Cargo 1.96.0, rustfmt 1.9.0, Clippy 0.1.96 |
| Native libraries inspected | x11 1.8.13, xcursor 1.2.3, xrandr 1.5.4, xi 1.8.3; X11/OpenGL window built and ran |

The game runs inside Gamescope, so its outer desktop fullscreen state is not
proof of every Wine/game display mode. Desktop scaling, exact DWM source revision,
chosen persistent tag/rule, and the complete launcher/display-mode matrix remain
to be established. No desktop rules or compositor settings were changed.

## Memory experiment

The Rust probe found one candidate using process identity and the uniquely mapped
DCGAME.EXE header. Wine maps this header from the executable file while mapping
code sections anonymously. Requiring file-backed executable permission on the
header would incorrectly reject this runtime; a synthetic regression covers it.

`inspect --pid PID` identified the mapped file using device/inode checks and a
bounded SHA-256 fingerprint, then successfully read its 64-byte live DOS header
with `process_vm_readv`. Every header byte matched the file. The implementation
checks process start identity around reads and revalidates the current unique
game mapping before and after fingerprinting. Partial reads are rejected.

This demonstrates a read-only access path on one running session, not a supported
game profile. DOS headers alone do not establish field offsets or active-character
identity. Reads are non-atomic, and mapping checks cannot make them atomic.

A user-supplied character name was used only through stdin for exact UTF-8 and
UTF-16LE search; the name and personal memory contents are not stored here.
The debug build read 2,928,704 bytes and the optimized build read 12,939,328 bytes
before their soft five-second limits. Both reported zero matches and incomplete
coverage. Neither result establishes absence of the character or a bad reference.
No character values, location coordinates, zone keys, or pointers were validated.

Next bounded experiment: add explicitly scoped region selection/checkpointed
coverage for discovery, keeping the same per-run byte/time limits and private
candidate evidence. Prove a candidate follows controlled character changes and
survives three launches before promoting a build-specific reader. A name hit
alone is insufficient. Game restarts and controlled routes were not performed.

## Managed window experiment

The egui/eframe 0.36.2 + winit 0.30.13 X11/OpenGL candidate creates its root window
hidden. The probe requires it to remain unmapped with override redirect disabled,
sets the exact identity/type, and reads properties back before the first frame.
It fails startup if those assumptions no longer hold.

Two consecutive launches with `scripts/check_x11.py --position 80 80` passed:

- `WM_CLASS`: instance `dcuo-companion`, class `DcuoCompanion`.
- `WM_NAME` and `_NET_WM_NAME`: `DCUO Companion`.
- `_NET_WM_WINDOW_TYPE_NORMAL`, no above/fullscreen request, override redirect off.
- Viewable, tiled window at (1056,30), size 862x1048, on HDMI-0 above the game.
  DWM adjusted the startup position hint; the application did not fight placement.
- Both bounded smoke windows closed normally with exit status zero.

A subsequent live run demonstrated keyboard tab switching to Map and Diagnostics,
text entry, a visible keyboard-focus indicator, and continuing frame updates.
Focus remained with Codex during a separate two-second background-refresh check.
Visual inspection found an unfilled lower tile area; the central frame now fills
the window, and the follow-up Character/Map/Diagnostics captures were readable.
Captures contain only prototype content and remain in ignored local storage.

No game memory/API data is connected to this UI. Character values and Map location
are explicitly unavailable; no decorative or synthetic marker suggests live data.
The backend remains a candidate until the remaining live matrix passes.

Still pending: in-process root recreation, persistent monitor/tag policy,
pointer/scroll checks, minimum size and scale matrix, game-focus/fullscreen mode
transitions, game restarts, compositor-off behavior, monitor disconnect/reconnect,
and measured CPU/memory/frame-time/latency budgets. Relaunch is not proof of all
internal window recreation paths.

## Checks and review

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo test --locked --workspace`: 14 tests passed (10 library, 2 memory CLI,
  2 window CLI), including bounded reads, malformed metadata, ambiguous candidates,
  Wine anonymous code mappings, stale mapping rejection, and search overlap.
- `cargo build --locked --workspace`: passed.
- `cargo build --locked --release --no-default-features --bin dcuo-probe`: passed.
- Synthetic self-read and live fixed-header inspection: passed.
- Live X11 startup/relaunch smoke check: passed twice; normal timed shutdown passed.
- Independent `gpt-6.1-sol` review found stale candidate mappings could survive
  pathname fallback during fingerprinting. Revalidation before/after hashing and
  regression tests resolve it; the reviewer confirmed the fix.
- The all-target lint run exposed a test-module ordering warning, fixed before
  the passing run. No required automated gate remains failing.

These checks do not complete Phase 0 or any map acceptance gate. API comparison,
session-driven live UI, profile persistence, and release packaging are not built.

## Map feasibility

See [source research and proposed experiments](map-feasibility.md). Both outdoor
and indoor/instance maps remain in first-release scope. There is no established
usable asset set, location reader, zone/floor matching, calibrated transform,
agreed launch inventory, or measured accuracy/latency budget yet.
