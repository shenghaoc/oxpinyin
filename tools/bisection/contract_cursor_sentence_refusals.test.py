#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for cursor and sentence refusal cases."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_cursor_sentence_refusals as family


def declarations():
    return {name: (metadata, fn) for name, _, metadata, fn in collect_declarations(family.register)}


class CursorSentenceRefusalsTests(unittest.TestCase):
    def test_past_sentence_case_uses_row_five_on_the_guessed_instance(self):
        kit = Mock()
        guess = Mock(return_value=True)
        kit.fn.return_value = guess
        with patch.object(family, 'full_inst', return_value='parsed') as parse, \
                patch.object(family, 'sentence_out', return_value=(False, 'untouched')) as sentence:
            metadata, run = declarations()['abort-get-sentence-past-rows']
            self.assertEqual(run(kit), {'ret': False})
        self.assertEqual(metadata, {'abort': False, 'issues': ('525',)})
        parse.assert_called_once_with(kit, b'nihao')
        guess.assert_called_once_with('parsed')
        sentence.assert_called_once_with(kit, 'parsed', 5)

    def test_leading_apostrophe_cases_capture_each_renderer_and_input(self):
        cases = declarations()
        for label, suffix, text in (
                ('full-pinyin', 'full_pinyin', b"'nihao"),
                ('double-pinyin', 'double_pinyin', b"'nihao"),
                ('chewing', 'chewing', b"'nihao"),
                ('full-pinyin-doubled', 'full_pinyin', b"''ni")):
            with self.subTest(label=label):
                kit = Mock()
                render = Mock(return_value=False)
                kit.fn.return_value = render
                name = ('abort-full-pinyin-auxiliary-text-doubled-leading-apostrophe' if label.endswith('doubled')
                        else 'abort-' + label + '-auxiliary-text-leading-apostrophe')
                with patch.object(family, 'full_inst', return_value='inst') as parse:
                    self.assertEqual(cases[name][1](kit), {'ret': False})
                parse.assert_called_once_with(kit, text)
                self.assertEqual(kit.fn.call_args.args[0], 'get_' + suffix + '_auxiliary_text')
                self.assertEqual(render.call_args.args[:2], ('inst', 2))


if __name__ == '__main__':
    unittest.main()
