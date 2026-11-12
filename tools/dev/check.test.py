#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Check stdout, statuses and launch failures without Rust builds."""
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import cell, check, native


class CheckFixture(unittest.TestCase):
    """A scratch directory first on PATH, no oracle variables, the declared cell."""

    def setUp(self):
        cell.declared_default()
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        env = dict(os.environ, PATH=self.temp.name + os.pathsep + os.environ['PATH'],
                   CARGO_TARGET_DIR='fixture-target', FIXTURE_CARGO_STATUS='0')
        for name in ('PINYIN_MODEL_DIR', 'PINYIN_ORACLE_PREFIX', 'SYSTEM_DEPS_BDB_INCLUDE'):
            env.pop(name, None)
        patch = mock.patch.dict(os.environ, env, clear=True)
        patch.start()
        self.addCleanup(patch.stop)


class CheckTests(CheckFixture):
    def test_unknown_backend_is_usage_and_missing_tool_is_status_one(self):
        out, err = io.StringIO(), io.StringIO()
        self.assertEqual(check.run('bogus', out=out, err=err), 2)
        self.assertEqual(out.getvalue(), '')
        self.assertIn('usage:', err.getvalue())
        os.environ['PATH'] = self.temp.name
        out, err = io.StringIO(), io.StringIO()
        self.assertEqual(check.run('bdb', out=out, err=err), 1)
        self.assertIn('missing cargo; see docs/runbooks/setup.md', err.getvalue())


if sys.platform != 'win32':
    # The stub tools are #!/bin/sh scripts with an exec bit, which Windows cannot run.
    class StubbedCheckTests(CheckFixture):
        def setUp(self):
            super().setUp()
            script = '''#!/bin/sh
case "${0##*/}" in
uname)
  if [ "$1" = -s ]; then printf 'Linux\\n'; else printf 'Linux x86_64\\n'; fi ;;
rustup) printf 'fixture-toolchain (default)\\n' ;;
rustc) printf 'rustc fixture\\nhost: x86_64-unknown-linux-gnu\\n' ;;
pkg-config)
  case "$2" in
  libclang) printf 'fixture-clang\\n' ;;
  glib-2.0) printf 'fixture-glib\\n' ;;
  *) exit 2 ;;
  esac ;;
cc) cat >/dev/null; printf 'header declarations\\n"fixture Berkeley DB"\\n' ;;
cargo) printf 'fixture cargo stdout\\n'; exit "${FIXTURE_CARGO_STATUS:-0}" ;;
esac
'''
            for name in ('uname', 'rustup', 'rustc', 'pkg-config', 'cc', 'cargo'):
                path = Path(self.temp.name) / name
                path.write_text(script)
                path.chmod(0o755)

        def exercise(self, status=0, error=None):
            """Run check.run with a fake supervised launch and return the streams."""
            os.environ['FIXTURE_CARGO_STATUS'] = str(status)
            out, err = io.StringIO(), io.StringIO()
            child_calls = []

            def run_command(command, *, env):
                child_calls.append((list(command), dict(env)))
                if error is not None:
                    raise error
                child = subprocess.run(command, env=env, capture_output=True, text=True)
                # Child stdout stays on stdout and child stderr on stderr.
                output.write(child.stdout)
                err.write(child.stderr)
                return child.returncode

            output = out
            with mock.patch.object(native.slot, 'run_command', side_effect=run_command), \
                    mock.patch.object(check.platform, 'system', return_value='Linux'), \
                    mock.patch.object(check.platform, 'machine', return_value='x86_64'):
                rc = check.run('bdb', out=out, err=err)
            return child_calls, rc, out.getvalue(), err.getvalue()

        def test_child_output_reaches_stdout_and_child_errors_reach_stderr(self):
            calls, rc, actual, err = self.exercise()
            self.assertEqual(rc, 0)
            self.assertIn('fixture cargo stdout', actual)
            self.assertNotIn('fixture cargo stdout', err)
            self.assertEqual(calls[0][0], ['cargo', 'test', '--locked', '--no-default-features',
                                           '--features', 'bdb', '-p', 'oxpinyin-store',
                                           '-p', 'oxpinyin-capi'])
            self.assertEqual(calls[0][1]['FIXTURE_CARGO_STATUS'], '0')

        def test_child_failure_preserves_stdout_and_status(self):
            _, rc, actual, err = self.exercise(status=17)
            self.assertEqual(rc, 17)
            self.assertIn('fixture cargo stdout', actual)
            self.assertNotIn('fixture cargo stdout', err)

        def test_launch_failure_reports_the_reason_with_status_one(self):
            _, rc, actual, err = self.exercise(error=OSError('fixture launch failure'))
            self.assertEqual(rc, 1)
            self.assertNotIn('fixture cargo stdout', actual)
            self.assertIn('FAIL: fixture launch failure', err)


if sys.platform == 'win32':
    class WindowsToolTests(unittest.TestCase):
        """Windows needs MSVC and the pkg-config program PKG_CONFIG names, not cc and pkg-config."""

        def setUp(self):
            temporary = tempfile.TemporaryDirectory()
            self.addCleanup(temporary.cleanup)
            self.bin = Path(temporary.name, 'bin')
            self.bin.mkdir()
            # The empty files are found by PATH and PATHEXT; the tests launch none of them.
            for name in ('cargo.exe', 'rustup.exe', 'cl.exe', 'pkgconf.exe'):
                (self.bin / name).write_bytes(b'')
            self.empty = Path(temporary.name, 'empty')
            self.empty.mkdir()
            # Only a compiler: the real cargo and rustup stay first on PATH for the run tests.
            self.compiler = Path(temporary.name, 'compiler')
            self.compiler.mkdir()
            (self.compiler / 'cl.exe').write_bytes(b'')
            self.environment = {'PATH': str(self.bin), 'PKG_CONFIG': str(self.bin / 'pkgconf.exe'),
                                'ProgramFiles(x86)': str(self.empty)}

        def missing(self, **changes):
            environment = {**self.environment, **changes}
            return check.missing_tool(native.Tools(environment={key: value for key, value in environment.items()
                                                                if value is not None}))

        def test_cargo_rustup_msvc_and_the_named_pkg_config_are_enough(self):
            self.assertIsNone(self.missing())

        def test_cc_is_not_required(self):
            self.assertFalse((self.bin / 'cc.exe').exists())
            self.assertIsNone(self.missing())

        def test_each_missing_tool_is_named(self):
            for name in ('cargo', 'rustup'):
                with self.subTest(tool=name):
                    (self.bin / f'{name}.exe').unlink()
                    self.assertEqual(self.missing(), name)
                    (self.bin / f'{name}.exe').write_bytes(b'')
            (self.bin / 'cl.exe').unlink()
            self.assertEqual(self.missing(), 'MSVC (cl, or the Visual Studio C++ tools)')
            (self.bin / 'cl.exe').write_bytes(b'')
            (self.bin / 'pkgconf.exe').unlink()
            self.assertEqual(self.missing(), str(self.bin / 'pkgconf.exe'))

        def test_pkg_config_defaults_to_the_plain_name(self):
            self.assertEqual(check.pkg_config_program({}), 'pkg-config')
            self.assertEqual(check.pkg_config_program({'Pkg_Config': 'C:\\tools\\pkgconf.exe'}), 'C:\\tools\\pkgconf.exe')
            self.assertEqual(self.missing(PKG_CONFIG=None), 'pkg-config')
            (self.bin / 'pkg-config.exe').write_bytes(b'')
            self.assertIsNone(self.missing(PKG_CONFIG=None))

        def test_the_compiler_is_found_on_path_in_the_developer_environment_or_through_vswhere(self):
            self.assertTrue(check.windows_compiler(self.environment))
            bare = {**self.environment, 'PATH': str(self.empty)}
            # No cl and no vswhere under ProgramFiles(x86): vswhere cannot launch.
            self.assertFalse(check.windows_compiler(bare))
            self.assertTrue(check.windows_compiler({**bare, 'VCToolsInstallDir': 'C:\\VC\\Tools\\'}))

            def vswhere(command, **options):
                if isinstance(command, list):
                    self.assertEqual(Path(command[0]).name, 'vswhere.exe')
                    return subprocess.CompletedProcess(command, 0, stdout=str(self.empty) + '\n', stderr='')
                return subprocess.CompletedProcess(command, 0, stdout='VCToolsInstallDir=C:\\VC\\Tools\\\n', stderr='')

            self.assertTrue(check.windows_compiler(bare, capture=vswhere))

            def none_found(command, **options):
                return subprocess.CompletedProcess(command, 1, stdout='', stderr='')

            self.assertFalse(check.windows_compiler(bare, capture=none_found))

        def test_run_uses_the_named_pkg_config_for_every_query(self):
            log = self.empty / 'pkg-config.log'
            stub = self.empty / 'pkgconf.cmd'
            stub.write_text(f'@echo %* >> "{log}"\r\n@echo fixture-version\r\n')
            environment = {**os.environ, 'PATH': os.environ['PATH'] + os.pathsep + str(self.compiler),
                           'PKG_CONFIG': str(stub), 'CARGO_TARGET_DIR': 'fixture-target'}
            out, err = io.StringIO(), io.StringIO()
            with mock.patch.dict(os.environ, environment, clear=True), \
                    mock.patch.object(native.slot, 'run_command', return_value=0) as cargo:
                status = check.run('tkrzw', out=out, err=err)
            self.assertEqual((status, err.getvalue()), (0, ''))
            self.assertEqual([line.strip() for line in log.read_text().splitlines()],
                             ['--modversion libclang', '--modversion tkrzw'])
            self.assertEqual(cargo.call_args.args[0][:2], ['cargo', 'test'])

        def test_bdb_is_refused_without_cc(self):
            out, err = io.StringIO(), io.StringIO()
            stub = self.empty / 'pkgconf.cmd'
            stub.write_text('@echo fixture-version\r\n')
            environment = {**os.environ, 'PATH': os.environ['PATH'] + os.pathsep + str(self.compiler),
                           'PKG_CONFIG': str(stub)}
            with mock.patch.dict(os.environ, environment, clear=True):
                self.assertEqual(check.run('bdb', out=out, err=err), 1)
            self.assertIn('bdb: reading db.h needs cc', err.getvalue())


class MsvcTests(unittest.TestCase):
    def test_vcvars_preserves_spaced_paths_and_skips_cmd_drive_entries(self):
        calls = []
        def capture(command, **kwargs):
            calls.append(command)
            return mock.Mock(returncode=0, stdout=('C:\\Visual Studio\\\n' if len(calls) == 1 else
                        '=C:=C:\\ignored\nPath=C:\\VC Tools\nVCToolsInstallDir=C:\\VC Tools\n'))
        with mock.patch.object(check.platform, 'machine', return_value='ARM64'):
            env = check.msvc_environment({'programfiles(x86)': 'C:\\Program Files (x86)'}, capture=capture)
        self.assertIn('vcvarsarm64.bat', calls[1])
        self.assertEqual(check.lookup(env, 'PATH'), 'C:\\VC Tools')
        self.assertNotIn('=C:', env)


if __name__ == '__main__':
    unittest.main()
