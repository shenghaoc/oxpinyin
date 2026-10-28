#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the user index and phrase mask cases (#1175)."""
import inspect
import unittest
from unittest.mock import Mock

import contract_user_index_phrase_mask as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('issue784-index-keys-no-mask-pinyin',
             (),
             {'control': True, 'issues': ('784',), 'mode': 'pinyin'}),
            ('issue784-index-keys-mask-save-pinyin',
             (),
             {'control': True, 'issues': ('784',), 'mode': 'pinyin'}),
            ('issue784-index-keys-mask-middle-pinyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'pinyin'}),
            ('issue784-index-keys-mask-highest-pinyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'pinyin'}),
            ('issue784-index-keys-mask-middle-highest-pinyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'pinyin'}),
            ('issue784-index-keys-mask-unsaved-pinyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'pinyin'}),
            ('issue784-index-keys-mask-all-pinyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'pinyin'}),
            ('issue784-index-keys-mask-reopen-pinyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'pinyin'}),
            ('issue784-index-keys-mask-lookup-pinyin',
             (),
             {'control': True, 'issues': ('784',), 'mode': 'pinyin'}),
            ('issue784-index-keys-mask-reopen-lookup-pinyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'pinyin'}),
            ('issue784-index-keys-no-mask-zhuyin',
             (),
             {'control': True, 'issues': ('784',), 'mode': 'zhuyin'}),
            ('issue784-index-keys-mask-save-zhuyin',
             (),
             {'control': True, 'issues': ('784',), 'mode': 'zhuyin'}),
            ('issue784-index-keys-mask-middle-zhuyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'zhuyin'}),
            ('issue784-index-keys-mask-highest-zhuyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'zhuyin'}),
            ('issue784-index-keys-mask-middle-highest-zhuyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'zhuyin'}),
            ('issue784-index-keys-mask-unsaved-zhuyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'zhuyin'}),
            ('issue784-index-keys-mask-all-zhuyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'zhuyin'}),
            ('issue784-index-keys-mask-reopen-zhuyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'zhuyin'}),
            ('issue784-index-keys-mask-lookup-zhuyin',
             (),
             {'control': True, 'issues': ('784',), 'mode': 'zhuyin'}),
            ('issue784-index-keys-mask-reopen-lookup-zhuyin',
             (),
             {'control': False, 'issues': ('784',), 'mode': 'zhuyin'}),
            ('issue784-index-keys-remove-save', (), {'control': True, 'issues': ('784',)}),
            ('issue784-index-keys-remove-middle', (), {'control': False, 'issues': ('784',)}),
            ('issue784-index-keys-remove-unsaved', (), {'control': False, 'issues': ('784',)}),
            ('issue784-index-keys-remove-reopen', (), {'control': False, 'issues': ('784',)}),
            ('issue784-index-keys-remove-lookup', (), {'control': True, 'issues': ('784',)}),
            ('issue784-index-keys-remove-reopen-lookup', (), {'control': False, 'issues': ('784',)}),
            ('issue785-pinyin-tails-add-pinyin', (), {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-hit-pinyin', (), {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-hit-shrink-pinyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-suggestion-pinyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-mask-twice-pinyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-hit-tailed-pinyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-hit-order-pinyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-hit-order-reverse-pinyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-remove-pinyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-markers-pinyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-markers-hit-pinyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-reopen-pinyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-reopen-mask-pinyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-lookup-pinyin',
             (),
             {'control': True, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-mask-none-pinyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-mask-all-pinyin',
             (),
             {'control': True, 'issues': ('785',), 'mode': 'pinyin'}),
            ('issue785-pinyin-tails-add-zhuyin', (), {'control': False, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-hit-zhuyin', (), {'control': False, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-hit-shrink-zhuyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-suggestion-zhuyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-mask-twice-zhuyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-hit-tailed-zhuyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-hit-order-zhuyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-hit-order-reverse-zhuyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-markers-zhuyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-markers-hit-zhuyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-reopen-zhuyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-reopen-mask-zhuyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-lookup-zhuyin',
             (),
             {'control': True, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-mask-none-zhuyin',
             (),
             {'control': False, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-mask-all-zhuyin',
             (),
             {'control': True, 'issues': ('785',), 'mode': 'zhuyin'}),
            ('issue785-pinyin-tails-kept-entry-zhuyin', (), {'issues': ('785', '814'), 'mode': 'zhuyin'}),
            ('abort-guess-predicted-short-bigram-value', (), {'abort': False}),
            ('abort-guess-predicted-empty-bigram-gram', (), {'abort': False}),
            ('guess-predicted-unrelated-short-bigram-value', (), {'control': True}),
            ('guess-predicted-unrelated-empty-bigram-gram', (), {'control': True})]
INJECTED = ('predicted_bigram_context', 'token_reuse_files')


def stub(name):
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, predicted_bigram_context=state['predicted_bigram_context'], token_reuse_files=state['token_reuse_files'])
    return cases


class UserIndexPhraseMaskTests(unittest.TestCase):
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
