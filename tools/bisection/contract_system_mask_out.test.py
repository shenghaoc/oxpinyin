#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the system mask out cases (#1175)."""
import unittest

import contract_system_mask_out as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('mask-out-system-unsaved-pinyin', (), {'issues': ('728',), 'mode': 'pinyin'}),
            ('mask-out-system-saved-pinyin', (), {'issues': ('728',), 'mode': 'pinyin'}),
            ('abort-mask-out-surviving-add-pinyin',
             (),
             {'abort': False, 'issues': ('728',), 'mode': 'pinyin'}),
            ('mask-out-system-selected-add-pinyin',
             (),
             {'control': True, 'issues': ('728',), 'mode': 'pinyin'}),
            ('mask-out-system-headerless-add-pinyin', (), {'issues': ('728',), 'mode': 'pinyin'}),
            ('mask-out-dropped-add-full-pinyin', (), {'issues': ('728',), 'mode': 'pinyin'}),
            ('mask-out-dropped-add-headerless-pinyin', (), {'issues': ('728',), 'mode': 'pinyin'}),
            ('mask-out-system-unsaved-zhuyin', (), {'issues': ('728',), 'mode': 'zhuyin'}),
            ('mask-out-system-saved-zhuyin', (), {'issues': ('728',), 'mode': 'zhuyin'}),
            ('abort-mask-out-surviving-add-zhuyin',
             (),
             {'abort': False, 'issues': ('728',), 'mode': 'zhuyin'}),
            ('mask-out-system-selected-add-zhuyin',
             (),
             {'control': True, 'issues': ('728',), 'mode': 'zhuyin'}),
            ('mask-out-system-headerless-add-zhuyin', (), {'issues': ('728',), 'mode': 'zhuyin'}),
            ('mask-out-dropped-add-full-zhuyin', (), {'issues': ('728',), 'mode': 'zhuyin'}),
            ('mask-out-dropped-add-headerless-zhuyin', (), {'issues': ('728',), 'mode': 'zhuyin'}),
            ('mask-out-system-modify-pinyin', (), {'issues': ('728',), 'mode': 'pinyin'}),
            ('mask-out-system-no-header-pinyin', (), {'control': True, 'issues': ('728',), 'mode': 'pinyin'}),
            ('mask-out-system-disk-replacement-pinyin', (), {'issues': ('728',), 'mode': 'pinyin'}),
            ('mask-out-system-modify-zhuyin', (), {'issues': ('728',), 'mode': 'zhuyin'}),
            ('mask-out-system-no-header-zhuyin', (), {'control': True, 'issues': ('728',), 'mode': 'zhuyin'}),
            ('mask-out-system-disk-replacement-zhuyin', (), {'issues': ('728',), 'mode': 'zhuyin'})]


def declarations():
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case)
    return cases


class SystemMaskOutTests(unittest.TestCase):
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
