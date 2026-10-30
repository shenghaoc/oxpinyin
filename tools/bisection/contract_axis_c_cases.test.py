#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the axis C cases (#1175)."""
import hashlib
import unittest
from unittest.mock import Mock

import contract_axis_c
import contract_axis_c_cases as family


# (case ID, positional options, keyword options) in registration order: the count, the
# first and last entries, and the SHA-256 of the repr of the whole ordered list.
EXPECTED_COUNT = 888
EXPECTED_FIRST = ('axis-c-export-user', (), {})
EXPECTED_LAST = ('axis-c-layout-zhuyin-2147483647-e0000070', (), {'mode': 'zhuyin', 'stderr': True})
EXPECTED_SHA256 = '5cbf8dc4d18bded7b9e90d763f03c614e29715b03e0ba58ee102ecf0eeeabdfe'
INJECTED = ('AXIS_C_DEFINITION', 'AXIS_C_GROUPS', 'AXIS_C_LAYOUT_COVERED', 'WORKER')


def stub(name):
    if name == 'AXIS_C_DEFINITION':
        return contract_axis_c.axis_c_definition()
    if name == 'AXIS_C_GROUPS':
        return {name: [] for name in ('options', 'encoding', 'layouts')}
    if name == 'AXIS_C_LAYOUT_COVERED':
        return {}
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, AXIS_C_DEFINITION=state['AXIS_C_DEFINITION'], AXIS_C_GROUPS=state['AXIS_C_GROUPS'],
                    AXIS_C_LAYOUT_COVERED=state['AXIS_C_LAYOUT_COVERED'], WORKER=state['WORKER'])
    return cases


class AxisCCasesTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        meta = [(name, args, kwargs) for name, args, kwargs, _ in declarations({n: stub(n) for n in INJECTED})]
        self.assertEqual(len(meta), EXPECTED_COUNT)
        self.assertEqual((meta[0], meta[-1]), (EXPECTED_FIRST, EXPECTED_LAST))
        self.assertEqual(len({name for name, _, _ in meta}), EXPECTED_COUNT)
        self.assertEqual(hashlib.sha256(repr(meta).encode()).hexdigest(), EXPECTED_SHA256)


if __name__ == '__main__':
    unittest.main()
