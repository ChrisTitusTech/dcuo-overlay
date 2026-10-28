# DCUO Companion 0.1.2

Fixes links that appeared clickable but did not open a browser on Linux or Windows.
**Open wiki token settings**, mission sources and feat sources now use the system
browser. The previous packages omitted the desktop framework's browser-link feature.

Includes the token saving and automatic missing-item contributions from
[0.1.1](release-0.1.1.md). The matching wiki server is already deployed.

## Installation and upgrade

Close DCUO Companion before upgrading. Settings, saved tokens and maps are retained.

- Windows 10/11 x86_64: run `DCUO-Companion-0.1.2-windows-x86_64-setup.exe`.
- Linux AppImage: `chmod +x DCUO-Companion-0.1.2-x86_64.AppImage`, then launch.
  Without FUSE, add `--appimage-extract-and-run` before app arguments.
- Fedora RPM: `sudo dnf install ./dcuo-companion-0.1.2-1.x86_64.rpm`.

Verify downloads against SHA256SUMS. Linux requires X11 and glibc 2.35 or newer;
Windows requires an OpenGL 2.0+ driver. Windows binaries are unsigned. Browser
links require a configured system browser. License: GPL-3.0-only.

Automated Linux/Windows package checks must pass before publication. Windows
live-game acceptance remains pending with the user. The experimental feature
limits described in earlier release notes still apply.
