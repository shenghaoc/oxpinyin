#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the #573 double finalization cases."""
import unittest
from unittest.mock import Mock, call

import contract_double_fini as family


def declarations():
    cases = {}

    def case(name, *args, **kwargs):
        def collect(fn):
            cases[name] = (args, kwargs, fn)
            return fn
        return collect

    family.register(case)
    return cases


class DoubleFiniTests(unittest.TestCase):
    def test_registration_metadata_and_the_shared_function(self):
        cases = declarations()
        crash = {'crash': {'second fini returned': True}, 'stderr_varies': True, 'issues': ('573',)}
        self.assertEqual({name: (args, kwargs) for name, (args, kwargs, _) in cases.items()},
                         {'fini-twice': ((), crash), 'zhuyin-fini-twice': ((), dict(crash, mode='zhuyin'))})
        self.assertIs(cases['fini-twice'][2], family.fini_twice)
        self.assertIs(cases['zhuyin-fini-twice'][2], family.fini_twice)
        self.assertIs(declarations()['fini-twice'][2], family.fini_twice)

    def test_the_context_is_finalized_twice_after_its_instance_is_freed(self):
        operations, kit = Mock(), Mock()
        kit.init.return_value = 'context'
        operations.alloc.return_value = 42
        kit.fn.side_effect = lambda name, *types: {'alloc_instance': operations.alloc,
                                                   'free_instance': operations.free, 'fini': operations.fini}[name]
        self.assertEqual(family.fini_twice(kit), {'second fini returned': True})
        self.assertEqual(operations.mock_calls, [call.alloc('context'), call.free(42),
                                                  call.fini('context'), call.fini('context')])


if __name__ == '__main__':
    unittest.main()
