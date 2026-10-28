#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the final step surface cases (#1175)."""
import unittest

import contract_final_step_surface as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [("final-step-pinyin-ba'kua-20", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-pinyin-li'shi-20", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-pinyin-xi'an-20", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-pinyin-ni'hao-20", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-pinyin-ba'ku-20", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-pinyin-ni'h-20", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-pinyin-nihao'-20", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ('final-step-pinyin-n-20', (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ('final-step-pinyin-nih-20', (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ('final-step-pinyin-ni-20', (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ('final-step-pinyin-nihao-20', (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ('final-step-pinyin-zhongguo-20', (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-pinyin-ba'kua-28", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-pinyin-li'shi-28", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-pinyin-xi'an-28", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-pinyin-ni'hao-28", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-pinyin-ba'ku-28", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-pinyin-ni'h-28", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-pinyin-nihao'-28", (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ('final-step-pinyin-n-28', (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ('final-step-pinyin-nih-28', (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ('final-step-pinyin-ni-28', (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ('final-step-pinyin-nihao-28', (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ('final-step-pinyin-zhongguo-28', (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ('final-step-pinyin-import-ba-kua', (), {'control': True, 'issues': ('594',), 'mode': 'pinyin'}),
            ("final-step-zhuyin-ba'kua-20", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ("final-step-zhuyin-li'shi-20", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ("final-step-zhuyin-xi'an-20", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ("final-step-zhuyin-ni'hao-20", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ("final-step-zhuyin-ba'ku-20", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ("final-step-zhuyin-ni'h-20", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ("final-step-zhuyin-nihao'-20", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ('final-step-zhuyin-n-20', (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ('final-step-zhuyin-nih-20', (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ('final-step-zhuyin-ni-20', (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ('final-step-zhuyin-nihao-20', (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ('final-step-zhuyin-zhongguo-20', (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ("final-step-zhuyin-ba'kua-28", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ("final-step-zhuyin-li'shi-28", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ("final-step-zhuyin-xi'an-28", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ("final-step-zhuyin-ni'hao-28", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ("final-step-zhuyin-ba'ku-28", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ("final-step-zhuyin-ni'h-28", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ("final-step-zhuyin-nihao'-28", (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ('final-step-zhuyin-n-28', (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ('final-step-zhuyin-nih-28', (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ('final-step-zhuyin-ni-28', (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ('final-step-zhuyin-nihao-28', (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ('final-step-zhuyin-zhongguo-28', (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'}),
            ('final-step-zhuyin-import-ba-kua', (), {'control': True, 'issues': ('594',), 'mode': 'zhuyin'})]


def declarations():
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case)
    return cases


class FinalStepSurfaceTests(unittest.TestCase):
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
