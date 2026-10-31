#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the separator and before-cursor cases (#1175)."""
from pathlib import Path
import sys

import hashlib
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_separator_before_cursor as family


# (case ID, positional options, keyword options) in registration order: the count, the
# first and last entries, and the SHA-256 of the repr of the whole ordered list.
EXPECTED_COUNT = 1360
EXPECTED_FIRST = ('separator/pinyin/0/parse/0', (), {'mode': 'pinyin', 'control': True, 'issues': ('640',)})
EXPECTED_LAST = ("allocator-zero/zhuyin/xi1'an1/key/4", (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'issues': ('681',)})
EXPECTED_SHA256 = '5ebe7d51002cd8c07d766454c48b978a4053fe3ea6dd4fa65ac9d962af77a7e3'


def declarations():
    return collect_declarations(family.register)


class SeparatorBeforeCursorTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        meta = [(name, args, kwargs) for name, args, kwargs, _ in declarations()]
        self.assertEqual(len(meta), EXPECTED_COUNT)
        self.assertEqual((meta[0], meta[-1]), (EXPECTED_FIRST, EXPECTED_LAST))
        self.assertEqual(len({name for name, _, _ in meta}), EXPECTED_COUNT)
        self.assertEqual(hashlib.sha256(repr(meta).encode()).hexdigest(), EXPECTED_SHA256)

if __name__ == '__main__':
    unittest.main()
