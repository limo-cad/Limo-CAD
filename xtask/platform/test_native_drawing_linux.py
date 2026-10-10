"""Coordinate guard tests; these do not claim to exercise an X server."""
import importlib.util
from pathlib import Path
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location("drawing_input", Path(__file__).with_name("native-drawing-linux.py"))
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


class PublishedCoordinates(unittest.TestCase):
    def test_logical_point_maps_to_owned_physical_origin_at_both_scales(self):
        client = {"x": 0, "y": 0, "width": 1000, "height": 700}
        for scale in (1, 2):
            geometry = {"X": 30, "Y": 40, "WIDTH": 1000 * scale, "HEIGHT": 700 * scale}
            point, actual_scale = helper.physical_point(client, geometry, [321.5, 200])
            self.assertEqual(point, [30 + round(321.5 * scale), 40 + 200 * scale])
            self.assertEqual(actual_scale, [scale, scale])

    def test_rejects_nonfinite_outside_and_inconsistent_client(self):
        client = {"x": 0, "y": 0, "width": 1000, "height": 700}
        geometry = {"X": 30, "Y": 40, "WIDTH": 1000, "HEIGHT": 700}
        for point in ([float("nan"), 10], [10, float("inf")], [-1, 10], [1000, 10], [10, 700]):
            with self.assertRaises(RuntimeError):
                helper.physical_point(client, geometry, point)
        with self.assertRaises(RuntimeError):
            helper.physical_point(client, {**geometry, "WIDTH": 2000}, [10, 10])


class PointerCompletion(unittest.TestCase):
    def test_existing_endpoint_is_verified_without_waiting_for_a_motion_event(self):
        with mock.patch.object(helper, "command", side_effect=["", "X=800\nY=446\nSCREEN=0\nWINDOW=7"]) as command:
            helper.move_pointer([800, 446])
        self.assertEqual(command.call_args_list, [
            mock.call("xdotool", "mousemove", "800", "446"),
            mock.call("xdotool", "getmouselocation", "--shell"),
        ])

    def test_move_waits_for_the_actual_requested_root_coordinates(self):
        with mock.patch.object(helper, "command", side_effect=["", "X=800\nY=446", "X=830\nY=456"]) as command, \
             mock.patch.object(helper.time, "sleep") as sleep:
            helper.move_pointer([830, 456])
        self.assertEqual(command.call_count, 3)
        sleep.assert_called_once_with(0.01)

    def test_wrong_coordinates_fail_instead_of_allowing_the_button_event(self):
        with mock.patch.object(helper, "command", side_effect=["", "X=799\nY=446"]):
            with self.assertRaisesRegex(RuntimeError, "actual root position is \\[799, 446\\]"):
                helper.move_pointer([800, 446], timeout=0)


if __name__ == "__main__":
    unittest.main()
