#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the token reuse and addon import cases (#1175)."""
import inspect
import unittest
from unittest.mock import Mock

import contract_token_reuse_addon_import as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('issue762-token-reuse-no-mask-pinyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-all-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-highest-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-middle-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-twice-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-import-mask-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-reopen-pinyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-unsaved-reopen-pinyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-mask-reopen-middle-pinyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-stale-bigram-pinyin',
             (),
             {'cells': ('bdb',), 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-stale-bigram-no-mask-pinyin',
             (),
             {'cells': ('bdb',), 'control': True, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-token-reuse-no-mask-zhuyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-all-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-highest-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-middle-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-twice-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-import-mask-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-reopen-zhuyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-unsaved-reopen-zhuyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-mask-reopen-middle-zhuyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-stale-bigram-zhuyin',
             (),
             {'cells': ('bdb',), 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-token-reuse-stale-bigram-no-mask-zhuyin',
             (),
             {'cells': ('bdb',), 'control': True, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-addon-import-no-mask-pinyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-addon-import-mask-highest-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-addon-import-mask-middle-pinyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'pinyin'}),
            ('issue762-addon-import-no-mask-zhuyin',
             (),
             {'control': True, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-addon-import-mask-highest-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('issue762-addon-import-mask-middle-zhuyin',
             (),
             {'control': False, 'issues': ('762',), 'mode': 'zhuyin'}),
            ('addon-promoted-listing-unsorted', (), {'issues': ('913',), 'mode': 'pinyin'}),
            ('addon-promoted-listing-by-frequency',
             (),
             {'control': True, 'issues': ('913',), 'mode': 'pinyin'})]
INJECTED = ('token_reuse_sequence',)


def stub(name):
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, token_reuse_sequence=state['token_reuse_sequence'])
    return cases


class TokenReuseAddonImportTests(unittest.TestCase):
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
