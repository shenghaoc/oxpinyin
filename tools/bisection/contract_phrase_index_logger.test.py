#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the phrase-index logger cases (#1175)."""
from pathlib import Path
import sys

import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_phrase_index_logger as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('abort-init-log-header-token-pinyin', (), {'abort': False, 'issues': ('525',), 'mode': 'pinyin'}),
            ('abort-mask-out-log-header-token-pinyin',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'pinyin'}),
            ('abort-mask-out-multiple-log-headers-pinyin',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'pinyin'}),
            ('abort-mask-out-multiple-log-headers-bad-tail-pinyin',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'pinyin'}),
            ('abort-mask-out-log-header-untouched-pinyin',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'pinyin'}),
            ('mask-out-unloaded-library-log-pinyin',
             (),
             {'control': True, 'issues': ('525',), 'mode': 'pinyin'}),
            ('init-multiple-log-headers-pinyin', (), {'control': True, 'issues': ('525',), 'mode': 'pinyin'}),
            ('mask-out-log-header-bad-tail-pinyin',
             (),
             {'control': True, 'issues': ('525',), 'mode': 'pinyin'}),
            ('abort-init-log-header-token-zhuyin', (), {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-mask-out-log-header-token-zhuyin',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-mask-out-multiple-log-headers-zhuyin',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-mask-out-multiple-log-headers-bad-tail-zhuyin',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-mask-out-log-header-untouched-zhuyin',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('mask-out-unloaded-library-log-zhuyin',
             (),
             {'control': True, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('init-multiple-log-headers-zhuyin', (), {'control': True, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('mask-out-log-header-bad-tail-zhuyin',
             (),
             {'control': True, 'issues': ('525',), 'mode': 'zhuyin'})]


def declarations():
    return collect_declarations(family.register)


class PhraseIndexLoggerTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations()
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

if __name__ == '__main__':
    unittest.main()
