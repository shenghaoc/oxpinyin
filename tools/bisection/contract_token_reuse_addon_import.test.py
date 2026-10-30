#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the token reuse and addon import cases (#1175)."""
import unittest
from unittest.mock import Mock

import contract_token_reuse_addon_import as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('issue762-token-reuse-no-mask-pinyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-all-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-highest-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-middle-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-twice-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-import-mask-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-reopen-pinyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-unsaved-reopen-pinyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-reopen-middle-pinyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-stale-bigram-pinyin',
             (),
             {'cells': ('bdb',), 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-stale-bigram-no-mask-pinyin',
             (),
             {'cells': ('bdb',), 'control': True, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-no-mask-zhuyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-all-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-highest-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-middle-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-twice-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-import-mask-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-reopen-zhuyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-unsaved-reopen-zhuyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-reopen-middle-zhuyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-stale-bigram-zhuyin',
             (),
             {'cells': ('bdb',), 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-stale-bigram-no-mask-zhuyin',
             (),
             {'cells': ('bdb',), 'control': True, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-addon-import-no-mask-pinyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-addon-import-mask-highest-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-addon-import-mask-middle-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-addon-import-no-mask-zhuyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-addon-import-mask-highest-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-addon-import-mask-middle-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('addon-promoted-listing-unsorted', (), {'issues': ('913',), 'mode': 'pinyin'}),
            ('addon-promoted-listing-by-frequency',
             (),
             {'control': True, 'issues': ('913',), 'mode': 'pinyin'})]
INJECTED = ('token_reuse_sequence',)


def stub(name):
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, token_reuse_sequence=state['token_reuse_sequence'])
    return cases


class TokenReuseAddonImportTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
