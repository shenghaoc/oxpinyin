#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the bigram mask and prediction cases (#1175)."""
import inspect
import unittest
from unittest.mock import Mock

import contract_bigram_mask_prediction as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('issue1114-collision-load-save-pinyin',
             (),
             {'cells': ('tkrzw',), 'issues': ('1114',), 'mode': 'pinyin'}),
            ('issue1114-collision-load-save-zhuyin',
             (),
             {'cells': ('tkrzw',), 'issues': ('1114',), 'mode': 'zhuyin'}),
            ('predicted-bigram-is-user-candidate', (), {'issues': ('1231',)}),
            ('abort-choose-candidate-predicted-bigram-row', (), {'abort': 0, 'cells': ('bdb',)}),
            ('abort-mask-out-short-bigram-value', (), {'abort': False}),
            ('abort-zhuyin-mask-out-short-bigram-value', (), {'abort': False, 'mode': 'zhuyin'}),
            ('mask-out-short-bigram-value-masked-key', (), {'control': True}),
            ('abort-mask-out-non-token-bigram-key', (), {'abort': False}),
            ('abort-zhuyin-mask-out-non-token-bigram-key', (), {'abort': False, 'mode': 'zhuyin'}),
            ('abort-mask-out-residual-bigram-gram', (), {'abort': False}),
            ('abort-zhuyin-mask-out-residual-bigram-gram', (), {'abort': False, 'mode': 'zhuyin'}),
            ('abort-begin-bigram-phrases-non-token-key', (), {'abort': False}),
            ('issue677-mask-save-sequence-pinyin', (), {'issues': ('677',), 'mode': 'pinyin'}),
            ('issue677-mask-save-sequence-zhuyin', (), {'issues': ('677',), 'mode': 'zhuyin'})]
INJECTED = ('_craft_hash', 'crafted_bigram_context', 'index_file_rows')


def stub(name):
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, _craft_hash=state['_craft_hash'], crafted_bigram_context=state['crafted_bigram_context'], index_file_rows=state['index_file_rows'])
    return cases


class BigramMaskPredictionTests(unittest.TestCase):
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
