#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Check stdout compatibility and visible slot diagnostics without Rust builds."""
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

from tools.dev import check, host


@unittest.skipUnless(sys.platform.startswith('linux'), 'Linux setup-check comparison')
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
        os.environ['FIXTURE_CARGO_STATUS'] = str(status)
        legacy = subprocess.run(['bash', str(ROOT / 'tools/dev/check.sh'), 'bdb'],
                                env=dict(os.environ), capture_output=True, text=True)
        out, err = io.StringIO(), io.StringIO()

        def run_command(command, *, env, out):
            self.assertIs(out, err)
            print('fixture slot wait and resource diagnostics', file=out)
            if error is not None:
                raise error
            child = subprocess.run(command, env=env, capture_output=True, text=True)
            output.write(child.stdout)
            err.write(child.stderr)
            return child.returncode

        output = out
        with mock.patch.object(check.slot, 'run_command', side_effect=run_command), \
                mock.patch.object(check.platform, 'system', return_value='Linux'), \
                mock.patch.object(check.platform, 'machine', return_value='x86_64'):
            rc = check.run('bdb', out=out, err=err)
        # The native plan already removes these two optional oracle lines.
        expected = ''.join(line for line in legacy.stdout.splitlines(True)
                           if not line.startswith(('PINYIN_MODEL_DIR:', 'PINYIN_ORACLE_PREFIX:')))
        self.assertIn('fixture slot wait and resource diagnostics', err.getvalue())
        self.assertNotIn('fixture slot wait and resource diagnostics', out.getvalue())
        return legacy.returncode, rc, expected, out.getvalue(), err.getvalue()

    def test_slot_diagnostics_reach_stderr_and_stdout_matches_the_shell_check(self):
        old_rc, rc, expected, actual, _ = self.exercise()
        self.assertEqual((old_rc, rc), (0, 0))
        self.assertEqual(actual, expected)

    def test_child_failure_preserves_the_shell_stdout_and_status(self):
        old_rc, rc, expected, actual, _ = self.exercise(status=17)
        self.assertEqual((old_rc, rc), (17, 17))
        self.assertEqual(actual, expected)

    def test_blocked_check_keeps_wait_diagnostics_and_the_final_reason(self):
        _, rc, expected, actual, err = self.exercise(
            error=host.ResourceBlocked('resource-blocked: fixture memory limit'))
        self.assertEqual(rc, 1)
        self.assertEqual(actual, expected.split('fixture cargo stdout\n', 1)[0])
        self.assertIn('FAIL: resource-blocked: fixture memory limit', err)


if __name__ == '__main__':
    unittest.main()
