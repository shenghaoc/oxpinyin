#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the zhuyin null arguments cases (#1175)."""
import unittest

import contract_zhuyin_null_arguments as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('zhuyin-add-phrase-library-16',
             (),
             {'control': True, 'crash': {'ret': False}, 'issues': ('563', 'row:41'), 'mode': 'zhuyin'}),
            ('zhuyin-null-instance-parse-chewing',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-text-parse-chewing',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-key-parse-chewing',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-instance-in-chewing-keyboard',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-symbols-in-chewing-keyboard',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': True},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-instance-get-pinyin-string',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-key-get-zhuyin-string',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-out-get-zhuyin-string',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-rest-get-zhuyin-key-rest-positions',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-rest-get-zhuyin-key-rest-length',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-length-get-zhuyin-key-rest-length',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': True},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-candidate-get-candidate-type',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-out-get-candidate-type',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': True},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-candidate-get-candidate-string',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-out-get-candidate-string',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': True},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-instance-get-n-phrase',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-out-get-n-phrase',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': True},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-instance-get-phrase-token',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'}),
            ('zhuyin-null-out-get-phrase-token',
             (),
             {'control': True,
              'crash': {'logs': [], 'ret': False},
              'issues': ('563', 'row:41'),
              'mode': 'zhuyin'})]


def declarations():
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case)
    return cases


class ZhuyinNullArgumentsTests(unittest.TestCase):
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
