#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the c string inputs cases (#1175)."""
import inspect
import unittest
from unittest.mock import Mock

import contract_c_string_inputs as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('c-string-input/pinyin-full-key-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-full-key-malformed',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/zhuyin-full-key-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-full-key-malformed',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/pinyin-full-key-apostrophe-valid',
             (),
             {'abort': False, 'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-full-key-apostrophe-invalid',
             (),
             {'abort': False, 'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/zhuyin-full-key-apostrophe-valid',
             (),
             {'abort': False, 'control': True, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-full-key-apostrophe-invalid',
             (),
             {'abort': False, 'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/pinyin-lookup-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-lookup-surrogate',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-lookup-above-unicode',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-lookup-five-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-lookup-six-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-lookup-invalid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-import-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-import-surrogate',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-import-above-unicode',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-import-five-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-import-six-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-import-invalid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-import-null',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-remember-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-remember-surrogate',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-remember-above-unicode',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-remember-five-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-remember-six-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-remember-invalid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-remember-null',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/zhuyin-lookup-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-lookup-surrogate',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-lookup-above-unicode',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-lookup-five-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-lookup-six-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-lookup-invalid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-import-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-import-surrogate',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-import-above-unicode',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-import-five-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-import-six-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-import-invalid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-import-null',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-direct-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-direct-later-invalid',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-direct-same-invalid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/pinyin-segment-state-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin', 'pin_not_run': None}),
            ('c-string-input/pinyin-segment-state-null',
             (),
             {'control': False,
              'issues': ('1279',),
              'mode': 'pinyin',
              'pin_not_run': {'count': 0, 'logs': [['GLib', 8]], 'ret': True}}),
            ('c-string-input/pinyin-lookup-state-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin', 'pin_not_run': None}),
            ('c-string-input/pinyin-lookup-state-null',
             (),
             {'control': False,
              'issues': ('1279',),
              'mode': 'pinyin',
              'pin_not_run': {'count': 0, 'logs': [], 'ret': False}}),
            ('c-string-input/zhuyin-segment-state-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin', 'pin_not_run': None}),
            ('c-string-input/zhuyin-segment-state-null',
             (),
             {'control': False,
              'issues': ('1279',),
              'mode': 'zhuyin',
              'pin_not_run': {'count': 0, 'logs': [['GLib', 8]], 'ret': True}}),
            ('c-string-input/zhuyin-lookup-state-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin', 'pin_not_run': None}),
            ('c-string-input/zhuyin-lookup-state-null',
             (),
             {'control': False,
              'issues': ('1279',),
              'mode': 'zhuyin',
              'pin_not_run': {'count': 0, 'logs': [], 'ret': False}}),
            ('c-string-input/pinyin-parse_full_pinyin-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin', 'pin_not_run': None}),
            ('c-string-input/pinyin-parse_full_pinyin-null',
             (),
             {'control': True,
              'issues': ('1279',),
              'mode': 'pinyin',
              'pin_not_run': {'logs': [], 'ret': False, 'untouched key': True}}),
            ('c-string-input/pinyin-parse_chewing-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin', 'pin_not_run': None}),
            ('c-string-input/pinyin-parse_chewing-null',
             (),
             {'control': True,
              'issues': ('1279',),
              'mode': 'pinyin',
              'pin_not_run': {'logs': [], 'ret': False, 'untouched key': True}}),
            ('c-string-input/pinyin-parse_more_full_pinyins-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin', 'pin_not_run': None}),
            ('c-string-input/pinyin-parse_more_full_pinyins-null',
             (),
             {'control': True,
              'issues': ('1279',),
              'mode': 'pinyin',
              'pin_not_run': {'length': 0, 'logs': [], 'ret': 0}}),
            ('c-string-input/pinyin-parse_more_chewings-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin', 'pin_not_run': None}),
            ('c-string-input/pinyin-parse_more_chewings-null',
             (),
             {'control': True,
              'issues': ('1279',),
              'mode': 'pinyin',
              'pin_not_run': {'length': 0, 'logs': [], 'ret': 0}}),
            ('c-string-input/pinyin-parse_double_pinyin-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin', 'pin_not_run': None}),
            ('c-string-input/pinyin-parse_double_pinyin-null',
             (),
             {'control': True,
              'issues': ('1279',),
              'mode': 'pinyin',
              'pin_not_run': {'logs': [], 'ret': False, 'untouched key': True}}),
            ('c-string-input/pinyin-parse_more_double_pinyins-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin', 'pin_not_run': None}),
            ('c-string-input/pinyin-parse_more_double_pinyins-null',
             (),
             {'control': True,
              'issues': ('1279',),
              'mode': 'pinyin',
              'pin_not_run': {'length': 0, 'logs': [], 'ret': 0}}),
            ('c-string-input/zhuyin-parse_full_pinyin-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin', 'pin_not_run': None}),
            ('c-string-input/zhuyin-parse_full_pinyin-null',
             (),
             {'control': True,
              'issues': ('1279',),
              'mode': 'zhuyin',
              'pin_not_run': {'logs': [], 'ret': False, 'untouched key': True}}),
            ('c-string-input/zhuyin-parse_chewing-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin', 'pin_not_run': None}),
            ('c-string-input/zhuyin-parse_chewing-null',
             (),
             {'control': True,
              'issues': ('1279',),
              'mode': 'zhuyin',
              'pin_not_run': {'logs': [], 'ret': False, 'untouched key': True}}),
            ('c-string-input/zhuyin-parse_more_full_pinyins-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin', 'pin_not_run': None}),
            ('c-string-input/zhuyin-parse_more_full_pinyins-null',
             (),
             {'control': True,
              'issues': ('1279',),
              'mode': 'zhuyin',
              'pin_not_run': {'length': 0, 'logs': [], 'ret': 0}}),
            ('c-string-input/zhuyin-parse_more_chewings-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin', 'pin_not_run': None}),
            ('c-string-input/zhuyin-parse_more_chewings-null',
             (),
             {'control': True,
              'issues': ('1279',),
              'mode': 'zhuyin',
              'pin_not_run': {'length': 0, 'logs': [], 'ret': 0}}),
            ('c-string-input/pinyin-character-offset-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-character-offset-invalid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-character-offset-null',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-character-offset-surrogate',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-character-offset-above-unicode',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-character-offset-five-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-character-offset-six-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-character-offset-null-past-matrix',
             (),
             {'abort': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/pinyin-character-offset-invalid-past-matrix',
             (),
             {'abort': False, 'issues': ('1279',), 'mode': 'pinyin'}),
            ('c-string-input/zhuyin-character-offset-valid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-character-offset-invalid',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-character-offset-null',
             (),
             {'control': True, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-character-offset-surrogate',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-character-offset-above-unicode',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-character-offset-five-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-character-offset-six-byte',
             (),
             {'control': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-character-offset-null-past-matrix',
             (),
             {'abort': False, 'issues': ('1279',), 'mode': 'zhuyin'}),
            ('c-string-input/zhuyin-character-offset-invalid-past-matrix',
             (),
             {'abort': False, 'issues': ('1279',), 'mode': 'zhuyin'})]
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


class CStringInputsTests(unittest.TestCase):
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
