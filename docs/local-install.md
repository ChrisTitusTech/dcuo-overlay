# User-local installation and rollback

The installer copies a prebuilt companion binary, desktop launcher, and SVG icon
into your own prefix. It runs no build commands and needs no root access. This is
local installation, not publication or a supported first release. The live memory,
map calibration, and desktop acceptance limits in the other evidence documents
still apply. Game assets are not bundled by this installer.

From the repository checkout, build and validate the application:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 scripts/test_install_local.py
cargo build --release --bin dcuo-companion
```

Install into the default prefix, `$HOME/.local`:

```sh
python3 scripts/install_local.py install
```

For optional initial placement on your companion monitor, supply desktop pixel
coordinates appropriate to your actual monitor layout. The example matches the
current machine's documented position; it is not a universal monitor assignment:

```sh
python3 scripts/install_local.py install --position 80 80
```

The desktop entry uses an absolute, quoted executable path, so menu launch does
not depend on the desktop session's PATH. Its icon also uses an absolute path.
The launcher retains the normal `DcuoCompanion` window class. DWM still controls
window management; initial placement is not a permanent monitor policy.
Executable quoting follows the
[Desktop Entry specification](https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html).

Installed files, relative to the chosen prefix:

| File | Installed mode |
| --- | --- |
| `bin/dcuo-companion` | 755 |
| `share/applications/dcuo-companion.desktop` | 644 |
| `share/icons/hicolor/scalable/apps/dcuo-companion.svg` | 644 |
| `share/dcuo-companion/install/manifest.json` | 600 |
| `share/dcuo-companion/install/previous/0`, `1`, `2` after upgrade | 600 |

The installer metadata directory uses mode 700. The manifest records SHA-256,
size, and mode for exactly the three owned application files. Backups contain
exactly one previous version of those files. Settings, imported maps, history,
and unrelated files are not installer targets and remain after removal.

After updating the checkout, rebuild and upgrade with the same placement option:

```sh
git pull --ff-only
cargo build --release --bin dcuo-companion
python3 scripts/test_install_local.py
python3 scripts/install_local.py upgrade --position 80 80
```

Before an upgrade or removal, every installed owned file and any previous backup
must match the manifest. A changed file, invalid manifest, missing owned file,
unknown occupied destination, or symlink causes refusal before file changes.
The installer never treats an existing unowned file as disposable. An upgrade
replaces the older backup with the immediately preceding installation, so it
cannot roll back across several upgrades.

Restore the preceding binary, launcher, and icon:

```sh
python3 scripts/install_local.py rollback
```

Rollback swaps the current and previous versions. Running rollback again restores
the version you just replaced. Placement is part of the backed-up launcher and
returns with that launcher; rollback does not accept a new `--position` value.

Remove the owned installation, keeping your settings and imported maps:

```sh
python3 scripts/install_local.py uninstall
```

Only manifest-owned files that still match their hashes and modes are removed.
Empty installer metadata directories are removed when possible; shared prefix
folders and unrelated files are preserved. Stop the companion before changing
versions so the running process and installed binary do not differ.

For another absolute user-owned prefix, pass the same prefix for every operation:

```sh
python3 scripts/install_local.py install --prefix "$HOME/companion-local"
python3 scripts/install_local.py upgrade --prefix "$HOME/companion-local"
python3 scripts/install_local.py rollback --prefix "$HOME/companion-local"
python3 scripts/install_local.py uninstall --prefix "$HOME/companion-local"
```

A custom prefix's desktop file may need to be opened directly because desktop
menus normally search standard XDG application directories. The generated
launcher uses its absolute binary and icon paths. To select an already-built
binary elsewhere, use `--binary /absolute/path/to/dcuo-companion` on install or
upgrade. Source files and all destination ancestors must be regular directories
without symlinks; parent traversal is rejected. The binary must be a nonempty
executable file no larger than 64 MiB. The launcher, SVG, and manifest have
smaller explicit limits. Paths containing desktop-entry control characters or
an equals sign in the executable path are rejected.

## Existing manual installation

An installation without this ownership manifest is intentionally refused. If all
three existing files already exactly match the requested binary, generated
launcher, icon, and expected modes, explicitly adopt it without replacing files:

```sh
python3 scripts/install_local.py install --adopt --position 80 80
```

Adoption creates only ownership metadata. A different binary, launcher placement,
icon path, content, or mode causes refusal. It does not convert or overwrite an
older manual installation. Preserve and reconcile those files separately before
using a fresh install; there is no force-overwrite option.

## Failure behavior and verification

Each file replacement is atomic within its destination directory. If a normal
write or removal fails, the installer restores completed changes from bounded
in-memory snapshots and reports failure. If restoration also fails, its error
identifies the files that still need attention. This is not a guarantee against
power loss, filesystem failure, or concurrently running installers; run one
installer at a time. A leftover unowned or changed file is refused on the next
run rather than silently deleted.

The direct test command uses temporary prefixes and synthetic executables/assets.
It covers install/upgrade/rollback/uninstall, one-version backups, settings/maps
preservation, exact adoption, modified files/backups, malformed manifests,
symlink/traversal rejection, desktop quoting, file-size/mode limits, and injected
write failures with restoration. It never invokes the game or touches an actual
user installation. Actual desktop launch, target-machine install/upgrade/removal,
and profile/map rollback remain separate acceptance checks for the lead.
