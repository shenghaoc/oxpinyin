#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the toned keys and zhuyin refusals cases (#1175)."""
from pathlib import Path
import sys

import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_toned_keys_zhuyin_refusals as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('abort-is-incomplete-toned-initial', (), {'sigabrt': True, 'abort': False, 'issues': ('525', 'row:4')}),
            ('abort-guess-candidates-toned-initial-before-offset',
             (),
             {'sigabrt': True, 'abort': False, 'issues': ('525', 'row:4', '662')}),
            ('abort-guess-candidates-toned-initial', (), {'sigabrt': True, 'abort': False, 'issues': ('525', 'row:4')}),
            ('abort-guess-sentence-toned-initial', (), {'sigabrt': True, 'abort': False, 'issues': ('525', 'row:4')}),
            ('abort-guess-sentence-toned-initial-after-key', (), {'sigabrt': True, 'abort': False, 'issues': ('525', 'row:4')}),
            ('toned-initial-neighbours', (), {'control': True, 'issues': ('525', 'row:4')}),
            ('abort-zhuyin-get-left-zhuyin-offset-past-matrix', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'issues': ('525',)}),
            ('abort-zhuyin-get-right-zhuyin-offset-past-matrix', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'issues': ('525',)}),
            ('abort-zhuyin-get-right-zhuyin-offset-empty-matrix', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'issues': ('525',)}),
            ('abort-zhuyin-guess-candidates-after-cursor-past-matrix', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'issues': ('525',)}),
            ('abort-zhuyin-guess-candidates-before-cursor-past-matrix', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'issues': ('525',)}),
            ('abort-zhuyin-guess-candidates-after-cursor-reserved-slot-plus-one', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'issues': ('525',)}),
            ('abort-zhuyin-guess-candidates-before-cursor-reserved-slot-plus-one', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'issues': ('525',)}),
            ('abort-zhuyin-get-character-offset-past-matrix', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'issues': ('525',)}),
            ('abort-zhuyin-get-zhuyin-key-empty-matrix', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'issues': ('525',)}),
            ('abort-zhuyin-get-zhuyin-key-rest-empty-matrix', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'issues': ('525',)}),
            ('zhuyin-total-zero-ranks-nothing-after-cursor',
             (),
             {'control': True, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('zhuyin-total-zero-ranks-nothing-before-cursor',
             (),
             {'control': True, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-parse-full-pinyin-apostrophe', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'issues': ('525',)}),
            ('abort-zhuyin-guess-candidates-total-zero', (), {'mode': 'zhuyin', 'abort': False, 'sigabrt': True, 'issues': ('525',)}),
            ('zhuyin-cursor-neighbours', (), {'control': True, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('zhuyin-scheme-and-library-neighbours',
             (),
             {'control': True, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-get-right-pinyin-offset-empty-matrix', (), {'sigabrt': True, 'abort': False, 'issues': ('525',)}),
            ('abort-guess-candidates-past-matrix', (), {'sigabrt': True, 'abort': False, 'issues': ('525',)})]


def declarations():
    return collect_declarations(family.register, family.register_2)


class TonedKeysZhuyinRefusalsTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations()
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

if __name__ == '__main__':
    unittest.main()
