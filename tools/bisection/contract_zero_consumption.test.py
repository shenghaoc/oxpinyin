#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for zero-consumption sentence cases."""
import unittest
from unittest.mock import Mock

import contract_zero_consumption as family


def declarations():
    result = {}

    def case(name, **metadata):
        def collect(fn):
            result[name] = (metadata, fn)
            return fn
        return collect

    family.register(case)
    return result


class ZeroConsumptionTests(unittest.TestCase):
    def test_fresh_and_stale_instances_use_the_selected_prefix_path(self):
        for mode in ('pinyin', 'zhuyin'):
            with self.subTest(mode=mode):
                kit, calls = Mock(mode=mode), {}

                def resolve(name, *types):
                    if name not in calls:
                        calls[name] = (Mock(side_effect=lambda inst, text: 5 if text == b'nihao' else 0)
                                       if name == 'parse_more_full_pinyins' else Mock(return_value=True))
                    return calls[name]

                kit.fn.side_effect = resolve
                name = f'zero-consumption-{mode}-full_pinyins-empty-prefix'
                metadata, run = declarations()[name]
                result = run(kit)
                self.assertEqual(metadata, {'mode': mode, 'control': mode == 'pinyin', 'issues': ('710', 'row:53')})
                self.assertEqual(result['fresh']['parse'], 0)
                self.assertEqual(result['stale']['initial_parse'], 5)
                self.assertEqual(result['stale']['parse'], 0)
                self.assertEqual(result['fresh']['sentence'], [True, 'untouched'])
                self.assertEqual(calls['guess_sentence_with_prefix'].call_count, 2)
                self.assertEqual([call.args[-1] for call in calls['guess_sentence_with_prefix'].call_args_list],
                                 ['你'.encode(), '你'.encode()])


if __name__ == '__main__':
    unittest.main()
