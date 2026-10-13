#!/usr/bin/env python3
"""Bounded row drag through XTEST on the same privately-owned drawing host."""
import ctypes
import ctypes.util
import importlib.util
import json
from pathlib import Path
import sys
import time

spec = importlib.util.spec_from_file_location("drawing_input", Path(__file__).with_name("native-drawing-linux.py"))
drawing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(drawing)
require, command = drawing.require, drawing.command


def waypoints(request):
    points = request.get("points")
    require(isinstance(points, list) and 1 <= len(points) <= 8, "CAM drag requires 1..8 waypoints")
    require(isinstance(request.get("cancel"), bool), "CAM cancel must be a boolean")
    total = 0
    for point in points:
        require(isinstance(point, dict), "CAM waypoint must be an object")
        hold = point.get("hold_ms")
        require(type(hold) is int and 0 <= hold <= 800, "CAM hold must be 0..800 integral milliseconds")
        total += hold
    require(total <= 1600, "CAM total dwell exceeds 1600 milliseconds")
    return points


def main():
    pid, window = int(sys.argv[1]), sys.argv[2]
    xvfb = drawing.private_xvfb()
    request = json.load(sys.stdin)
    points = waypoints(request)
    client = request["client"]
    geometry = dict(line.split("=", 1) for line in command("xdotool", "getwindowgeometry", "--shell", window).splitlines())
    geometry = {key: int(geometry[key]) for key in ("X", "Y", "WIDTH", "HEIGHT")}
    start = [request["x"], request["y"]]
    physical_start, scale = drawing.physical_point(client, geometry, start)
    for point in points:
        drawing.physical_point(client, geometry, [point["x"], point["y"]])
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
    require(display, "Cannot open owned Xvfb display")
    root = x11.XDefaultRootWindow(display)

    def recipient(point):
        require(int(command("xdotool", "getwindowpid", window)) == pid, "CAM window ownership changed")
        require(command("xdotool", "getwindowfocus") == window, "Owned CAM window lost focus")
        pixel, _ = drawing.physical_point(client, geometry, point)
        x, y, child = ctypes.c_int(), ctypes.c_int(), ctypes.c_ulong()
        require(x11.XTranslateCoordinates(display, root, root, *pixel, ctypes.byref(x), ctypes.byref(y), ctypes.byref(child)), "Cannot resolve CAM gesture target")
        require(child.value == int(window), "Owned CAM gesture target is occluded")
        return pixel

    def move(point):
        drawing.move_pointer(recipient(point))
        time.sleep(.035)

    try:
        recipient(start)
        for point in points:
            recipient([point["x"], point["y"]])
        move(start)
        command("xdotool", "mousedown", "1")
        try:
            previous = start
            for waypoint in points:
                end = [waypoint["x"], waypoint["y"]]
                for step in range(1, 7):
                    move([previous[i] + (end[i]-previous[i])*step/6 for i in range(2)])
                deadline = time.monotonic() + waypoint["hold_ms"] / 1000
                while True:
                    recipient(end)
                    if time.monotonic() >= deadline:
                        break
                    time.sleep(.02)
                previous = end
            if request["cancel"]:
                recipient(end)
                command("xdotool", "key", "Escape")
                time.sleep(.1)
        finally:
            command("xdotool", "mouseup", "1")
        time.sleep(.15)
        physical_end = recipient(end)
    finally:
        x11.XCloseDisplay(display)

    def logical(pixel):
        return [client["x"] + (pixel[0]-geometry["X"])/scale[0],
                client["y"] + (pixel[1]-geometry["Y"])/scale[1]]

    print(json.dumps({"source":"X11 XTEST","pid":pid,"window":window,"xvfb_pid":xvfb,
                      "operation":"cam-row-drag","cancel":request["cancel"],"client":client,
                      "window_geometry":geometry,"scale":scale,"points":points,
                      "physical_start":physical_start,"physical_end":physical_end,
                      "logical_start":logical(physical_start),"logical_end":logical(physical_end)}))


if __name__ == "__main__":
    main()
