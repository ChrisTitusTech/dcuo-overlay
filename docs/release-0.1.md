# DCUO Companion 0.1.0

First experimental desktop release for Linux x86_64 (X11) and Windows x86_64.
The companion automatically finds one running supported DCUO session. Multiple
sessions require explicit selection; unsupported executable builds are rejected.

## Downloads and installation

- **AppImage:** Ubuntu 22.04 or newer / compatible Linux with glibc 2.35 or newer.
  Make executable (`chmod +x DCUO-Companion-0.1.0-x86_64.AppImage`) and launch.
  If FUSE is unavailable, use `--appimage-extract-and-run` before app arguments.
  A working X11 desktop and OpenGL driver are required.
- **RPM:** Fedora x86_64. Install using
  `sudo dnf install ./dcuo-companion-0.1.0-1.x86_64.rpm`.
  Launch DCUO Companion from the desktop menu.
- **Windows installer:** Windows 10/11 x86_64. Run the setup EXE; it installs for
  the current user without requiring administrator privileges. Launch from Start.
  Installer and application are unsigned; Windows may show a publisher warning.

Verify downloads against SHA256SUMS before running them. The release's source
archive corresponds to the tagged code and Cargo.lock. Code is GPL-3.0-only;
third-party crates retain their own licenses. Game binaries, artwork, private
snapshots, account tokens and imported maps are never bundled.

## Features and requirements

Live resources, position, feat completion and equipped-item details use an exact
supported executable profile and bounded read-only memory. Map and item artwork
are decoded from your local DCUO installation. Settings allows a CookedPC override.
The companion does not bypass process access restrictions or modify game memory.
Run the game and companion under the same user at compatible privilege levels.

Broker prices reflect observed cache pages, not the whole market. The optional
Ctrl+Alt+P helper requires Tesseract 5 with English data available on PATH and the
supported English 2560x1440 Broker layout. Install Tesseract separately if you
need OCR; map, character and wiki features work without it. Search and price
pasting require the game to have focus and never submit a sale or purchase.

Wiki contribution uses an explicitly supplied token kept only in memory, with
capture, preview and publication remaining explicit user actions. Closing the
companion loses that token.

## Known limits

This is an experimental 0.1 release, not a claim of all-map calibration, floor
support, verified Census identity, complete feat coverage or universal game-build
compatibility. Only proven area contexts place a live marker. Loading, missing
context and unsupported builds show explicit unavailable states. Windows live-game
acceptance is tracked separately from successful compilation and automated tests.

Linux preferences use XDG config/data directories. Windows preferences use
APPDATA and imported maps use LOCALAPPDATA. Uninstall preserves these user files.
Linux Wayland-native support and non-x86_64 platforms are not part of 0.1.
