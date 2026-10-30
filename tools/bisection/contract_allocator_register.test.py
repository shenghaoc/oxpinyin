#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for key-string allocator register cases."""
from pathlib import Path
import sys

import ctypes as C
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_allocator_register as family


def declarations():
    return {name: (metadata, fn) for name, _, metadata, fn in collect_declarations(family.register)}


class AllocatorRegisterTests(unittest.TestCase):
    def test_failed_key_getter_returns_early_for_each_facade(self):
        for mode in ('pinyin', 'zhuyin'):
            with self.subTest(mode=mode):
                kit, calls = Mock(), {}
                kit.fn.side_effect = lambda name, *types: calls.setdefault(name, Mock(return_value=name != 'get_' + mode + '_key'))
                name = f'alloc-register-{mode}_get_{mode}-zero-key'
                metadata, run = declarations()[name]
                self.assertEqual(metadata, {'mode': mode, 'control': True, 'issues': ('681',)})
                self.assertEqual(run(kit), {'guess_sentence': True, 'key_ret': False})
                self.assertNotIn('get_' + mode + '_string', calls)

    def test_real_key_is_read_and_owned_output_is_returned(self):
        key = C.c_uint16(0x1234)
        kit, calls = Mock(), {}

        def get_key(inst, offset, out):
            C.cast(out, C.POINTER(family.P))[0] = C.addressof(key)
            return True

        kit.fn.side_effect = lambda name, *types: (get_key if name == 'get_pinyin_key'
                                                  else calls.setdefault(name, Mock(return_value=True)))
        with patch.object(family, 'owned_text', return_value='reading') as owned:
            run = declarations()['alloc-register-pinyin_get_pinyin-real-key'][1]
            result = run(kit)
        self.assertEqual(result['key_bytes'], bytes(key).hex())
        self.assertEqual((result['ret'], result['out']), (True, 'reading'))
        self.assertEqual(owned.call_count, 1)
        calls['set_options'].assert_called_once_with(kit.ctx, 0x1aa)


if __name__ == '__main__':
    unittest.main()
