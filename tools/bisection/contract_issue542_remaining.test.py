#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the remaining issue 542 cases."""
import unittest
from unittest.mock import Mock, patch

import contract_issue542_remaining as family


def declarations():
    result = {}

    def case(name, **metadata):
        def collect(fn):
            result[name] = (metadata, fn)
            return fn
        return collect

    family.register(case)
    return result


class Issue542RemainingTests(unittest.TestCase):
    def test_training_without_selection_uses_each_facades_parse_and_train_signature(self):
        cases = declarations()
        for mode, parser, text in (('pinyin', 'parse_more_full_pinyins', b'nihao'),
                                   ('zhuyin', 'parse_more_chewings', b'su3cl3')):
            with self.subTest(mode=mode), patch.object(family.os, 'listdir', return_value=['z', 'a']):
                kit, calls = Mock(mode=mode, user='user'), {}
                kit.fn.side_effect = lambda name, *types: calls.setdefault(name, Mock(return_value=name))
                result = cases['issue542-train-no-choose-save-' + mode][1](kit)
                self.assertEqual(result, {'parse': parser, 'guess': 'guess_sentence', 'train': 'train',
                                          'save': 'save', 'files': ['a', 'z']})
                calls[parser].assert_called_once_with(kit.inst, text)
                calls['train'].assert_called_once_with(*((kit.inst, 0) if mode == 'pinyin' else (kit.inst,)))
                calls['save'].assert_called_once_with(kit.ctx)

    def test_invalid_phrase_segment_returns_the_live_log_messages(self):
        kit = Mock(log_messages=['warning'])
        segment = Mock(return_value=False)
        kit.fn.return_value = segment
        run = declarations()['issue542-phrase-segment-invalid-utf8-pinyin'][1]
        result = run(kit)
        self.assertEqual(result, {'ret': False, 'messages': ['warning']})
        self.assertIs(result['messages'], kit.log_messages)
        segment.assert_called_once_with(kit.inst, b'\xff\xfe')


if __name__ == '__main__':
    unittest.main()
