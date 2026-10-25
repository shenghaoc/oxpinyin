#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Unit tests for the registry catalog and its selection rules."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import registry


def row(**fields):
    return dict(name='fixture', command=['unused'], cells=['bdb'], **fields)


def target_file(root, implementation):
    """The file an implementation names: a path form's path as given, or a
    python form's `<module>.py`, else its package's `__main__.py`."""
    form, _, target = implementation.partition(':')
    if form != 'python':
        return root / target
    base = root.joinpath(*target.split('.'))
    return base.with_suffix('.py') if base.with_suffix('.py').is_file() else base / '__main__.py'


class ImplementationTests(unittest.TestCase):
    def load(self, rows):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / 'runners.json'
            path.write_text(json.dumps(rows))
            return registry.load(path)

    def rejects(self, implementation, message):
        fields = {} if implementation is None else {'implementation': implementation}
        with self.assertRaises(registry.RegistryError) as raised:
            self.load([row(**fields)])
        self.assertEqual(str(raised.exception), message)

    def test_every_checked_in_row_names_an_existing_implementation(self):
        rows = registry.load()
        self.assertTrue(rows)
        for entry in rows:
            with self.subTest(row=entry['name']):
                self.assertIn(entry['implementation'].partition(':')[0], registry.IMPLEMENTATION_FORMS)
                self.assertTrue(target_file(ROOT, entry['implementation']).is_file())

    def test_checked_in_row_logic_resolves_python_modules_and_keeps_paths_literal(self):
        for implementation, expected in (('python:tools.dev.registry', 'tools/dev/registry.py'),
                                         ('python:tools.dev', 'tools/dev/__main__.py'),
                                         ('legacy:tools/bisection/run-bisect.sh', 'tools/bisection/run-bisect.sh')):
            with self.subTest(implementation=implementation):
                self.assertEqual(target_file(ROOT, implementation), ROOT / expected)
                self.assertTrue(target_file(ROOT, implementation).is_file())
        self.assertFalse(target_file(ROOT, 'legacy:tools.dev.registry').is_file())

    def test_the_three_forms_load_when_the_command_runs_them(self):
        python = ['uv', 'run', 'python', '-X', 'lazy_imports=normal', '-m']
        for implementation, command in (
                ('legacy:tools/bisection/run-bisect.sh', ['bash', '{root}/tools/bisection/run-bisect.sh']),
                ('legacy:tools/bisection/run-bisect.sh', ['bash', './tools/bisection/run-bisect.sh']),
                ('legacy:tools/bisection/run-bisect.sh',
                 ['bash', (ROOT / 'tools/bisection/run-bisect.sh').as_posix()]),
                ('legacy:./tools/bisection/run-bisect.sh', ['{root}/tools/bisection/run-bisect.sh', 'x']),
                ('instrument:tools/bisection/run-perf-baseline.sh',
                 ['bash', '{root}/tools/bisection/run-perf-baseline.sh']),
                ('python:tools.dev.registry', [*python, 'tools.dev.registry', 'list']),
                ('python:tools.dev', [*python, 'tools.dev'])):
            with self.subTest(implementation=implementation, command=command):
                loaded = self.load([row(implementation=implementation) | {'command': command}])
                self.assertEqual(loaded[0]['implementation'], implementation)

    def test_path_not_in_the_command(self):
        for implementation, command in (
                ('legacy:tools/bisection/run-bisect.sh', ['bash', '{root}/tools/bisection/run-w11-diff.sh']),
                ('legacy:tools/bisection/run-bisect.sh', ['bash', '{root}/tools/bisection/run-bisect.sh.bak']),
                ('legacy:tools/bisection/run-bisect.sh', ['bash', '-c', '{root}/tools/bisection/run-bisect.sh x']),
                ('instrument:tools/bisection/run-perf-baseline.sh',
                 ['bash', '{prefix}/tools/bisection/run-perf-baseline.sh'])):
            with self.subTest(implementation=implementation, command=command):
                with self.assertRaises(registry.RegistryError) as raised:
                    self.load([row(implementation=implementation) | {'command': command}])
                self.assertEqual(str(raised.exception),
                                 f"registry row 'fixture' implementation {implementation!r} does not "
                                 f'match its command {json.dumps(command)}')

    def test_python_module_not_run_with_dash_m(self):
        implementation = 'python:tools.dev.registry'
        for command in (['python', 'tools/dev/registry.py'], ['python', 'tools.dev.registry'],
                        ['python', '-m', 'tools.dev'], ['python', '-m'],
                        ['python', '-m', 'tools.dev.registry.extra'], ['python', '-mtools.dev.registry']):
            with self.subTest(command=command):
                with self.assertRaises(registry.RegistryError) as raised:
                    self.load([row(implementation=implementation) | {'command': command}])
                self.assertEqual(str(raised.exception),
                                 f"registry row 'fixture' implementation {implementation!r} does not "
                                 f'match its command {json.dumps(command)}')

    def test_missing_field(self):
        for implementation in (None, '', 7, ['legacy:tools/bisection/run-bisect.sh']):
            with self.subTest(implementation=implementation):
                self.rejects(implementation, "registry row 'fixture' has no implementation")

    def test_unknown_form(self):
        for implementation in ('native', 'native:tools/bisection/run-bisect.sh',
                               'bash:tools/bisection/run-bisect.sh', 'legacy:',
                               'tools/bisection/run-bisect.sh', 'Legacy:tools/bisection/run-bisect.sh'):
            with self.subTest(implementation=implementation):
                self.rejects(implementation,
                             f"registry row 'fixture' implementation {implementation!r} is not "
                             'legacy:<path>, python:<module> or instrument:<path>')

    def test_path_or_module_that_does_not_exist(self):
        for implementation in ('legacy:tools/bisection/run-missing.sh',
                               'instrument:tools/bisection',
                               'python:tools.dev.missing'):
            with self.subTest(implementation=implementation):
                self.rejects(implementation,
                             f"registry row 'fixture' implementation {implementation!r} does not exist")

    def test_path_outside_the_checkout(self):
        for implementation in ('legacy:/bin/sh', 'legacy:../oxpinyin/tools/bisection/run-bisect.sh',
                               'instrument:tools\\bisection\\run-perf-baseline.sh'):
            with self.subTest(implementation=implementation):
                self.rejects(implementation,
                             f"registry row 'fixture' implementation {implementation!r} must be a "
                             'checkout-relative path')

    def test_malformed_module_name(self):
        for implementation in ('python:tools/dev/registry.py', 'python:tools..dev', 'python:tools.dev-x'):
            with self.subTest(implementation=implementation):
                self.rejects(implementation,
                             f"registry row 'fixture' implementation {implementation!r} is not a module name")

    def fake_checkout(self):
        scratch = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        root, outside = scratch / 'checkout', scratch / 'outside'
        (root / 'pkg').mkdir(parents=True)
        (root / 'pkg/__init__.py').write_text('')
        (root / 'runnable').mkdir()
        (root / 'runnable/__init__.py').write_text('')
        (root / 'runnable/__main__.py').write_text('')
        outside.mkdir()
        (outside / 'tool.sh').write_text('')
        (outside / 'mod.py').write_text('')
        (outside / 'pkg').mkdir()
        (outside / 'pkg/__init__.py').write_text('')
        (outside / 'pkg/__main__.py').write_text('')
        (root / 'tool.sh').symlink_to(outside / 'tool.sh')
        (root / 'mod.py').symlink_to(outside / 'mod.py')
        (root / 'linked').symlink_to(outside / 'pkg', target_is_directory=True)
        (root / 'inside.sh').write_text('')
        (root / 'alias.sh').symlink_to(root / 'inside.sh')
        self.enterContext(mock.patch.object(registry, 'ROOT', root))
        return root

    def load_command(self, implementation, command):
        return self.load([row(implementation=implementation) | {'command': command}])

    def test_package_needs_a_main_module(self):
        self.fake_checkout()
        with self.assertRaises(registry.RegistryError) as raised:
            self.load_command('python:pkg', ['python', '-m', 'pkg'])
        self.assertEqual(str(raised.exception),
                         "registry row 'fixture' implementation 'python:pkg' is a package without __main__.py")
        self.assertEqual(self.load_command('python:runnable', ['python', '-m', 'runnable'])[0]['implementation'],
                         'python:runnable')

    def test_symlink_outside_the_checkout_is_rejected(self):
        self.fake_checkout()
        for implementation, command in (('legacy:tool.sh', ['bash', '{root}/tool.sh']),
                                        ('instrument:tool.sh', ['bash', '{root}/tool.sh']),
                                        ('python:mod', ['python', '-m', 'mod']),
                                        ('python:linked', ['python', '-m', 'linked'])):
            with self.subTest(implementation=implementation):
                with self.assertRaises(registry.RegistryError) as raised:
                    self.load_command(implementation, command)
                self.assertEqual(str(raised.exception),
                                 f"registry row 'fixture' implementation {implementation!r} resolves "
                                 'outside the checkout')

    def test_symlink_inside_the_checkout_is_accepted(self):
        self.fake_checkout()
        self.assertEqual(self.load_command('legacy:alias.sh', ['bash', '{root}/alias.sh'])[0]['implementation'],
                         'legacy:alias.sh')

    def test_existing_checks_run_before_the_implementation_check(self):
        with self.assertRaisesRegex(registry.RegistryError, "registry row 'fixture' has no command"):
            self.load([dict(name='fixture', cells=['bdb'])])
        with self.assertRaisesRegex(registry.RegistryError, "issues must be a list of strings"):
            self.load([row(issues=[541])])


class SelectionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rows = registry.load()

    def test_default_selection_is_the_catalog_in_file_order(self):
        self.assertEqual(registry.select(self.rows), self.rows)

    def test_names_keep_catalog_order_instead_of_selector_order(self):
        chosen = registry.select(self.rows, names=['union-diff', 'bisect'])
        self.assertEqual([row['name'] for row in chosen], ['bisect', 'union-diff'])

    def test_cells_select_rows_supporting_any_requested_cell(self):
        self.assertEqual([row['name'] for row in registry.select(self.rows, cells=['native'])],
                         ['native-startup'])
        self.assertNotIn('fuzz', [row['name'] for row in registry.select(self.rows, cells=['kc'])])
        self.assertIn('fuzz', [row['name'] for row in registry.select(self.rows, cells=['bdb'])])

    def test_issues_match_normalized_tags_and_keep_order(self):
        chosen = registry.select(self.rows, issues=['Row 33'])
        self.assertEqual([row['name'] for row in chosen], ['abi-probe-diff', 'residue-a-tail-diff'])

    def test_kind_and_require_selection(self):
        chosen = registry.select(self.rows, kind='not-a-gate')
        self.assertTrue(chosen)
        self.assertTrue(all(row['kind'] == 'not-a-gate' for row in chosen))
        with self.assertRaisesRegex(registry.RegistryError, 'plan needs an explicit selection'):
            registry.select(self.rows, require_selection=True)
        self.assertEqual(registry.select(self.rows, names=['bisect'], require_selection=True),
                         [row for row in self.rows if row['name'] == 'bisect'])

    def test_empty_and_unknown_selectors_fail(self):
        for kwargs, message in (
                ({'names': []}, 'empty runner selection'),
                ({'names': ['']}, 'empty runner selection'),
                ({'names': ['missing']}, 'unknown runner(s): missing'),
                ({'cells': []}, 'empty cell selection'),
                ({'cells': ['']}, 'empty cell selection'),
                ({'cells': ['missing']}, 'unknown cell(s): missing'),
                ({'issues': []}, 'empty issue selection'),
                ({'issues': ['']}, 'empty issue selection'),
                ({'issues': ['missing']}, 'unknown issue(s): missing'),
                ({'kind': 'missing'}, 'unknown kind(s): missing')):
            with self.subTest(kwargs=kwargs):
                with self.assertRaises(registry.RegistryError) as raised:
                    registry.select(self.rows, **kwargs)
                self.assertIn(message, str(raised.exception))

    def test_no_match_raises_a_distinct_error(self):
        with self.assertRaises(registry.NoMatch) as raised:
            registry.select(self.rows, names=['bisect'], issues=['W11'])
        self.assertIsInstance(raised.exception, registry.RegistryError)
        self.assertEqual(str(raised.exception), 'selection matched no registry rows')


class MalformedCatalogTests(unittest.TestCase):
    implementation = 'legacy:tools/bisection/run-bisect.sh'
    command = ['bash', '{root}/tools/bisection/run-bisect.sh']

    def row(self, **fields):
        base = dict(name='fixture', command=self.command, cells=['bdb'],
                    implementation=self.implementation)
        return base | fields

    def load(self, rows):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / 'runners.json'
            path.write_text(rows if isinstance(rows, str) else json.dumps(rows))
            return registry.load(path)

    def rejects(self, rows, message):
        with self.assertRaisesRegex(registry.RegistryError, message):
            self.load(rows)

    def test_a_minimal_catalog_loads(self):
        rows = [self.row()]
        self.assertEqual(self.load(rows), rows)

    def test_unreadable_and_non_list_catalogs_fail(self):
        self.rejects('{', 'cannot read registry')
        self.rejects('{}', 'must be a JSON list')

    def test_row_shape_failures_are_clear(self):
        self.rejects([{}], 'has a row without a name')
        self.rejects([{'name': '', 'command': self.command, 'cells': ['bdb']}],
                     'has a row without a name')
        self.rejects([self.row(), self.row()], "names 'fixture' twice")
        self.rejects([self.row(command=[])], "row 'fixture' has no command")
        self.rejects([self.row(cells=[])], "row 'fixture' has no cells")
        self.rejects([self.row(issues=[541])], 'issues must be a list of strings')
        self.rejects([self.row(issues='541')], 'issues must be a list of strings')


if __name__ == '__main__':
    unittest.main()
