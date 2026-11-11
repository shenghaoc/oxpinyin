#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""The tkrzw build recipe: the source check, the per-host install fixes and the
install check that every build must pass before it is marked complete."""
import contextlib
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.tkrzw import build


def resolved_to(library, status=0):
    """A fake runtime.resolve that reports LIBRARY for any object."""
    def resolve(obj, env=None):
        return 'Linux', subprocess.CompletedProcess([], status, ''), [str(library)]
    return resolve


class FetchTests(unittest.TestCase):
    def fake_git(self, head, failures=0):
        calls = []

        def runner(command, *, cwd, env, capture=False):
            calls.append(command[1])
            if command[1] == 'fetch' and calls.count('fetch') <= failures:
                raise build.RecipeError('git exited with status 128')
            return head + '\n' if capture else None
        return runner, calls

    def test_the_checkout_must_be_the_provisioning_revision(self):
        runner, _ = self.fake_git('0' * 40)
        with contextlib.redirect_stdout(io.StringIO()), self.assertRaisesRegex(build.RecipeError, 'refusing to build'):
            build.fetch(Path('.'), {}, runner=runner)
        runner, calls = self.fake_git(build.TKRZW_REV)
        with contextlib.redirect_stdout(io.StringIO()):
            build.fetch(Path('.'), {}, runner=runner)
        self.assertEqual(calls, ['init', 'fetch', '-c', 'rev-parse'])

    def test_fetch_failures_are_retried_then_reported(self):
        runner, calls = self.fake_git(build.TKRZW_REV, failures=2)
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            build.fetch(Path('.'), {}, runner=runner, sleep=lambda delay: None)
        self.assertEqual(calls.count('fetch'), 3)
        runner, calls = self.fake_git(build.TKRZW_REV, failures=99)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(build.RecipeError):
            build.fetch(Path('.'), {}, runner=runner, delays=(1, 1), sleep=lambda delay: None)
        self.assertEqual(calls.count('fetch'), 3)


class InstallTests(unittest.TestCase):
    def test_make_takes_the_admitted_cargo_budget(self):
        cases = [({}, 8), ({'CARGO_BUILD_JOBS': 'default'}, 8), ({'CARGO_BUILD_JOBS': '3'}, 3),
                 ({'CARGO_BUILD_JOBS': '-2'}, 6), ({'CARGO_BUILD_JOBS': '-20'}, 1)]
        for env, jobs in cases:
            with self.subTest(env=env):
                self.assertEqual(build.make_jobs(env, cpus=8), jobs)
        for setting in ('0', 'many'):
            with self.subTest(setting=setting), self.assertRaisesRegex(build.RecipeError, 'CARGO_BUILD_JOBS'):
                build.make_jobs({'CARGO_BUILD_JOBS': setting}, cpus=8)


class InstallCheckTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.work = Path(temporary.name).resolve()
        self.prefix = self.work / 'tkrzw'
        for name in ('include/tkrzw_dbm.h', 'lib/libtkrzw.so.1', 'lib/pkgconfig/tkrzw.pc', 'bin/tkrzw_dbm_util'):
            (self.prefix / name).parent.mkdir(parents=True, exist_ok=True)
            (self.prefix / name).write_bytes(b'x')
        self.env = {'PATH': os.environ['PATH']}

    def check(self, *, library=None, system='Linux'):
        with contextlib.redirect_stdout(io.StringIO()) as output:
            build.check_install(self.prefix, self.env, system=system,
                                resolve=resolved_to(library or self.prefix / 'lib/libtkrzw.so.1'))
        return output.getvalue()

    def test_a_build_that_loads_its_own_library_passes(self):
        self.assertIn('tkrzw_dbm_util loads', self.check())

    def test_a_utility_that_loads_another_library_is_refused(self):
        other = self.work / 'other.so.1'
        other.write_bytes(b'other')
        with self.assertRaisesRegex(build.RecipeError, 'resolves libtkrzw to'):
            self.check(library=other)

    def test_an_incomplete_install_is_refused(self):
        (self.prefix / 'lib/pkgconfig/tkrzw.pc').unlink()
        with self.assertRaisesRegex(build.RecipeError, 'lacks .*tkrzw.pc'):
            self.check()

    def test_only_a_marked_prefix_counts_as_built(self):
        self.assertFalse(build.built(self.prefix))
        (self.prefix / build.MARKER).write_text('0' * 40 + '\n')
        self.assertFalse(build.built(self.prefix))
        (self.prefix / build.MARKER).write_text(build.TKRZW_REV + '\n')
        self.assertTrue(build.built(self.prefix))

    def test_build_refuses_a_non_empty_prefix(self):
        with mock.patch.object(build, 'host', return_value='Linux'), \
                self.assertRaisesRegex(build.RecipeError, 'is not empty'):
            build.build(self.prefix, env=self.env)


class CheckCommandTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.prefix = Path(temporary.name).resolve() / 'tkrzw'
        self.prefix.mkdir()
        (self.prefix / build.MARKER).write_text(build.TKRZW_REV + '\n')

    def test_check_accepts_a_marked_install_and_refuses_the_rest(self):
        with mock.patch.object(build, 'check_install') as install:
            self.assertEqual(build.main(['check', '--prefix', str(self.prefix)]), 0)
        install.assert_called_once_with(self.prefix)
        with mock.patch.object(build, 'check_install', side_effect=build.RecipeError('the install is incomplete')), \
                contextlib.redirect_stderr(io.StringIO()) as error:
            self.assertEqual(build.main(['check', '--prefix', str(self.prefix)]), 1)
        self.assertIn('FAIL: the install is incomplete', error.getvalue())
        (self.prefix / build.MARKER).unlink()
        with mock.patch.object(build, 'check_install') as install, \
                contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(build.main(['check', '--prefix', str(self.prefix)]), 1)
        install.assert_not_called()


if __name__ == '__main__':
    unittest.main()
