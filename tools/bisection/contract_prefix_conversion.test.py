#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the prefix conversion cases (#1175)."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_prefix_conversion as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('prefix-convert/predicted-valid', (), {'control': True, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-valid', (), {'control': True, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-valid',
             (),
             {'control': True, 'crash': None, 'issues': ('1277',), 'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-valid',
             (),
             {'control': True, 'crash': None, 'issues': ('1277',), 'mode': 'zhuyin'}),
            ('prefix-convert/predicted-invalid', (), {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-invalid', (), {'control': True, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-invalid',
             (),
             {'control': True, 'crash': None, 'issues': ('1277',), 'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-invalid',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'zhuyin'}),
            ('prefix-convert/predicted-surrogate', (), {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-surrogate',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-surrogate',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-surrogate',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'zhuyin'}),
            ('prefix-convert/predicted-above-unicode',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-above-unicode',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-above-unicode',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-above-unicode',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'zhuyin'}),
            ('prefix-convert/predicted-five-byte', (), {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-five-byte',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-five-byte',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-five-byte',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'zhuyin'}),
            ('prefix-convert/predicted-six-byte', (), {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-six-byte',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-six-byte',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-six-byte',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'zhuyin'}),
            ('prefix-convert/predicted-null',
             (),
             {'control': False, 'crash': {'ret': False, 'rows': []}, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-null',
             (),
             {'control': True, 'crash': {'ret': True, 'rows': []}, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-null',
             (),
             {'control': False,
              'crash': {'ret': True, 'seeded': True, 'sentence': [True, 'e4b899']},
              'issues': ('1277',),
              'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-null',
             (),
             {'control': False,
              'crash': {'ret': True, 'seeded': True, 'sentence': [True, 'e4b899']},
              'issues': ('1277',),
              'mode': 'zhuyin'})]
INJECTED = ('items_system',)


def stub(name):
    return Mock(name=name)


def declarations(state):
    return collect_declarations(family.register, items_system=state['items_system'])


class PrefixConversionTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
