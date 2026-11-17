#!/usr/bin/env python3
"""Read one exact owned X11 window before the lesson's Bevy capture; no input."""
import importlib.util
import json
import os
from pathlib import Path
import resource
import signal
import struct
import subprocess
import sys


PNG_LIMIT = 8 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def command(*args, output=None):
    result = subprocess.run(args, text=output is None, encoding="utf-8" if output is None else None,
                            errors="replace" if output is None else None,
                            stdout=subprocess.PIPE if output is None else output,
                            stderr=subprocess.PIPE, timeout=5)
    error = result.stderr if isinstance(result.stderr, str) else result.stderr.decode("utf-8", "replace")
    require(result.returncode == 0, f"{args[0]} failed: {error[:512]}")
    if output is None:
        require(len(result.stdout) <= 65536, "X11 command output exceeded its bound")
        return result.stdout.strip()


def process_start(pid):
    return Path(f"/proc/{pid}/stat").read_text(encoding="utf-8").rsplit(") ", 1)[1].split()[19]


def visible_window(pid):
    windows = command("xdotool", "search", "--onlyvisible", "--pid", str(pid)).split()
    require(len(windows) == 1, "Expected exactly one visible window for the owned host")
    return int(windows[0])


def geometry(window):
    fields = dict(line.split("=", 1) for line in
                  command("xdotool", "getwindowgeometry", "--shell", str(window)).splitlines())
    result = {key: int(fields[key]) for key in ("X", "Y", "WIDTH", "HEIGHT")}
    require(result["WIDTH"] > 0 and result["HEIGHT"] > 0, "Owned window bounds are empty")
    return result


def deadline(_signum, _frame):
    raise TimeoutError("Owned X11 capture exceeded its 15-second total deadline")


def main():
    signal.signal(signal.SIGALRM, deadline)
    signal.alarm(15)
    require(len(sys.argv) == 3, "Expected owned PID and absolute fixture output directory")
    require(os.environ.get("LIMO_CAD_NATIVE_SCRIPT_INPUT") == "1",
            "This diagnostic requires the owned real-script-input fixture")
    pid = int(sys.argv[1])
    require(pid > 1 and os.environ.get("LIMO_CAD_NATIVE_OWNED_PID") == str(pid),
            "Owned native host PID is absent or differs")
    out = Path(sys.argv[2])
    require(out.is_absolute() and out.is_dir(), "Expected an explicit absolute fixture directory")
    out = out.resolve(strict=True)
    capture = out / "native-lessons.x11-before-bevy.png"
    receipt_path = capture.with_suffix(".json")
    require(not capture.exists() and not capture.is_symlink()
            and not receipt_path.exists() and not receipt_path.is_symlink(),
            "X11 evidence must use fresh PNG and receipt paths")
    started = process_start(pid)
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location(
        "native_drawing", Path(__file__).with_name("native-drawing-linux.py"))
    drawing = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(drawing)
    xvfb_pid = drawing.private_xvfb()
    window = visible_window(pid)
    bounds = geometry(window)

    def guard():
        require(process_start(pid) == started, "Owned host lifetime changed")
        require(visible_window(pid) == window, "Owned visible window changed")
        require(command("xdotool", "getwindowpid", str(window)) == str(pid),
                "Native window owner changed")
        require(geometry(window) == bounds, "Owned window geometry changed")

    guard()
    # The OS enforces the file ceiling even if image encoding exceeds its budget.
    resource.setrlimit(resource.RLIMIT_FSIZE, (PNG_LIMIT, PNG_LIMIT))
    with capture.open("xb") as image:
        command("import", "-limit", "memory", "64MiB", "-limit", "map", "128MiB",
                "-window", str(window), "png:-", output=image)
    guard()
    size = capture.stat().st_size
    require(1024 < size <= PNG_LIMIT, "Owned window PNG is empty or exceeds 8 MiB")
    with capture.open("rb") as image:
        header = image.read(24)
    require(header[:8] == b"\x89PNG\r\n\x1a\n" and header[12:16] == b"IHDR",
            "Owned window output is not a PNG")
    require(struct.unpack(">II", header[16:24]) == (bounds["WIDTH"], bounds["HEIGHT"]),
            "PNG dimensions differ from the guarded native window")
    receipt = {"source": "X11 owned main window", "ordering": "before_bevy_window_capture",
               "capture_name": "native-lessons", "owned_pid": pid, "process_start_ticks": started,
               "owned_window": window, "window_geometry": bounds, "xvfb_pid": xvfb_pid,
               "private_xvfb": True, "input_sent": False, "png": str(capture), "png_bytes": size}
    with receipt_path.open("x", encoding="utf-8") as output:
        output.write(json.dumps(receipt, indent=2) + "\n")
    signal.alarm(0)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"Owned X11 capture failed: {error}"[:2048], file=sys.stderr)
        sys.exit(1)
