#!/usr/bin/env python3
"""Check the actual probe window on the caller's X11 display, then close it.

This checks startup/relaunch attributes, not fullscreen, input, monitor recovery,
or game compatibility. It never moves or controls other windows.
"""

import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time


def run(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True, timeout=5).stdout


def check_window(binary, position, round_number):
    command = [str(binary), "--smoke-seconds", "8"]
    if position:
        command += ["--position", *map(str, position)]
    with tempfile.TemporaryFile(mode="w+") as log:
        child = subprocess.Popen(command, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 6
            window = None
            while time.monotonic() < deadline and child.poll() is None:
                search = subprocess.run(
                    ["xdotool", "search", "--onlyvisible", "--pid", str(child.pid),
                     "--name", "^DCUO Companion$"],
                    capture_output=True, text=True, timeout=2,
                )
                if search.returncode == 0 and search.stdout.strip():
                    windows = search.stdout.split()
                    if len(windows) != 1:
                        raise RuntimeError("Expected exactly one visible probe window")
                    window = windows[0]
                    break
                time.sleep(0.1)
            if window is None:
                log.seek(0)
                raise RuntimeError("Probe did not show a window: " + log.read(4000))

            props = run("xprop", "-id", window, "WM_CLASS", "WM_NAME", "_NET_WM_NAME",
                        "_NET_WM_WINDOW_TYPE", "_NET_WM_STATE")
            required = (
                'WM_CLASS(STRING) = "dcuo-companion", "DcuoCompanion"',
                'WM_NAME(STRING) = "DCUO Companion"',
                '_NET_WM_NAME(UTF8_STRING) = "DCUO Companion"',
                '_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_NORMAL',
            )
            for line in required:
                if line not in props.splitlines():
                    raise RuntimeError(f"Missing required X11 property: {line}\n{props}")
            if "_NET_WM_STATE_ABOVE" in props or "_NET_WM_STATE_FULLSCREEN" in props:
                raise RuntimeError("Probe unexpectedly requests above/fullscreen state")
            info = run("xwininfo", "-id", window)
            if "Override Redirect State: no" not in info or "Map State: IsViewable" not in info:
                raise RuntimeError("Probe is not a viewable, normally managed window")
            geometry = re.findall(r"(?:Absolute upper-left [XY]|Width|Height):\s+-?\d+", info)
            print(f"Launch {round_number}: normal window, exact identity, override_redirect=false; "
                  + ", ".join(geometry), flush=True)
            # Let the bounded smoke timer exercise normal application shutdown.
            result = child.wait(timeout=12)
            if result != 0:
                log.seek(0)
                raise RuntimeError(f"Probe exited {result}: {log.read(4000)}")
        finally:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/dcuo-window-probe"))
    parser.add_argument("--position", type=int, nargs=2, metavar=("X", "Y"))
    args = parser.parse_args()
    if not os.environ.get("DISPLAY"):
        parser.error("DISPLAY must identify an X11 server")
    for tool in ("xdotool", "xprop", "xwininfo"):
        if shutil.which(tool) is None:
            parser.error(f"Required tool is missing: {tool}")
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f"Build the window probe first: {binary}")
    for round_number in (1, 2):
        check_window(binary, args.position, round_number)
    print("Startup/relaunch checks passed; manual DWM and game acceptance remains required.")


if __name__ == "__main__":
    main()
