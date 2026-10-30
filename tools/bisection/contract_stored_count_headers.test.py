#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the stored pronunciation-count fixtures."""
from pathlib import Path
import sys

import ctypes as C
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_stored_count_headers as family


def declarations(chunk):
    return {name: (metadata, fn) for name, _, metadata, fn in collect_declarations(family.register, _chunk=chunk)}


class StoredCountHeadersTests(unittest.TestCase):
    def test_system_and_user_fixtures_use_the_injected_chunk_writer(self):
        with tempfile.TemporaryDirectory() as tmp:
            kit = Mock(user=tmp)
            chunk = Mock(return_value=b'encoded')
            count_tokens = []

            def count(inst, token, out):
                count_tokens.append(token)
                C.cast(out, C.POINTER(family.U))[0] = 9
                return True

            kit.fn.side_effect = lambda name, *types: (count if name == 'token_get_n_pronunciation'
                                                       else Mock(return_value='handle'))
            with patch.object(family, '_write_user_conf') as write_conf, \
                    patch.object(family, 'conf_system', return_value=tmp) as system:
                cases = declarations(chunk)
                system_out = cases['stored-count-headers-system-pinyin'][1](kit)
                self.assertEqual(Path(tmp, 'count.bin').read_bytes(), b'encoded')
                self.assertEqual(system_out['1'], [True, 9])
                system.assert_called_once_with(kit, ('gb_char.bin', 'count.bin'))
                self.assertEqual(count_tokens[1], 0x01000001)
                count_tokens.clear()
                user_out = cases['stored-count-headers-user-zhuyin'][1](kit)
                self.assertEqual(Path(tmp, 'user.bin').read_bytes(), b'encoded')
                self.assertEqual(user_out['no sub-index'], [True, 9])
                self.assertEqual(count_tokens[1], 0x07000001)
                write_conf.assert_called_once_with(kit)
            self.assertEqual(chunk.call_count, 2)
            self.assertEqual(chunk.call_args_list[0].args, chunk.call_args_list[1].args)


if __name__ == '__main__':
    unittest.main()
