#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the sort and pinyin length cases (#1175)."""
import collections
import unittest
from unittest.mock import Mock

import contract_sort_pinyin_length as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('glib-sort-predicted', (), {'control': True, 'issues': ('927',)}),
            ('glib-sort-tails-pinyin', (), {'control': True, 'stderr': True, 'issues': ('921',), 'mode': 'pinyin'}),
            ('glib-sort-candidates-pinyin', (), {'control': True, 'issues': ('927',), 'mode': 'pinyin'}),
            ('glib-sort-tails-zhuyin', (), {'control': True, 'stderr': True, 'issues': ('921',), 'mode': 'zhuyin'}),
            ('glib-sort-candidates-zhuyin', (), {'control': True, 'issues': ('927',), 'mode': 'zhuyin'}),
            ('trellis-tail-sort/qian-on', (), {'control': True, 'stderr': True, 'issues': ('1347', '921')}),
            ('trellis-tail-sort/qian-off', (), {'control': True, 'stderr': True, 'issues': ('1347', '921')}),
            ('trellis-adjusted-score/resplit-jianai-on', (), {'control': True, 'issues': ('1347',)}),
            ('trellis-adjusted-score/resplit-jianai-off', (), {'control': True, 'issues': ('1347',)}),
            ('trellis-adjusted-score/divided-bian-on', (), {'control': True, 'issues': ('1347',)}),
            ('trellis-adjusted-score/divided-bian-off', (), {'control': True, 'issues': ('1347',)}),
            ('pinyin-length-wrap/span-5', (), {'control': True, 'issues': ('1275',)}),
            ('pinyin-length-wrap/span-15', (), {'control': True, 'issues': ('1275',)}),
            ('pinyin-length-wrap/span-255', (), {'control': True, 'issues': ('1275',)}),
            ('pinyin-length-wrap/span-256', (), {'control': False, 'issues': ('1275',)}),
            ('pinyin-length-wrap/span-257', (), {'control': False, 'issues': ('1275',)}),
            ('pinyin-length-wrap/span-512', (), {'control': False, 'issues': ('1275',)})]
INJECTED = ('CASES',)


def stub(name):
    return collections.defaultdict(dict) if name == 'CASES' else Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, CASES=state['CASES'])
    return cases


class SortPinyinLengthTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
