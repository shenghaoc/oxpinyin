#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Crate cfg-placement lint fixtures, diagnostics and statuses.

The shell harness's fixtures moved here with the linter: the service takes
explicit paths, so no case needs a git repository.
"""
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import check

VIOLATION_HEADER = ('crate-cfg-placement: a crate-level `#![cfg(...)]` must sit BELOW '
                    'its `//!` docs.\n\n')
VIOLATION_FOOTER = (
    'A false crate-level `#![cfg(...)]` discards the crate attributes that follow\n'
    'it, so these docs vanish on the excluded platform and `missing_docs` fires\n'
    'there. Move the `#![cfg(...)]` below the `//!` block; the gate keeps its\n'
    'predicate and its whole-crate scope.\n')


class LintTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def fixture(self, name, text):
        path = self.root / f'{name}.rs'
        path.write_text(text)
        return str(path)

    def lint(self, *args, root=None):
        out, err = io.StringIO(), io.StringIO()
        status = check.lint_crate_cfg_placement(list(args), out=out, err=err,
                                                root=root or self.root)
        return status, out.getvalue(), err.getvalue()

    def test_docs_above_the_gate_pass(self):
        path = self.fixture('docs_then_gate', '//! Crate docs.\n'
                            '#![cfg(target_os = "linux")]\n\n#[test]\nfn t() {}\n')
        self.assertEqual(self.lint(path), (0, f'crate-cfg-placement: 1 file(s) checked, '
                                            'no violations\n', ''))

    def test_gate_without_docs_and_docs_without_gate_pass(self):
        gate = self.fixture('gate_only', '#![cfg(target_os = "linux")]\n\n#[test]\nfn t() {}\n')
        docs = self.fixture('docs_only', '//! Crate docs.\n\n#[test]\nfn t() {}\n')
        status, out, err = self.lint(gate, docs)
        self.assertEqual((status, err), (0, ''))
        self.assertIn('2 file(s) checked, no violations', out)

    def test_cfg_attr_is_not_a_crate_stripping_gate(self):
        path = self.fixture('cfg_attr_then_docs', '#![cfg_attr(not(target_os = "linux"), '
                            'allow(dead_code))]\n//! Crate docs.\n')
        self.assertEqual(self.lint(path)[0], 0)

    def test_indented_module_docs_do_not_fire(self):
        path = self.fixture('nested', '//! Crate docs.\n#![cfg(target_os = "linux")]\n\n'
                            'mod inner {\n    //! Module docs, not crate docs.\n}\n')
        self.assertEqual(self.lint(path)[0], 0)

    def test_docs_below_the_gate_report_both_lines(self):
        path = self.fixture('gate_then_docs', '#![cfg(target_os = "linux")]\n'
                            '//! Crate docs.\n')
        status, out, err = self.lint(path)
        self.assertEqual(status, 1)
        self.assertEqual(out, '')
        self.assertEqual(err, VIOLATION_HEADER +
                         f'  {path}:2: `//!` doc comment below the crate-level '
                         '`#![cfg(...)]` on line 1\n\n' + VIOLATION_FOOTER)

    def test_distance_from_the_gate_does_not_excuse_it(self):
        path = self.fixture('spaced', '#![cfg(any(feature = "a", feature = "b"))]\n'
                            '#![allow(clippy::pedantic)]\n\n// An ordinary comment.\n\n'
                            '//! Crate docs arriving far too late.\n')
        status, _, err = self.lint(path)
        self.assertEqual(status, 1)
        self.assertIn(f'{path}:6:', err)
        self.assertIn('on line 1', err)

    def test_one_bad_file_among_good_ones_fails_the_run(self):
        good = self.fixture('good', '//! Crate docs.\n#![cfg(target_os = "linux")]\n')
        bad = self.fixture('bad', '#![cfg(target_os = "linux")]\n//! Crate docs.\n')
        docs = self.fixture('docs', '//! Crate docs.\n')
        status, _, err = self.lint(good, bad, docs)
        self.assertEqual(status, 1)
        self.assertIn(f'{bad}:2:', err)

    def test_help_is_usage_status_two(self):
        for flag in ('--help', '-h'):
            with self.subTest(flag=flag):
                status, out, err = self.lint(flag)
                self.assertEqual((status, out), (2, ''))
                self.assertTrue(err)

    def test_named_paths_must_be_readable_regular_files(self):
        missing = str(self.root / 'no-such-file.rs')
        good = self.fixture('good', '//! Crate docs.\n')
        for args, want in (([missing], missing),
                           ([str(self.root)], str(self.root)),
                           ([good, missing], missing)):
            with self.subTest(args=args):
                status, out, err = self.lint(*args)
                self.assertEqual((status, out), (2, ''))
                self.assertEqual(err, 'crate-cfg-placement: not a readable regular file:\n'
                                 f'  {want}\n')

    def test_no_arguments_outside_a_repository_is_an_environment_error(self):
        status, out, err = self.lint()
        self.assertEqual((status, out), (2, ''))
        self.assertEqual(err, 'crate-cfg-placement: not a git repository and no files given\n')

    def test_no_arguments_scans_tracked_rust_files(self):
        subprocess.run(['git', 'init', '-q'], cwd=self.root, check=True)
        clean = self.root / 'clean.rs'
        clean.write_text('//! Crate docs.\n#![cfg(target_os = "linux")]\n')
        bad = self.root / 'bad.rs'
        bad.write_text('#![cfg(target_os = "linux")]\n//! Crate docs.\n')
        subprocess.run(['git', 'add', 'clean.rs', 'bad.rs'], cwd=self.root, check=True)
        status, out, err = self.lint(root=self.root)
        self.assertEqual((status, out), (1, ''))
        self.assertIn('bad.rs:2:', err)


if __name__ == '__main__':
    unittest.main()
