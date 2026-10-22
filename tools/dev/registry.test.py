#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Unit tests for the registry's implementation field."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import registry


def row(**fields):
    return dict(name='fixture', command=['unused'], cells=['bdb'], **fields)


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
                form, _, target = entry['implementation'].partition(':')
                self.assertIn(form, registry.IMPLEMENTATION_FORMS)
                self.assertTrue((ROOT / target).is_file())

    def test_the_three_forms_load(self):
        for implementation in ('legacy:tools/bisection/run-bisect.sh',
                               'instrument:tools/bisection/run-perf-baseline.sh',
                               'python:tools.dev.registry', 'python:tools.dev'):
            with self.subTest(implementation=implementation):
                self.assertEqual(self.load([row(implementation=implementation)])[0]['implementation'],
                                 implementation)

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

    def test_existing_checks_run_before_the_implementation_check(self):
        with self.assertRaisesRegex(registry.RegistryError, "registry row 'fixture' has no command"):
            self.load([dict(name='fixture', cells=['bdb'])])
        with self.assertRaisesRegex(registry.RegistryError, "issues must be a list of strings"):
            self.load([row(issues=[541])])


if __name__ == '__main__':
    unittest.main()
