#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the user key spelling cases (#1418)."""
import unittest

import contract_user_key_spellings as family

EXPECTED = [
    ('user-key-spelling/pinyin-egn', (), {'issues': ('1418',), 'mode': 'pinyin'}),
    ('user-key-spelling/pinyin-emg', (), {'issues': ('1418',), 'mode': 'pinyin'}),
    ('user-key-spelling/pinyin-nuen', (), {'issues': ('1418',), 'mode': 'pinyin'}),
    ('user-key-spelling/zhuyin-den', (), {'issues': ('1418',), 'mode': 'zhuyin'}),
    ('user-key-spelling/zhuyin-den-tone4', (), {'issues': ('1418',), 'mode': 'zhuyin'}),
    ('user-key-spelling/zhuyin-eng', (), {'issues': ('1418',), 'mode': 'zhuyin'}),
]


def declarations():
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case)
    return cases


class UserKeySpellingTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        self.assertEqual([(n, a, k) for n, a, k, _ in declarations()], EXPECTED)

    def test_every_case_runs_on_every_cell(self):
        for name, _, kwargs, _ in declarations():
            with self.subTest(name=name):
                self.assertNotIn('cells', kwargs)

    def test_every_reading_stores_the_key_the_pin_writes(self):
        for tag, mode, _, reading, typed, spelling in family.READINGS:
            with self.subTest(tag=tag):
                self.assertIn(spelling, ('eng', 'nun', 'den'))
                self.assertEqual(typed, reading.encode())
                self.assertEqual(mode, 'zhuyin' if any(ord(c) > 0x3000 for c in reading) else 'pinyin')


if __name__ == '__main__':
    unittest.main()
