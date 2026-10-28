#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for stored-total overflow and header cases."""
import unittest
from unittest.mock import Mock, patch

import contract_stored_total_overflow as family


def declarations(total, stored, facade, modify, write):
    result = {}

    def case(name, **metadata):
        def collect(fn):
            result[name] = (metadata, fn)
            return fn
        return collect

    family.register(case, FACADE_TOTAL=total, _stored_library_total=stored,
                    _facade_total_out=facade, _modify_header=modify, _write_log=write)
    return result


class StoredTotalOverflowTests(unittest.TestCase):
    def test_chained_headers_use_shared_total_writer_and_facade_helpers(self):
        kit, stored, facade = Mock(), Mock(return_value=10), Mock(return_value={'ranking': 'seen'})
        modify, write = Mock(side_effect=[b'A', b'B']), Mock()
        kit.fn.return_value = Mock(return_value=True)
        with patch.object(family, '_write_user_conf') as conf:
            result = declarations(100, stored, facade, modify, write)['log-headers-chained-both-apply'][1](kit)
        self.assertEqual(result, {'total': 10, 'ranking': 'seen'})
        shift = (2 ** 32 + 2000000 - 100) % 2 ** 32
        self.assertEqual([call.args for call in modify.call_args_list],
                         [(0, 10, 1000000010), (0, 1000000010, (10 + shift) % 2 ** 32)])
        write.assert_called_once_with(kit, b'AB')
        conf.assert_called_once_with(kit)
        stored.assert_called_once_with(kit, 'gb_char.bin')
        facade.assert_called_once_with(kit)

    def test_below_limit_uses_the_expected_delta(self):
        kit, calls = Mock(), {}
        stored, facade, modify, write = Mock(return_value=100), Mock(return_value='facade'), Mock(return_value=b'log'), Mock()
        kit.fn.side_effect = lambda name, *types: calls.setdefault(name, Mock(return_value=True))
        with patch.object(family, '_write_user_conf'), patch.object(family, 'tokens_of', return_value=[0x01000001]), \
                patch.object(family, 'unigram_of', return_value=17):
            result = declarations(500, stored, facade, modify, write)['stored-total-overflow-below'][1](kit)
        self.assertEqual(result, {'before': 17, 'add': True, 'after': 17, 'facade': 'facade'})
        calls['token_add_unigram_frequency'].assert_called_once_with(kit.inst, 0x01000001, 4999999)
        modify.assert_called_once_with(0, 100, 2 ** 32 - 5000000)


if __name__ == '__main__':
    unittest.main()
