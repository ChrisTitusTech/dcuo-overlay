#!/usr/bin/env python3
"""Install only the prebuilt companion, launcher, and icon into a user prefix.

No build commands or application processes are run. Changes use an ownership
manifest and one previous version; configuration and maps are never targeted.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import tempfile

REPO = Path(__file__).resolve().parent.parent
TARGETS = {
    "bin/dcuo-companion": (64 * 1024 * 1024, 0o755),
    "share/applications/dcuo-companion.desktop": (64 * 1024, 0o644),
    "share/icons/hicolor/scalable/apps/dcuo-companion.svg": (1024 * 1024, 0o644),
}
STATE = "share/dcuo-companion/install"
MANIFEST = f"{STATE}/manifest.json"
BACKUPS = {name: f"{STATE}/previous/{index}" for index, name in enumerate(TARGETS)}
MANIFEST_LIMIT = 64 * 1024


class InstallError(Exception):
    """A safe refusal or an operation that could not be completed."""


def normalized(path):
    path = Path(path).expanduser().absolute()
    if ".." in path.parts:
        raise InstallError("Paths must not contain parent traversal")
    return path


def inspect_path(path):
    """Reject every existing symlink ancestor, not just the final filename."""
    path = normalized(path)
    ancestor = Path(path.anchor)
    for part in path.parts[1:]:
        ancestor /= part
        try:
            metadata = ancestor.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(metadata.st_mode):
            raise InstallError(f"Symlink path refused: {ancestor}")
        if ancestor != path and not stat.S_ISDIR(metadata.st_mode):
            raise InstallError(f"Parent is not a directory: {ancestor}")
    return path


def read_file(path, limit):
    path = inspect_path(path)
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd, "rb") as stream:
            metadata = os.fstat(stream.fileno())
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > limit:
                raise InstallError(f"Not a regular bounded file: {path}")
            data = stream.read(limit + 1)
            if len(data) > limit:
                raise InstallError(f"File exceeds size limit: {path}")
            return data, stat.S_IMODE(metadata.st_mode)
    except OSError as error:
        raise InstallError(f"Cannot read {path}: {error}") from error


def record(data, mode):
    return {"sha256": hashlib.sha256(data).hexdigest(), "size": len(data), "mode": mode}


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise InstallError("Duplicate ownership manifest key")
        result[key] = value
    return result


def validate_records(files):
    if not isinstance(files, dict) or set(files) != set(TARGETS):
        raise InstallError("Ownership manifest targets differ from this installer")
    for name, value in files.items():
        limit, mode = TARGETS[name]
        if not isinstance(value, dict) or set(value) != {"sha256", "size", "mode"}:
            raise InstallError("Invalid ownership record")
        checksum = value["sha256"]
        if (not isinstance(checksum, str) or len(checksum) != 64
                or any(c not in "0123456789abcdef" for c in checksum)
                or type(value["size"]) is not int or not 0 < value["size"] <= limit
                or type(value["mode"]) is not int or value["mode"] != mode):
            raise InstallError("Invalid ownership checksum, size, or mode")


def load_state(prefix):
    state_dir = inspect_path(prefix / STATE)
    if state_dir.exists():
        metadata = state_dir.stat()
        if not stat.S_ISDIR(metadata.st_mode) or stat.S_IMODE(metadata.st_mode) != 0o700:
            raise InstallError("Installer metadata directory must be private (mode 700)")
    path = inspect_path(prefix / MANIFEST)
    if not path.exists():
        return None, {}, {}
    data, mode = read_file(path, MANIFEST_LIMIT)
    if mode != 0o600:
        raise InstallError("Ownership manifest permissions changed; expected 600")
    try:
        state = json.loads(data, object_pairs_hook=unique_object)
    except (ValueError, UnicodeDecodeError) as error:
        raise InstallError("Invalid ownership manifest") from error
    if (not isinstance(state, dict) or set(state) != {"version", "files", "previous"}
            or type(state["version"]) is not int or state["version"] != 1):
        raise InstallError("Unsupported ownership manifest")
    validate_records(state["files"])
    previous = state["previous"]
    if previous is not None:
        if not isinstance(previous, dict) or set(previous) != {"files"}:
            raise InstallError("Invalid previous-version record")
        validate_records(previous["files"])
    current_data, previous_data = {}, {}
    for name, (limit, expected_mode) in TARGETS.items():
        content, actual_mode = read_file(prefix / name, limit)
        if record(content, actual_mode) != state["files"][name]:
            raise InstallError(f"Installed file changed; refusing all changes: {name}")
        current_data[name] = (content, expected_mode)
        backup = inspect_path(prefix / BACKUPS[name])
        if previous is None:
            if backup.exists():
                raise InstallError("Unowned previous-version backup exists")
        else:
            content, backup_mode = read_file(backup, limit)
            if backup_mode != 0o600 or record(content, expected_mode) != previous["files"][name]:
                raise InstallError(f"Previous-version backup changed: {name}")
            previous_data[name] = (content, expected_mode)
    return state, current_data, previous_data


def quote_exec(path):
    value = str(path)
    if "=" in value or any(ord(c) < 32 or ord(c) == 127 for c in value):
        raise InstallError("Executable path contains unsupported desktop-entry characters")
    # Exec quoting follows string unescaping: quoted special characters therefore
    # need two file-level backslashes, and a literal backslash needs four.
    escaped = "".join("\\" + c if c in '\\"`$' else c for c in value)
    escaped = escaped.replace("\\", "\\\\").replace("%", "%%")
    return '"' + escaped + '"'


def source_payload(prefix, binary, desktop, icon, position=None):
    binary_data, binary_mode = read_file(binary, TARGETS["bin/dcuo-companion"][0])
    if not binary_data or not binary_mode & 0o111:
        raise InstallError("A nonempty prebuilt executable is required; build it first")
    desktop_data, _ = read_file(desktop, 64 * 1024)
    icon_data, _ = read_file(icon, 1024 * 1024)
    if not icon_data:
        raise InstallError("Application icon is empty")
    try:
        text = desktop_data.decode("utf-8")
    except UnicodeDecodeError as error:
        raise InstallError("Desktop entry is not UTF-8") from error
    lines = text.splitlines()
    if (lines.count("[Desktop Entry]") != 1
            or sum(line.startswith("Exec=") for line in lines) != 1
            or sum(line.startswith("Icon=") for line in lines) != 1):
        raise InstallError("Desktop template must have one Desktop Entry, Exec, and Icon")
    executable = quote_exec(prefix / "bin/dcuo-companion")
    if position is not None:
        if len(position) != 2 or any(type(v) is not int or abs(v) > 100_000 for v in position):
            raise InstallError("Position requires two bounded integer coordinates")
        executable += " --position " + " ".join(map(str, position))
    icon_path = str(prefix / "share/icons/hicolor/scalable/apps/dcuo-companion.svg")
    if any(ord(c) < 32 or ord(c) == 127 for c in icon_path):
        raise InstallError("Icon path contains control characters")
    icon_path = icon_path.replace("\\", "\\\\")
    text = "\n".join("Exec=" + executable if line.startswith("Exec=")
                     else "Icon=" + icon_path if line.startswith("Icon=") else line
                     for line in lines) + "\n"
    contents = [binary_data, text.encode("utf-8"), icon_data]
    return {name: (data, TARGETS[name][1]) for name, data in zip(TARGETS, contents)}


def encode_state(payload, previous=None):
    state = {"version": 1, "files": {name: record(*value) for name, value in payload.items()},
             "previous": None if previous is None else {
                 "files": {name: record(*value) for name, value in previous.items()}}}
    data = (json.dumps(state, indent=2, sort_keys=True) + "\n").encode("utf-8")
    if len(data) > MANIFEST_LIMIT:
        raise InstallError("Ownership manifest exceeds limit")
    return data, 0o600


def make_parents(path, created):
    inspect_path(path)
    pending = []
    parent = path.parent
    while not parent.exists():
        pending.append(parent)
        parent = parent.parent
    if not parent.is_dir():
        raise InstallError(f"Parent is not a directory: {parent}")
    for directory in reversed(pending):
        directory.mkdir(mode=0o700 if STATE in str(directory) else 0o755)
        created.append(directory)


def atomic_write(path, value, created):
    data, mode = value
    make_parents(path, created)
    descriptor, temporary = tempfile.mkstemp(prefix=".dcuo-install-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            os.fchmod(stream.fileno(), mode)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        inspect_path(path)
        os.replace(temporary, path)
    finally:
        if os.path.lexists(temporary):
            os.unlink(temporary)


def transaction(prefix, changes):
    """Preflight all paths, then restore completed writes if any operation fails."""
    snapshots = {}
    for relative in changes:
        path = inspect_path(prefix / relative)
        if path.exists():
            limit = MANIFEST_LIMIT if relative == MANIFEST else (
                TARGETS[relative][0] if relative in TARGETS else
                TARGETS[next(name for name, backup in BACKUPS.items() if backup == relative)][0])
            snapshots[relative] = read_file(path, limit)
        else:
            snapshots[relative] = None
    completed, created = [], []
    try:
        for relative, value in changes.items():
            path = prefix / relative
            if value is None:
                if path.exists():
                    path.unlink()
            else:
                atomic_write(path, value, created)
            completed.append(relative)
    except Exception as error:
        restoration_errors = []
        for relative in reversed(completed):
            try:
                value = snapshots[relative]
                path = prefix / relative
                if value is None:
                    if path.exists():
                        path.unlink()
                else:
                    atomic_write(path, value, created)
            except Exception as restoration:
                restoration_errors.append(f"{relative}: {restoration}")
        for directory in reversed(created):
            try:
                directory.rmdir()
            except OSError:
                pass
        if restoration_errors:
            raise InstallError("Operation failed; incomplete restoration: " + "; ".join(restoration_errors)) from error
        raise InstallError(f"Operation failed; previous files restored: {error}") from error


def operate(action, prefix, binary=None, desktop=None, icon=None, position=None, adopt=False):
    prefix = inspect_path(normalized(prefix))
    state, current, previous = load_state(prefix)
    if adopt and action != "install":
        raise InstallError("--adopt is available only for install")
    if action == "install":
        if state is not None:
            raise InstallError("An owned installation exists; use upgrade")
        payload = source_payload(prefix, binary, desktop, icon, position)
        for name in list(BACKUPS.values()) + [MANIFEST]:
            if inspect_path(prefix / name).exists():
                raise InstallError(f"Unowned installer target is occupied: {name}")
        for name, value in payload.items():
            path = inspect_path(prefix / name)
            if adopt:
                if read_file(path, TARGETS[name][0]) != value:
                    raise InstallError(f"Adoption requires byte-identical files and expected modes: {name}")
            elif path.exists():
                raise InstallError(f"Unowned target is occupied: {name}")
        changes = {} if adopt else dict(payload)
        changes[MANIFEST] = encode_state(payload)
    elif action == "upgrade":
        if state is None:
            raise InstallError("No owned installation; use install or explicit exact-file adoption")
        payload = source_payload(prefix, binary, desktop, icon, position)
        changes = {BACKUPS[name]: (value[0], 0o600) for name, value in current.items()}
        changes.update(payload)
        changes[MANIFEST] = encode_state(payload, current)
    elif action == "rollback":
        if state is None or not previous:
            raise InstallError("No previous owned version is available")
        changes = {BACKUPS[name]: (value[0], 0o600) for name, value in current.items()}
        changes.update(previous)
        changes[MANIFEST] = encode_state(previous, current)
    elif action == "uninstall":
        if state is None:
            raise InstallError("No ownership manifest; unknown files will not be removed")
        changes = {name: None for name in TARGETS}
        if previous:
            changes.update({name: None for name in BACKUPS.values()})
        changes[MANIFEST] = None
    else:
        raise InstallError("Unknown installer operation")
    transaction(prefix, changes)
    if action == "uninstall":
        for relative in [f"{STATE}/previous", STATE]:
            try:
                (prefix / relative).rmdir()
            except OSError:
                pass
    return prefix / "bin/dcuo-companion"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["install", "upgrade", "rollback", "uninstall"])
    parser.add_argument("--prefix", type=Path, default=Path.home() / ".local")
    parser.add_argument("--binary", type=Path, default=REPO / "target/release/dcuo-companion")
    parser.add_argument("--position", type=int, nargs=2, metavar=("X", "Y"))
    parser.add_argument("--adopt", action="store_true", help="Own an existing exact-file installation without replacing it")
    args = parser.parse_args(argv)
    if args.action in {"rollback", "uninstall"} and args.position is not None:
        parser.error("--position applies only to install or upgrade")
    try:
        binary = operate(args.action, args.prefix, args.binary,
                         REPO / "assets/dcuo-companion.desktop", REPO / "assets/dcuo-companion.svg",
                         args.position, args.adopt)
    except (InstallError, OSError) as error:
        print(f"Installation refused or failed: {error}", file=sys.stderr)
        return 1
    print(f"{args.action.capitalize()} complete: {binary}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
