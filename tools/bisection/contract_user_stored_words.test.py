#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the user stored words cases (#1175)."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_user_stored_words as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('user-stored-words/pinyin-1-valid', (), {'control': True, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-1-surrogate',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-1-above-unicode',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-1-nul', (), {'control': True, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-5-valid', (), {'control': True, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-5-surrogate',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-5-above-unicode',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-5-nul', (), {'control': True, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-6-valid', (), {'control': True, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-6-surrogate',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-6-above-unicode',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-6-nul', (), {'control': True, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-7-valid', (), {'control': True, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-7-surrogate',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-7-above-unicode',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/pinyin-7-nul', (), {'control': True, 'issues': ('1198',), 'mode': 'pinyin'}),
            ('user-stored-words/zhuyin-1-valid', (), {'control': True, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-1-surrogate',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-1-above-unicode',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-1-nul', (), {'control': True, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-5-valid', (), {'control': True, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-5-surrogate',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-5-above-unicode',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-5-nul', (), {'control': True, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-6-valid', (), {'control': True, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-6-surrogate',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-6-above-unicode',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-6-nul', (), {'control': True, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-7-valid', (), {'control': True, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-7-surrogate',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-7-above-unicode',
             (),
             {'control': False, 'issues': ('1198',), 'mode': 'zhuyin'}),
            ('user-stored-words/zhuyin-7-nul', (), {'control': True, 'issues': ('1198',), 'mode': 'zhuyin'})]
INJECTED = ('phrase_text_system',)


def stub(name):
    return Mock(name=name)


def declarations(state):
    return collect_declarations(family.register, phrase_text_system=state['phrase_text_system'])


class UserStoredWordsTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
