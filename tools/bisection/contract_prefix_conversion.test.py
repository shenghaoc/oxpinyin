#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the prefix conversion cases (#1175)."""
import inspect
import unittest
from unittest.mock import Mock

import contract_prefix_conversion as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('prefix-convert/predicted-valid', (), {'control': True, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-valid', (), {'control': True, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-valid',
             (),
             {'control': True, 'crash': None, 'issues': ('1277',), 'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-valid',
             (),
             {'control': True, 'crash': None, 'issues': ('1277',), 'mode': 'zhuyin'}),
            ('prefix-convert/predicted-invalid', (), {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-invalid', (), {'control': True, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-invalid',
             (),
             {'control': True, 'crash': None, 'issues': ('1277',), 'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-invalid',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'zhuyin'}),
            ('prefix-convert/predicted-surrogate', (), {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-surrogate',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-surrogate',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-surrogate',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'zhuyin'}),
            ('prefix-convert/predicted-above-unicode',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-above-unicode',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-above-unicode',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-above-unicode',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'zhuyin'}),
            ('prefix-convert/predicted-five-byte', (), {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-five-byte',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-five-byte',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-five-byte',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'zhuyin'}),
            ('prefix-convert/predicted-six-byte', (), {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-six-byte',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-six-byte',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-six-byte',
             (),
             {'control': False, 'crash': None, 'issues': ('1277',), 'mode': 'zhuyin'}),
            ('prefix-convert/predicted-null',
             (),
             {'control': False, 'crash': {'ret': False, 'rows': []}, 'issues': ('1277',)}),
            ('prefix-convert/punctuation-null',
             (),
             {'control': True, 'crash': {'ret': True, 'rows': []}, 'issues': ('1277',)}),
            ('prefix-convert/pinyin-sentence-null',
             (),
             {'control': False,
              'crash': {'ret': True, 'seeded': True, 'sentence': [True, 'e4b899']},
              'issues': ('1277',),
              'mode': 'pinyin'}),
            ('prefix-convert/zhuyin-sentence-null',
             (),
             {'control': False,
              'crash': {'ret': True, 'seeded': True, 'sentence': [True, 'e4b899']},
              'issues': ('1277',),
              'mode': 'zhuyin'})]
INJECTED = ('items_system',)


def stub(name):
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, items_system=state['items_system'])
    return cases


class PrefixConversionTests(unittest.TestCase):
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
