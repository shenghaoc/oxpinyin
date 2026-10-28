#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for zhuyin per-entry case builders."""
import ctypes as C
import unittest
from unittest.mock import Mock

import contract_zhuyin_per_entry as family


class ZhuyinPerEntryTests(unittest.TestCase):
    def test_single_key_parser_keeps_failed_outputs_untouched_for_each_scheme(self):
        kit, setters = Mock(), Mock(return_value=True)

        def parse(inst, text, out):
            if text == b'su3':
                C.cast(out, C.POINTER(C.c_ushort))[0] = 0x1234
                return True
            return False

        kit.fn.side_effect = lambda name, *types: parse if name == 'parse_chewing' else setters
        result = family.parse_chewing_probe((None, 2), (b'su3', b''))(kit)
        self.assertEqual(result['None']["b'su3'"], [True, 0x1234])
        self.assertEqual(result['2']["b''"], [False, family.UNTOUCHED & 0xFFFF])
        setters.assert_called_once_with(kit.ctx, 2)

    def test_symbol_vector_distinguishes_untouched_and_null(self):
        kit = Mock()
        self.assertEqual(family.zy_strv(kit, family.UNTOUCHED), 'untouched')
        self.assertIsNone(family.zy_strv(kit, None))
        kit.glib.g_strfreev.assert_not_called()


if __name__ == '__main__':
    unittest.main()
