#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Tests for the measure recipe execution checks."""
import contextlib
import io
from pathlib import Path
import sys
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev.measure import recipes
from tools.dev.runners import Finished


def run(operation, argv=(), **options):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        status = recipes.run(operation, argv, {'GITHUB_ACTIONS': 'true'}, **options)
    return status, out.getvalue(), err.getvalue()


class RecipeTests(unittest.TestCase):
    def test_the_shared_failure_ladder_maps_every_ending(self):
        def raises(error):
            def recipe(argv, env):
                raise error
            return recipe
        self.assertEqual(run(raises(Finished(9)))[0], 9)
        self.assertEqual(run(raises(FileNotFoundError('taskset')))[0], 127)
        status, _, err = run(raises(ValueError('bad')))
        self.assertEqual((status, err), (1, 'FAIL: bad\n'))


    def test_a_pin_comparison_is_refused_before_running_the_operation(self):
        operation = mock.Mock()
        status, _, err = run(operation, pin_comparison=True)
        self.assertEqual(status, 2)
        self.assertIn('require the local oracle performance stage', err)
        operation.assert_not_called()


if __name__ == '__main__':
    unittest.main()
