#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the phrase-index logger cases (#1175)."""
import unittest

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
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case)
    return cases


class PhraseIndexLoggerTests(unittest.TestCase):
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
