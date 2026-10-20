#!/usr/bin/env python3
"""Enter one path into an exact owned host's transient chooser in private Xvfb."""
import ctypes
import ctypes.util
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def command(*args, missing=False):
    result = subprocess.run(args, text=True, encoding="utf-8", errors="replace", capture_output=True, timeout=5)
    require(result.returncode == 0 or missing and result.returncode == 1,
            f"{args[0]} failed: {result.stderr[:512]}")
    return result.stdout.strip()


def main():
    spec = importlib.util.spec_from_file_location(
        "native_drawing", Path(__file__).with_name("native-drawing-linux.py"))
    drawing = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(drawing)
    drawing.private_xvfb()
    pid = int(sys.argv[1])
    title = os.environ["LIMO_CAD_SCRIPT_DIALOG_TITLE"]
    require(title in ("Open Limo CAD script", "Save Limo CAD script"), "Unexpected script chooser title")
    path = sys.stdin.buffer.read(4097).decode("utf-8")
    require(0 < len(path.encode("utf-8")) <= 4096 and Path(path).is_absolute()
            and not any(c in path for c in "\n\r\0"), "Expected one bounded absolute path")
    output = Path(os.environ["LIMO_CAD_SCRIPT_DIALOG_CAPTURE"])
    require(output.is_absolute() and output.suffix == ".png" and output.parent.is_dir()
            and not output.exists(), "Chooser evidence must be a fresh PNG in the fixture directory")
    started = Path(f"/proc/{pid}/stat").read_text(encoding="utf-8").rsplit(") ", 1)[1].split()[19]
    x11 = ctypes.CDLL(ctypes.util.find_library("X11"))
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XGetTransientForHint.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                        ctypes.POINTER(ctypes.c_ulong)]
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = x11.XOpenDisplay(None)
    require(display, "Cannot inspect the private display")

    def transient_chain(window):
        seen = []
        for _ in range(8):
            parent = ctypes.c_ulong()
            if not x11.XGetTransientForHint(display, int(window), ctypes.byref(parent)):
                break
            if not parent.value or parent.value in seen:
                break
            seen.append(parent.value)
            window = parent.value
        return seen

    def windows(*args):
        result = command("xdotool", "search", "--onlyvisible", *args, missing=True).split()
        require(len(result) <= 32, "Ambiguous chooser window inventory")
        return [int(window) for window in result]

    try:
        deadline = time.monotonic() + 15
        selected = None
        while selected is None:
            owners = windows("--pid", str(pid))
            candidates = windows("--name", "^" + re.escape(title) + "$")
            eligible = [(window, transient_chain(window)) for window in candidates]
            eligible = [(window, chain) for window, chain in eligible
                        if any(owner in chain for owner in owners)]
            require(len(eligible) <= 1, "Multiple choosers claim the owned native window")
            if eligible:
                selected, chain = eligible[0]
                owner = next(owner for owner in owners if owner in chain)
            else:
                require(time.monotonic() < deadline,
                        "No script chooser is transient for this exact owned host")
                time.sleep(0.1)

        def guard(focus=False):
            require(Path(f"/proc/{pid}/stat").read_text(encoding="utf-8").rsplit(") ", 1)[1].split()[19] == started,
                    "Owned host lifetime changed")
            require(command("xdotool", "getwindowpid", str(owner)) == str(pid), "Native window owner changed")
            require(selected in windows("--name", "^" + re.escape(title) + "$")
                    and owner in transient_chain(selected), "Owned chooser relation changed")
            if focus:
                require(int(command("xdotool", "getwindowfocus")) == selected,
                        "Owned chooser lost focus before keyboard input")

        guard()
        command("import", "-limit", "memory", "64MiB", "-limit", "map", "128MiB",
                "-window", str(selected), str(output))
        require(1024 < output.stat().st_size <= 8 * 1024 * 1024, "Chooser capture exceeds its PNG budget")
        receipt = {
            "owned_pid": pid, "owned_window": owner, "chooser_window": selected,
            "transient_chain": chain, "title": title, "private_xvfb": True,
            "status": "observed", "physical_path_entry": False}
        output.with_suffix(".json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
        guard()
        command("xdotool", "windowfocus", "--sync", str(selected))
        require(int(command("xdotool", "getwindowfocus")) == selected, "Owned chooser did not gain focus")
        guard(focus=True)
        command("xdotool", "key", "--clearmodifiers", "ctrl+l", "ctrl+a")
        guard(focus=True)
        command("xdotool", "type", "--clearmodifiers", "--delay", "1", "--", path)
        guard(focus=True)
        command("xdotool", "key", "--clearmodifiers", "Return")
        time.sleep(0.25)
        if selected in windows("--name", "^" + re.escape(title) + "$"):
            guard(focus=True)
            command("xdotool", "key", "--clearmodifiers", "Return")
        deadline = time.monotonic() + 5
        while selected in windows("--name", "^" + re.escape(title) + "$"):
            guard()
            require(time.monotonic() < deadline, "Owned script chooser did not close after path entry")
            time.sleep(0.1)
        receipt.update(status="completed", physical_path_entry=True)
        output.with_suffix(".json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    finally:
        x11.XCloseDisplay(display)


if __name__ == "__main__":
    main()
