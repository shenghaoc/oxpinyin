#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the phrase table mask cases (#1175)."""
import inspect
import unittest
from unittest.mock import Mock

import contract_phrase_table_mask as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('phrase-table-mask-long-short', (), {'issues': ('759', '785')}),
            ('phrase-table-mask-duplicate-long-short', (), {'issues': ('759', '785')}),
            ('phrase-table-mask-post-removal-shrink', (), {'issues': ('759', '785')}),
            ('phrase-table-add-search-remove-roundtrip', (), {'control': True, 'issues': ('759', '785')}),
            ('phrase-table-never-loaded-library-filter', (), {'issues': ('799',)}),
            ('phrase-table-predict-nonunicode-raw-key', (), {}),
            ('phrase-table-predict-mismatched-live-prefix', (), {'control': True})]
INJECTED = ('_craft_db', 'index_file_rows')


def stub(name):
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, _craft_db=state['_craft_db'], index_file_rows=state['index_file_rows'])
    return cases


class PhraseTableMaskTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

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
                if name == 'CASES':
                    self.assertTrue(state[name], "the registration writes the registry it is given")
                else:
                    self.assertIn(id(state[name]), reached)


if __name__ == '__main__':
    unittest.main()
