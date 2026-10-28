#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for auxiliary text renderer cases."""
import unittest
from unittest.mock import Mock, patch

import contract_auxiliary_text as family


class AuxiliaryTextTests(unittest.TestCase):
    def test_matrix_visits_each_renderer_and_cursor_with_untouched_outputs(self):
        kit, calls = Mock(), {}
        kit.fn.side_effect = lambda name, *types: calls.setdefault(name, Mock(return_value=True))
        result = family.aux_matrix(kit, 'parse_more_full_pinyins', b'ni', (0, 2))
        self.assertEqual(result, {'full_pinyin@0': [True, 'untouched'], 'full_pinyin@2': [True, 'untouched'],
                                  'double_pinyin@0': [True, 'untouched'], 'double_pinyin@2': [True, 'untouched'],
                                  'chewing@0': [True, 'untouched'], 'chewing@2': [True, 'untouched']})
        self.assertEqual(calls['parse_more_full_pinyins'].call_count, 6)
        for name in ('full_pinyin', 'double_pinyin', 'chewing'):
            self.assertEqual([call.args[1] for call in calls['get_' + name + '_auxiliary_text'].call_args_list], [0, 2])

    def test_options_limit_double_pinyin_cursors_to_the_supported_set(self):
        kit = Mock()
        setter = Mock(return_value=True)
        kit.fn.return_value = setter
        calls = []

        def cursors(k, text, values, functions):
            calls.append((text, tuple(values), functions))
            return {str(len(calls)): 'row'}

        with patch.object(family, 'aux_cursors', side_effect=cursors):
            result = family.aux_options(kit, 0xA0, (b'nihao',))
        setter.assert_called_once_with(kit.ctx, 0xA0)
        self.assertEqual(calls, [(b'nihao', (0, 1, 2, 3, 4, 5, 6), ('full_pinyin', 'chewing')),
                                 (b'nihao', (0, 1, 2, 5, 6), ('double_pinyin',))])
        self.assertEqual(result, {'1': 'row', '2': 'row'})


if __name__ == '__main__':
    unittest.main()
