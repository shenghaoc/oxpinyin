#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the zero-facade-total cases."""
import ctypes as C
import unittest
from unittest.mock import Mock

import contract_facade_total as family


def declarations(wrap):
    result = {}

    def case(name, **metadata):
        def collect(fn):
            result[name] = (metadata, fn)
            return fn
        return collect

    family.register(case, wrap_total_to_zero=wrap)
    return result


class FacadeTotalTests(unittest.TestCase):
    def test_empty_windows_use_their_offset_sort_and_shared_wrap(self):
        kit, wrap, calls = Mock(), Mock(return_value=True), {}

        def count(inst, out):
            C.cast(out, C.POINTER(family.U))[0] = 3

        def fn(name, *types):
            return calls.setdefault(name, Mock(side_effect=count) if name == 'get_n_candidate'
                                    else Mock(return_value=True))
        kit.fn.side_effect = fn
        cases = declarations(wrap)
        for label, offset, sort in (('end-sort-3', 5, 3), ('mid-key-sort-3', 3, 3), ('end-sort-2', 5, 2)):
            with self.subTest(label=label):
                metadata, run = cases['facade-total-zero-ranks-nothing-' + label]
                self.assertEqual(metadata, {'control': True, 'issues': ('540',)})
                self.assertEqual(run(kit), {'ret': True, 'rows': 3})
                calls['guess_candidates'].assert_called_with(kit.alloc.return_value, offset, sort)
        self.assertEqual(wrap.call_count, 3)
        wrap.assert_called_with(kit, kit.alloc.return_value)

    def test_abort_case_passes_the_cached_instance_and_reports_the_wrap_result(self):
        kit, wrap = Mock(), Mock(return_value=False)
        kit.fn.return_value = Mock(return_value=True)
        metadata, run = declarations(wrap)['facade-total-zero']
        self.assertEqual(metadata, {'abort': False, 'issues': ('540',)})
        self.assertEqual(run(kit), {'add': False, 'ret': True})
        wrap.assert_called_once_with(kit, kit.inst)


if __name__ == '__main__':
    unittest.main()
