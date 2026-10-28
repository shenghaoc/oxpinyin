#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the gLib path construction cases (#1175)."""
import unittest

import contract_glib_path_construction as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('init-system-empty-pinyin',
             (),
             {'control': False, 'issues': ('643',), 'mode': 'pinyin', 'stderr': True}),
            ('init-system-empty-missing-pinyin',
             (),
             {'control': True, 'issues': ('643',), 'mode': 'pinyin', 'stderr': True}),
            ('init-system-null-pinyin',
             (),
             {'control': False, 'issues': ('643',), 'mode': 'pinyin', 'stderr': True}),
            ('init-system-dot-pinyin',
             (),
             {'control': True, 'issues': ('643',), 'mode': 'pinyin', 'stderr': True}),
            ('init-system-absolute-pinyin',
             (),
             {'control': True, 'issues': ('643',), 'mode': 'pinyin', 'stderr': True}),
            ('init-system-empty-zhuyin',
             (),
             {'control': False, 'issues': ('643',), 'mode': 'zhuyin', 'stderr': True}),
            ('init-system-empty-missing-zhuyin',
             (),
             {'control': True, 'issues': ('643',), 'mode': 'zhuyin', 'stderr': True}),
            ('init-system-null-zhuyin',
             (),
             {'control': False, 'issues': ('643',), 'mode': 'zhuyin', 'stderr': True}),
            ('init-system-dot-zhuyin',
             (),
             {'control': True, 'issues': ('643',), 'mode': 'zhuyin', 'stderr': True}),
            ('init-system-absolute-zhuyin',
             (),
             {'control': True, 'issues': ('643',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-dir-system-dup-trailing-pinyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-dir-both-leading-dup-pinyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-dir-both-dot-segments-pinyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-dir-both-dotdot-pinyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-dir-user-dup-trailing-pinyin',
             (),
             {'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-dir-system-dup-trailing-zhuyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-dir-both-leading-dup-zhuyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-dir-both-dot-segments-zhuyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-dir-both-dotdot-zhuyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-dir-user-dup-trailing-zhuyin',
             (),
             {'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-save-write-failed-dup-trailing-pinyin',
             (),
             {'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-save-write-failed-dup-trailing-zhuyin',
             (),
             {'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-fini-write-failed-dup-trailing-pinyin', (), {'issues': ('922',), 'stderr': True}),
            ('glib-922-row-leading-slash-pinyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-row-double-leading-slash-pinyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-row-nested-pinyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-row-dotdot-pinyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-row-missing-leading-slash-pinyin',
             (),
             {'control': True, 'crash': {'ctx': True, 'logs': []}, 'issues': ('922',), 'mode': 'pinyin'}),
            ('glib-922-row-missing-double-leading-slash-pinyin',
             (),
             {'control': False, 'crash': {'ctx': True, 'logs': []}, 'issues': ('922',), 'mode': 'pinyin'}),
            ('glib-922-row-missing-nested-pinyin',
             (),
             {'control': False, 'crash': {'ctx': True, 'logs': []}, 'issues': ('922',), 'mode': 'pinyin'}),
            ('glib-922-row-missing-dup-trailing-dir-pinyin',
             (),
             {'control': False, 'crash': {'ctx': True, 'logs': []}, 'issues': ('922',), 'mode': 'pinyin'}),
            ('glib-922-row-leading-slash-zhuyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-row-double-leading-slash-zhuyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-row-nested-zhuyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-row-dotdot-zhuyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-row-missing-leading-slash-zhuyin',
             (),
             {'control': True, 'crash': {'ctx': True, 'logs': []}, 'issues': ('922',), 'mode': 'zhuyin'}),
            ('glib-922-row-missing-double-leading-slash-zhuyin',
             (),
             {'control': False, 'crash': {'ctx': True, 'logs': []}, 'issues': ('922',), 'mode': 'zhuyin'}),
            ('glib-922-row-missing-nested-zhuyin',
             (),
             {'control': False, 'crash': {'ctx': True, 'logs': []}, 'issues': ('922',), 'mode': 'zhuyin'}),
            ('glib-922-row-missing-dup-trailing-dir-zhuyin',
             (),
             {'control': False, 'crash': {'ctx': True, 'logs': []}, 'issues': ('922',), 'mode': 'zhuyin'}),
            ('glib-922-table-conf-missing-slash-pinyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-table-conf-missing-slashes-pinyin',
             (),
             {'control': False, 'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-table-conf-missing-many-slashes-pinyin',
             (),
             {'control': False, 'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-table-conf-missing-missing-dup-trailing-pinyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-table-conf-rejected-dup-trailing-pinyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'pinyin', 'stderr': True}),
            ('glib-922-table-conf-missing-slash-zhuyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-table-conf-missing-slashes-zhuyin',
             (),
             {'control': False, 'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-table-conf-missing-many-slashes-zhuyin',
             (),
             {'control': False, 'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-table-conf-missing-missing-dup-trailing-zhuyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'zhuyin', 'stderr': True}),
            ('glib-922-table-conf-rejected-dup-trailing-zhuyin',
             (),
             {'control': True, 'issues': ('922',), 'mode': 'zhuyin', 'stderr': True})]


def declarations():
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case)
    return cases


class GlibPathConstructionTests(unittest.TestCase):
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
