#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for row 54 empty-parse and forcing cases."""
import unittest
from unittest.mock import Mock

import contract_row54_empty_forcing as family


class Row54EmptyForcingTests(unittest.TestCase):
    def test_empty_parse_sequence_keeps_all_three_results_and_input_order(self):
        kit, parse, guess = Mock(), Mock(side_effect=[5, 0, 5]), Mock(side_effect=[True, False, True])
        kit.fn.side_effect = lambda name, *types: {'parse_more_full_pinyins': parse, 'guess_sentence': guess}.get(
            name, Mock(return_value=True))
        result = family.row54_empty('parse_more_full_pinyins', b'nihao')(kit)
        self.assertEqual(result, {'first_parse': 5, 'first_guess': True, 'empty_parse': 0,
                                  'empty_guess': False, 'reparse': 5, 'recovered_guess': True})
        self.assertEqual([call.args[1] for call in parse.call_args_list], [b'nihao', b'', b'nihao'])

    def test_forcing_cases_keep_the_requested_sequence_and_control_metadata(self):
        cases = {name: kwargs for name, _, kwargs, _ in family._REGISTRATIONS}
        self.assertEqual(cases['row54-forcing-pinyin-empty'],
                         {'mode': 'pinyin', 'control': False, 'issues': ('709', '695', 'row:54')})
        self.assertEqual(cases['row54-forcing-zhuyin-no-empty'],
                         {'mode': 'zhuyin', 'control': True, 'issues': ('709', '695', 'row:54')})


if __name__ == '__main__':
    unittest.main()
