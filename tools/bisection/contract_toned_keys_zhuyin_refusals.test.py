#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the toned keys and zhuyin refusals cases (#1175)."""
import unittest

import contract_toned_keys_zhuyin_refusals as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('abort-is-incomplete-toned-initial', (), {'abort': False, 'issues': ('525', 'row:4')}),
            ('abort-guess-candidates-toned-initial-before-offset',
             (),
             {'abort': False, 'issues': ('525', 'row:4', '662')}),
            ('abort-guess-candidates-toned-initial', (), {'abort': False, 'issues': ('525', 'row:4')}),
            ('abort-guess-sentence-toned-initial', (), {'abort': False, 'issues': ('525', 'row:4')}),
            ('abort-guess-sentence-toned-initial-after-key', (), {'abort': False, 'issues': ('525', 'row:4')}),
            ('toned-initial-neighbours', (), {'control': True, 'issues': ('525', 'row:4')}),
            ('abort-zhuyin-get-left-zhuyin-offset-past-matrix',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-get-right-zhuyin-offset-past-matrix',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-get-right-zhuyin-offset-empty-matrix',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-guess-candidates-after-cursor-past-matrix',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-guess-candidates-before-cursor-past-matrix',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-guess-candidates-after-cursor-reserved-slot-plus-one',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-guess-candidates-before-cursor-reserved-slot-plus-one',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-get-character-offset-past-matrix',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-get-zhuyin-key-empty-matrix',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-get-zhuyin-key-rest-empty-matrix',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('zhuyin-total-zero-ranks-nothing-after-cursor',
             (),
             {'control': True, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('zhuyin-total-zero-ranks-nothing-before-cursor',
             (),
             {'control': True, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-parse-full-pinyin-apostrophe',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-guess-candidates-total-zero',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('zhuyin-cursor-neighbours', (), {'control': True, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('zhuyin-scheme-and-library-neighbours',
             (),
             {'control': True, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-get-right-pinyin-offset-empty-matrix', (), {'abort': False, 'issues': ('525',)}),
            ('abort-guess-candidates-past-matrix', (), {'abort': False, 'issues': ('525',)})]


def declarations():
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case)
    family.register_2(case)
    return cases


class TonedKeysZhuyinRefusalsTests(unittest.TestCase):
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
