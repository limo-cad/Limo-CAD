"""Pure preflight checks; no display connection or OS input is created."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("cam_row_input", Path(__file__).with_name("native-cam-row-linux.py"))
cam = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cam)


class CamRowPreflight(unittest.TestCase):
    def test_bounded_dwell_and_cancellation(self):
        request = {"points":[{"x":50,"y":230,"hold_ms":420}, {"x":50,"y":200,"hold_ms":0}], "cancel":False}
        self.assertEqual(cam.waypoints(request), request["points"])
        request["cancel"] = True
        self.assertEqual(cam.waypoints(request), request["points"])

    def test_rejects_unbounded_or_invalid_sequences(self):
        for request in [
            {"points":[],"cancel":False},
            {"points":[{"hold_ms":0}]*9,"cancel":False},
            {"points":[{"hold_ms":801}],"cancel":False},
            {"points":[{"hold_ms":-1}],"cancel":False},
            {"points":[{"hold_ms":1.5}],"cancel":False},
            {"points":[{"hold_ms":True}],"cancel":False},
            {"points":[{"hold_ms":800}]*3,"cancel":False},
            {"points":[{"hold_ms":0}],"cancel":"false"},
        ]:
            with self.subTest(request=request), self.assertRaises(RuntimeError):
                cam.waypoints(request)

    def test_physical_rounding_and_owned_client_boundary(self):
        client = {"x":10.,"y":20.,"width":100.,"height":80.}
        geometry = {"X":30,"Y":40,"WIDTH":200,"HEIGHT":160}
        pixel, scale = cam.drawing.physical_point(client, geometry, [35.25, 50.25])
        self.assertEqual(pixel, [80,100])
        self.assertEqual(scale, [2.,2.])
        for point in [[9.,20.],[110.,20.],[10.,100.],[float("nan"),20.]]:
            with self.subTest(point=point), self.assertRaises(RuntimeError):
                cam.drawing.physical_point(client, geometry, point)


if __name__ == "__main__":
    unittest.main()
