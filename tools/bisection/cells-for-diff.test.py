#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Regressions for cells-for-diff.py's rule-2 checks, report and exit status."""
import os
from pathlib import Path
import re
import runpy
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('cells-for-diff.py')
cells_for_diff = runpy.run_path(str(SCRIPT))
BACKENDS = ('bdb', 'tkrzw', 'kyotocabinet')
# Repositories are built without the host's git configuration; the script under test runs with it.
BUILD_ENV = {**os.environ, 'GIT_CONFIG_GLOBAL': os.devnull, 'GIT_CONFIG_NOSYSTEM': '1',
             'GIT_AUTHOR_NAME': 'test', 'GIT_AUTHOR_EMAIL': 'test@example.invalid',
             'GIT_COMMITTER_NAME': 'test', 'GIT_COMMITTER_EMAIL': 'test@example.invalid'}


def gate(backend):
    """A feature gate for backend. Built here so this file never holds a gate itself."""
    return f'#[cfg(feature = "{backend}")]'


def fired(matches):
    return {number for number, found in matches.items() if found}


class Repo:
    """A throwaway repository: the branch `base` holds the first commit and HEAD the second."""

    def __init__(self, test, base, head):
        scratch = tempfile.TemporaryDirectory(prefix='cells-for-diff-')
        test.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        self.git('init', '-q', '-b', 'main')
        self.commit(base)
        self.git('branch', 'base')
        self.commit(head)

    def git(self, *args):
        result = subprocess.run(('git', *args), cwd=self.root, env=BUILD_ENV, capture_output=True,
                                text=True, check=True)
        return result.stdout.strip()

    def commit(self, files):
        """Write files ({path: text}, None deletes the path) and commit them."""
        for name, text in files.items():
            path = self.root / name
            if text is None:
                path.unlink()
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text)
        self.git('add', '-A')
        self.git('commit', '-q', '--no-verify', '-m', 'commit')

    def checks(self, base='base'):
        """Each check's matches by check number, as the script computes them."""
        _, _, results = cells_for_diff['run_checks'](base, self.root)
        return {number: matches for number, _, matches in results}

    def cli(self, *args, cwd=None):
        return subprocess.run((sys.executable, str(SCRIPT), *args), cwd=cwd or self.root,
                              capture_output=True, text=True)


class StoreCrateTests(unittest.TestCase):
    def test_a_path_under_the_store_crate_fires_check_1(self):
        repo = Repo(self, {'README.md': 'a\n', 'crates/oxpinyin-store/src/old.rs': '// old\n'},
                    {'crates/oxpinyin-store/src/new.rs': '// new\n', 'crates/oxpinyin-store/src/old.rs': None})
        got = repo.checks()
        self.assertEqual(got[1], ['crates/oxpinyin-store/src/new.rs', 'crates/oxpinyin-store/src/old.rs'])
        self.assertEqual(fired(got), {1})

    def test_a_file_moved_out_of_the_store_crate_fires_check_1(self):
        body = ''.join(f'fn f{n}() {{}}\n' for n in range(20))
        repo = Repo(self, {'crates/oxpinyin-store/src/moved.rs': body},
                    {'crates/oxpinyin-store/src/moved.rs': None, 'crates/oxpinyin-core/src/moved.rs': body})
        for setting in ('true', 'false'):
            with self.subTest(diff_renames=setting):
                repo.git('config', 'diff.renames', setting)
                self.assertEqual(repo.checks()[1], ['crates/oxpinyin-store/src/moved.rs'])


class FeatureGateTests(unittest.TestCase):
    def test_a_gate_in_a_changed_file_fires_check_2_for_each_backend(self):
        head = {f'crates/x/src/{backend}.rs': f'fn f() {{}}\n{gate(backend)}\nfn g() {{}}\n'
                for backend in BACKENDS}
        got = Repo(self, {'README.md': 'a\n'}, head).checks()
        self.assertEqual(got[2], [f'crates/x/src/{backend}.rs:2: {gate(backend)}'
                                  for backend in sorted(BACKENDS)])
        self.assertEqual(fired(got), {2})

    def test_the_pattern_is_the_rules_spacing_and_names(self):
        lines = [f'#[cfg(feature="{BACKENDS[0]}")]',
                 f'#[cfg(feature   =   "{BACKENDS[1]}")]',
                 '#[cfg(feature = "other")]',
                 f'#[cfg(not(feature = "{BACKENDS[2]}"))]',
                 f'// feature = {BACKENDS[0]}']
        repo = Repo(self, {'a.txt': 'a\n'}, {'crates/x/src/lib.rs': '\n'.join(lines) + '\n'})
        self.assertEqual([found.split(':')[1] for found in repo.checks()[2]], ['1', '2', '4'])

    def test_a_changed_file_is_read_whole_as_committed_at_head(self):
        repo = Repo(self, {'crates/x/src/lib.rs': gate('bdb') + '\nfn a() {}\n', 'crates/x/src/gone.rs': gate('tkrzw') + '\n'},
                    {'crates/x/src/lib.rs': gate('bdb') + '\nfn b() {}\n', 'crates/x/src/gone.rs': None})
        self.assertEqual(repo.checks()[2], [f'crates/x/src/lib.rs:1: {gate("bdb")}'])

    def test_markdown_and_unchanged_files_do_not_fire_check_2(self):
        text = '\n'.join(gate(backend) for backend in BACKENDS) + '\n'
        repo = Repo(self, {'crates/x/src/gated.rs': text, 'NOTES.md': 'a\n', 'crates/x/src/other.rs': 'a\n'},
                    {'NOTES.md': text, 'crates/x/src/other.rs': 'b\n'})
        self.assertEqual(fired(repo.checks()), set())


class CargoTomlTests(unittest.TestCase):
    def test_a_line_that_starts_with_a_backend_name_fires_check_3(self):
        head = '[features]\nold = []\nbdb = []\ntkrzw = ["dep:x"]\nkyotocabinet = []\n'
        got = Repo(self, {'crates/x/Cargo.toml': '[features]\nold = []\n'}, {'crates/x/Cargo.toml': head}).checks()
        self.assertEqual(got[3], ['crates/x/Cargo.toml: +bdb = []', 'crates/x/Cargo.toml: +tkrzw = ["dep:x"]',
                                  'crates/x/Cargo.toml: +kyotocabinet = []'])
        self.assertEqual(fired(got), {3})

    def test_removed_and_edited_lines_that_start_with_a_backend_name_fire_check_3(self):
        got = Repo(self, {'crates/x/Cargo.toml': 'bdb = []\ntkrzw = []\nkyotocabinet = []\n'},
                   {'crates/x/Cargo.toml': 'tkrzw = ["dep:y"]\nkyotocabinet = []\n'}).checks()
        self.assertEqual(got[3], ['crates/x/Cargo.toml: -bdb = []', 'crates/x/Cargo.toml: -tkrzw = []',
                                  'crates/x/Cargo.toml: +tkrzw = ["dep:y"]'])

    def test_a_backend_named_later_in_a_line_fires_check_3(self):
        lines = ['default = ["bdb"]', 'store = { path = "../store", features = ["tkrzw"] }',
                 '# kyotocabinet only on Linux']
        got = Repo(self, {'crates/x/Cargo.toml': 'x = 1\n'},
                   {'crates/x/Cargo.toml': 'x = 1\n' + '\n'.join(lines) + '\n'}).checks()
        self.assertEqual(got[3], [f'crates/x/Cargo.toml: +{line}' for line in lines])

    def test_file_headers_alone_fire_nothing(self):
        def crates(version):
            return {f'crates/{backend}-notes/Cargo.toml': f'[package]\nversion = "{version}"\n'
                    for backend in BACKENDS}
        repo = Repo(self, crates('0.1.0'), crates('0.1.1'))
        patch = repo.git('diff', '-U0', 'base', 'HEAD')
        for backend in BACKENDS:
            self.assertIn(f'+++ b/crates/{backend}-notes/Cargo.toml', patch)
        self.assertEqual(fired(repo.checks()), set())

    def test_content_that_looks_like_a_file_header_is_still_content(self):
        lines = ['- bdb notes', '-- tkrzw notes', '++ kyotocabinet notes']
        text = 'x = 1\ndescription = """\n' + '\n'.join(lines) + '\n"""\n'
        got = Repo(self, {'crates/x/Cargo.toml': 'x = 1\n'}, {'crates/x/Cargo.toml': text}).checks()
        self.assertEqual(got[3], [f'crates/x/Cargo.toml: +{line}' for line in lines])

    def test_unchanged_backend_lines_beside_a_change_do_not_fire(self):
        text = '[package]\nversion = "{}"\n[features]\nbdb = []\ntkrzw = []\nkyotocabinet = []\n'
        repo = Repo(self, {'crates/x/Cargo.toml': text.format('0.1.0')},
                    {'crates/x/Cargo.toml': text.format('0.1.1')})
        self.assertEqual(fired(repo.checks()), set())

    def test_a_moved_cargo_toml_counts_at_both_paths(self):
        text = '[features]\nbdb = []\n'
        repo = Repo(self, {'crates/a/Cargo.toml': text}, {'crates/a/Cargo.toml': None, 'crates/b/Cargo.toml': text})
        for setting in ('true', 'false'):
            with self.subTest(diff_renames=setting):
                repo.git('config', 'diff.renames', setting)
                self.assertEqual(repo.checks()[3], ['crates/a/Cargo.toml: -bdb = []', 'crates/b/Cargo.toml: +bdb = []'])


class SelectionTests(unittest.TestCase):
    def test_a_diff_touching_none_fires_nothing(self):
        base = {'README.md': 'a\n', 'crates/oxpinyin-core/src/lib.rs': 'a\n', 'settings.toml': 'a = 1\n', 'Cargo.lock': 'a\n'}
        head = {'README.md': 'bdb and tkrzw notes\n', 'crates/oxpinyin-core/src/lib.rs': '// bdb, kyotocabinet\n',
                'settings.toml': 'tkrzw = 1\n', 'Cargo.lock': 'name = "kyotocabinet-sys"\n'}
        self.assertEqual(Repo(self, base, head).checks(), {1: [], 2: [], 3: []})

    def test_uncommitted_changes_are_not_part_of_the_diff(self):
        repo = Repo(self, {'crates/x/Cargo.toml': 'x = 1\n', 'crates/x/src/lib.rs': 'a\n'}, {'crates/x/src/lib.rs': 'b\n'})
        (repo.root / 'crates/x/Cargo.toml').write_text('x = 1\nbdb = []\n')
        (repo.root / 'crates/x/src/lib.rs').write_text(gate('bdb') + '\n')
        self.assertEqual(fired(repo.checks()), set())

    def test_every_check_that_fires_is_reported(self):
        head = {'crates/oxpinyin-store/src/lib.rs': gate('bdb') + '\n', 'crates/y/Cargo.toml': 'tkrzw = []\n'}
        got = Repo(self, {'a.txt': 'a\n'}, head).checks()
        self.assertEqual(got, {1: ['crates/oxpinyin-store/src/lib.rs'],
                               2: [f'crates/oxpinyin-store/src/lib.rs:1: {gate("bdb")}'],
                               3: ['crates/y/Cargo.toml: +tkrzw = []']})

    def test_repository_configuration_cannot_change_the_answer(self):
        head = {'crates/oxpinyin-store/src/lib.rs': gate('bdb') + '\n', 'crates/y/Cargo.toml': 'tkrzw = []\n'}
        repo = Repo(self, {'a.txt': 'a\n'}, head)
        plain = repo.checks()
        for key, value in (('color.ui', 'always'), ('color.diff', 'always'), ('color.grep', 'always'),
                           ('grep.column', 'true'), ('grep.patternType', 'fixed'), ('diff.noprefix', 'true'),
                           ('diff.mnemonicPrefix', 'true'), ('diff.context', '5'), ('diff.renames', 'copies'),
                           ('core.quotepath', 'false'), ('diff.external', 'false')):
            repo.git('config', key, value)
        self.assertEqual(repo.checks(), plain)

    def test_the_cells_are_the_ones_run_all_runs(self):
        run_all = runpy.run_path(str(SCRIPT.with_name('run-all.py')))
        self.assertEqual(cells_for_diff['CELLS'], run_all['CELLS'])
        self.assertIn(cells_for_diff['REFERENCE_CELL'], cells_for_diff['CELLS'])


class DirectoryTests(unittest.TestCase):
    """The answer does not depend on where in the work tree the script is run."""

    BASE = {'README.md': 'a\n', 'tools/notes.txt': 'a\n', 'crates/oxpinyin-store/src/lib.rs': 'fn a() {}\n',
            'crates/oxpinyin-user/Cargo.toml': '[features]\nold = []\n', 'crates/oxpinyin-user/src/lib.rs': 'fn a() {}\n'}
    DIRECTORIES = ('.', 'tools', 'crates/oxpinyin-store')

    def reports(self, repo):
        """The report printed from each directory; every run must succeed."""
        reports = {}
        for directory in self.DIRECTORIES:
            result = repo.cli('base', cwd=repo.root / directory)
            self.assertEqual(result.returncode, 0, f'from {directory}: {result.stderr}')
            reports[directory] = result.stdout
        return reports

    def assert_same_everywhere(self, reports):
        for directory, report in reports.items():
            with self.subTest(directory=directory):
                self.assertEqual(report, reports['.'])

    def test_a_change_under_the_store_crate_selects_the_same_cells_from_every_directory(self):
        repo = Repo(self, self.BASE, {'crates/oxpinyin-store/src/lib.rs': 'fn changed() {}\n'})
        reports = self.reports(repo)
        self.assert_same_everywhere(reports)
        self.assertIn('Cells: bdb,kc,tkrzw (check 1 fired).', reports['.'])
        self.assertIn('\n  crates/oxpinyin-store/src/lib.rs\n', reports['.'])

    def test_checks_two_and_three_see_changes_outside_the_directory_they_run_in(self):
        cases = (('check 2', {'crates/oxpinyin-user/src/lib.rs': gate('bdb') + '\n'},
                  f'\n  crates/oxpinyin-user/src/lib.rs:1: {gate("bdb")}\n'),
                 ('check 3', {'crates/oxpinyin-user/Cargo.toml': '[features]\nold = []\ntkrzw = []\n'},
                  '\n  crates/oxpinyin-user/Cargo.toml: +tkrzw = []\n'))
        for check, head, evidence in cases:
            with self.subTest(check=check):
                reports = self.reports(Repo(self, self.BASE, head))
                self.assert_same_everywhere(reports)
                self.assertIn(f'Cells: bdb,kc,tkrzw ({check} fired).', reports['.'])
                self.assertIn(evidence, reports['.'])

    def test_outside_a_git_work_tree_it_fails_with_a_clear_message(self):
        scratch = tempfile.TemporaryDirectory(prefix='cells-for-diff-')
        self.addCleanup(scratch.cleanup)
        # Git must not find a repository above the scratch directory.
        env = {**os.environ, 'GIT_CEILING_DIRECTORIES': str(Path(scratch.name).resolve().parent)}
        result = subprocess.run((sys.executable, str(SCRIPT)), cwd=scratch.name, env=env,
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, '')
        self.assertTrue(result.stderr.startswith('FAIL: not inside a git work tree'), result.stderr)
        self.assertEqual(result.stderr.count('\n'), 1, result.stderr)


class CommandTests(unittest.TestCase):
    def test_exit_status_reports_success_not_the_selection(self):
        everything = Repo(self, {'a.txt': 'a\n'}, {'crates/oxpinyin-store/src/lib.rs': 'a\n'})
        nothing = Repo(self, {'a.txt': 'a\n'}, {'a.txt': 'b\n'})
        for repo, cells in ((everything, 'bdb,kc,tkrzw'), (nothing, 'bdb')):
            with self.subTest(cells=cells):
                result = repo.cli('base')
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(result.stdout.splitlines()[1].startswith(f'Cells: {cells} ('), result.stdout)

    def test_the_default_base_is_origin_main(self):
        repo = Repo(self, {'a.txt': 'a\n'}, {'a.txt': 'b\n'})
        repo.git('update-ref', 'refs/remotes/origin/main', 'base')
        result = repo.cli()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(result.stdout.startswith('Diff: origin/main...HEAD ('), result.stdout)

    def test_a_base_that_is_not_a_commit_fails(self):
        repo = Repo(self, {'a.txt': 'a\n'}, {'a.txt': 'b\n'})
        for args in (('no-such-ref',), ('--', '-x')):
            with self.subTest(args=args):
                result = repo.cli(*args)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(result.stdout, '')
                self.assertTrue(result.stderr.startswith('FAIL: '), result.stderr)
                self.assertIn(args[-1], result.stderr)

    def test_the_report_names_cells_checks_paths_and_lines(self):
        head = {'crates/oxpinyin-store/src/lib.rs': gate('bdb') + '\n', 'crates/y/Cargo.toml': 'tkrzw = []\n'}
        result = Repo(self, {'a.txt': 'a\n'}, head).cli('base')
        self.assertEqual(re.sub(r'\b[0-9a-f]{8}\b', 'HASH', result.stdout), '\n'.join([
            'Diff: base...HEAD (merge base HASH, head HASH).',
            'Cells: bdb,kc,tkrzw (checks 1, 2, 3 fired).',
            '',
            'Check 1, a changed path under crates/oxpinyin-store/: fired',
            '  crates/oxpinyin-store/src/lib.rs',
            'Check 2, a feature = "<backend>" line in a changed non-Markdown file: fired',
            f'  crates/oxpinyin-store/src/lib.rs:1: {gate("bdb")}',
            'Check 3, a changed Cargo.toml line that names a backend: fired',
            '  crates/y/Cargo.toml: +tkrzw = []',
            '']))

    def test_the_report_for_a_diff_touching_none_names_the_reference_cell(self):
        result = Repo(self, {'a.txt': 'a\n'}, {'a.txt': 'b\n'}).cli('base')
        self.assertEqual(re.sub(r'\b[0-9a-f]{8}\b', 'HASH', result.stdout), '\n'.join([
            'Diff: base...HEAD (merge base HASH, head HASH).',
            'Cells: bdb (the reference cell; no check fired).',
            '',
            'Check 1, a changed path under crates/oxpinyin-store/: not fired',
            'Check 2, a feature = "<backend>" line in a changed non-Markdown file: not fired',
            'Check 3, a changed Cargo.toml line that names a backend: not fired',
            '']))

    def test_limit_caps_the_matches_shown_per_check(self):
        repo = Repo(self, {'a.txt': 'a\n'}, {f'crates/oxpinyin-store/src/{name}.rs': 'a\n' for name in 'abc'})
        capped = repo.cli('--limit', '2', 'base').stdout
        self.assertIn('  crates/oxpinyin-store/src/b.rs\n  ... and 1 more (--limit 0 lists all)\n', capped)
        self.assertNotIn('c.rs', capped)
        everything = repo.cli('--limit', '0', 'base').stdout
        self.assertIn('  crates/oxpinyin-store/src/c.rs\n', everything)
        self.assertNotIn('more', everything)


if __name__ == '__main__':
    unittest.main()
