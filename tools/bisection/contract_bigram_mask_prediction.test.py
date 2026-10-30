#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the bigram mask and prediction cases (#1175)."""
import unittest
from unittest.mock import Mock

import contract_bigram_mask_prediction as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('issue1114-collision-load-save-pinyin',
             (),
             {'cells': ('tkrzw',), 'issues': ('1114',), 'mode': 'pinyin'}),
            ('issue1114-collision-load-save-zhuyin',
             (),
             {'cells': ('tkrzw',), 'issues': ('1114',), 'mode': 'zhuyin'}),
            ('predicted-bigram-is-user-candidate', (), {'issues': ('1231',)}),
            ('abort-choose-candidate-predicted-bigram-row', (), {'abort': 0, 'cells': ('bdb',)}),
            ('abort-mask-out-short-bigram-value', (), {'abort': False}),
            ('abort-zhuyin-mask-out-short-bigram-value', (), {'abort': False, 'mode': 'zhuyin'}),
            ('mask-out-short-bigram-value-masked-key', (), {'control': True}),
            ('abort-mask-out-non-token-bigram-key', (), {'abort': False}),
            ('abort-zhuyin-mask-out-non-token-bigram-key', (), {'abort': False, 'mode': 'zhuyin'}),
            ('abort-mask-out-residual-bigram-gram', (), {'abort': False}),
            ('abort-zhuyin-mask-out-residual-bigram-gram', (), {'abort': False, 'mode': 'zhuyin'}),
            ('abort-begin-bigram-phrases-non-token-key', (), {'abort': False}),
            ('issue677-mask-save-sequence-pinyin', (), {'issues': ('677',), 'mode': 'pinyin'}),
            ('issue677-mask-save-sequence-zhuyin', (), {'issues': ('677',), 'mode': 'zhuyin'})]
INJECTED = ('_craft_hash', 'crafted_bigram_context', 'index_file_rows')


def stub(name):
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, _craft_hash=state['_craft_hash'], crafted_bigram_context=state['crafted_bigram_context'], index_file_rows=state['index_file_rows'])
    return cases


class BigramMaskPredictionTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
