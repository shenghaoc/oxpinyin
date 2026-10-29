#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the stored user gram cases (#1175)."""
import unittest

import contract_stored_user_gram as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('user-gram-unsorted-train-save-pinyin', (), {'issues': ('757',), 'mode': 'pinyin'}),
            ('user-gram-unsorted-mask-save-pinyin', (), {'issues': ('757',), 'mode': 'pinyin'}),
            ('user-gram-unsorted-train-save-zhuyin', (), {'issues': ('757',), 'mode': 'zhuyin'}),
            ('user-gram-unsorted-mask-save-zhuyin', (), {'issues': ('757',), 'mode': 'zhuyin'}),
            ('user-gram-unsorted-remove-candidate-save', (), {'issues': ('757',)}),
            ('user-gram-unsorted-export', (), {'issues': ('757', 'row:1'), 'strjoinv_over_read': True}),
            ('user-gram-unsorted-predicted', (), {'issues': ('757',)}),
            ('user-gram-duplicate-train-save-pinyin', (), {'issues': ('757',), 'mode': 'pinyin'}),
            ('user-gram-duplicate-mask-save-pinyin', (), {'issues': ('757',), 'mode': 'pinyin'}),
            ('user-gram-duplicate-train-save-zhuyin', (), {'issues': ('757',), 'mode': 'zhuyin'}),
            ('user-gram-duplicate-mask-save-zhuyin', (), {'issues': ('757',), 'mode': 'zhuyin'}),
            ('user-gram-duplicate-remove-candidate-save', (), {'issues': ('757',)}),
            ('user-gram-duplicate-export', (), {'issues': ('757', 'row:1'), 'strjoinv_over_read': True}),
            ('user-gram-duplicate-predicted', (), {'issues': ('757',)}),
            ('abort-train-unsorted-set-freq-miss-pinyin',
             (),
             {'abort': False, 'issues': ('757',), 'mode': 'pinyin', 'policy_record': '1297-gram-partition-training.md'}),
            ('abort-train-unsorted-set-freq-miss-zhuyin',
             (),
             {'abort': False, 'issues': ('757',), 'mode': 'zhuyin', 'policy_record': '1297-gram-partition-training.md'}),
            ('mask-out-itemless-gram-untouched-pinyin-total-zero', (), {'issues': ('757',), 'mode': 'pinyin'}),
            ('mask-out-itemless-gram-untouched-pinyin-total-nonzero',
             (),
             {'issues': ('757',), 'mode': 'pinyin'}),
            ('mask-out-itemless-gram-untouched-zhuyin-total-zero', (), {'issues': ('757',), 'mode': 'zhuyin'}),
            ('mask-out-itemless-gram-untouched-zhuyin-total-nonzero',
             (),
             {'issues': ('757',), 'mode': 'zhuyin'}),
            ('mask-out-itemless-gram-untouched-remove-candidate-total-zero',
             (),
             {'issues': ('757',), 'mode': 'pinyin'}),
            ('mask-out-itemless-gram-untouched-remove-candidate-total-nonzero',
             (),
             {'issues': ('757',), 'mode': 'pinyin'}),
            ('itemless-user-total-phrase-segment', (), {'issues': ('757',)}),
            ('itemless-user-total-guess', (), {'control': True, 'issues': ('757',)}),
            ('itemless-user-total-guess-above-one', (), {'issues': ('791',)}),
            ('itemless-user-total-guess-above-one-control', (), {'control': True, 'issues': ('791',)})]


def declarations():
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case)
    return cases


class StoredUserGramTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations()
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

    def test_every_case_function_belongs_to_this_module(self):
        for name, _, _, fn in declarations():
            while hasattr(fn, "func"):
                fn = fn.func
            with self.subTest(name=name):
                self.assertIn(fn.__module__, (family.__name__, 'contract_case_helpers'))

    def test_replay_registers_the_same_functions(self):
        first, second = declarations(), declarations()
        self.assertEqual([c[0] for c in first], [c[0] for c in second])
        for (name, _, _, a), (_, _, _, b) in zip(first, second):
            with self.subTest(name=name):
                self.assertIs(a, b)


if __name__ == '__main__':
    unittest.main()
