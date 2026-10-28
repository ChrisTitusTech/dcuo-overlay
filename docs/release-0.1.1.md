# DCUO Companion 0.1.1

This update saves wiki tokens and automatically contributes missing item data.

## Changes

- Tokens save automatically and restore on restart. Windows uses user-bound DPAPI
  encryption; Linux uses a private user-owned file. Clear token deletes the copy.
- While connected to the matching linked character, loaded Census item IDs are
  checked against the wiki. Unknown item definitions and available icons upload
  automatically, including while another tab is open.
- Batches contain at most 50 IDs and run at least 90 seconds apart. Existing wiki
  identities are skipped, including when another contributor uploads concurrently.
- Pause automatic missing-item uploads in the Wiki tab. Manual capture/send remains
  available for enriching existing items. Storage and upload failures remain visible.

The wiki server must support missing-only contributions. Older servers block
automatic uploads. This covers supported item definitions, not all Census data.
A valid wiki token and a linked character with refreshed wiki equipment are required.

## Installation and upgrade

Close DCUO Companion before replacing an existing installation.

- Windows 10/11 x86_64: run `DCUO-Companion-0.1.1-windows-x86_64-setup.exe`.
  It updates the current user's installation. Settings and imported maps are retained.
- Linux AppImage: `chmod +x DCUO-Companion-0.1.1-x86_64.AppImage`, then launch.
  If FUSE is unavailable, add `--appimage-extract-and-run` before app arguments.
- Fedora RPM: `sudo dnf install ./dcuo-companion-0.1.1-1.x86_64.rpm`.

Verify downloads using SHA256SUMS. Linux requires X11 and glibc 2.35 or newer;
Windows requires a working OpenGL 2.0 or newer driver. Windows binaries are unsigned.
The app and installer contain no game files or user data. Licensed GPL-3.0-only.

## Validation and limits

Automated Linux and Windows package checks are required before publication.
Synthetic tests cover token restart/clear, background uploads, stale sessions,
known IDs, failed requests and concurrent wiki contributions. Native Windows
live-game testing remains pending with the user. No real token or production
item submission is used to validate this release.

The remaining experimental feature limits from [0.1.0](release-0.1.md) still apply.
