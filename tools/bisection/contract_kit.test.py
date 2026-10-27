#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Native-kit injection preserves the driver's loader, cell and class patches."""
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))


def load_driver():
    spec = importlib.util.spec_from_file_location('kit_boundary_driver', HERE / 'contract-diff.py')
    driver = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(driver)
    return driver


class KitBoundaryTests(unittest.TestCase):
    def test_constructor_reads_patched_cell_and_guard_at_call_time_and_environment_still_wins(self):
        driver = load_driver()
        with tempfile.TemporaryDirectory() as scratch, patch.object(driver.C, 'CDLL', return_value=MagicMock()), \
                patch.object(driver, 'check_tkrzw') as guard, patch.object(driver, 'CELL', 'kc'), \
                patch.dict(os.environ, OXPINYIN_RUNTIME_SIDE='pin'):
            os.environ.pop('CONTRACT_DIFF_CELL', None)
            kit = driver.Kit('pinyin', '/subject.so', '/data', scratch)
            guard.assert_called_once_with('kc', 'contract-diff.py', 'pin', '/subject.so')
            self.assertEqual((kit.mode, kit.data, kit.scratch), ('pinyin', '/data', scratch))
            self.assertEqual(Path(kit.user).parent, Path(scratch))
            self.assertEqual((kit.logs, kit.log_messages), ([], []))
            guard.reset_mock()
            with patch.dict(os.environ, CONTRACT_DIFF_CELL='tkrzw'):
                driver.Kit('zhuyin', '/other.so', '/data', scratch)
            guard.assert_called_once_with('tkrzw', 'contract-diff.py', 'pin', '/other.so')

    def test_inherited_alloc_still_reaches_a_patch_on_the_driver_class_method(self):
        driver = load_driver()
        kit = object.__new__(driver.Kit)
        kit._ctx = 'context'
        with patch.object(driver.Kit, 'fn', return_value=lambda context: ('instance', context)) as fn:
            self.assertEqual(kit.alloc(), ('instance', 'context'))
        fn.assert_called_once_with('alloc_instance', driver.P, driver.P)


if __name__ == '__main__':
    unittest.main()
