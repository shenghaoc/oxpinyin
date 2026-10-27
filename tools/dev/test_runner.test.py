#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Unit tests for the nextest argument mapping."""
from pathlib import Path
import contextlib
import io
import json
import os
import signal
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import host, slot, test_runner, test_support


class HarnessTests(unittest.TestCase):
    def legacy_files(self, root):
        found = subprocess.run(['find', 'tools', '-type', 'f', '-name', '*.test.py'],
                               cwd=root, capture_output=True, check=True)
        return subprocess.run(['sort'], input=found.stdout, env=dict(os.environ, LC_ALL='C'),
                              capture_output=True, check=True).stdout.decode().splitlines()

    def test_discovery_matches_ci_find_and_sort_on_the_checkout(self):
        self.assertEqual(test_runner.harness_files(), self.legacy_files(ROOT))
        self.assertEqual(test_runner.HARNESS_EXCLUDED, {'tools/bisection/internal-ledger.test.py'})

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
        excluded = {'tools/bisection/internal-ledger.test.py': "raise RuntimeError('must be excluded')"}
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
                  '.github/actions/function-order-policy/action.yml',
                  '.github/workflows/oracle-image.yml', '.github/workflows/verify-nightly.yml',
                  'tools/cloud/setup.sh', 'tools/cloud/env.sh', 'tools/cloud/env.test.sh')
        for path in shared:
            with self.subTest(path=path):
                self.assertNotIn(path, test_runner.HARNESS_DEPENDENCIES)
                self.assertEqual(test_runner.changed_harness_files(files, [path]), files)

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
                   'fixtures/datagen-toned/README.md', 'tools/bisection/Dockerfile.callgrind',
                   'tools/bisection/Dockerfile.perf-matrix')
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
                     '.github/actions/function-order-policy/action.yml',
                     'tools/bisection/runners.json', 'tools/dev/pool.py',
                     'tools/dev/bash_floor.py', 'tools/dev/doctor.py',
                     'tools/generate-function-order.py', 'docs/packaging.md',
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
                   'tools/bisection/internal-ledger.test.py': "raise RuntimeError('must be excluded')",
                   'tools/z.test.py': "print('LAST')"}
        status, out, err = self.fake_run(sources, cpus=1)
        self.assertEqual(status, 1)
        self.assertIn('LAST', out)
        self.assertIn('excluded (missing CI input)', out)
        self.assertNotIn('RuntimeError', out)
        self.assertIn('one or more tools/ Python harness tests failed', err)

    def test_memory_backed_tmpdir_refuses_full_and_selected_runs(self):
        for kind in ('tmpfs', 'ramfs'):
            for files in ((), ['tools/a.test.py']):
                with self.subTest(kind=kind, files=files):
                    status, out, err = self.fake_run({'tools/a.test.py': "print('launched')"},
                                                     files, filesystem=kind)
                    self.assertEqual(status, 2)
                    self.assertEqual(out, '')
                    self.assertIn('disk-backed TMPDIR', err)
                    self.assertIn('on ' + kind, err)

    def test_disk_backed_or_unreadable_tmpdir_runs(self):
        for kind in ('ext4', 'overlay', None):
            status, out, err = self.fake_run({'tools/a.test.py': "print('launched')"}, filesystem=kind)
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


class MappingTests(unittest.TestCase):
    def test_include_ignored_maps_to_all(self):
        self.assertEqual(test_runner._nextest_args(['--include-ignored', '-p', 'x']),
                         ['--run-ignored=all', '-p', 'x'])

    def test_ignored_maps_to_only(self):
        self.assertEqual(test_runner._nextest_args(['--ignored']), ['--run-ignored=only'])

    def test_separator_is_removed_once(self):
        self.assertEqual(test_runner._strip_separator(['--', '--workspace']), ['--workspace'])
        self.assertEqual(test_runner._strip_separator(['--workspace']), ['--workspace'])

    def test_unknown_profile_is_rejected(self):
        with mock.patch.dict(os.environ, {'PATH': os.environ['PATH']}, clear=True), \
                mock.patch.object(slot.subprocess, 'Popen') as launch, \
                contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(test_runner.run([], profile='other'), 2)
        test_support.assert_not_called(self, launch)


class AdmissionTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.environ = {'PATH': os.environ['PATH'], 'OXPINYIN_LOCK_DIR': scratch.name,
                        'OXPINYIN_RESOURCE_WAIT_SECONDS': '0'}
        self.resources = dict(available_mib=host.HEADROOM_MIB + 700,
                              cpus=16, cargo_cpus=16, notes=['synthetic resources'])

    def run_nextest(self, args=(), env=None, resources=None):
        child = mock.MagicMock()
        child.__enter__.return_value = child
        child.wait.return_value = 0
        child.poll.return_value = 0
        out = io.StringIO()
        snapshots = ({'side_effect': resources} if isinstance(resources, list)
                     else {'return_value': resources or self.resources})
        with mock.patch.dict(os.environ, dict(self.environ, **(env or {})), clear=True), \
                mock.patch.object(host, 'resource_snapshot', **snapshots), \
                mock.patch.object(slot.subprocess, 'Popen', return_value=child) as launch, \
                contextlib.redirect_stderr(out):
            status = test_runner.run(list(args))
        return status, launch, out.getvalue()

    def test_admitted_count_below_cpus_reaches_nextest(self):
        status, launch, output = self.run_nextest()
        self.assertEqual(status, 0, output)
        self.assertEqual(launch.call_args.kwargs['env']['NEXTEST_TEST_THREADS'], '10')
        self.assertLess(10, self.resources['cpus'])
        self.assertIn('nextest test threads: 10 (admitted)', output)
        self.assertEqual(launch.call_args.args[0][:5],
                         ['cargo', 'nextest', 'run', '--no-fail-fast', '--profile'])

    def test_final_resource_read_controls_the_reported_child_count(self):
        provisional = dict(self.resources, available_mib=host.HEADROOM_MIB + 2000)
        status, launch, output = self.run_nextest(resources=[provisional, dict(self.resources)])
        self.assertEqual(status, 0, output)
        self.assertEqual(launch.call_args.kwargs['env']['NEXTEST_TEST_THREADS'], '10')
        self.assertIn('provisional defaults: NEXTEST_TEST_THREADS=16', output)
        self.assertIn('nextest test threads: 10 (admitted)', output)

    def test_environment_override_is_used_in_admission(self):
        status, launch, output = self.run_nextest(env={'NEXTEST_TEST_THREADS': '3'})
        self.assertEqual(status, 0, output)
        self.assertEqual(launch.call_args.kwargs['env']['NEXTEST_TEST_THREADS'], '3')
        self.assertEqual(launch.call_args.kwargs['env']['RUST_TEST_THREADS'], '3')
        self.assertIn('3 (user-set (NEXTEST_TEST_THREADS))', output)

    def test_cli_aliases_override_the_environment(self):
        for args in (['--test-threads', '2'], ['--test-threads=2'],
                     ['--jobs', '2'], ['--jobs=2'], ['-j', '2'], ['-j2'], ['-j=2']):
            with self.subTest(args=args):
                status, launch, output = self.run_nextest(args, {'NEXTEST_TEST_THREADS': 'invalid'})
                self.assertEqual(status, 0, output)
                self.assertEqual(launch.call_args.kwargs['env']['NEXTEST_TEST_THREADS'], '2')
                self.assertEqual(launch.call_args.args[0][-len(args):], args)
                self.assertIn('2 (user-set (nextest CLI))', output)

    def test_override_that_does_not_fit_never_starts_nextest(self):
        for args, env in ((['--test-threads=20'], {}), ([], {'NEXTEST_TEST_THREADS': '20'})):
            with self.subTest(args=args, env=env):
                status, launch, output = self.run_nextest(args, env)
                self.assertEqual(status, 2)
                test_support.assert_not_called(self, launch)
                self.assertIn('resource-blocked: 1280 MiB needed, 700 MiB available', output)

    def test_ci_profile_keeps_four_in_the_estimate_and_child(self):
        resources = dict(self.resources, available_mib=host.HEADROOM_MIB + 601)
        for args, env in (([], {'NEXTEST_PROFILE': 'ci'}), (['--profile=ci'], {}),
                          (['-P', 'ci'], {}), (['-Pci'], {})):
            with self.subTest(args=args, env=env):
                status, launch, output = self.run_nextest(args, env, resources)
                self.assertEqual(status, 0, output)
                self.assertEqual(launch.call_args.kwargs['env']['NEXTEST_TEST_THREADS'], '4')
                self.assertEqual(launch.call_args.kwargs['env']['RUST_TEST_THREADS'], '4')
                self.assertIn('4 (profile (ci))', output)

    def test_cli_profile_overrides_profile_environment(self):
        status, launch, output = self.run_nextest(['--profile=local'], {'NEXTEST_PROFILE': 'ci'})
        self.assertEqual(status, 0, output)
        self.assertEqual(launch.call_args.kwargs['env']['NEXTEST_TEST_THREADS'], '10')
        self.assertIn('nextest profile: local', output)

    def test_internal_argument_boundary_is_preserved(self):
        args = ['-p', 'crate', '--', 'name with spaces', '--test-threads=99', '--ignored']
        status, launch, output = self.run_nextest(args)
        self.assertEqual(status, 0, output)
        self.assertEqual(launch.call_args.args[0][-len(args):], args)
        self.assertEqual(launch.call_args.kwargs['env']['NEXTEST_TEST_THREADS'], '10')

    def test_uncertain_or_invalid_override_syntax_fails_clearly(self):
        for args in (['--test-threads=0'], ['--test-threads=-2'], ['--test-threads=num-cpus'],
                     ['--test-threads'], ['--test-threads='], ['-vj2'],
                     ['--test-threads=2', '-j3'], ['--tool-config-file=tool:/tmp/other.toml']):
            with self.subTest(args=args):
                status, launch, output = self.run_nextest(args)
                self.assertEqual(status, 2, output)
                test_support.assert_not_called(self, launch)
                self.assertIn('FAIL:', output)

    def test_no_capture_uses_nextest_serial_semantics(self):
        status, launch, output = self.run_nextest(['--test-threads=5', '--no-capture'])
        self.assertEqual(status, 0, output)
        self.assertEqual(launch.call_args.kwargs['env']['NEXTEST_TEST_THREADS'], '1')
        self.assertIn('1 (user-set (no-capture))', output)

    def test_explicit_build_jobs_are_also_budgeted(self):
        status, launch, output = self.run_nextest(['--build-jobs=2'])
        self.assertEqual(status, 2)
        test_support.assert_not_called(self, launch)
        self.assertIn('1200 MiB needed, 700 MiB available', output)

    def test_rust_test_threads_caller_value_is_preserved(self):
        status, launch, output = self.run_nextest(['--test-threads=2'], {'RUST_TEST_THREADS': '3'})
        self.assertEqual(status, 0, output)
        self.assertEqual(launch.call_args.kwargs['env']['RUST_TEST_THREADS'], '3')
        self.assertEqual(launch.call_args.kwargs['env']['NEXTEST_TEST_THREADS'], '2')

    def run_doctests(self, args=(), status=0, resources=None):
        child = mock.MagicMock()
        child.__enter__.return_value = child
        child.wait.return_value = status
        out = io.StringIO()
        with mock.patch.dict(os.environ, self.environ, clear=True), \
                mock.patch.object(host, 'resource_snapshot', return_value=resources or self.resources), \
                mock.patch.object(slot.subprocess, 'Popen', return_value=child) as launch, \
                contextlib.redirect_stderr(out):
            result = test_runner.run_doc(list(args))
        return result, launch, out.getvalue()

    def test_doctests_run_cargo_through_run_command(self):
        # No just: run_command admits here and supervises cargo in its own session.
        args = ['-p', 'oxpinyin-core', '--', 'name with spaces']
        for status, expected in ((0, 0), (101, 101), (-15, 143)):
            with self.subTest(status=status):
                result, launch, output = self.run_doctests(['--', *args], status)
                self.assertEqual(result, expected, output)
                self.assertEqual(launch.call_count, 1)
                self.assertEqual(launch.call_args.args[0], ['cargo', 'test', '--doc', *args])
                options = launch.call_args.kwargs
                marker = json.loads(options['env'][slot.HEAVY_SLOT_HELD])
                self.assertEqual(marker['pid'], os.getpid())
                self.assertEqual(options['pass_fds'], (marker['fd'],))
                self.assertTrue(options['start_new_session'])
                self.assertEqual((options['cwd'], options['stdout']), (None, None))
                self.assertIn('resources (admitted)', output)

    def test_windows_doctests_take_the_job_object_path_without_pass_fds(self):
        with mock.patch.dict(os.environ, self.environ, clear=True), \
                test_support.windows_launch(status=3) as (launch, jobs, ntdll), \
                contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(test_runner.run_doc(['-p', 'oxpinyin-core']), 3)
        test_support.assert_windows_launches(self, launch, jobs, ntdll)
        self.assertEqual(launch.call_args.args[0], ['cargo', 'test', '--doc', '-p', 'oxpinyin-core'])

    def test_a_cancelled_doctest_run_exits_with_128_plus_the_signal(self):
        with mock.patch.object(slot, 'run_command', side_effect=slot.Cancelled('cancelled')), \
                mock.patch.object(slot, 'CANCELLED_SIGNAL', signal.SIGTERM), \
                contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(test_runner.run_doc([]), 128 + signal.SIGTERM)

    def test_doctests_never_start_without_admission(self):
        resources = dict(self.resources, available_mib=host.HEADROOM_MIB + 100)
        result, launch, output = self.run_doctests(resources=resources)
        self.assertEqual(result, 2)
        test_support.assert_not_called(self, launch)
        self.assertIn('FAIL: resource-blocked', output)


if __name__ == '__main__':
    unittest.main()
