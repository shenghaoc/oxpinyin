#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Coverage state isolation, argument transport and cleanup regressions."""
import concurrent.futures
import contextlib
import io
import os
from pathlib import Path
import sys
import tempfile
import threading
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import coverage, slot


class CoverageTests(unittest.TestCase):
    def test_overlapping_calls_isolate_profiles_and_preserve_caller_state(self):
        args = ['--locked', '-p', 'oxpinyin-chewing', '--', '', '/path with spaces']
        barrier = threading.Barrier(2, timeout=10)
        targets = []

        def run(command, *, env):
            self.assertEqual(command, ['cargo', 'llvm-cov', *args])
            target = Path(env['CARGO_LLVM_COV_TARGET_DIR'])
            self.assertEqual(env['CARGO_LLVM_COV_BUILD_DIR'], str(target))
            self.assertNotEqual(target, shared)
            self.assertTrue(target.parent.is_dir())
            self.assertFalse(target.exists())
            targets.append(target)
            target.mkdir()
            profile = target / 'profile'
            profile.write_text(str(target))
            barrier.wait()
            self.assertEqual(profile.read_text(), str(target))
            return 0

        with tempfile.TemporaryDirectory() as scratch:
            shared = Path(scratch)
            sentinel = shared / 'caller-profile'
            sentinel.write_text('keep')
            with mock.patch.dict(os.environ, CARGO_LLVM_COV_TARGET_DIR=str(shared),
                                 CARGO_LLVM_COV_BUILD_DIR=str(shared)), \
                    mock.patch.object(slot, 'run_command', side_effect=run), \
                    contextlib.redirect_stderr(io.StringIO()), \
                    concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                futures = [pool.submit(coverage.run, args) for _ in range(2)]
                self.assertEqual([future.result(timeout=15) for future in futures], [0, 0])
                self.assertEqual(os.environ['CARGO_LLVM_COV_TARGET_DIR'], str(shared))
                self.assertEqual(os.environ['CARGO_LLVM_COV_BUILD_DIR'], str(shared))
            self.assertEqual(sentinel.read_text(), 'keep')
        self.assertEqual(len(set(targets)), 2)
        self.assertTrue(all(not target.exists() for target in targets))
        self.assertTrue(all(not target.parent.exists() for target in targets))

    def test_child_statuses_and_cleanup(self):
        for status, expected in ((0, 0), (1, 1), (77, 77), (-15, 143)):
            with self.subTest(status=status):
                targets = []

                def run(command, *, env):
                    targets.append(Path(env['CARGO_LLVM_COV_TARGET_DIR']))
                    self.assertTrue(targets[0].parent.is_dir())
                    self.assertFalse(targets[0].exists())
                    targets[0].mkdir()
                    return status

                with mock.patch.object(slot, 'run_command', side_effect=run), \
                        contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(coverage.run([]), expected)
                self.assertFalse(targets[0].exists())
                self.assertFalse(targets[0].parent.exists())

    def test_launch_and_admission_failures_cleanup(self):
        for error, expected in ((OSError('missing cargo'), 1),
                                (ValueError('resource-blocked'), 1),
                                (slot.Cancelled('cancelled'), 130)):
            with self.subTest(error=error):
                targets = []

                def run(command, *, env):
                    targets.append(Path(env['CARGO_LLVM_COV_TARGET_DIR']))
                    raise error

                with mock.patch.object(slot, 'run_command', side_effect=run), \
                        contextlib.redirect_stderr(io.StringIO()) as output:
                    self.assertEqual(coverage.run([]), expected)
                self.assertFalse(targets[0].exists())
                self.assertFalse(targets[0].parent.exists())
                self.assertIn('coverage: ', output.getvalue())


if __name__ == '__main__':
    unittest.main()
