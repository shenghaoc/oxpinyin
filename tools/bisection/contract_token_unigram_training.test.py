#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the token unigram and training cases (#1175)."""
from pathlib import Path
import sys

import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_token_unigram_training as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('system-zhuyin-only-readings', (), {'issues': ('809',), 'mode': 'pinyin'}),
            ('user-zhuyin-only-readings', (), {'control': True, 'issues': ('809',), 'mode': 'pinyin'}),
            ('zhuyin-system-zhuyin-only-readings', (), {'issues': ('809',), 'mode': 'zhuyin'}),
            ('zhuyin-user-zhuyin-only-readings', (), {'control': True, 'issues': ('809',), 'mode': 'zhuyin'}),
            ('token-unigram-system', (), {'control': True, 'issues': ('808',), 'mode': 'pinyin'}),
            ('token-unigram-user', (), {'control': True, 'issues': ('808',), 'mode': 'pinyin'}),
            ('token-unigram-trained', (), {'control': True, 'issues': ('808',), 'mode': 'pinyin'}),
            ('token-unigram-user-libraries', (), {'control': False, 'issues': ('808',), 'mode': 'pinyin'}),
            ('zhuyin-token-unigram-system', (), {'control': False, 'issues': ('808',), 'mode': 'zhuyin'}),
            ('zhuyin-token-unigram-user', (), {'control': False, 'issues': ('808',), 'mode': 'zhuyin'}),
            ('zhuyin-token-unigram-trained', (), {'control': False, 'issues': ('808',), 'mode': 'zhuyin'}),
            ('zhuyin-token-unigram-user-libraries',
             (),
             {'control': True, 'issues': ('808',), 'mode': 'zhuyin'}),
            ('train-unigram-total', (), {'issues': ('540',)}),
            ('predicted-unigram-overflow', (), {'issues': ('540',)}),
            ('predicted-prefix-user-phrase-unigram', (), {'issues': ('1418',)})]


def declarations():
    return collect_declarations(family.register)


class TokenUnigramTrainingTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations()
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

if __name__ == '__main__':
    unittest.main()
