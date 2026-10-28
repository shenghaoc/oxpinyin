#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the separator and before-cursor cases (#1175)."""
import hashlib
import unittest

import contract_separator_before_cursor as family


# (case ID, positional options, keyword options) in registration order: the count, the
# first and last entries, and the SHA-256 of the repr of the whole ordered list.
EXPECTED_COUNT = 1360
EXPECTED_FIRST = ('separator/pinyin/0/parse/0', (), {'mode': 'pinyin', 'control': True, 'issues': ('640',)})
EXPECTED_LAST = ("allocator-zero/zhuyin/xi1'an1/key/4", (), {'mode': 'zhuyin', 'abort': False, 'issues': ('681',)})
EXPECTED_SHA256 = '7c54dcf8bd614532e8f65e86e0e45bf170c6133ff5f06fd319a68feacff691ff'


def declarations():
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case)
    return cases


class SeparatorBeforeCursorTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        meta = [(name, args, kwargs) for name, args, kwargs, _ in declarations()]
        self.assertEqual(len(meta), EXPECTED_COUNT)
        self.assertEqual((meta[0], meta[-1]), (EXPECTED_FIRST, EXPECTED_LAST))
        self.assertEqual(len({name for name, _, _ in meta}), EXPECTED_COUNT)
        self.assertEqual(hashlib.sha256(repr(meta).encode()).hexdigest(), EXPECTED_SHA256)

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
