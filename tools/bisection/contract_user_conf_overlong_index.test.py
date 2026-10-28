#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the user configuration and overlong index cases (#1175)."""
import inspect
import unittest
from unittest.mock import Mock

import contract_user_conf_overlong_index as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('user-conf-920-nul-database-token',
             (),
             {'control': False, 'issues': ('920',), 'mode': 'pinyin'}),
            ('user-conf-920-nul-database-token-zhuyin',
             (),
             {'control': False, 'issues': ('920',), 'mode': 'zhuyin'}),
            ('user-conf-920-version-overflow-conforms',
             (),
             {'control': True,
              'issues': ('920',),
              'mode': 'pinyin'}),
            ('user-conf-920-version-overflow-conforms-zhuyin',
             (),
             {'control': True,
              'issues': ('920',),
              'mode': 'zhuyin'}),
            ('user-conf-920-version-overflow-nonconforms',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'pinyin'}),
            ('user-conf-920-version-overflow-nonconforms-zhuyin',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'zhuyin'}),
            ('user-conf-920-counter-overflow-conforms',
             (),
             {'control': True,
              'issues': ('920',),
              'mode': 'pinyin'}),
            ('user-conf-920-counter-overflow-conforms-zhuyin',
             (),
             {'control': True,
              'issues': ('920',),
              'mode': 'zhuyin'}),
            ('user-conf-920-counter-overflow-nonconforms',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'pinyin'}),
            ('user-conf-920-counter-overflow-nonconforms-zhuyin',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'zhuyin'}),
            ('user-conf-920-negative-overflow-nonconforms',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'pinyin'}),
            ('user-conf-920-negative-overflow-nonconforms-zhuyin',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'zhuyin'}),
            ('user-conf-920-database-missing-at-eof',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'pinyin'}),
            ('user-conf-920-database-missing-at-eof-zhuyin',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'zhuyin'}),
            ('user-conf-920-counter-missing',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'pinyin'}),
            ('user-conf-920-counter-missing-zhuyin',
             (),
             {'control': True, 'issues': ('920',), 'mode': 'zhuyin'}),
            ('user-conf-marker-unwritable', (), {'issues': ('920',), 'mode': 'pinyin', 'stderr': True}),
            ('user-conf-marker-unwritable-zhuyin', (), {'issues': ('920',), 'mode': 'zhuyin', 'stderr': True}),
            ('abort-mask-out-overlong-index-key', (), {'abort': False, 'issues': ('525', 'row:84')}),
            ('abort-zhuyin-mask-out-overlong-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84'), 'mode': 'zhuyin'}),
            ('abort-guess-candidates-overlong-index-key', (), {'abort': False, 'issues': ('525', 'row:84')}),
            ('abort-guess-candidates-overlong-odd-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84')}),
            ('abort-mask-out-overlong-odd-index-key', (), {'abort': False, 'issues': ('525', 'row:84')}),
            ('abort-mask-out-empty-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84'), 'userdir': True}),
            ('abort-mask-out-one-byte-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84'), 'userdir': True}),
            ('abort-zhuyin-mask-out-empty-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84'), 'mode': 'zhuyin', 'userdir': True}),
            ('abort-guess-candidates-overlong-system-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84'), 'userdir': True}),
            ('abort-guess-candidates-overlong-odd-system-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84'), 'userdir': True}),
            ('guess-after-overlong-index-key-without-prefix-answers-true', (), {'issues': ('525', 'row:84')}),
            ('reparse-after-guess-ignores-overlong-index-key', (), {'issues': ('525', 'row:84')}),
            ('abort-guess-candidates-overlong-incomplete-index-key',
             (),
             {'abort': False, 'issues': ('525', 'row:84')})]
INJECTED = ('_craft_db',)


def stub(name):
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, _craft_db=state['_craft_db'])
    return cases


class UserConfOverlongIndexTests(unittest.TestCase):
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
