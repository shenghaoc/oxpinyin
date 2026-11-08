#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Unit tests for the nextest argument mapping."""
from pathlib import Path
import contextlib
import io
import os
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import test_runner


class HarnessTests(unittest.TestCase):
    def legacy_files(self, root):
        found = subprocess.run(['find', 'tools', '-type', 'f', '-name', '*.test.py'],
                               cwd=root, capture_output=True, check=True)
        return subprocess.run(['sort'], input=found.stdout, env=dict(os.environ, LC_ALL='C'),
                              capture_output=True, check=True).stdout.decode().splitlines()

    def test_discovery_matches_ci_find_and_sort_on_the_checkout(self):
        self.assertEqual(test_runner.harness_files(), self.legacy_files(ROOT))
        self.assertEqual(test_runner.HARNESS_EXCLUDED, {'tools/bisection/internal-ledger-toolchain.test.py'})

    def test_discovery_includes_hidden_files_but_not_symlinks_or_directories(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name in ('z.test.py', 'A.test.py', '.hidden.test.py', 'a space.test.py',
                         'nested/é.test.py', 'nested/a.test.py', 'other.py'):
                path = root / 'tools' / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.touch()
            (root / 'tools/directory.test.py').mkdir()
            (root / 'tools/link.test.py').symlink_to('A.test.py')
            (root / 'tools/link-dir').symlink_to('nested', target_is_directory=True)
            with mock.patch.object(test_runner, 'ROOT', root):
                found = test_runner.harness_files()
            self.assertEqual(found, self.legacy_files(root))
            self.assertEqual(len(found), 6)

    def fake_run(self, sources, files=(), pattern=None, changed=None, filesystem='ext4', cpus=2):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name, source in sources.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source)
            out, err = io.StringIO(), io.StringIO()
            with mock.patch.object(test_runner, 'ROOT', root), \
                    mock.patch.object(test_runner.host, 'allowed_cpus', return_value=set(range(cpus))), \
                    mock.patch.object(test_runner.mounts, 'filesystem', return_value=filesystem), \
                    contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                status = test_runner.run_tools(files, pattern, changed)
            return status, out.getvalue(), err.getvalue()

    def test_selection_deduplicates_and_rejects_unknown(self):
        sources = {'tools/a.test.py': "print('A')", 'tools/z.test.py': "print('Z')"}
        status, out, _ = self.fake_run(sources, ['tools/z.test.py', './tools/z.test.py'])
        self.assertEqual(status, 0)
        self.assertNotIn('tools/a.test.py', out)
        self.assertEqual(out.count('tools/z.test.py'), 1)
        for name in ('tools/missing.test.py', '../outside.test.py'):
            status, out, err = self.fake_run(sources, [name])
            self.assertEqual(status, 2)
            self.assertEqual(out, '')
            self.assertIn('not a discovered harness file', err)

    def test_directory_errors_report_failure_and_run_later_files(self):
        for phase in ('create', 'tmp', 'locks', 'cleanup'):
            with self.subTest(phase=phase):
                calls = []

                @contextlib.contextmanager
                def temporary_directory(**kwargs):
                    calls.append(phase)
                    fail = len(calls) == 1
                    if fail and phase == 'create':
                        raise OSError('directory creation refused')
                    with tempfile.TemporaryDirectory(**kwargs) as directory:
                        if fail and phase in ('tmp', 'locks'):
                            # A real mkdir collision exercises each setup failure.
                            (Path(directory) / phase).mkdir()
                        yield directory
                        if fail and phase == 'cleanup':
                            raise OSError('directory cleanup refused')

                temporary = SimpleNamespace(gettempdir=tempfile.gettempdir,
                                            TemporaryDirectory=temporary_directory)
                with mock.patch.object(test_runner, 'tempfile', temporary):
                    status, out, err = self.fake_run({'tools/a.test.py': "print('FIRST')",
                                                     'tools/z.test.py': "print('LAST')"}, cpus=1)
                self.assertEqual(status, 1)
                self.assertEqual(len(calls), 2)
                self.assertIn('tools harness: tools/a.test.py', out)
                self.assertIn('tools harness: tools/z.test.py\nLAST', out)
                self.assertIn('File exists' if phase in ('tmp', 'locks') else 'refused', out)
                self.assertIn('one or more tools/ Python harness tests failed', err)

    def test_invalid_changed_base_prints_git_stderr_after_summary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(['git', 'init', '-q', str(root)], check=True, capture_output=True)
            out, err = io.StringIO(), io.StringIO()
            with mock.patch.object(test_runner, 'ROOT', root), \
                    mock.patch.object(test_runner.mounts, 'filesystem', return_value='ext4'), \
                    mock.patch.dict(os.environ, LC_ALL='C'), \
                    contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                status = test_runner.run_tools(changed='not-a-revision')
        self.assertEqual(status, 2)
        self.assertEqual(out.getvalue(), '')
        self.assertTrue(err.getvalue().startswith('just test-tools: '))
        self.assertIn("\nfatal: bad revision 'not-a-revision'", err.getvalue())

    def test_k_selects_a_test_within_the_selected_file(self):
        source = """import unittest
class Tests(unittest.TestCase):
    def test_chosen(self): print('CHOSEN')
    def test_other(self): self.fail('not selected')
if __name__ == '__main__': unittest.main()
"""
        status, out, _ = self.fake_run({'tools/a.test.py': source}, ['tools/a.test.py'], 'chosen')
        self.assertEqual(status, 0, out)
        self.assertIn('CHOSEN', out)
        self.assertIn('Ran 1 test', out)

    def test_k_without_files_reports_nonmatching_files_as_unselected(self):
        source = """import unittest
class Tests(unittest.TestCase):
    def test_chosen(self): print('CHOSEN')
if __name__ == '__main__': unittest.main()
"""
        sources = {'tools/a.test.py': source,
                   'tools/z.test.py': source.replace('test_chosen', 'test_other')}
        status, out, err = self.fake_run(sources, pattern='chosen')
        self.assertEqual(status, 0, out + err)
        self.assertIn('Ran 1 test', out)
        self.assertIn('CHOSEN', out)
        self.assertIn('Ran 0 tests', out)
        self.assertIn("unselected (no tests match -k 'chosen'): tools/z.test.py", out)
        self.assertNotIn('one or more tools/ Python harness tests failed', err)
        self.assertLess(out.index('tools harness: tools/z.test.py'), out.index('unselected'))

    def test_k_without_any_matches_is_a_failure(self):
        source = """import unittest
class Tests(unittest.TestCase):
    def test_other(self): self.fail('must not run')
if __name__ == '__main__': unittest.main()
"""
        excluded = {'tools/bisection/internal-ledger-toolchain.test.py': "raise RuntimeError('must be excluded')"}
        for sources, unmatched in ((dict(excluded, **{'tools/a.test.py': source,
                                                     'tools/z.test.py': source}), 2),
                                   (excluded, 0), ({}, 0)):
            with self.subTest(files=list(sources)):
                status, out, err = self.fake_run(sources, pattern='typo')
                self.assertEqual(status, 1)
                self.assertEqual(out.count('unselected (no tests match'), unmatched)
                self.assertIn("no tests match -k 'typo' in any selected file", err)
                self.assertNotIn('RuntimeError', out)

    def test_k_named_file_without_matches_is_a_failure(self):
        source = """import unittest
class Tests(unittest.TestCase):
    def test_other(self): self.fail('must not run')
if __name__ == '__main__': unittest.main()
"""
        status, out, err = self.fake_run({'tools/a.test.py': source}, ['tools/a.test.py'], 'chosen')
        self.assertEqual(status, 1)
        self.assertIn('Ran 0 tests', out)
        self.assertNotIn('unselected', out)
        self.assertIn('one or more tools/ Python harness tests failed', err)

    def test_no_tests_without_pattern_is_a_failure(self):
        status, out, err = self.fake_run({'tools/a.test.py': 'import unittest; unittest.main()'})
        self.assertEqual(status, 1)
        self.assertIn('Ran 0 tests', out)
        self.assertNotIn('unselected', out)
        self.assertIn('one or more tools/ Python harness tests failed', err)

    def test_selected_and_changed_children_filter_credentials(self):
        sources = {'tools/a.test.py': "import os; assert 'OXPINYIN_TEST_TOKEN' not in os.environ; print('safe')"}
        with mock.patch.dict(os.environ, OXPINYIN_TEST_TOKEN='test-only-sentinel'):
            for files, changed in (((), None), (['tools/a.test.py'], None), ((), 'HEAD')):
                with mock.patch.object(test_runner, 'changed_paths', return_value=['tools/a.test.py']):
                    status, out, err = self.fake_run(sources, files, changed=changed)
                self.assertEqual(status, 0, err)
                self.assertIn('safe', out)
                self.assertNotIn('test-only-sentinel', out + err)

    def test_conflicting_selection_and_invalid_changed_base_refuse(self):
        sources = {'tools/a.test.py': "print('launched')"}
        status, out, err = self.fake_run(sources, ['tools/a.test.py'], changed='HEAD')
        self.assertEqual(status, 2)
        self.assertEqual(out, '')
        self.assertIn('cannot be combined', err)
        with mock.patch.object(test_runner, 'changed_paths', side_effect=subprocess.CalledProcessError(128, 'git')):
            status, out, err = self.fake_run(sources, changed='unknown')
        self.assertEqual(status, 2)
        self.assertEqual(out, '')

    def test_pool_worker_count_is_bounded_by_allowed_cpus_and_files(self):
        for cpus, expected in ((1, 1), (2, 2), (20, 2)):
            with mock.patch.object(test_runner.pool, 'run_units', wraps=test_runner.pool.run_units) as run:
                status, out, err = self.fake_run({'tools/a.test.py': 'pass', 'tools/b.test.py': 'pass'}, cpus=cpus)
            self.assertEqual(status, 0, out + err)
            self.assertEqual(run.call_args.args[1], expected)

    def test_changed_mapping_and_unknown_fallback(self):
        sources = {name: "print('ran')" for name in
                   ('tools/dev/test_runner.test.py', 'tools/dev/cli.test.py',
                    'tools/dev/sanitize.test.py', 'tools/other.test.py')}
        for paths, expected in ((['tools/dev/test_runner.py'], 3),
                                (['tools/other.test.py'], 1),
                                (['tools/unknown.py'], 4),
                                (['tools/dev/test_runner.py', 'justfile'], 4), ([], 0)):
            with self.subTest(paths=paths), \
                    mock.patch.object(test_runner, 'changed_paths', return_value=paths):
                status, out, _ = self.fake_run(sources, changed='HEAD')
                self.assertEqual(status, 0, out)
                self.assertEqual(out.splitlines().count('ran'), expected)
        self.assertEqual(test_runner.changed_harness_files(['tools/other.test.py'],
                                                         ['tools/dev/test_runner.py']),
                         ['tools/other.test.py'])

    def test_shared_files_including_justfile_select_every_harness(self):
        files = test_runner.harness_files()
        self.assertIn('tools/dev/python-request.test.py', files)
        self.assertGreater(len(files), len(test_runner.COMPAT_HARNESS))
        shared = ('justfile', 'tools/bisection/runners.json', 'tools/dev/bash_floor.py',
                  '.github/actions/example/action.yml',
                  '.github/workflows/oracle-image.yml', '.github/workflows/verify-nightly.yml',
                  'docs/plans/tooling-shell-audit.md')
        for path in shared:
            with self.subTest(path=path):
                self.assertNotIn(path, test_runner.HARNESS_DEPENDENCIES)
                self.assertEqual(test_runner.changed_harness_files(files, [path]), files)

    def test_contract_families_keep_own_tests_integration_and_policy_edges(self):
        files = test_runner.harness_files()
        shared = {'tools/bisection/contract-modules.test.py', 'tools/bisection/contract-diff.test.py',
                  'tools/bisection/teardown.test.py'}
        for name in ('abi', 'reading_256th', 'runner'):
            with self.subTest(module=name):
                expected = shared | {f'tools/bisection/contract_{name}.test.py'}
                self.assertEqual(test_runner.changed_harness_files(files, [f'tools/bisection/contract_{name}.py']),
                                 [path for path in files if path in expected])
        policy = {'tools/bisection/contract_policy.test.py', 'tools/bisection/contract_verdict.test.py',
                  'tools/bisection/contract-modules.test.py', 'tools/bisection/contract-diff.test.py',
                  'tools/dev/policy.test.py'}
        self.assertEqual(test_runner.changed_harness_files(files, ['tools/bisection/contract_policy.py']),
                         [path for path in files if path in policy])
        self.assertEqual(test_runner.changed_harness_files(files, ['tools/bisection/contract_equivalence.py']),
                         files)

    def test_provision_owned_file_selects_its_readers_and_justfile_selects_all(self):
        files = test_runner.harness_files()
        selected = test_runner.changed_harness_files(files, ['tools/dev/provision.py'])
        self.assertEqual(selected, [name for name in files if name in test_runner.PROVISION_HARNESS])
        self.assertEqual(test_runner.changed_harness_files(files, ['justfile']), files)

    def test_tooling_shell_audit_and_other_audited_text_select_their_reader(self):
        # python-request.test.py reads every tracked text file outside its
        # explicit allowlist, not just paths named literally in its source.
        files = test_runner.harness_files()
        reader = 'tools/dev/python-request.test.py'
        self.assertIn(reader, files)
        audited = ('ROADMAP.md', 'docs/plans/tooling-shell-audit.md',
                   'docs/plans/tooling-python.md', 'docs/plans/tooling-just.md',
                   'docs/runbooks/benches.md', 'docs/runbooks/cloud.md',
                   'docs/runbooks/oracle.md', 'docs/testing/README.md',
                   'docs/testing/capture-fixtures.md', 'docs/testing/oracle-environment.md',
                   'fixtures/datagen-toned/README.md')
        for path in audited:
            with self.subTest(path=path):
                self.assertNotIn(path, test_runner.HARNESS_DEPENDENCIES)
                self.assertIn(reader, test_runner.changed_harness_files(files, [path]))

    def test_shared_packaging_inputs_select_all_discovered_harnesses(self):
        files = test_runner.harness_files()
        self.assertIn('tools/dev/doctor.test.py',
                      test_runner.changed_harness_files(files, ['tools/dev/doctor.py']))
        for path in ('justfile', '.github/workflows/ci.yml',
                     '.github/workflows/release-packages.yml',
                     '.github/actions/example/action.yml',
                     'tools/bisection/runners.json', 'tools/dev/pool.py',
                     'tools/dev/bash_floor.py', 'tools/dev/doctor.py',
                     'tools/example.py', 'docs/packaging.md',
                     'docs/plans/tooling-shell-audit.md', 'docs/plans/tooling-python.md',
                     'docs/runbooks/benches.md', 'docs/runbooks/goldens-and-pins.md',
                     'docs/runbooks/release.md', 'docs/testing/oracle-environment.md'):
            with self.subTest(path=path):
                self.assertEqual(test_runner.changed_harness_files(files, [path]), files)

    def test_changed_paths_include_all_git_states(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            def git(*args):
                return subprocess.run(['git', *args], cwd=root, check=True, capture_output=True,
                                      env=dict(os.environ, GIT_AUTHOR_NAME='Harness',
                                               GIT_AUTHOR_EMAIL='harness@example.invalid',
                                               GIT_COMMITTER_NAME='Harness',
                                               GIT_COMMITTER_EMAIL='harness@example.invalid'))
            git('init', '-q')
            for name in ('committed', 'staged', 'unstaged', 'deleted'):
                (root / name).write_text('base')
            git('add', '.')
            git('-c', 'commit.gpgsign=false', 'commit', '-qm', 'base')
            base = git('rev-parse', 'HEAD').stdout.decode().strip()
            (root / 'committed').write_text('changed')
            git('add', 'committed')
            git('-c', 'commit.gpgsign=false', 'commit', '-qm', 'change')
            (root / 'staged').write_text('changed')
            git('add', 'staged')
            (root / 'unstaged').write_text('changed')
            (root / 'untracked').touch()
            (root / 'deleted').unlink()
            with mock.patch.object(test_runner, 'ROOT', root):
                self.assertEqual(test_runner.changed_paths(base),
                                 ['committed', 'deleted', 'staged', 'unstaged', 'untracked'])
                with self.assertRaises(subprocess.CalledProcessError):
                    test_runner.changed_paths('not-a-revision')

    def test_parallel_isolation_and_contiguous_path_order(self):
        # Real children rendezvous to prove overlap; the deadline bounds a broken scheduler.
        source = """import json, os, pathlib, sys, time
root = pathlib.Path(__file__).parent
name = pathlib.Path(__file__).name
(root / (name + '.ready')).touch()
deadline = time.monotonic() + 10
while len(list(root.glob('*.ready'))) < 2:
    if time.monotonic() > deadline: raise RuntimeError('files did not overlap')
    time.sleep(.005)
print('BEGIN ' + name, flush=True)
print(json.dumps([os.environ['TMPDIR'], os.environ['OXPINYIN_LOCK_DIR']]))
assert pathlib.Path(os.environ['TMPDIR']).is_dir()
assert pathlib.Path(os.environ['OXPINYIN_LOCK_DIR']).is_dir()
print('END ' + name, file=sys.stderr, flush=True)
"""
        status, out, err = self.fake_run({'tools/z.test.py': source, 'tools/a.test.py': source})
        self.assertEqual(status, 0, out + err)
        lines = out.splitlines()[1:]
        self.assertEqual([lines[i] for i in (0, 1, 3, 4, 5, 7)],
                         ['tools harness: tools/a.test.py', 'BEGIN a.test.py', 'END a.test.py',
                          'tools harness: tools/z.test.py', 'BEGIN z.test.py', 'END z.test.py'])
        import json
        directories = json.loads(lines[2]) + json.loads(lines[6])
        self.assertEqual(len(set(directories)), 4)
        self.assertTrue(all(not Path(path).exists() for path in directories))

    def test_failure_runs_later_files_and_preserves_exclusion(self):
        sources = {'tools/a.test.py': 'raise SystemExit(7)',
                   'tools/bisection/internal-ledger-toolchain.test.py': "raise RuntimeError('must be excluded')",
                   'tools/z.test.py': "print('LAST')"}
        status, out, err = self.fake_run(sources, cpus=1)
        self.assertEqual(status, 1)
        self.assertIn('LAST', out)
        self.assertIn('excluded (missing CI input)', out)
        self.assertNotIn('RuntimeError', out)
        self.assertIn('one or more tools/ Python harness tests failed', err)

    def test_memory_backed_tmpdir_refuses_only_selections_needing_eviction(self):
        cold = 'tools/bisection/teardown.test.py'
        sources = {cold: "print('cold')", 'tools/a.test.py': "print('launched')"}
        for kind in ('tmpfs', 'ramfs'):
            for files in ((), [cold], [cold, 'tools/a.test.py']):
                with self.subTest(kind=kind, files=files):
                    status, out, err = self.fake_run(sources, files, filesystem=kind)
                    self.assertEqual(status, 2)
                    self.assertEqual(out, '')
                    self.assertIn('disk-backed TMPDIR', err)
                    self.assertIn('on ' + kind, err)
            with mock.patch.object(test_runner, 'memory_backed_tmpdir') as probe:
                status, out, err = self.fake_run(sources, ['tools/a.test.py'], filesystem=kind)
            self.assertEqual(status, 0, err)
            self.assertIn('launched', out)
            probe.assert_not_called()

    def test_changed_selection_precedes_the_tmpdir_requirement(self):
        cold = 'tools/bisection/teardown.test.py'
        sources = {cold: "print('cold')", 'tools/dev/check.test.py': "print('launched')"}
        for paths, expected in ((['tools/dev/check.test.py'], 0), ([cold], 2), (['unknown'], 2), ([], 0)):
            with self.subTest(paths=paths), \
                    mock.patch.object(test_runner, 'changed_paths', return_value=paths):
                status, out, err = self.fake_run(sources, changed='HEAD', filesystem='tmpfs')
            self.assertEqual(status, expected, out + err)
            self.assertEqual('disk-backed TMPDIR' in err, expected == 2)
        status, out, err = self.fake_run(sources, ['tools/missing.test.py'], filesystem='tmpfs')
        self.assertEqual((status, out), (2, ''))
        self.assertIn('not a discovered harness file', err)

    def test_disk_backed_or_unreadable_tmpdir_runs(self):
        for kind in ('ext4', 'overlay', None):
            status, out, err = self.fake_run({'tools/bisection/teardown.test.py': "print('launched')"},
                                             filesystem=kind)
            self.assertEqual(status, 0, err)
            self.assertIn('launched', out)
        for error in (OSError('unreadable'), ValueError('unknown')):
            with mock.patch.object(test_runner.mounts, 'filesystem', side_effect=error):
                self.assertIsNone(test_runner.memory_backed_tmpdir())

    def test_credential_patterns_match_names_case_insensitively(self):
        removed = ['DEPLOY_KEY', 'npm_token', 'Client_Secret', 'MYSECRETS', 'DB_PASSWORD_FILE', 'GH_PAT',
                   'AWS_CREDENTIALS_FILE', 'GITHUB_TOKEN', 'GH_TOKEN', 'ANTHROPIC_AUTH_TOKEN', 'ANTHROPIC_API_KEY',
                   'AWS_ACCESS_KEY_ID', 'AWS_SECRET_ACCESS_KEY', 'DOCKER_AUTH_CONFIG', 'NPM_CONFIG__AUTH',
                   'npm_config__auth', 'DEEPSEEK_APIKEY', 'SSH_AUTH_SOCK', 'DB_PASSWD', 'OAUTH_CONFIG',
                   'NETRC', 'BUILD_NETRC']
        kept = ['PATH', 'HOME', 'TMPDIR', 'LANG', 'CARGO_HOME', 'RUSTUP_TOOLCHAIN', 'GIT_AUTHOR_NAME',
                'GIT_AUTHOR_EMAIL', 'GIT_COMMITTER_NAME', 'GIT_COMMITTER_EMAIL', 'KEY', 'PAT', 'PATTERN',
                'KEYCHAIN', 'TOKENIZERS', 'OXPINYIN_LOCK_DIR']
        env, count = test_runner.harness_environment({name: 'value' for name in removed + kept})
        self.assertEqual(sorted(env), sorted(kept))
        self.assertEqual(count, len(removed))

    def test_a_harness_child_neither_receives_nor_prints_a_credential(self):
        # The probe harness prints the sentinel if the runner lets it through.
        sentinel = 'credential-sentinel-3b9e'
        probe = ("import os\n"
                 "print(os.environ.get('OXPINYIN_TEST_TOKEN'), os.environ.get('OXPINYIN_TEST_KEPT'))\n")
        runner = ('import sys\nfrom pathlib import Path\nsys.path.insert(0, sys.argv[2])\n'
                  'from tools.dev import test_runner\ntest_runner.ROOT = Path(sys.argv[1])\n'
                  'test_runner.mounts.filesystem = lambda path: "ext4"\n'
                  'raise SystemExit(test_runner.run_tools())\n')
        environ, _ = test_runner.harness_environment(os.environ)
        with tempfile.TemporaryDirectory() as temp:
            (Path(temp) / 'tools').mkdir()
            (Path(temp) / 'tools/probe.test.py').write_text(probe)
            result = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c', runner, temp, str(ROOT)],
                                    env=dict(environ, OXPINYIN_TEST_TOKEN=sentinel, OXPINYIN_TEST_KEPT='kept'),
                                    capture_output=True, text=True, timeout=60)
        # Not assertNotIn(sentinel, output): a failure would print the output.
        self.assertFalse(sentinel in result.stdout + result.stderr, 'the harness output carries the sentinel')
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout.splitlines(), [
            'tools harness: credential-shaped environment variables removed: 1',
            'tools harness: tools/probe.test.py',
            'None kept'])


if __name__ == '__main__':
    unittest.main()
