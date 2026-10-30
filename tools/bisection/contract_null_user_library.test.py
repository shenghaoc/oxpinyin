#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the null user and library operations cases (#1175)."""
from pathlib import Path
import sys

import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_null_user_library as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('null-user-session-pinyin', (), {'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('null-user-session-zhuyin', (), {'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-1-ordinary-pinyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-1-null-pinyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-2-ordinary-pinyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-2-null-pinyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-3-ordinary-pinyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-3-null-pinyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-4-ordinary-pinyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-4-null-pinyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-5-ordinary-pinyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-5-null-pinyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-6-ordinary-pinyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-6-null-pinyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-7-ordinary-pinyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-7-null-pinyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'pinyin', 'stderr': True}),
            ('user-library-1-ordinary-zhuyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-1-null-zhuyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-2-ordinary-zhuyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-2-null-zhuyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-3-ordinary-zhuyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-3-null-zhuyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-4-ordinary-zhuyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-4-null-zhuyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-5-ordinary-zhuyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-5-null-zhuyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-6-ordinary-zhuyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-6-null-zhuyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-7-ordinary-zhuyin',
             (),
             {'control': True, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True}),
            ('user-library-7-null-zhuyin',
             (),
             {'control': False, 'issues': ('642',), 'mode': 'zhuyin', 'stderr': True})]


def declarations():
    return collect_declarations(family.register)


class NullUserLibraryTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations()
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

if __name__ == '__main__':
    unittest.main()
