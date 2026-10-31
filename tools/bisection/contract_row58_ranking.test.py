#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the row 58 ranking cases (#1175)."""
from pathlib import Path
import sys

import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_row58_ranking as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('row58-bigram-scoring-off', (), {'issues': ('697', 'row:58')}),
            ('row58-bigram-scoring-on', (), {'issues': ('697', 'row:58')}),
            ('row58-prefix-no-bigram-off', (), {'issues': ('697', 'row:58')}),
            ('row58-prefix-live-21-off', (), {'issues': ('697', 'row:58')}),
            ('row58-bigram-choices-off', (), {'issues': ('697', 'row:58')}),
            ('row58-conversions-off', (), {'issues': ('697', 'row:58')}),
            ('row58-lambda-ties-off', (), {'issues': ('697', 'row:58')}),
            ('row58-prefix-no-bigram-on', (), {'issues': ('697', 'row:58')}),
            ('row58-prefix-live-21-on', (), {'issues': ('697', 'row:58')}),
            ('row58-bigram-choices-on', (), {'issues': ('697', 'row:58')}),
            ('row58-conversions-on', (), {'issues': ('697', 'row:58')}),
            ('row58-lambda-ties-on', (), {'issues': ('697', 'row:58')}),
            ('row58-prefix-overflow-off', (), {'issues': ('697', 'row:58')}),
            ('row58-prefix-overflow-on', (), {'issues': ('697', 'row:58')}),
            ('row58-signed-score-off', (), {'issues': ('697', 'row:58')}),
            ('row58-signed-duplicate-off', (), {'issues': ('697', 'row:58')}),
            ('row58-unsigned-score-off', (), {'issues': ('697', 'row:58'), 'policy_record': 'arm64-pin-portability-1137.md'}),
            ('row58-signed-score-on', (), {'issues': ('697', 'row:58')}),
            ('row58-signed-duplicate-on', (), {'issues': ('697', 'row:58')}),
            ('row58-unsigned-score-on', (), {'issues': ('697', 'row:58'), 'policy_record': 'arm64-pin-portability-1137.md'}),
            ('row58-total-zero-off', (), {'sigabrt': True, 'abort': False, 'issues': ('697', 'row:58')}),
            ('row58-total-zero-punct-off', (), {'sigabrt': True, 'abort': False, 'issues': ('697', 'row:58')}),
            ('row58-total-zero-on', (), {'sigabrt': True, 'abort': False, 'issues': ('697', 'row:58')}),
            ('row58-total-zero-punct-on', (), {'sigabrt': True, 'abort': False, 'issues': ('697', 'row:58')}),
            ('row58-prefix-zero-total-off', (), {'control': True, 'issues': ('697', 'row:58')}),
            ('row58-prefix-zero-total-on', (), {'control': True, 'issues': ('697', 'row:58')}),
            ('row58-lambda-rounding-off', (), {'issues': ('697', 'row:58')}),
            ('row58-lambda-rounding-on', (), {'issues': ('697', 'row:58')}),
            ('row58-system-import-1-off', (), {'issues': ('712', '697', 'row:58')}),
            ('row58-system-import-1-on', (), {'issues': ('712', '697', 'row:58')}),
            ('row58-system-import-zhuyin-1',
             (),
             {'control': True, 'issues': ('712', '697', 'row:58'), 'mode': 'zhuyin'}),
            ('row58-system-import-2-off', (), {'issues': ('712', '697', 'row:58')}),
            ('row58-system-import-2-on', (), {'issues': ('712', '697', 'row:58')}),
            ('row58-system-import-zhuyin-2',
             (),
             {'control': True, 'issues': ('712', '697', 'row:58'), 'mode': 'zhuyin'}),
            ('row58-system-import-3-off', (), {'issues': ('712', '697', 'row:58')}),
            ('row58-system-import-3-on', (), {'issues': ('712', '697', 'row:58')}),
            ('row58-system-import-zhuyin-3',
             (),
             {'control': True, 'issues': ('712', '697', 'row:58'), 'mode': 'zhuyin'}),
            ('row58-system-import-4-off', (), {'issues': ('712', '697', 'row:58')}),
            ('row58-system-import-4-on', (), {'issues': ('712', '697', 'row:58')}),
            ('row58-system-import-zhuyin-4',
             (),
             {'control': True, 'issues': ('712', '697', 'row:58'), 'mode': 'zhuyin'}),
            ('row58-system-existing-off', (), {'control': True, 'issues': ('712', '697', 'row:58')}),
            ('row58-system-existing-on', (), {'control': True, 'issues': ('712', '697', 'row:58')}),
            ('row58-system-existing-zhuyin',
             (),
             {'control': True, 'issues': ('712', '697', 'row:58'), 'mode': 'zhuyin'})]


def declarations():
    return collect_declarations(family.register)


class Row58RankingTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations()
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

if __name__ == '__main__':
    unittest.main()
