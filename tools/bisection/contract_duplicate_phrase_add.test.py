#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for duplicate phrase-add fixture wiring."""
from pathlib import Path
import sys

import os
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_duplicate_phrase_add as family


def declarations(craft, key):
    return {name: (metadata, fn) for name, _, metadata, fn in collect_declarations(family.register, _craft_db=craft, _ucs4_key=key)}


class DuplicatePhraseAddTests(unittest.TestCase):
    def test_zhuyin_duplicate_uses_injected_key_and_backend_writer(self):
        kit, craft, key, calls = Mock(user='/user'), Mock(), Mock(return_value=b'key'), {}
        kit.init.return_value = 'ctx'
        kit.fn.side_effect = lambda name, *types: calls.setdefault(name, Mock(return_value=True))
        with patch.object(family, '_write_user_conf') as conf:
            metadata, run = declarations(craft, key)['abort-zhuyin-add-phrase-duplicate-library-token']
            self.assertEqual(run(kit), {'ret': True})
        self.assertEqual(metadata, {'mode': 'zhuyin', 'abort': False, 'issues': ('525',)})
        key.assert_called_once_with('你好')
        craft.assert_called_once_with(os.path.join('/user', 'user_phrase_index.bin'),
                                      [(b'key', b'\x01\x00\x00\x07\x02\x00\x00\x07')], 1)
        conf.assert_called_once_with(kit)

    def test_pinyin_missing_item_keeps_each_registration_bound_to_its_own_writer(self):
        first, second = Mock(), Mock()
        key = Mock(return_value=b'encoded')
        kit = Mock(user='/user')
        kit.init.return_value = 'ctx'
        kit.fn.return_value = Mock(return_value=True)
        name = 'abort-add-phrase-index-item-missing'
        one, two = declarations(first, key)[name][1], declarations(second, key)[name][1]
        with patch.object(family, '_write_user_conf'):
            self.assertEqual(one(kit), {'ret': True})
            self.assertEqual(two(kit), {'ret': True})
        expected = (os.path.join('/user', 'user_phrase_index.bin'), [(b'encoded', b'\x01\x00\x00\x07')], 1)
        first.assert_called_once_with(*expected)
        second.assert_called_once_with(*expected)


if __name__ == '__main__':
    unittest.main()
