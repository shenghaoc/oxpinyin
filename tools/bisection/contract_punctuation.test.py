#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the punctuation cases (#1175)."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_punctuation as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('punct-decode/valid', (), {'control': True, 'issues': ('1144',), 'policy_record': None}),
            ('punct-decode/zero-length', (), {'control': True, 'issues': ('1144',), 'policy_record': None}),
            ('punct-decode/surrogate', (), {'control': False, 'issues': ('1144',), 'policy_record': None}),
            ('punct-decode/above-unicode', (), {'control': False, 'issues': ('1144',), 'policy_record': None}),
            ('punct-decode/unterminated', (), {'control': False, 'issues': ('1144',), 'policy_record': None}),
            ('punct-decode/unterminated-zero',
             (),
             {'control': False, 'issues': ('1144',), 'policy_record': None}),
            ('punct-decode/misaligned-terminated',
             (),
             {'control': False, 'issues': ('1144',), 'policy_record': '1297-punct-words.md'}),
            ('punct-decode/misaligned-unterminated',
             (),
             {'control': False, 'issues': ('1144',), 'policy_record': None}),
            ('punct-decode/misaligned-short',
             (),
             {'control': False, 'issues': ('1144',), 'policy_record': '1297-punct-words.md'}),
            ('punct-decode/empty', (), {'control': False, 'issues': ('1144',), 'policy_record': None}),
            ('punct-decode/empty-leading', (), {'control': False, 'issues': ('1144',), 'policy_record': None}),
            ('punct-dedup/distinct', (), {'control': True, 'issues': ('928',)}),
            ('punct-dedup/within', (), {'control': False, 'issues': ('928',)}),
            ('punct-dedup/across-prefixes', (), {'control': False, 'issues': ('928',)}),
            ('punct-dedup/empty', (), {'control': False, 'issues': ('928',)}),
            ('punct-dedup/empty-forms', (), {'control': False, 'issues': ('928',)}),
            ('punct-dedup/non-scalar', (), {'control': False, 'issues': ('928',)}),
            ('punct-dedup/candidate', (), {'control': True, 'issues': ('928',)}),
            ('punct-alias/borrowed-candidates',
             (),
             {'issues': ('1272', '542'),
              'pin_not_run': {'candidates': 0, 'kinds': [], 'ret': True, 'sentences kept': True}}),
            ('punct-alias/owned-candidates', (), {'control': True, 'issues': ('1272',)}),
            ('punct-alias/borrowed-predicted',
             (),
             {'issues': ('1272', '542'),
              'pin_not_run': {'candidates': 0, 'kinds': [], 'ret': True, 'sentences kept': True}}),
            ('punct-alias/owned-predicted', (), {'control': True, 'issues': ('1272',)})]
INJECTED = ('_craft_db',)


def stub(name):
    return Mock(name=name)


def declarations(state):
    return collect_declarations(family.register, _craft_db=state['_craft_db'])


class PunctuationTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
