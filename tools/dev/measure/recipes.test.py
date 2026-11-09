#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the measure recipe dispatcher: a recipe runs under the shared failure ladder, and the
CLI refuses an unknown recipe."""
import contextlib
import io
from pathlib import Path
import sys
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import cli
from tools.dev.measure import recipes
from tools.dev.runners import Finished


def dispatch(name, argv=(), table=None):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        with mock.patch.object(recipes, 'table', lambda: table):
            status = recipes.main(name, argv, {'A': 'b'})
    return status, out.getvalue(), err.getvalue()


class DispatchTests(unittest.TestCase):
    def test_the_shared_failure_ladder_maps_every_ending(self):
        def raises(error):
            def recipe(argv, env):
                raise error
            return {'x': recipe}
        self.assertEqual(dispatch('x', table=raises(Finished(9)))[0], 9)
        self.assertEqual(dispatch('x', table=raises(FileNotFoundError('taskset')))[0], 127)
        status, _, err = dispatch('x', table=raises(ValueError('bad')))
        self.assertEqual((status, err), (1, 'FAIL: bad\n'))


class CommandLineTests(unittest.TestCase):
    def test_an_unknown_recipe_is_refused_by_the_parser(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as stop:
            cli.main(['measure', 'no-such-recipe'])
        self.assertEqual(stop.exception.code, 2)


if __name__ == '__main__':
    unittest.main()
