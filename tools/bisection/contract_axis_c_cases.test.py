#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the axis C cases (#1175)."""
import hashlib
import inspect
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

    def test_every_case_function_belongs_to_this_module(self):
        for name, _, _, fn in declarations({n: stub(n) for n in INJECTED}):
            while hasattr(fn, "func"):
                fn = fn.func
            with self.subTest(name=name):
                self.assertIn(fn.__module__, (family.__name__, 'contract_case_helpers'))

    def test_register_takes_exactly_the_injected_driver_bindings(self):
        names = set()
        for attr in [a for a in vars(family) if a.startswith('register')]:
            params = inspect.signature(getattr(family, attr)).parameters.values()
            names.update(p.name for p in params if p.kind is inspect.Parameter.KEYWORD_ONLY)
        self.assertEqual(names, set(INJECTED))

    def test_each_injected_binding_reaches_a_case_or_the_registration(self):
        state = {n: stub(n) for n in INJECTED}
        cases = declarations(state)

        def captured(fn, seen):
            while hasattr(fn, "func"):
                fn = fn.func
            if id(fn) in seen or not hasattr(fn, "__closure__"):
                return set()
            seen.add(id(fn))
            out = set()
            for cell in fn.__closure__ or ():
                value = cell.cell_contents
                out.add(id(value))
                if callable(value) and not isinstance(value, Mock):
                    out |= captured(value, seen)
            return out
        reached = set()
        for _, _, _, fn in cases:
            reached |= captured(fn, set())
        for name in INJECTED:
            with self.subTest(name=name):
                if name in ('AXIS_C_DEFINITION', 'AXIS_C_GROUPS', 'AXIS_C_LAYOUT_COVERED'):
                    self.assertTrue(state[name], "the registration writes the registry it is given")
                else:
                    self.assertIn(id(state[name]), reached)


if __name__ == '__main__':
    unittest.main()
