#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the registration and source equivalence check."""
import functools
import importlib.util
from pathlib import Path
import tempfile
import types
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('contract_equivalence', HERE / 'contract_equivalence.py')
equivalence = importlib.util.module_from_spec(spec)
spec.loader.exec_module(equivalence)


def make_function(value, default=1):
    def probe(argument=default):
        return value + argument
    return probe


class EquivalenceTests(unittest.TestCase):
    def setUp(self):
        self.sentinel = object()

    def shape(self, value):
        return equivalence.value_shape(value, self.sentinel)

    def test_code_defaults_closures_and_partials_are_compared_but_module_is_not(self):
        left, right = make_function(3), make_function(3)
        right.__module__ = 'different_module'
        self.assertEqual(self.shape(left), self.shape(right))
        self.assertNotEqual(self.shape(left), self.shape(make_function(4)))
        self.assertNotEqual(self.shape(left), self.shape(make_function(3, 2)))
        self.assertNotEqual(self.shape(functools.partial(left, 1)), self.shape(functools.partial(right, 2)))

    def test_different_code_and_qualname_are_not_equal(self):
        def another(argument=1):
            return argument + 4
        self.assertNotEqual(self.shape(make_function(3)), self.shape(another))
        same = make_function(3)
        same.__qualname__ = 'other'
        self.assertNotEqual(self.shape(make_function(3)), self.shape(same))

    def test_each_requested_code_field_changes_the_fingerprint(self):
        original = make_function(3)
        code = original.__code__
        for name, replacement in (('co_code', code.co_code + b'\x09\x00'),
                                  ('co_consts', code.co_consts + ('changed',)),
                                  ('co_names', code.co_names + ('changed',)),
                                  ('co_varnames', ('changed',))):
            with self.subTest(field=name):
                changed_code = code.replace(**{name: replacement})
                changed = types.FunctionType(changed_code, original.__globals__, original.__name__,
                                             original.__defaults__, original.__closure__)
                changed.__qualname__ = original.__qualname__
                self.assertNotEqual(self.shape(original), self.shape(changed))

    def test_unsupported_inventory_values_fail_closed(self):
        with self.assertRaises(TypeError):
            self.shape(object())
        self.assertEqual(self.shape(self.sentinel), ['no-abort'])

    def test_registration_order_metadata_and_selections_are_checked(self):
        base = dict(records=[['a', 'x'], ['b', 'y']], tags='t', skips='s',
                    selections=[['default', 2, 'p']], moved={}, helpers={})
        equivalence.compare_snapshots(base, dict(base), 'family')
        for field, value in (('records', list(reversed(base['records']))),
                             ('records', [['a', 'changed'], ['b', 'y']]),
                             ('tags', 'changed'), ('skips', 'changed'),
                             ('selections', [['default', 2, 'changed']])):
            with self.subTest(field=field), self.assertRaises(AssertionError):
                equivalence.compare_snapshots(base, {**base, field: value}, 'family')

    def test_ast_keeps_decorators_and_body_and_only_normalizes_declared_keyword_state(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'probe.py'
            path.write_text('def original(x):\n    return x + state\n\n'
                            'def moved(x, *, state):\n    return x + state\n\n'
                            'def changed(x, *, state):\n    return x - state\n')
            module_spec = importlib.util.spec_from_file_location('probe', path)
            module = importlib.util.module_from_spec(module_spec)
            module_spec.loader.exec_module(module)
            original = equivalence.function_ast(module.original)
            moved = equivalence.function_ast(module.moved, ('state',))
            changed = equivalence.function_ast(module.changed, ('state',))
            # These examples use distinct names; normalize just that spelling.
            self.assertEqual(original.replace("name='original'", "name='moved'"), moved)
            self.assertNotEqual(moved.replace("name='moved'", "name='changed'"), changed)
            self.assertNotEqual(equivalence.function_ast(module.moved), moved)


if __name__ == '__main__':
    unittest.main()
