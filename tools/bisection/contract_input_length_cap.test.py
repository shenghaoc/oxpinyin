#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the #540 input length cap cases."""
import unittest
from unittest.mock import Mock, patch

import contract_input_length_cap as family


def declarations():
    cases = {}

    def case(name, *args, **kwargs):
        def collect(fn):
            cases[name] = (args, kwargs, fn)
            return fn
        return collect

    family.register(case)
    return cases


class InputLengthCapTests(unittest.TestCase):
    def test_registration_order_metadata_and_replay(self):
        first, second = declarations(), declarations()
        self.assertEqual(list(first), ['input-length-cap', 'long-input-sentence'])
        for name, (args, kwargs, fn) in first.items():
            self.assertEqual((args, kwargs), ((), {'issues': ('540',)}))
            self.assertIs(fn, second[name][2])
            self.assertEqual(fn.__module__, family.__name__)

    def test_each_length_is_parsed_on_a_fresh_instance_truncated_to_that_many_bytes(self):
        kit = Mock()
        kit.alloc.side_effect = range(6)
        parse, length = Mock(side_effect=lambda inst, text: len(text)), Mock(side_effect=lambda inst: -inst)
        kit.fn.side_effect = lambda name, *types: {'parse_more_full_pinyins': parse,
                                                   'get_parsed_input_length': length}[name]
        result = declarations()['input-length-cap'][2](kit)
        self.assertEqual(result, {'4096': [4096, 0], '4097': [4097, -1], '32766': [32766, -2],
                                  '32767': [32767, -3], '32768': [32768, -4], '65537': [65537, -5]})
        self.assertEqual(parse.call_args_list[1].args[1], b'ni' * 2048 + b'n')

    def test_a_long_sentence_is_reduced_to_its_length(self):
        kit = Mock()
        kit.fn.side_effect = lambda name, *types: Mock(return_value=4800 if name.startswith('parse') else True)
        for row, expected in (((True, '你' * 2400), [True, 2400]), ((False, None), [False, None])):
            with self.subTest(row=row), patch.object(family, 'sentence_out', return_value=row) as sentence:
                result = declarations()['long-input-sentence'][2](kit)
                self.assertEqual(result, {'parsed': 4800, 'guess': True, 'sentence': expected})
                sentence.assert_called_once_with(kit, kit.alloc.return_value, 0)


if __name__ == '__main__':
    unittest.main()
