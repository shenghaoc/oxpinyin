#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the changed parse constraints cases (#1175)."""
from pathlib import Path
import sys

import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_changed_parse_constraints as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('parse-constraints-pinyin-changed-suffix-direct', (), {'control': False, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-changed-suffix-guess', (), {'control': False, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-changed-suffix-empty', (), {'control': False, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-changed-suffix-empty-guess', (), {'control': True, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-shorter-direct', (), {'control': True, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-longer-direct', (), {'control': True, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-different-first-direct', (), {'control': True, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-span-past-end-direct', (), {'control': True, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-span-past-end-empty-guess', (), {'control': True, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-whole-span-past-end-direct', (), {'control': True, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-edit-within-span-direct', (), {'control': True, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-whole-span-kept-direct', (), {'control': False, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-two-choices-direct', (), {'control': False, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-temporary-mismatch-direct', (), {'control': False, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-temporary-mismatch-intermediate-guess',
             (),
             {'control': True, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-temporary-shorter-direct', (), {'control': False, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-pinyin-temporary-shorter-intermediate-guess',
             (),
             {'control': True, 'issues': ('710',), 'mode': 'pinyin'}),
            ('parse-constraints-zhuyin-changed-suffix-direct', (), {'control': False, 'issues': ('710',), 'mode': 'zhuyin'}),
            ('parse-constraints-zhuyin-span-past-end-direct', (), {'control': True, 'issues': ('710',), 'mode': 'zhuyin'}),
            ('parse-constraints-zhuyin-edit-within-span-direct', (), {'control': True, 'issues': ('710',), 'mode': 'zhuyin'}),
            ('parse-constraints-zhuyin-temporary-shorter-intermediate-guess',
             (),
             {'control': False, 'issues': ('710',), 'mode': 'zhuyin'})]
def declarations():
    return collect_declarations(family.register)


class ChangedParseConstraintsTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations()
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

if __name__ == '__main__':
    unittest.main()
