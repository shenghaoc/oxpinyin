#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the measure recipe table and its dispatcher: the fixed name tuple matches what the
modules contribute, a recipe runs under the shared failure ladder, and the CLI forwards to it."""
import contextlib
import io
from pathlib import Path
import sys
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import cli, host
from tools.dev.measure import recipes
from tools.dev.runners import Finished


def dispatch(name, argv=(), table=None):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        with mock.patch.object(recipes, 'table', lambda: table):
            status = recipes.main(name, argv, {'A': 'b'})
    return status, out.getvalue(), err.getvalue()


class TableTests(unittest.TestCase):
    def test_the_fixed_names_are_exactly_what_the_modules_contribute(self):
        self.assertEqual(sorted(recipes.NAMES), sorted(recipes.table()))
        self.assertEqual(list(recipes.NAMES), sorted(recipes.NAMES))

    def test_every_recipe_takes_argv_and_env_and_is_not_a_startup_name(self):
        for name, function in recipes.table().items():
            with self.subTest(name=name):
                self.assertNotIn(name, ('startup', 'cold-qualify'))
                self.assertEqual(function.__code__.co_argcount, 2)

    def test_the_registry_rows_that_were_shell_runners_run_the_recipe_of_their_name(self):
        import json
        rows = {row['name']: row for row in json.loads((ROOT / 'tools/bisection/runners.json').read_text())}
        for name in ('callgrind-differential', 'perf-baseline', 'perf-matrix', 'perf-same-data', 'rss-diag',
                     'shipped-check', 'system-import-profile', 'tree-recipe-control'):
            with self.subTest(name=name):
                self.assertEqual(rows[name]['implementation'], 'python:tools.dev')
                self.assertEqual(rows[name]['command'][-3:], ['tools.dev', 'measure', name])
                self.assertIn(name, recipes.NAMES)


class DispatchTests(unittest.TestCase):
    def test_a_recipe_gets_its_arguments_and_the_environment_and_returns_its_status(self):
        seen = []
        status, out, err = dispatch('x', ['--a', 'b'], {'x': lambda argv, env: seen.append((argv, env['A'])) or 0})
        self.assertEqual((status, out, err, seen), (0, '', '', [(['--a', 'b'], 'b')]))

    def test_an_unknown_name_is_a_usage_error(self):
        self.assertEqual(dispatch('nope', table={}), (2, '', 'unknown measure recipe: nope\n'))

    def test_the_shared_failure_ladder_maps_every_ending(self):
        def raises(error):
            def recipe(argv, env):
                raise error
            return {'x': recipe}
        self.assertEqual(dispatch('x', table=raises(Finished(9)))[0], 9)
        self.assertEqual(dispatch('x', table=raises(FileNotFoundError('taskset')))[0], 127)
        status, _, err = dispatch('x', table=raises(ValueError('bad')))
        self.assertEqual((status, err), (1, 'FAIL: bad\n'))
        status, _, err = dispatch('x', table=raises(host.ResourceBlocked('resource-blocked: no room')))
        self.assertEqual((status, err), (1, 'resource-blocked: no room\n'))


class CommandLineTests(unittest.TestCase):
    def test_measure_accepts_every_recipe_and_keeps_startup_and_cold_qualify(self):
        parser = cli.build_parser()
        for name in ('startup', 'cold-qualify', *recipes.NAMES):
            with self.subTest(name=name):
                args = parser.parse_args(['measure', name, '--out', 'x', '-h'])
                self.assertEqual((args.recipe, args.args), (name, ['--out', 'x', '-h']))

    def test_the_command_forwards_a_recipe_name_and_arguments_to_the_dispatcher(self):
        with mock.patch.object(recipes, 'main', return_value=4) as main:
            status = cli.main(['measure', 'system-import-profile', '--jobs', '1'])
        self.assertEqual(status, 4)
        main.assert_called_once_with('system-import-profile', ['--jobs', '1'])

    def test_an_unknown_recipe_is_refused_by_the_parser(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as stop:
            cli.main(['measure', 'no-such-recipe'])
        self.assertEqual(stop.exception.code, 2)


if __name__ == '__main__':
    unittest.main()
