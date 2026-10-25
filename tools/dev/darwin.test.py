#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Import tests for the shared Darwin binding and the native CLI."""
import mmap
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
from tools.dev import darwin


class DarwinImportTests(unittest.TestCase):
    def run_code(self, code):
        """Run a fresh interpreter at the checkout root and return its result."""
        return subprocess.run([sys.executable, '-c', code], cwd=ROOT,
                              capture_output=True, text=True, timeout=60)

    def test_cli_imports_when_os_sysconf_is_absent(self):
        # Windows has no os.sysconf. Remove it on this host before importing
        # the CLI, whose host module imports the shared Darwin binding.
        code = ("import os\n"
                "if hasattr(os, 'sysconf'):\n"
                "    del os.sysconf\n"
                "import tools.dev.cli\n"
                "print('ok')\n")
        result = self.run_code(code)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), 'ok')

    def test_import_does_not_load_libsystem(self):
        # Loading libSystem and resolving its symbols stay inside _system()
        # and the observation functions; importing the module must not do it.
        code = ("import ctypes\n"
                "def fail(*args, **kwargs):\n"
                "    raise AssertionError('libc loaded at import')\n"
                "ctypes.CDLL = fail\n"
                "import tools.dev.darwin\n"
                "print('ok')\n")
        result = self.run_code(code)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), 'ok')

    def test_page_size_is_the_portable_mmap_page_size(self):
        self.assertEqual(darwin.PAGE_SIZE, mmap.PAGESIZE)


if __name__ == '__main__':
    unittest.main()
