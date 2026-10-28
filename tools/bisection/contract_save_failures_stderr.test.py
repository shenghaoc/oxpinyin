#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the save failures and stderr cases (#1175)."""
import collections
import inspect
import unittest
from unittest.mock import Mock

import contract_save_failures_stderr as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('stderr-save-one-tmp-blocked', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-save-one-final-blocked', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-save-one-tmp-blocked-zhuyin', (), {'issues': ('545',), 'mode': 'zhuyin', 'stderr': True}),
            ('save-929-rename-user.bin-pinyin', (), {'issues': ('929',), 'mode': 'pinyin', 'stderr': True}),
            ('save-929-rename-user_pinyin_index.bin-pinyin',
             (),
             {'issues': ('929',), 'mode': 'pinyin', 'stderr': True}),
            ('save-929-rename-user_phrase_index.bin-pinyin',
             (),
             {'issues': ('929',), 'mode': 'pinyin', 'stderr': True}),
            ('save-929-rename-user_bigram.db-pinyin',
             (),
             {'issues': ('929',), 'mode': 'pinyin', 'stderr': True}),
            ('save-929-reload-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-sync-rejecting-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-rename-user.bin-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin', 'stderr': True}),
            ('save-929-rename-user_pinyin_index.bin-zhuyin',
             (),
             {'issues': ('929',), 'mode': 'zhuyin', 'stderr': True}),
            ('save-929-rename-user_phrase_index.bin-zhuyin',
             (),
             {'issues': ('929',), 'mode': 'zhuyin', 'stderr': True}),
            ('save-929-rename-user_bigram.db-zhuyin',
             (),
             {'issues': ('929',), 'mode': 'zhuyin', 'stderr': True}),
            ('save-929-reload-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin'}),
            ('save-929-sync-rejecting-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin'}),
            ('save-929-boundary-save-marker-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-boundary-after-save-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-boundary-after-fini-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-boundary-fini-marker-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-logical-cross-open-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-boundary-save-marker-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin'}),
            ('save-929-boundary-after-save-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin'}),
            ('save-929-boundary-after-fini-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin'}),
            ('save-929-logical-cross-open-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin'}),
            ('abort-save-chunk-header-length', (), {'abort': False, 'issues': ('525',)}),
            ('abort-save-chunk-header-checksum', (), {'abort': False, 'issues': ('525',)}),
            ('abort-zhuyin-save-chunk-header-length',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('abort-zhuyin-save-chunk-header-checksum',
             (),
             {'abort': False, 'issues': ('525',), 'mode': 'zhuyin'}),
            ('stderr-fresh-user-dir', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-fresh-user-dir-zhuyin', (), {'issues': ('545',), 'mode': 'zhuyin', 'stderr': True}),
            ('stderr-init-missing-system-dir', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-init-missing-system-dir-zhuyin',
             (),
             {'issues': ('545',), 'mode': 'zhuyin', 'stderr': True}),
            ('stderr-init-system-dir-without-table-conf', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-save-dir-removed', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-save-dir-removed-zhuyin', (), {'issues': ('545',), 'mode': 'zhuyin', 'stderr': True}),
            ('stderr-fini-dir-removed', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-fini-dir-removed-zhuyin', (), {'issues': ('545',), 'mode': 'zhuyin', 'stderr': True}),
            ('stderr-non-utf8-user-dir', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-quiet-saves', (), {'issues': ('545',), 'stderr': True}),
            ('stderr-unmodified-save-dir-removed', (), {'issues': ('545',), 'stderr': True})]
INJECTED = ('CASES', 'crafted_bigram_context', 'index_file_rows', 'token_reuse_sequence')


def stub(name):
    return collections.defaultdict(dict) if name == 'CASES' else Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, CASES=state['CASES'], crafted_bigram_context=state['crafted_bigram_context'], index_file_rows=state['index_file_rows'], token_reuse_sequence=state['token_reuse_sequence'])
    return cases


class SaveFailuresStderrTests(unittest.TestCase):
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
