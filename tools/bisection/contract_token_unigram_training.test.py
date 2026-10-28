#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the token unigram and training cases (#1175)."""
import unittest

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
            ('predicted-unigram-overflow', (), {'issues': ('540',)})]


def declarations():
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case)
    return cases


class TokenUnigramTrainingTests(unittest.TestCase):
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
