#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the user configuration and overlong index cases (#1175)."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_user_conf_overlong_index as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('user-conf-920-nul-database-token',
             (),
             {'control': False, 'issues': ('920',), 'mode': 'pinyin'}),
            ('user-conf-920-nul-database-token-zhuyin',
             (),
             {'control': False, 'issues': ('920',), 'mode': 'zhuyin'}),
            ('user-conf-920-version-overflow-conforms',
             (),
             {'control': True,
              'issues': ('920',),
              'mode': 'pinyin'}),
            ('user-conf-920-version-overflow-conforms-zhuyin',
             (),
             {'control': True,
              'issues': ('920',),
              'mode': 'zhuyin'}),
            ('user-conf-920-version-overflow-nonconforms',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'pinyin'}),
            ('user-conf-920-version-overflow-nonconforms-zhuyin',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'zhuyin'}),
            ('user-conf-920-counter-overflow-conforms',
             (),
             {'control': True,
              'issues': ('920',),
              'mode': 'pinyin'}),
            ('user-conf-920-counter-overflow-conforms-zhuyin',
             (),
             {'control': True,
              'issues': ('920',),
              'mode': 'zhuyin'}),
            ('user-conf-920-counter-overflow-nonconforms',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'pinyin'}),
            ('user-conf-920-counter-overflow-nonconforms-zhuyin',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'zhuyin'}),
            ('user-conf-920-negative-overflow-nonconforms',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'pinyin'}),
            ('user-conf-920-negative-overflow-nonconforms-zhuyin',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'zhuyin'}),
            ('user-conf-920-database-missing-at-eof',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'pinyin'}),
            ('user-conf-920-database-missing-at-eof-zhuyin',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'zhuyin'}),
            ('user-conf-920-counter-missing',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'pinyin'}),
            ('user-conf-920-counter-missing-zhuyin',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'zhuyin'}),
            ('user-conf-marker-unwritable', (), {'issues': ('920',), 'mode': 'pinyin', 'stderr': True}),
            ('user-conf-marker-unwritable-zhuyin', (), {'issues': ('920',), 'mode': 'zhuyin', 'stderr': True}),
            ('abort-mask-out-overlong-index-key', (), {'abort': False, 'issues': ('525', 'row:84')}),
            ('abort-zhuyin-mask-out-overlong-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84'), 'mode': 'zhuyin'}),
            ('abort-guess-candidates-overlong-index-key', (), {'abort': False, 'issues': ('525', 'row:84')}),
            ('abort-guess-candidates-overlong-odd-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84')}),
            ('abort-mask-out-overlong-odd-index-key', (), {'abort': False, 'issues': ('525', 'row:84')}),
            ('abort-mask-out-empty-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84'), 'userdir': True}),
            ('abort-mask-out-one-byte-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84'), 'userdir': True}),
            ('abort-zhuyin-mask-out-empty-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84'), 'mode': 'zhuyin', 'userdir': True}),
            ('abort-guess-candidates-overlong-system-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84'), 'userdir': True}),
            ('abort-guess-candidates-overlong-odd-system-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84'), 'userdir': True}),
            ('guess-after-overlong-index-key-without-prefix-answers-true', (), {'issues': ('525', 'row:84')}),
            ('reparse-after-guess-ignores-overlong-index-key', (), {'issues': ('525', 'row:84')}),
            ('abort-guess-candidates-overlong-incomplete-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84')})]
INJECTED = ('_craft_db',)


def stub(name):
    return Mock(name=name)


def declarations(state):
    return collect_declarations(family.register, _craft_db=state['_craft_db'])


class UserConfOverlongIndexTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
