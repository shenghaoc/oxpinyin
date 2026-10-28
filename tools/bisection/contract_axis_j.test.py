#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the axis J reachability control cases."""
import unittest
from unittest.mock import Mock, patch

import contract_axis_j as family


def declarations():
    cases = {}

    def case(name, *args, **kwargs):
        def collect(fn):
            cases[name] = (args, kwargs, fn)
            return fn
        return collect

    family.register(case)
    return cases


class AxisJTests(unittest.TestCase):
    def test_registration_order_metadata_and_replay(self):
        first, second = declarations(), declarations()
        self.assertEqual(list(first), ['j-choose-nbest-diff-result', 'j-chewing-multi-key-pinyin-2',
                                       'j-chewing-multi-key-pinyin-9', 'j-chewing-multi-key-zhuyin-2',
                                       'j-chewing-multi-key-zhuyin-9'])
        self.assertEqual([kwargs for _, kwargs, _ in first.values()],
                         [{'control': True}] + [{'mode': mode, 'control': True}
                                                for mode in ('pinyin', 'pinyin', 'zhuyin', 'zhuyin')])
        for name, (args, _, fn) in first.items():
            self.assertEqual(args, ())
            self.assertEqual(fn.__module__, family.__name__)
        self.assertIs(first['j-choose-nbest-diff-result'][2], second['j-choose-nbest-diff-result'][2])

    def test_the_multi_key_parse_selects_each_facade_scheme_setter(self):
        for mode, setter in (('pinyin', 'set_zhuyin_scheme'), ('zhuyin', 'set_chewing_scheme')):
            for scheme in (2, 9):
                with self.subTest(mode=mode, scheme=scheme):
                    kit, calls = Mock(mode=mode), {}
                    kit.fn.side_effect = lambda name, *types: calls.setdefault(name, Mock(return_value=name))
                    result = declarations()['j-chewing-multi-key-%s-%d' % (mode, scheme)][2](kit)
                    self.assertEqual(result, {'scheme': scheme, 'selected': setter,
                                              'parsed': 'parse_more_chewings', 'length': 'get_parsed_input_length'})
                    calls[setter].assert_called_once_with(kit.ctx, scheme)
                    calls['parse_more_chewings'].assert_called_once_with(kit.inst, b'abc')

    def test_choosing_a_displayed_nbest_row_records_rank_and_choice(self):
        kit = Mock()
        ranked, chosen = Mock(return_value=True), Mock(return_value=-1)
        kit.fn.side_effect = lambda name, *types: {'get_candidate_nbest_index': ranked,
                                                   'choose_candidate': chosen}[name]
        with patch.object(family, 'guessed', return_value='inst'), patch.object(family, 'row_of', return_value='row'):
            result = declarations()['j-choose-nbest-diff-result'][2](kit)
        self.assertEqual(result, {'ranked': True, 'rank': 0xff, 'chosen': -1})
        chosen.assert_called_once_with('inst', 0, 'row')
        with patch.object(family, 'guessed'), patch.object(family, 'row_of', return_value=None), \
                self.assertRaisesRegex(AssertionError, 'no displayed NBEST candidate'):
            declarations()['j-choose-nbest-diff-result'][2](kit)


if __name__ == '__main__':
    unittest.main()
