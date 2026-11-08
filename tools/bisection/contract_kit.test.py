#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""The native kit keeps the facade's handle, cell and loader guard at call time."""
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, call, patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import contract_abi
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

    def test_init_accepts_paths(self):
        kit = object.__new__(contract_kit.Kit)
        kit._data, kit.user = Path('/data'), Path('/user')
        seen = {}

        def fn(name, res, *args):
            def call(system, user):
                seen.update(system=system, user=user)
                return 1
            return call
        kit.fn = fn
        self.assertEqual(kit.init(), 1)
        self.assertEqual(seen, {'system': b'/data', 'user': b'/user'})

    def test_alloc_reaches_every_facade_symbol_through_fn(self):
        kit = object.__new__(contract_kit.Kit)
        kit._ctx = 'context'
        with patch.object(contract_kit.Kit, 'fn',
                          return_value=lambda context: ('instance', context)) as fn:
            self.assertEqual(kit.alloc(), ('instance', 'context'))
        fn.assert_called_once_with('alloc_instance', contract_abi.P, contract_abi.P)


if __name__ == '__main__':
    unittest.main()
