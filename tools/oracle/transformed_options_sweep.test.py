"""Unit tests for the cursor comparison in tools/oracle/transformed_options_sweep.py."""
import importlib.util
import signal
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name('transformed_options_sweep.py')
SPEC = importlib.util.spec_from_file_location('transformed_options_sweep', MODULE_PATH)
sweep = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sweep)


def observation(sig=None, result=None, warnings=0):
    return {'signal': sig, 'result': result, 'warnings': warnings, 'diagnostic': ''}


ABORT = signal.SIGABRT


class CursorObservationsTest(unittest.TestCase):
    def test_both_abort_with_no_extra_warning_is_accepted(self):
        self.assertTrue(sweep.cursor_observations_agree(
            observation(ABORT), observation(ABORT)))

    def test_a_subject_abort_after_an_extra_warning_is_rejected(self):
        self.assertFalse(sweep.cursor_observations_agree(
            observation(ABORT), observation(ABORT, warnings=1)))

    def test_a_pin_return_against_another_value_or_an_abort_is_rejected(self):
        pin = observation(None, [True, 4])
        self.assertTrue(sweep.cursor_observations_agree(
            pin, observation(None, [True, 4])))
        self.assertFalse(sweep.cursor_observations_agree(
            pin, observation(None, [True, 5])))
        self.assertFalse(sweep.cursor_observations_agree(
            pin, observation(ABORT)))


if __name__ == '__main__':
    unittest.main()
