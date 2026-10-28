#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the integer limits D18 cases (#1175)."""
import unittest

import contract_integer_limits_d18 as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('d18-remember-reported', (), {'control': True, 'issues': ('540',)}),
            ('d18-remember-signed-boundary', (), {'control': True, 'issues': ('540',)}),
            ('d18-remember-multiply-715827882', (), {'control': True, 'issues': ('540',)}),
            ('d18-remember-multiply-715827883', (), {'control': True, 'issues': ('540',)}),
            ('d18-remember-multiply-1431655765', (), {'control': True, 'issues': ('540',)}),
            ('d18-remember-multiply-1431655766', (), {'control': True, 'issues': ('540',)}),
            ('remember-nul-double-pinyin', (), {'issues': ('752',), 'stderr': True}),
            ('remember-nul-chewing-pinyin', (), {'issues': ('752',), 'stderr': True}),
            ('remember-known-nul-suffix-pinyin', (), {'control': True, 'issues': ('752',), 'stderr': True}),
            ('d18-unigram-max-pinyin', (), {'control': True, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-unigram-total-boundary-pinyin', (), {'control': True, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-input-boundaries-pinyin', (), {'control': True, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-merge-total-before-pinyin', (), {'control': True, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-merge-total-zero-pinyin', (), {'control': False, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-merge-total-after-pinyin', (), {'control': False, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-merge-count-before-pinyin', (), {'control': True, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-merge-count-zero-pinyin', (), {'control': False, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-merge-count-after-pinyin', (), {'control': False, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-train-stored-zero-pinyin', (), {'control': False, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-train-seed-before-pinyin', (), {'control': True, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-train-seed-zero-pinyin', (), {'control': False, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-train-seed-after-pinyin', (), {'control': False, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-train-total-max-pinyin', (), {'control': True, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-train-total-overflow-pinyin', (), {'control': False, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-unigram-max-zhuyin', (), {'control': True, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-unigram-total-boundary-zhuyin', (), {'control': True, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-input-boundaries-zhuyin', (), {'control': True, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-merge-total-before-zhuyin', (), {'control': True, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-merge-total-zero-zhuyin', (), {'control': False, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-merge-total-after-zhuyin', (), {'control': False, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-merge-count-before-zhuyin', (), {'control': True, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-merge-count-zero-zhuyin', (), {'control': False, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-merge-count-after-zhuyin', (), {'control': False, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-train-stored-zero-zhuyin', (), {'control': False, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-train-seed-before-zhuyin', (), {'control': True, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-train-seed-zero-zhuyin', (), {'control': False, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-train-seed-after-zhuyin', (), {'control': False, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-train-total-max-zhuyin', (), {'control': True, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-train-total-overflow-zhuyin', (), {'control': False, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-predicted-total-max', (), {'control': True, 'issues': ('540',)}),
            ('d18-predicted-total-zero', (), {'control': False, 'issues': ('540',)}),
            ('d18-predicted-total-after', (), {'control': False, 'issues': ('540',)}),
            ('d18-bigram-export-3fffffff', (), {'control': True, 'issues': ('540',)}),
            ('d18-bigram-export-40000000', (), {'control': True, 'issues': ('540',)}),
            ('d18-bigram-export-7fffffff', (), {'control': True, 'issues': ('540',)}),
            ('d18-bigram-export-80000000', (), {'control': True, 'issues': ('540',)}),
            ('d18-bigram-export-80000001', (), {'control': True, 'issues': ('540',)}),
            ('d18-bigram-export-ffffffff', (), {'control': True, 'issues': ('540',)}),
            ('d18-review-user-only-zero-total-pinyin', (), {'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-review-zero-count-zero-total-default-pinyin',
             (),
             {'control': True, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-review-zero-count-zero-total-half-pinyin',
             (),
             {'control': True, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-review-zero-count-zero-total-zero-lambda-pinyin',
             (),
             {'control': True, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-review-unigram-library-two-pinyin',
             (),
             {'control': True, 'issues': ('540',), 'mode': 'pinyin'}),
            ('d18-review-user-only-zero-total-zhuyin', (), {'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-review-zero-count-zero-total-default-zhuyin',
             (),
             {'control': False, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-review-zero-count-zero-total-half-zhuyin',
             (),
             {'control': True, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-review-zero-count-zero-total-zero-lambda-zhuyin',
             (),
             {'control': True, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-review-unigram-library-two-zhuyin',
             (),
             {'control': True, 'issues': ('540',), 'mode': 'zhuyin'}),
            ('d18-review-predicted-user-only-zero-total', (), {'issues': ('540',)}),
            ('issue542-stale-user_pinyin_index.bin-pinyin',
             (),
             {'control': True, 'issues': ('542',), 'mode': 'pinyin', 'stderr': True}),
            ('issue542-stale-user_phrase_index.bin-pinyin',
             (),
             {'control': True, 'issues': ('542',), 'mode': 'pinyin', 'stderr': True}),
            ('issue542-stale-user_pinyin_index.bin-zhuyin',
             (),
             {'control': False, 'issues': ('542',), 'mode': 'zhuyin', 'stderr': True}),
            ('issue542-stale-user_phrase_index.bin-zhuyin',
             (),
             {'control': False, 'issues': ('542',), 'mode': 'zhuyin', 'stderr': True})]


def declarations():
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case)
    return cases


class IntegerLimitsD18Tests(unittest.TestCase):
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
