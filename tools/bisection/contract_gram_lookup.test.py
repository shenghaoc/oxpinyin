#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the gram lookup cases (#1175)."""
import inspect
import unittest
from unittest.mock import Mock

import contract_gram_lookup as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('user-gram-unsorted-guess', (), {'control': False, 'issues': ('757',)}),
            ('user-gram-unsorted-dynamic-adjust', (), {'control': False, 'issues': ('757',)}),
            ('user-gram-unsorted-phrase-segment', (), {'control': False, 'issues': ('757',), 'policy_record': '1297-gram-partition-segmentation.md'}),
            ('user-gram-duplicate-guess', (), {'control': False, 'issues': ('757',)}),
            ('user-gram-duplicate-dynamic-adjust', (), {'control': False, 'issues': ('757',)}),
            ('user-gram-duplicate-phrase-segment', (), {'control': True, 'issues': ('757',)}),
            ('user-gram-unsorted-range-boundary', (), {'issues': ('757',)}),
            ('system-gram-unsorted-guess', (), {'issues': ('757',)}),
            ('system-gram-duplicate-guess', (), {'issues': ('757',)}),
            ('user-gram-zero-count-guess', (), {'issues': ('757',)}),
            ('abort-guess-sentence-short-user-gram-system-row-pinyin',
             (),
             {'abort': False, 'issues': ('757',), 'mode': 'pinyin'}),
            ('abort-guess-sentence-short-user-gram-system-row-zhuyin',
             (),
             {'abort': False, 'issues': ('757',), 'mode': 'zhuyin'}),
            ('abort-guess-sentence-short-user-gram-no-system-row', (), {'abort': False, 'issues': ('757',)}),
            ('abort-guess-candidates-dynamic-adjust-short-user-gram-pinyin',
             (),
             {'abort': False, 'issues': ('757',), 'mode': 'pinyin'}),
            ('abort-guess-candidates-dynamic-adjust-short-user-gram-zhuyin',
             (),
             {'abort': False, 'issues': ('757',), 'mode': 'zhuyin'}),
            ('parse-dynamic-adjust-short-user-gram-pinyin', (), {'issues': ('757',), 'mode': 'pinyin'}),
            ('parse-dynamic-adjust-short-user-gram-zhuyin', (), {'issues': ('757',), 'mode': 'zhuyin'}),
            ('abort-phrase-segment-short-user-gram', (), {'abort': False, 'issues': ('757',)}),
            ('abort-phrase-segment-prefix-short-user-gram-pinyin',
             (),
             {'abort': False, 'issues': ('918',), 'mode': 'pinyin'}),
            ('phrase-segment-prefix-sound-user-gram-pinyin',
             (),
             {'control': True, 'issues': ('918',), 'mode': 'pinyin', 'stderr': True}),
            ('phrase-segment-nonscalar-before-short-user-gram-pinyin',
             (),
             {'control': True, 'issues': ('918',), 'mode': 'pinyin', 'stderr': True}),
            ('phrase-segment-prefix-unrelated-short-user-gram-pinyin',
             (),
             {'control': True, 'issues': ('918',), 'mode': 'pinyin', 'stderr': True}),
            ('abort-phrase-segment-prefix-short-user-gram-zhuyin',
             (),
             {'abort': False, 'issues': ('918',), 'mode': 'zhuyin'}),
            ('phrase-segment-prefix-sound-user-gram-zhuyin',
             (),
             {'control': True, 'issues': ('918',), 'mode': 'zhuyin', 'stderr': True}),
            ('phrase-segment-nonscalar-before-short-user-gram-zhuyin',
             (),
             {'control': True, 'issues': ('918',), 'mode': 'zhuyin', 'stderr': True}),
            ('phrase-segment-prefix-unrelated-short-user-gram-zhuyin',
             (),
             {'control': True, 'issues': ('918',), 'mode': 'zhuyin', 'stderr': True}),
            ('phrase-segment-nonscalar-raw-key-pinyin',
             (),
             {'issues': ('918',), 'mode': 'pinyin', 'stderr': True}),
            ('phrase-segment-nonscalar-raw-key-zhuyin',
             (),
             {'issues': ('918',), 'mode': 'zhuyin', 'stderr': True}),
            ('guess-unrelated-short-user-gram', (), {'control': True, 'issues': ('757',)}),
            ('guess-candidates-short-user-gram-dynamic-adjust-off', (), {'control': True, 'issues': ('757',)})]
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


class GramLookupTests(unittest.TestCase):
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
