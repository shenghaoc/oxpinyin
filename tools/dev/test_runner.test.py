#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Unit tests for the nextest argument mapping."""
from pathlib import Path
import contextlib
import io
import os
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import host, slot, test_runner


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

    def test_runner_keeps_commands_exclusion_order_and_runs_after_failures(self):
        files = ['tools/a.test.py', 'tools/bisection/internal-ledger.test.py', 'tools/z.test.py']
        environ = {'PATH': os.environ['PATH'], 'OXPINYIN_TEST_KEPT': 'kept', 'OXPINYIN_TEST_TOKEN': 'withheld'}
        for results, expected in (([0, 0], 0), ([1, 0], 1), ([0, -15], 1),
                                  ([OSError('missing uv'), 0], 1)):
            with self.subTest(results=results):
                replies = [value if isinstance(value, OSError) else subprocess.CompletedProcess([], value)
                           for value in results]
                out, err = io.StringIO(), io.StringIO()
                with mock.patch.dict(os.environ, environ, clear=True), \
                        mock.patch.object(test_runner, 'harness_files', return_value=files), \
                        mock.patch.object(test_runner.subprocess, 'run', side_effect=replies) as run, \
                        contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                    self.assertEqual(test_runner.run_tools(), expected)
                prefix = ['uv', 'run', '--no-project', '--managed-python', '--python',
                          'cpython@3.15+gil', 'python', '-X', 'lazy_imports=normal']
                child = {'PATH': environ['PATH'], 'OXPINYIN_TEST_KEPT': 'kept'}
                self.assertEqual(run.call_args_list, [mock.call(prefix + [path], cwd=ROOT, env=child)
                                                      for path in (files[0], files[2])])
                self.assertEqual(out.getvalue().splitlines(), [
                    'tools harness: credential-shaped environment variables removed: 1',
                    'tools harness: tools/a.test.py',
                    'tools harness excluded (missing CI input): tools/bisection/internal-ledger.test.py',
                    'tools harness: tools/z.test.py'])
                self.assertEqual('::error::one or more tools/ Python harness tests failed' in err.getvalue(),
                                 bool(expected))

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
        launch.assert_not_called()


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
                launch.assert_not_called()
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
                launch.assert_not_called()
                self.assertIn('FAIL:', output)

    def test_no_capture_uses_nextest_serial_semantics(self):
        status, launch, output = self.run_nextest(['--test-threads=5', '--no-capture'])
        self.assertEqual(status, 0, output)
        self.assertEqual(launch.call_args.kwargs['env']['NEXTEST_TEST_THREADS'], '1')
        self.assertIn('1 (user-set (no-capture))', output)

    def test_explicit_build_jobs_are_also_budgeted(self):
        status, launch, output = self.run_nextest(['--build-jobs=2'])
        self.assertEqual(status, 2)
        launch.assert_not_called()
        self.assertIn('1200 MiB needed, 700 MiB available', output)

    def test_rust_test_threads_caller_value_is_preserved(self):
        status, launch, output = self.run_nextest(['--test-threads=2'], {'RUST_TEST_THREADS': '3'})
        self.assertEqual(status, 0, output)
        self.assertEqual(launch.call_args.kwargs['env']['RUST_TEST_THREADS'], '3')
        self.assertEqual(launch.call_args.kwargs['env']['NEXTEST_TEST_THREADS'], '2')


if __name__ == '__main__':
    unittest.main()
