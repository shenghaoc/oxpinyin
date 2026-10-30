#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the bigram removal cases (#1175)."""
from pathlib import Path
import sys

import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_bigram_removal as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('removal-bigram-reported-mask-out-pinyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-underflow-mask-out-pinyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-independent-total-mask-out-pinyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-remove-zero-mask-out-pinyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-zero-survivors-mask-out-pinyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-wrap-after-mask-out-pinyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-ordinary-mask-out-pinyin',
             (),
             {'control': True, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-multiple-pinyin', (), {'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-reported-remove-candidate-pinyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-underflow-remove-candidate-pinyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-independent-total-remove-candidate-pinyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-remove-zero-remove-candidate-pinyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-zero-survivors-remove-candidate-pinyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-wrap-after-remove-candidate-pinyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-ordinary-remove-candidate-pinyin',
             (),
             {'control': True, 'issues': ('740',), 'mode': 'pinyin'}),
            ('removal-bigram-reported-mask-out-zhuyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'zhuyin'}),
            ('removal-bigram-underflow-mask-out-zhuyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'zhuyin'}),
            ('removal-bigram-independent-total-mask-out-zhuyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'zhuyin'}),
            ('removal-bigram-remove-zero-mask-out-zhuyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'zhuyin'}),
            ('removal-bigram-zero-survivors-mask-out-zhuyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'zhuyin'}),
            ('removal-bigram-wrap-after-mask-out-zhuyin',
             (),
             {'control': False, 'issues': ('740',), 'mode': 'zhuyin'}),
            ('removal-bigram-ordinary-mask-out-zhuyin',
             (),
             {'control': True, 'issues': ('740',), 'mode': 'zhuyin'}),
            ('removal-bigram-multiple-zhuyin', (), {'issues': ('740',), 'mode': 'zhuyin'})]


def declarations():
    return collect_declarations(family.register)


class BigramRemovalTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations()
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

if __name__ == '__main__':
    unittest.main()
