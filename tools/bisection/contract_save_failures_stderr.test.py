#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the save failures and stderr cases (#1175)."""
from pathlib import Path
import sys

import collections
import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_save_failures_stderr as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('stderr-save-one-tmp-blocked', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-save-one-final-blocked', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-save-one-tmp-blocked-zhuyin', (), {'issues': ('545',), 'mode': 'zhuyin', 'stderr': True}),
            ('save-929-rename-user.bin-pinyin', (), {'issues': ('929',), 'mode': 'pinyin', 'stderr': True}),
            ('save-929-rename-user_pinyin_index.bin-pinyin',
             (),
             {'issues': ('929',), 'mode': 'pinyin', 'stderr': True}),
            ('save-929-rename-user_phrase_index.bin-pinyin',
             (),
             {'issues': ('929',), 'mode': 'pinyin', 'stderr': True}),
            ('save-929-rename-user_bigram.db-pinyin',
             (),
             {'issues': ('929',), 'mode': 'pinyin', 'stderr': True}),
            ('save-929-reload-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-sync-rejecting-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-rename-user.bin-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin', 'stderr': True}),
            ('save-929-rename-user_pinyin_index.bin-zhuyin',
             (),
             {'issues': ('929',), 'mode': 'zhuyin', 'stderr': True}),
            ('save-929-rename-user_phrase_index.bin-zhuyin',
             (),
             {'issues': ('929',), 'mode': 'zhuyin', 'stderr': True}),
            ('save-929-rename-user_bigram.db-zhuyin',
             (),
             {'issues': ('929',), 'mode': 'zhuyin', 'stderr': True}),
            ('save-929-reload-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin'}),
            ('save-929-sync-rejecting-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin'}),
            ('save-929-boundary-save-marker-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-boundary-after-save-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-boundary-after-fini-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-boundary-fini-marker-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-logical-cross-open-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-boundary-save-marker-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin'}),
            ('save-929-boundary-after-save-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin'}),
            ('save-929-boundary-after-fini-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin'}),
            ('save-929-logical-cross-open-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin'}),
            ('abort-save-chunk-header-length', (),
             {'sigabrt': True, 'userdir': True, 'abort': False, 'issues': ('525',)}),
            ('abort-save-chunk-header-checksum', (),
             {'sigabrt': True, 'userdir': True, 'abort': False, 'issues': ('525',)}),
            ('abort-zhuyin-save-chunk-header-length',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-save-chunk-header-checksum',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('stderr-fresh-user-dir', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-fresh-user-dir-zhuyin', (), {'issues': ('545',), 'mode': 'zhuyin', 'stderr': True}),
            ('stderr-init-missing-system-dir', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-init-missing-system-dir-zhuyin',
             (),
             {'issues': ('545',), 'mode': 'zhuyin', 'stderr': True}),
            ('stderr-init-system-dir-without-table-conf', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-save-dir-removed', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-save-dir-removed-zhuyin', (), {'issues': ('545',), 'mode': 'zhuyin', 'stderr': True}),
            ('stderr-fini-dir-removed', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-fini-dir-removed-zhuyin', (), {'issues': ('545',), 'mode': 'zhuyin', 'stderr': True}),
            ('stderr-non-utf8-user-dir', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-quiet-saves', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-unmodified-save-dir-removed', (), {'issues': ('545',), 'stderr': True})]
INJECTED = ('CASES', 'crafted_bigram_context', 'index_file_rows', 'token_reuse_sequence')


def stub(name):
    return collections.defaultdict(dict) if name == 'CASES' else Mock(name=name)


def declarations(state):
    return collect_declarations(family.register, CASES=state['CASES'], crafted_bigram_context=state['crafted_bigram_context'], index_file_rows=state['index_file_rows'], token_reuse_sequence=state['token_reuse_sequence'])


class SaveFailuresStderrTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
