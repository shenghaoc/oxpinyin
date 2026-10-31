#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the bigram mask and prediction cases (#1175)."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_bigram_mask_prediction as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('issue1114-collision-load-save-pinyin',
             (),
             {'cells': ('tkrzw',), 'issues': ('1114',), 'mode': 'pinyin'}),
            ('issue1114-collision-load-save-zhuyin',
             (),
             {'cells': ('tkrzw',), 'issues': ('1114',), 'mode': 'zhuyin'}),
            ('predicted-bigram-is-user-candidate', (), {'issues': ('1231',)}),
            ('abort-choose-candidate-predicted-bigram-row', (), {'sigabrt': True, 'abort': 0, 'cells': ('bdb',)}),
            ('abort-mask-out-short-bigram-value', (), {'abort': False}),
            ('abort-zhuyin-mask-out-short-bigram-value', (), {'abort': False, 'mode': 'zhuyin'}),
            ('mask-out-short-bigram-value-masked-key', (), {'control': True}),
            ('abort-mask-out-non-token-bigram-key', (), {'sigabrt': True, 'abort': False}),
            ('abort-zhuyin-mask-out-non-token-bigram-key', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True}),
            ('abort-mask-out-residual-bigram-gram', (), {'sigabrt': True, 'abort': False}),
            ('abort-zhuyin-mask-out-residual-bigram-gram', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True}),
            ('abort-begin-bigram-phrases-non-token-key', (), {'sigabrt': True, 'abort': False}),
            ('issue677-mask-save-sequence-pinyin', (), {'issues': ('677',), 'mode': 'pinyin'}),
            ('issue677-mask-save-sequence-zhuyin', (), {'issues': ('677',), 'mode': 'zhuyin'})]
INJECTED = ('_craft_hash', 'crafted_bigram_context', 'index_file_rows')


def stub(name):
    return Mock(name=name)


def declarations(state):
    return collect_declarations(family.register, _craft_hash=state['_craft_hash'], crafted_bigram_context=state['crafted_bigram_context'], index_file_rows=state['index_file_rows'])


class BigramMaskPredictionTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
