#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the axis C cases (#1175)."""
from pathlib import Path
import sys

import hashlib
import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_axis_c
import contract_axis_c_cases as family


# (case ID, positional options, keyword options) in registration order: the count, the
# first and last entries, and the SHA-256 of the repr of the whole ordered list.
EXPECTED_COUNT = 211
EXPECTED_FIRST = ('axis-c-export-user', (), {})
EXPECTED_LAST = ('axis-c-layout-zhuyin-2-e0000070', (), {'mode': 'zhuyin', 'stderr': True})
EXPECTED_SHA256 = 'e59fbe4f0247b19e944a20dbdcc22c1bbfb69802a1ba0bd44e43ea8dde4f3109'
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
    return collect_declarations(family.register, AXIS_C_DEFINITION=state['AXIS_C_DEFINITION'], AXIS_C_GROUPS=state['AXIS_C_GROUPS'], AXIS_C_LAYOUT_COVERED=state['AXIS_C_LAYOUT_COVERED'], WORKER=state['WORKER'])


class AxisCCasesTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        meta = [(name, args, kwargs) for name, args, kwargs, _ in declarations({n: stub(n) for n in INJECTED})]
        self.assertEqual(len(meta), EXPECTED_COUNT)
        self.assertEqual((meta[0], meta[-1]), (EXPECTED_FIRST, EXPECTED_LAST))
        self.assertEqual(len({name for name, _, _ in meta}), EXPECTED_COUNT)
        self.assertEqual(hashlib.sha256(repr(meta).encode()).hexdigest(), EXPECTED_SHA256)

    def test_only_the_ri_layout_ties_are_policy_accepted(self):
        meta = {name: kwargs for name, _, kwargs, _ in declarations({n: stub(n) for n in INJECTED})}
        recorded = {name for name, kwargs in meta.items() if 'policy_record' in kwargs}
        tied = {'axis-c-layout-pinyin-3-00000000', 'axis-c-layout-pinyin-4-00000000'}
        # Encoding cases with their own record are not layout cases.
        self.assertEqual({name for name in recorded if name.startswith('axis-c-layout-')}, tied)
        for name in tied:
            self.assertEqual(meta[name]['policy_record'], 'trellis-adjusted-cost-s1-s2.md')


if __name__ == '__main__':
    unittest.main()
