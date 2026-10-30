#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the remember_user_input signed count cases."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_negative_remember as family


def declarations():
    return {name: (args, kwargs, fn) for name, args, kwargs, fn in collect_declarations(family.register)}


class NegativeRememberTests(unittest.TestCase):
    def test_registration_order_metadata_and_replay(self):
        first, second = declarations(), declarations()
        self.assertEqual(list(first), ['remember-count-2', 'remember-count-5', 'remember-count-min',
                                       'remember-count-default', 'remember-count-seven'])
        self.assertEqual([kwargs for _, kwargs, _ in first.values()], [{}] * 3 + [{'control': True}] * 2)
        for name, (args, _, fn) in first.items():
            self.assertEqual(args, ())
            self.assertIs(fn, second[name][2])
            self.assertEqual(fn.__module__, family.__name__)

    def test_each_case_remembers_with_its_own_signed_count(self):
        counts = {'remember-count-2': -2, 'remember-count-5': -5, 'remember-count-min': -2147483648,
                  'remember-count-default': -1, 'remember-count-seven': 7}
        for name, (_, _, fn) in declarations().items():
            with self.subTest(name=name):
                kit = Mock()
                functions = {'iterator_has_next_phrase': Mock(return_value=False)}
                kit.fn.side_effect = lambda function, *types: functions.setdefault(function, Mock(name=function))
                result = fn(kit)
                self.assertEqual(result['rows'], [])
                self.assertEqual(result['remember'], functions['remember_user_input'].return_value)
                functions['remember_user_input'].assert_called_once_with(
                    kit.inst, '你好世界'.encode(), counts[name])
                functions['parse_more_full_pinyins'].assert_called_once_with(kit.inst, b'nihaoshijie')
                functions['begin_get_phrases'].assert_called_once_with(kit.ctx, 7)


if __name__ == '__main__':
    unittest.main()
