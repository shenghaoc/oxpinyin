#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the system phrase text cases (#1175)."""
import unittest
from unittest.mock import Mock

import contract_system_phrase_text as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('phrase-text/pinyin-valid-candidate',
             (),
             {'control': True, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-valid-sentence', (), {'control': True, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-valid-export', (), {'control': True, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-valid-choose', (), {'control': True, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-surrogate-candidate',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-surrogate-sentence',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-surrogate-export',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-surrogate-choose',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-above-unicode-candidate',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-above-unicode-sentence',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-above-unicode-export',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-above-unicode-choose',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-nul-candidate', (), {'control': False, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-nul-sentence', (), {'control': False, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-nul-export', (), {'control': False, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/pinyin-nul-choose', (), {'control': False, 'issues': ('894',), 'mode': 'pinyin'}),
            ('phrase-text/zhuyin-valid-candidate',
             (),
             {'control': True, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-valid-sentence', (), {'control': True, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-valid-export', (), {'control': True, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-valid-choose', (), {'control': True, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-surrogate-candidate',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-surrogate-sentence',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-surrogate-export',
             (),
             {'control': True, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-surrogate-choose',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-above-unicode-candidate',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-above-unicode-sentence',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-above-unicode-export',
             (),
             {'control': True, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-above-unicode-choose',
             (),
             {'control': False, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-nul-candidate', (), {'control': False, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-nul-sentence', (), {'control': False, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-nul-export', (), {'control': True, 'issues': ('894',), 'mode': 'zhuyin'}),
            ('phrase-text/zhuyin-nul-choose', (), {'control': False, 'issues': ('894',), 'mode': 'zhuyin'})]
INJECTED = ('phrase_text_system',)


def stub(name):
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, phrase_text_system=state['phrase_text_system'])
    return cases


class SystemPhraseTextTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
