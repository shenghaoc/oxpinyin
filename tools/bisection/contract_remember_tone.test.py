#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the two remember-tone registration points."""
from pathlib import Path
import sys

import types
import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_remember_tone as family


def declarations(register, reader):
    return {name: (metadata, fn) for name, _, metadata, fn in collect_declarations(register, remember_export=reader)}


class RememberToneTests(unittest.TestCase):
    def test_pinyin_tones_use_the_injected_export_reader(self):
        kit, reader, calls = Mock(mode='pinyin', lib=types.SimpleNamespace()), Mock(return_value=['row']), {}
        kit.fn.side_effect = lambda name, *types: calls.setdefault(name, Mock(return_value=True))
        result = declarations(family.register_tones, reader)['remember-tones-pinyin'][1](kit)
        self.assertEqual(result['export'], ['row'])
        self.assertFalse(result['remember_api'])
        reader.assert_called_once_with(kit, 7, '你好你好')
        calls['remember_user_input'].assert_called_once_with(kit.inst, '你好你好'.encode(), 3)

    def test_review_cases_select_the_parser_and_keep_their_reader(self):
        for name, parser, phrase, expected in (
                ('remember-cleared-tone-pinyin', 'parse_more_full_pinyins', '你好', True),
                ('remember-double-tone-pinyin', 'parse_more_double_pinyins', '你', False)):
            with self.subTest(name=name):
                kit, reader, calls = Mock(), Mock(return_value=['reading']), {}
                kit.fn.side_effect = lambda key, *types: calls.setdefault(key, Mock(return_value=True))
                result = declarations(family.register_review, reader)[name][1](kit)
                self.assertEqual(result['export'], ['reading'])
                self.assertEqual('clear-tone' in result, expected)
                self.assertIn(parser, calls)
                reader.assert_called_once_with(kit, 7, phrase)

    def test_zhuyin_api_presence_does_not_invoke_the_export_reader(self):
        kit, reader = Mock(mode='zhuyin', lib=types.SimpleNamespace()), Mock()
        result = declarations(family.register_tones, reader)['remember-api-absent-zhuyin'][1](kit)
        self.assertEqual(result, {'remember_api': False, 'export_api': False})
        reader.assert_not_called()


if __name__ == '__main__':
    unittest.main()
