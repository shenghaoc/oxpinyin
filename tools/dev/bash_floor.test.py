#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""The Bash floor's source-line gate and its run-time check."""
from pathlib import Path
import shutil
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import bash_floor

FLOOR = ROOT / bash_floor.FLOOR_FILE
MESSAGE = r'^.+: needs Bash >= 5\.0 \(found .+\)$'


def run_source(shell, *arguments):
    """Source the floor file under SHELL and return the completed process."""
    return subprocess.run([shell, *arguments, '-c', f'. "{FLOOR}"'],
                          capture_output=True, text=True)


def posix_shell():
    """Return a shell that does not set BASH_VERSION, or None."""
    for name in ('dash', 'sh'):
        executable = shutil.which(name)
        if executable is None:
            continue
        probe = subprocess.run([executable, '-c', 'printf %s "${BASH_VERSION-}"'],
                               capture_output=True, text=True)
        if probe.returncode == 0 and not probe.stdout:
            return executable
    return None


def old_bash():
    """Return macOS's Bash older than the floor, or None."""
    for name in ('/bin/bash', '/usr/bin/bash'):
        executable = shutil.which(name)
        if executable is None:
            continue
        version = subprocess.run([executable, '-c', 'printf %s "${BASH_VERSION-}"'],
                                 capture_output=True, text=True).stdout
        major = version.split('.', 1)[0]
        if major.isdigit() and int(major) < 5:
            return executable, version
    return None


class FloorFileTests(unittest.TestCase):
    def test_inline_guard_is_rejected(self):
        text = ('#!/usr/bin/env bash\n'
                'case ${BASH_VERSION-} in [5-9].*) ;; *) exit 1 ;; esac\n')
        errors = bash_floor.check_text('tools/inline.sh', text)
        self.assertTrue(any('inline Bash version guard' in error for error in errors), errors)


class RunTimeTests(unittest.TestCase):
    if sys.platform != 'win32':
        def test_the_floor_accepts_the_bash_5_ci_uses(self):
            bash = shutil.which('bash')
            self.assertIsNotNone(bash, 'bash is required on this platform')
            version = subprocess.run([bash, '-c', 'printf %s "${BASH_VERSION%%.*}"'],
                                     capture_output=True, text=True).stdout
            self.assertTrue(version.isdigit() and int(version) >= 5,
                            f'bash on PATH is {version}, not the CI floor')
            result = run_source(bash)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stderr, '')

    if sys.platform.startswith('linux'):
        def test_the_floor_rejects_a_non_bash_shell(self):
            shell = posix_shell()
            self.assertIsNotNone(shell, 'Linux needs a POSIX shell that does not set BASH_VERSION')
            result = run_source(shell)
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertRegex(result.stderr, MESSAGE)
            self.assertIn('(found not bash)', result.stderr)

    if sys.platform == 'darwin':
        def test_the_floor_rejects_macos_bash_3_2(self):
            found = old_bash()
            self.assertIsNotNone(found, 'macOS /bin/bash must be older than the floor')
            executable, version = found
            result = run_source(executable)
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertRegex(result.stderr, MESSAGE)
            self.assertIn(f'(found {version})', result.stderr)


if __name__ == '__main__':
    unittest.main()
