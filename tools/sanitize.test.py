#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Selection and platform-refusal tests for tools/sanitize.sh."""
import os
from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'tools/sanitize.sh'


def run(*args, host='Darwin', arch='arm64'):
    """Run the selector with an explicit host identity and return the completed process."""
    env = dict(os.environ,
               OXPINYIN_SANITIZER_HOST_OS=host,
               OXPINYIN_SANITIZER_HOST_ARCH=arch)
    return subprocess.run(['bash', str(SCRIPT), *args], capture_output=True, text=True, env=env)


class SanitizeSelectionTests(unittest.TestCase):
    def test_default_is_address(self):
        result = run('--check', 'bdb')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('backend=bdb sanitizer=address host=Darwin/arm64', result.stdout)

    def test_darwin_accepts_thread(self):
        result = run('--check', 'bdb', 'tsan')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('sanitizer=thread', result.stdout)

    def test_darwin_all_accepts_thread(self):
        result = run('--check', 'all', 'thread')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('backend=all sanitizer=thread', result.stdout)

    def test_memory_is_refused_on_darwin(self):
        result = run('bdb', 'memory')
        self.assertEqual(result.returncode, 1)
        self.assertIn('MemorySanitizer is unsupported on macOS', result.stderr)

    def test_memory_is_refused_on_linux_without_an_in_tree_wrapper(self):
        result = run('bdb', 'memory', host='Linux', arch='x86_64')
        self.assertEqual(result.returncode, 1)
        self.assertIn('no in-tree wrapper', result.stderr)

    def test_leak_is_refused_on_macos_arm64(self):
        result = run('bdb', 'leak')
        self.assertEqual(result.returncode, 1)
        self.assertIn('standalone LeakSanitizer is unsupported on macOS arm64', result.stderr)

    def test_tkrzw_leak_is_refused_on_every_host(self):
        for host, arch in (('Darwin', 'arm64'), ('Linux', 'x86_64')):
            with self.subTest(host=host, arch=arch):
                result = run('tkrzw', 'leak', host=host, arch=arch)
                self.assertEqual(result.returncode, 1)
                self.assertIn('tkrzw LeakSanitizer is unsupported', result.stderr)
                self.assertIn('process-lifetime allocations', result.stderr)

    def test_all_leak_is_refused_because_tkrzw_is_included(self):
        result = run('all', 'leak', host='Linux', arch='x86_64')
        self.assertEqual(result.returncode, 1)
        self.assertIn('tkrzw LeakSanitizer is unsupported', result.stderr)

    def test_leak_is_accepted_on_macos_x86_64(self):
        result = run('--check', 'bdb', 'lsan', host='Darwin', arch='x86_64')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('backend=bdb sanitizer=leak host=Darwin/x86_64', result.stdout)

    def test_invalid_sanitizer_is_usage(self):
        result = run('--check', 'bdb', 'bogus')
        self.assertEqual(result.returncode, 2)
        self.assertIn('usage:', result.stderr)


if __name__ == '__main__':
    unittest.main()
