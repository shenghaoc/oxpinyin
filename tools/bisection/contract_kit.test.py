#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""The native kit keeps the facade's handle, cell and loader guard at call time."""
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import contract_kit


class KitTests(unittest.TestCase):
    def test_constructor_passes_the_cell_and_guard_at_call_time(self):
        guard = MagicMock()
        with tempfile.TemporaryDirectory() as scratch, \
                patch.object(contract_kit.C, 'CDLL', return_value=MagicMock()):
            kit = contract_kit.Kit('pinyin', '/subject.so', '/data', scratch,
                                   cell='kc', check_tkrzw=guard)
            guard.assert_called_once_with('kc', 'reverse_cross_open.py', 'subject', '/subject.so')
            self.assertEqual((kit.mode, kit.data, kit.scratch), ('pinyin', '/data', scratch))
            self.assertEqual(Path(kit.user).parent, Path(scratch))
            self.assertEqual((kit.logs, kit.log_messages), ([], []))

    def test_glib_resolves_through_the_facade_handle(self):
        with tempfile.TemporaryDirectory() as scratch, \
                patch.object(contract_kit.C, 'CDLL', return_value=MagicMock()) as cdll:
            kit = contract_kit.Kit('pinyin', '/subject.so', '/data', scratch,
                                   cell='tkrzw', check_tkrzw=MagicMock())
        self.assertEqual([call.args for call in cdll.call_args_list],
                         [('/subject.so',), ('/subject.so',)])
        self.assertIsNot(kit.glib, None)


if __name__ == '__main__':
    unittest.main()
