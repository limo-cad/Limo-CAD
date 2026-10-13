#!/usr/bin/env python3
"""Real XTEST paper gestures on one PID-verified window in private Xvfb.

This QA helper never targets the user's desktop and never synthesizes Bevy
events. Published logical client coordinates are mapped to X11 physical pixels.
"""
import ctypes
import ctypes.util
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import time


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def command(*args):
    return subprocess.check_output(args, text=True, timeout=5).strip()


def private_xvfb():
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
    require(int(command("ps", "-o", "ppid=", "-p", str(server))) in ancestors,
            "Xvfb does not belong to this fixture's process tree")
    return server


def physical_point(client, geometry, point):
    values = [client[k] for k in ("x", "y", "width", "height")] + list(point)
    require(all(isinstance(v, (int, float)) and math.isfinite(v) for v in values),
            "Gesture coordinates must be finite")
    require(client["width"] > 0 and client["height"] > 0, "Client bounds must be positive")
    scale = [geometry["WIDTH"] / client["width"], geometry["HEIGHT"] / client["height"]]
    require(all(0.5 <= n <= 4 for n in scale) and abs(scale[0] - scale[1]) < 0.01,
            "Owned window scale does not match its published client")
    local = [point[0] - client["x"], point[1] - client["y"]]
    require(0 <= local[0] < client["width"] and 0 <= local[1] < client["height"],
            "Gesture point lies outside the owned client")
    pixels = [round(local[i] * scale[i]) for i in range(2)]
    require(0 <= pixels[0] < geometry["WIDTH"] and 0 <= pixels[1] < geometry["HEIGHT"],
            "Rounded gesture point lies outside the owned client")
    return [geometry["X"] + pixels[0], geometry["Y"] + pixels[1]], scale


def move_pointer(pixel, timeout=1.0):
    command("xdotool", "mousemove", str(pixel[0]), str(pixel[1]))
    deadline = time.monotonic() + timeout
    while True:
        location = dict(line.split("=", 1) for line in
                        command("xdotool", "getmouselocation", "--shell").splitlines())
        actual = [int(location["X"]), int(location["Y"])]
        if actual == pixel:
            return
        require(time.monotonic() < deadline,
                f"Owned pointer did not reach {pixel}; actual root position is {actual}")
        time.sleep(0.01)


def main():
    if sys.argv[1:] == ["--verify-private-display"]:
        print(private_xvfb())
        return
    pid, operation, window = int(sys.argv[1]), sys.argv[2], sys.argv[3]
    require(operation in ("drawing-wheel", "drawing-pan", "drawing-click", "drawing-drag"), "Unknown paper gesture")
    xvfb = private_xvfb()
    request = json.load(sys.stdin)
    geometry = dict(line.split("=", 1) for line in command("xdotool", "getwindowgeometry", "--shell", window).splitlines())
    geometry = {key: int(geometry[key]) for key in ("X", "Y", "WIDTH", "HEIGHT")}
    client = request["client"]
    start = [request["x"], request["y"]]
    end = [request["to_x"], request["to_y"]] if operation in ("drawing-pan", "drawing-drag") else start
    physical_start, scale = physical_point(client, geometry, start)
    physical_end, _ = physical_point(client, geometry, end)
    if operation == "drawing-wheel":
        notches = request["notches"]
        require(isinstance(notches, int) and 0 < abs(notches) <= 10, "Wheel requires 1..10 signed notches")
    x11 = ctypes.CDLL(ctypes.util.find_library("X11"))
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
    x11.XDefaultRootWindow.restype = ctypes.c_ulong
    x11.XTranslateCoordinates.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong,
                                        ctypes.c_int, ctypes.c_int, ctypes.POINTER(ctypes.c_int),
                                        ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_ulong)]
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = x11.XOpenDisplay(None)
    require(display, "Cannot open the owned Xvfb display")
    root = x11.XDefaultRootWindow(display)

    def move(point):
        require(int(command("xdotool", "getwindowpid", window)) == pid, "Window ownership changed")
        require(command("xdotool", "getwindowfocus") == window, "Owned window lost focus")
        pixel, _ = physical_point(client, geometry, point)
        x, y, child = ctypes.c_int(), ctypes.c_int(), ctypes.c_ulong()
        require(x11.XTranslateCoordinates(display, root, root, *pixel, ctypes.byref(x), ctypes.byref(y), ctypes.byref(child)),
                "Cannot resolve gesture target")
        require(child.value == int(window), "Owned gesture target is occluded; no pointer input sent")
        move_pointer(pixel)
        time.sleep(0.035)

    try:
        move(start)
        if operation == "drawing-wheel":
            ctrl = request.get("ctrl", False)
            if ctrl:
                command("xdotool", "keydown", "ctrl")
            try:
                command("xdotool", "click", "--repeat", str(abs(notches)), "--delay", "35", "4" if notches > 0 else "5")
            finally:
                if ctrl:
                    command("xdotool", "keyup", "ctrl")
        elif operation == "drawing-click":
            command("xdotool", "click", "1")
        else:
            button = "2" if operation == "drawing-pan" else "1"
            command("xdotool", "mousedown", button)
            try:
                for step in range(1, 7):
                    move([start[i] + (end[i] - start[i]) * step / 6 for i in range(2)])
            finally:
                command("xdotool", "mouseup", button)
    finally:
        x11.XCloseDisplay(display)
    def logical(pixel):
        return [client["x"] + (pixel[0] - geometry["X"]) / scale[0],
                client["y"] + (pixel[1] - geometry["Y"]) / scale[1]]

    print(json.dumps({"source": "X11 XTEST", "pid": pid, "window": window,
                      "xvfb_pid": xvfb, "client": client, "window_geometry": geometry,
                      "scale": scale, "operation": operation,
                      "physical_start": physical_start, "physical_end": physical_end,
                      "logical_start": logical(physical_start), "logical_end": logical(physical_end)}))


if __name__ == "__main__":
    main()
