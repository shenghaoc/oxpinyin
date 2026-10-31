#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the library reload cases (#1175)."""
from pathlib import Path
import sys

import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_library_reload as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('reload-drops-session-pinyin-train', (), {'issues': ('805',), 'mode': 'pinyin'}),
            ('reload-keeps-saved-pinyin-train', (), {'control': True, 'issues': ('805',), 'mode': 'pinyin'}),
            ('reload-drops-session-pinyin-reading', (), {'issues': ('805',), 'mode': 'pinyin'}),
            ('reload-keeps-saved-pinyin-reading', (), {'control': True, 'issues': ('805',), 'mode': 'pinyin'}),
            ('reload-drops-session-pinyin-new-item', (), {'issues': ('805',), 'mode': 'pinyin'}),
            ('reload-keeps-saved-pinyin-new-item',
             (),
             {'control': True, 'issues': ('805',), 'mode': 'pinyin'}),
            ('reload-refused-pinyin-train-library-1',
             (),
             {'control': True, 'issues': ('805',), 'mode': 'pinyin'}),
            ('reload-refused-pinyin-import-library-1',
             (),
             {'control': True, 'issues': ('805',), 'mode': 'pinyin'}),
            ('reload-drops-session-zhuyin-train', (), {'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-keeps-saved-zhuyin-train', (), {'control': True, 'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-drops-session-zhuyin-reading', (), {'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-keeps-saved-zhuyin-reading', (), {'control': True, 'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-drops-session-zhuyin-new-item', (), {'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-keeps-saved-zhuyin-new-item',
             (),
             {'control': True, 'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-refused-zhuyin-train-library-1',
             (),
             {'control': True, 'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-refused-user-pinyin-item', (), {'control': True, 'issues': ('805',), 'mode': 'pinyin'}),
            ('reload-refused-user-pinyin-item-saved',
             (),
             {'control': True, 'issues': ('805',), 'mode': 'pinyin'}),
            ('reload-refused-user-pinyin-reading',
             (),
             {'control': True, 'issues': ('805',), 'mode': 'pinyin'}),
            ('reload-refused-user-pinyin-reading-saved',
             (),
             {'control': True, 'issues': ('805',), 'mode': 'pinyin'}),
            ('reload-user-zhuyin-item', (), {'control': False, 'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-user-zhuyin-item-saved', (), {'control': True, 'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-user-zhuyin-reading', (), {'control': False, 'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-user-zhuyin-reading-saved', (), {'control': True, 'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-user-zhuyin-reuse-other', (), {'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-user-zhuyin-reuse-other-save-reopen', (), {'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-user-zhuyin-reuse-other-mask', (), {'issues': ('805',), 'mode': 'zhuyin'}),
            ('reload-user-zhuyin-reuse-same', (), {'abort': False, 'issues': ('805',), 'mode': 'zhuyin', 'sigabrt': True}),
            ('reload-dangling-token-candidates',
             (),
             {'crash': {'candidates': [88, [['大大', 2], ['鞑靼', 2], ['达达', 2]], {}],
                        'readings': [False, 0],
                        'tokens': ['0x2003cfd'],
                        'unigram': [False, 0]},
              'issues': ('805',)}),
            ('reload-dangling-token-candidates-zhuyin',
             (),
             {'crash': {'candidates': [88, [['大大', 2], ['鞑靼', 2], ['达达', 2]], {}],
                        'readings': [False, 0],
                        'tokens': ['0x2003cfd'],
                        'unigram': [False, 0]},
              'issues': ('805',),
              'mode': 'zhuyin'}),
            ('reload-total-while-unloaded', (), {'issues': ('805',)}),
            ('mask-out-replaces-reload-offset', (), {'control': True, 'issues': ('805',)}),
            ('mask-out-after-compact-keeps-reload-offset', (), {'issues': ('805',)})]


def declarations():
    return collect_declarations(family.register)


class LibraryReloadTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations()
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

if __name__ == '__main__':
    unittest.main()
