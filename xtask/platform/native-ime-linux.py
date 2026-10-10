#!/usr/bin/env python3
"""Measure/capture an owned IBus popup in the fixture's private Xvfb session.

This is a QA helper for an OS window, never a product screenshot endpoint.
No root/desktop capture is allowed; import receives one PID-verified popup ID.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import sys


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def command(*args):
    return subprocess.check_output(args, text=True, timeout=5).strip()


def require_private_xvfb():
    display = re.fullmatch(r":(\d+)(?:\.\d+)?", os.environ.get("DISPLAY", ""))
    require(display is not None, "A local private Xvfb display is required")
    server = int(Path(f"/tmp/.X{display[1]}-lock").read_text().strip())
    executable = Path(f"/proc/{server}/cmdline").read_bytes().split(b"\0")[0]
    require(Path(os.fsdecode(executable)).name == "Xvfb", "The display is not Xvfb")
    ancestors = set()
    parent = os.getppid()
    while parent > 1 and parent not in ancestors:
        ancestors.add(parent)
        parent = int(command("ps", "-o", "ppid=", "-p", str(parent)))
    require(
        int(command("ps", "-o", "ppid=", "-p", str(server))) in ancestors,
        "Xvfb does not belong to this fixture's process tree",
    )
    return server


def descendants(parent):
    os.kill(parent, 0)
    rows = [line.split() for line in command("ps", "-e", "-o", "pid=,ppid=").splitlines()]
    owned = {parent}
    while True:
        expanded = owned | {int(pid) for pid, ppid in rows if int(ppid) in owned}
        if expanded == owned:
            return owned
        owned = expanded


def windows(pid):
    result = subprocess.run(
        ["xdotool", "search", "--onlyvisible", "--pid", str(pid)],
        text=True, capture_output=True, timeout=5,
    )
    return result.stdout.split() if result.returncode == 0 else []


def geometry(window):
    values = {}
    for line in command("xdotool", "getwindowgeometry", "--shell", window).splitlines():
        key, _, value = line.partition("=")
        values[key] = int(value)
    return {key.lower(): values[key] for key in ("X", "Y", "WIDTH", "HEIGHT")}


require(os.environ.get("LIMO_CAD_NATIVE_IME_TEST") == "1", "Private IME fixture required")
require(os.environ.get("XMODIFIERS") == "@im=ibus", "IBus XIM required")
xvfb_pid = require_private_xvfb()
if sys.argv[1:] == ["--verify-private-display"]:
    print(xvfb_pid)
    sys.exit(0)
native_pid = int(sys.argv[1])
daemon_pid = int(os.environ["LIMO_CAD_NATIVE_IME_DAEMON_PID"])
request = json.load(sys.stdin)
native_windows = windows(native_pid)
require(len(native_windows) == 1, f"Expected one owned native window: {native_windows}")
native = geometry(native_windows[0])
scale_x = native["width"] / request["client"]["width"]
scale_y = native["height"] / request["client"]["height"]
require(0.5 <= scale_x <= 4 and 0.5 <= scale_y <= 4, "Invalid native window scale")
field = request["field"]
field = {
    "x": native["x"] + field["x"] * scale_x,
    "y": native["y"] + field["y"] * scale_y,
    "width": field["width"] * scale_x,
    "height": field["height"] * scale_y,
}
owned = descendants(daemon_pid)
candidates = []
for pid in sorted(owned):
    for window in windows(pid):
        bounds = geometry(window)
        if bounds["width"] < 20 or bounds["height"] < 15:
            continue
        candidates.append({"pid": pid, "window": window, "bounds": bounds})
nearby = [candidate for candidate in candidates if (
    field["x"] - 32 * scale_x <= candidate["bounds"]["x"] <= field["x"] + field["width"]
    and field["y"] - 16 * scale_y <= candidate["bounds"]["y"] <= field["y"] + field["height"] + 80 * scale_y
)]
require(len(nearby) == 1, f"Expected one owned IME popup near field {field}; got {candidates}")
popup = nearby[0]
require(
    int(command("xdotool", "getwindowpid", popup["window"])) in descendants(daemon_pid),
    "IME popup ownership changed",
)
destination = Path(request["capture"])
require(destination.is_absolute() and not destination.exists(), "Use a fresh absolute popup capture path")
subprocess.run(["import", "-silent", "-window", popup["window"], str(destination)], check=True, timeout=10)
print(json.dumps({
    "xvfb_pid": xvfb_pid, "daemon_pid": daemon_pid, "native_pid": native_pid,
    "native_window": native, "scale": [scale_x, scale_y],
    "field_screen_bounds": field, "popup": popup,
    "placement_check": "popup origin near the active native field; exact caret alignment still needs pixel review",
    "capture": str(destination),
}))
