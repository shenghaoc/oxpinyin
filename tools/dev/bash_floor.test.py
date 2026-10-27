#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""The one Bash floor declaration, its source-line gate and its run-time check."""
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
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
    """Return a Bash older than the floor, or None."""
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
    def test_floor_is_declared_once(self):
        self.assertEqual(bash_floor.read_floor(), '5.0')
        self.assertEqual((FLOOR).read_text(encoding='utf-8').count('OXPINYIN_BASH_FLOOR='), 1)

    def test_every_executed_script_sources_the_floor(self):
        self.assertEqual(bash_floor.check_repository(), [])

    def test_inline_guard_is_rejected(self):
        text = ('#!/usr/bin/env bash\n'
                'case ${BASH_VERSION-} in [5-9].*) ;; *) exit 1 ;; esac\n')
        errors = bash_floor.check_text('tools/inline.sh', text)
        self.assertTrue(any('inline Bash version guard' in error for error in errors), errors)

    def test_source_line_is_accepted(self):
        text = '#!/usr/bin/env bash\n' + bash_floor.source_line('tools/example.sh') + '\n'
        self.assertEqual(bash_floor.check_text('tools/example.sh', text), [])

    def test_source_only_libraries_do_not_source_the_floor(self):
        for relative in bash_floor.SOURCED_ONLY:
            text = (ROOT / relative).read_text(encoding='utf-8')
            self.assertNotIn(bash_floor.source_line(relative), text, relative)
            self.assertNotIn('BASH_VERSION', text, relative)


class RunTimeTests(unittest.TestCase):
    def test_the_floor_rejects_a_non_bash_shell(self):
        shell = posix_shell()
        if shell is None:
            self.skipTest('no POSIX shell without BASH_VERSION')
        result = run_source(shell)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertRegex(result.stderr, MESSAGE)
        self.assertIn('(found not bash)', result.stderr)

    def test_the_floor_accepts_the_bash_5_ci_uses(self):
        bash = shutil.which('bash')
        if bash is None:
            self.skipTest('no bash on PATH')
        version = subprocess.run([bash, '-c', 'printf %s "${BASH_VERSION%%.*}"'],
                                 capture_output=True, text=True).stdout
        if not version.isdigit() or int(version) < 5:
            self.skipTest(f'bash on PATH is {version}, not the CI floor')
        result = run_source(bash)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, '')

    def test_the_floor_rejects_macos_bash_3_2_when_present(self):
        found = old_bash()
        if found is None:
            self.skipTest('no Bash older than the floor on this host')
        executable, version = found
        result = run_source(executable)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertRegex(result.stderr, MESSAGE)
        self.assertIn(f'(found {version})', result.stderr)


class StagedCopyTests(unittest.TestCase):
    """compat.sh runs compat-inside.sh as /input/inside.sh, away from tools/lib."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.input = Path(directory.name)
        # Stage the helpers with compat.sh's own copy lines, and run the copy it runs.
        compat = (ROOT / 'tools/oracle/compat.sh').read_text(encoding='utf-8')
        self.staged = {source: self.input / name
                       for source, name in re.findall(r'^cp (tools/\S+) "\$input/([^"/]+)"$', compat, re.M)}
        self.assertIn(bash_floor.FLOOR_FILE, self.staged, 'compat.sh does not stage the floor file')
        for source, path in self.staged.items():
            shutil.copyfile(ROOT / source, path)
        self.script = self.staged['tools/oracle/compat-inside.sh']
        self.assertEqual(re.search(r'\bbash /input/(\S+)', compat)[1], self.script.name)
        self.floor = self.staged[bash_floor.FLOOR_FILE]

    def run_staged(self):
        environment = {name: value for name, value in os.environ.items()
                       if name not in ('OXPINYIN_COMPAT_CONTAINER', 'OXPINYIN_BASH_FLOOR')}
        return subprocess.run(['bash', str(self.script)], env=environment,
                              capture_output=True, text=True, timeout=10)

    def test_the_staged_copy_sources_and_runs_the_floor_beside_it(self):
        # A floor no Bash meets shows the check runs from the copy and refuses.
        text, count = bash_floor.FLOOR_RE.subn('OXPINYIN_BASH_FLOOR=99.0', self.floor.read_text(encoding='utf-8'))
        self.assertEqual(count, 1)
        self.floor.write_text(text, encoding='utf-8')
        result = self.run_staged()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertRegex(result.stderr, rf'\A{re.escape(str(self.script))}: needs Bash >= 99\.0 \(found [^\n]+\)\n\Z')

    def test_bash_5_passes_the_staged_floor_to_the_next_check(self):
        result = self.run_staged()
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(result.stderr, 'FAIL: compatibility requires a controlled Linux amd64 or arm64 image\n')

    def test_a_missing_staged_floor_stops_the_script(self):
        self.floor.unlink()
        result = self.run_staged()
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn(f'FAIL: {self.script}: {self.input}/bash-floor.sh not sourced; Bash floor unchecked\n',
                      result.stderr)
        self.assertNotIn('controlled Linux', result.stderr)


if __name__ == '__main__':
    unittest.main()
