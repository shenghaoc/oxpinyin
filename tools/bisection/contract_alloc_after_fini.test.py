#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the pinyin allocation-after-finalization case."""
import unittest
from unittest.mock import Mock, call

import contract_alloc_after_fini as family


class AllocationAfterFiniTests(unittest.TestCase):
    def declarations(self):
        cases = {}

        def case(name, *args, **kwargs):
            def collect(fn):
                cases[name] = (args, kwargs, fn)
                return fn
            return collect

        family.register(case)
        return cases

    def test_registration_metadata_and_replay(self):
        first, second = self.declarations(), self.declarations()
        self.assertEqual(list(first), ['alloc-instance-after-fini'])
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


if __name__ == '__main__':
    unittest.main()
