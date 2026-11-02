#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the pinyin allocation-after-finalization case."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock, call

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_alloc_after_fini as family


class AllocationAfterFiniTests(unittest.TestCase):
    def declarations(self):
        return {name: (args, kwargs, fn) for name, args, kwargs, fn in collect_declarations(family.register)}

    def test_registration_metadata_and_replay(self):
        first, second = self.declarations(), self.declarations()
        self.assertEqual(list(first), ['alloc-instance-after-fini', 'instance_outlives_context',
                                       'zhuyin-instance_outlives_context'])
        args, kwargs, fn = first['alloc-instance-after-fini']
        self.assertEqual((args, kwargs), ((), {'issues': ('528',)}))
        self.assertIs(fn, second['alloc-instance-after-fini'][2])
        self.assertEqual(fn.__module__, family.__name__)

    def test_case_frees_then_finalizes_before_second_allocation(self):
        operations = Mock()
        operations.alloc.side_effect = [42, None]
        kit = Mock()
        kit.init.return_value = 'context'
        kit.fn.side_effect = lambda name, *types: {
            'alloc_instance': operations.alloc, 'free_instance': operations.free, 'fini': operations.fini,
        }[name]
        result = self.declarations()['alloc-instance-after-fini'][2](kit)
        self.assertEqual(result, {'live instance': True, '~instance after fini': False})
        self.assertEqual(operations.mock_calls, [call.alloc('context'), call.free(42),
                                                  call.fini('context'), call.alloc('context')])
        self.assertEqual(kit.fn.call_args_list, [call('alloc_instance', family.P, family.P),
                                                 call('free_instance', None, family.P),
                                                 call('fini', None, family.P),
                                                 call('alloc_instance', family.P, family.P)])

    def test_orphan_guesses_after_fini_and_then_frees(self):
        operations = Mock()
        operations.alloc.return_value = 42
        operations.guess.return_value = True
        kit = Mock()
        kit.init.return_value = 'context'
        kit.fn.side_effect = lambda name, *types: {
            'alloc_instance': operations.alloc, 'free_instance': operations.free,
            'fini': operations.fini, 'guess_sentence': operations.guess,
            'parse_more_chewings': operations.parse, 'parse_more_full_pinyins': operations.parse,
        }[name]
        for name in ('instance_outlives_context', 'zhuyin-instance_outlives_context'):
            operations.reset_mock()
            kit.mode = 'zhuyin' if name.startswith('zhuyin') else 'pinyin'
            _, metadata, case = self.declarations()[name]
            self.assertEqual(metadata['crash'], {'guess': True})
            self.assertEqual(case(kit), {'guess': True})
            self.assertEqual(operations.mock_calls, [call.alloc('context'),
                call.parse(42, b'su3cl3' if kit.mode == 'zhuyin' else b'nihao'),
                call.fini('context'), call.guess(42), call.free(42)])


if __name__ == '__main__':
    unittest.main()
