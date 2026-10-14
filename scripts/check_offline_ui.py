#!/usr/bin/env python3
"""Exercise empty Character/Map windows and keyboard tab navigation under X11.

Run with xvfb-run -a python3 scripts/check_offline_ui.py. Uses temporary settings,
never connects to a game or requests character data. CPU/RSS are observational;
shared CI load must not be mistaken for a gameplay performance measurement.
Optional screenshots stay in a caller-selected private directory.
"""

import argparse
import os
import json
import subprocess
import tempfile
import time
import pathlib

repo = pathlib.Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument(
    "--binary", type=pathlib.Path, default=repo / "target/release/dcuo-companion"
)
parser.add_argument("--captures", type=pathlib.Path)
parser.add_argument("--catalog", type=pathlib.Path)
args = parser.parse_args()
out = args.captures
if out:
    out.mkdir(parents=True, exist_ok=True)
binary = args.binary.resolve()


def capture(wid, name):
    if out:
        subprocess.run(
            ["import", "-window", wid, str(out / name)], check=True, timeout=5
        )


for scale in (0.75, 1.0, 2.0):
    with tempfile.TemporaryDirectory(prefix="dcuo-ui-") as d:
        conf = pathlib.Path(d) / "dcuo-companion"
        conf.mkdir()
        (conf / "settings.json").write_text(
            json.dumps(
                {
                    "ui_scale": scale,
                    "map_catalog": str(args.catalog.resolve()) if args.catalog else "",
                }
            )
        )
        env = dict(os.environ, XDG_CONFIG_HOME=d, LIBGL_ALWAYS_SOFTWARE="1")
        with tempfile.TemporaryFile(mode="w+") as log:
            p = subprocess.Popen(
                [str(binary), "--smoke-seconds", "60"], env=env, stdout=log, stderr=log
            )
            try:
                for i in range(60):
                    result = subprocess.run(
                        ["xdotool", "search", "--onlyvisible", "--pid", str(p.pid)],
                        capture_output=True,
                        text=True,
                        timeout=2,
                    )
                    if result.returncode == 0:
                        break
                    time.sleep(0.1)
                if result.returncode or not result.stdout.strip():
                    raise RuntimeError("Companion did not open a window")
                wid = result.stdout.split()[0]
                subprocess.run(
                    ["xdotool", "windowsize", wid, "640", "480"], check=True, timeout=5
                )
                subprocess.run(["xdotool", "windowfocus", wid], check=True, timeout=5)
                time.sleep(1)
                capture(wid, f"character-{scale}.png")

                def usage():
                    fields = pathlib.Path(f"/proc/{p.pid}/stat").read_text().split()
                    return (int(fields[13]) + int(fields[14])) / os.sysconf(
                        "SC_CLK_TCK"
                    ), int(fields[23]) * os.sysconf("SC_PAGE_SIZE") / 1024 / 1024

                before = usage()
                t = time.monotonic()
                time.sleep(10)
                after = usage()
                print(
                    f"scale={scale} Character idle CPU={(after[0] - before[0]) / (time.monotonic() - t) * 100:.2f}% RSS={after[1]:.1f}MiB",
                    flush=True,
                )
                # Exercise keyboard focus and activation in the actual managed window.
                for _ in range(4):
                    subprocess.run(
                        ["xdotool", "key", "--window", wid, "Tab"],
                        check=True,
                        timeout=5,
                    )
                subprocess.run(
                    ["xdotool", "key", "--window", wid, "space"], check=True, timeout=5
                )
                time.sleep(0.5)
                capture(wid, f"keyboard-{scale}.png")
                before = usage()
                t = time.monotonic()
                time.sleep(10)
                after = usage()
                print(
                    f"scale={scale} Map idle CPU={(after[0] - before[0]) / (time.monotonic() - t) * 100:.2f}% RSS={after[1]:.1f}MiB",
                    flush=True,
                )
                # From the focused Map tab: Diagnostics, Settings, About, Quit.
                for _ in range(4):
                    subprocess.run(
                        ["xdotool", "key", "--window", wid, "Tab"],
                        check=True,
                        timeout=5,
                    )
                subprocess.run(
                    ["xdotool", "key", "--window", wid, "space"], check=True, timeout=5
                )
                if p.wait(timeout=10) != 0:
                    raise RuntimeError("Companion did not exit cleanly")
            finally:
                if p.poll() is None:
                    p.terminate()
                    try:
                        p.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        p.kill()
                        p.wait()
print("All three scale windows exited cleanly.")
