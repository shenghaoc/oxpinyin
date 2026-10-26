#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Check stdout, statuses and visible slot diagnostics without Rust builds."""
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

from tools.dev import check, host, native


BASE = '''System: Linux x86_64
fixture-toolchain (default)
rustc fixture
host: x86_64-unknown-linux-gnu
Backend: bdb; target directory: fixture-target
fixture-clang
Berkeley DB header: "fixture Berkeley DB"
fixture-glib
fixture cargo stdout
'''
EXPECTED = BASE + '\nSetup check passed: native backend tests linked and ran. This is not oracle parity.\n'


class CheckTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
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
        env = dict(os.environ, PATH=self.temp.name + os.pathsep + os.environ['PATH'],
                   CARGO_TARGET_DIR='fixture-target', FIXTURE_CARGO_STATUS='0')
        for name in ('PINYIN_MODEL_DIR', 'PINYIN_ORACLE_PREFIX', 'OXPINYIN_BDB_INCLUDE_DIR'):
            env.pop(name, None)
        patch = mock.patch.dict(os.environ, env, clear=True)
        patch.start()
        self.addCleanup(patch.stop)

    def exercise(self, status=0, error=None):
        """Run check.run with a fake admitted launch and return the streams."""
        os.environ['FIXTURE_CARGO_STATUS'] = str(status)
        out, err = io.StringIO(), io.StringIO()
        child_calls = []

        def run_command(command, *, env, out):
            self.assertIs(out, err)
            child_calls.append((list(command), dict(env)))
            print('fixture slot wait and resource diagnostics', file=out)
            if error is not None:
                raise error
            child = subprocess.run(command, env=env, capture_output=True, text=True)
            # Match the shell control's visible stdout: the admitted child's
            # stdout is the check's stdout, while its diagnostics go to stderr.
            output.write(child.stdout)
            err.write(child.stderr)
            return child.returncode

        output = out
        with mock.patch.object(native.slot, 'run_command', side_effect=run_command), \
                mock.patch.object(check.platform, 'system', return_value='Linux'), \
                mock.patch.object(check.platform, 'machine', return_value='x86_64'):
            rc = check.run('bdb', out=out, err=err)
        return child_calls, rc, out.getvalue(), err.getvalue()

    def test_slot_diagnostics_reach_stderr_and_stdout_matches_the_shell_check(self):
        calls, rc, actual, err = self.exercise()
        self.assertEqual(rc, 0)
        self.assertEqual(actual, EXPECTED)
        self.assertIn('fixture slot wait and resource diagnostics', err)
        self.assertNotIn('fixture slot wait and resource diagnostics', actual)
        self.assertEqual(calls[0][0], ['cargo', 'test', '--locked', '--no-default-features',
                                       '--features', 'bdb', '-p', 'oxpinyin-store',
                                       '-p', 'oxpinyin-capi'])
        self.assertEqual(calls[0][1]['FIXTURE_CARGO_STATUS'], '0')

    def test_child_failure_preserves_the_shell_stdout_and_status(self):
        _, rc, actual, _ = self.exercise(status=17)
        self.assertEqual(rc, 17)
        self.assertEqual(actual, BASE)

    def test_blocked_check_keeps_wait_diagnostics_and_the_final_reason(self):
        _, rc, actual, err = self.exercise(
            error=host.ResourceBlocked('resource-blocked: fixture memory limit'))
        self.assertEqual(rc, 1)
        self.assertEqual(actual, BASE.rsplit('fixture cargo stdout\n', 1)[0])
        self.assertIn('FAIL: resource-blocked: fixture memory limit', err)

    def test_unknown_backend_is_usage_and_missing_tool_is_status_one(self):
        out, err = io.StringIO(), io.StringIO()
        self.assertEqual(check.run('bogus', out=out, err=err), 2)
        self.assertEqual(out.getvalue(), '')
        self.assertIn('usage:', err.getvalue())
        os.environ['PATH'] = self.temp.name
        (Path(self.temp.name) / 'cargo').unlink()
        out, err = io.StringIO(), io.StringIO()
        self.assertEqual(check.run('bdb', out=out, err=err), 1)
        self.assertIn('missing cargo; see docs/runbooks/setup.md', err.getvalue())


if __name__ == '__main__':
    unittest.main()
