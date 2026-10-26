#!/usr/bin/env python3
"""Offline regressions for expected chooser retirement after path entry."""
import importlib.util
from pathlib import Path
import sys
import unittest
from unittest.mock import Mock, patch

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "script_dialog", Path(__file__).with_name("native-script-dialog-linux.py"))
dialog = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dialog)


class Clock:
    def __init__(self):
        self.now = 0.0

    def monotonic(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


class ChooserCloseTests(unittest.TestCase):
    def setUp(self):
        self.clock = Clock()
        self.time_patch = patch.object(dialog, "time", self.clock)
        self.time_patch.start()
        self.addCleanup(self.time_patch.stop)

    def test_close_between_visibility_read_and_host_guard(self):
        open_window = True
        observed = []

        def visible():
            nonlocal open_window
            if open_window:
                open_window = False
                return True
            return False

        def guard_owner():
            # The dialog has retired; the exact host remains valid.
            observed.append(open_window)

        dialog.wait_for_close(visible, guard_owner)
        self.assertEqual(observed, [False, False])

    def test_already_closed_still_checks_host(self):
        guard_owner = Mock()
        dialog.wait_for_close(lambda: False, guard_owner)
        guard_owner.assert_called_once_with()

    def test_closed_dialog_does_not_hide_changed_host_owner(self):
        guard_owner = Mock(side_effect=RuntimeError("Native window owner changed"))
        with self.assertRaisesRegex(RuntimeError, "Native window owner changed"):
            dialog.wait_for_close(lambda: False, guard_owner)

    def test_host_lifetime_change_during_wait_fails(self):
        guard_owner = Mock(side_effect=[None, RuntimeError("Owned host lifetime changed")])
        with self.assertRaisesRegex(RuntimeError, "Owned host lifetime changed"):
            dialog.wait_for_close(lambda: True, guard_owner)

    def test_visible_dialog_keeps_existing_five_second_deadline(self):
        with self.assertRaisesRegex(RuntimeError, "did not close after path entry"):
            dialog.wait_for_close(lambda: True, Mock())
        self.assertGreaterEqual(self.clock.now, 5.0)
        self.assertLess(self.clock.now, 5.2)

    def test_window_inventory_error_is_not_a_successful_close(self):
        visible = Mock(side_effect=RuntimeError("Ambiguous chooser window inventory"))
        with self.assertRaisesRegex(RuntimeError, "Ambiguous chooser window inventory"):
            dialog.wait_for_close(visible, Mock())


if __name__ == "__main__":
    unittest.main()
