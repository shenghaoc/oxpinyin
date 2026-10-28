#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the registration and source equivalence check."""
import functools
import importlib.util
from pathlib import Path
import tempfile
import types
import unittest
from unittest.mock import patch
import sys

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
        for name, replacement in (('co_name', 'changed'),
                                  ('co_code', code.co_code + b'\x09\x00'),
                                  ('co_exceptiontable', code.co_exceptiontable + b'\x00'),
                                  ('co_argcount', 0), ('co_posonlyargcount', 1),
                                  ('co_flags', code.co_flags ^ (1 << 20)), ('co_stacksize', code.co_stacksize + 1),
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

    def test_moved_source_bindings_and_owner_are_checked(self):
        base = dict(records=[], tags='t', skips='s', selections=[], helpers={},
                    moved={'a': {'module': 'contract_diff', 'shape': {'ast': 'a', 'bindings': {'x': 1}}}})
        head = {**base, 'moved': {'a': {'module': 'family', 'shape': {'ast': 'a', 'bindings': {'x': 1}}}}}
        equivalence.compare_snapshots(base, head, {'a': 'family'})
        for moved in ({'a': {'module': 'family', 'shape': {'ast': 'a', 'bindings': {'x': 2}}}},
                      {'a': {'module': 'wrong', 'shape': {'ast': 'a', 'bindings': {'x': 1}}}}):
            with self.subTest(moved=moved), self.assertRaises(AssertionError):
                equivalence.compare_snapshots(base, {**head, 'moved': moved}, {'a': 'family'})

    def test_retained_helper_registrations_are_compared(self):
        base = dict(records=[], tags='t', skips='s', selections=[], helpers={}, moved={},
                    retained={'family:helper=1': {'cases': ['one'], 'shape': 'original'}})
        equivalence.compare_snapshots(base, dict(base), 'family')
        for change in ({'cases': ['two'], 'shape': 'original'}, {'cases': ['one'], 'shape': 'changed'}):
            with self.subTest(change=change), self.assertRaisesRegex(AssertionError, 'retained driver helpers'):
                equivalence.compare_snapshots(base, {**base, 'retained': {'family:helper=1': change}}, 'family')

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

    def test_a_lambda_argument_is_compared_as_its_own_expression(self):
        with tempfile.TemporaryDirectory() as tmp:
            sources = {'before': 'register(\n    lambda k, a=1: run(k, a))\n',
                       'after': 'x = 0\nregister(\n    lambda k, a=1: run(k, a))\n',
                       'changed': 'register(\n    lambda k, a=2: run(k, a))\n',
                       'paired': 'register(lambda k: k); register(lambda k: (k, k))\n'}
            found = {}
            for name, text in sources.items():
                path = Path(tmp) / (name + '.py')
                path.write_text('captured = []\nregister = captured.append\n' + text)
                module_spec = importlib.util.spec_from_file_location(name, path)
                module = importlib.util.module_from_spec(module_spec)
                module_spec.loader.exec_module(module)
                found[name] = module.captured[0]
            self.assertEqual(equivalence.function_ast(found['before']), equivalence.function_ast(found['after']))
            self.assertNotEqual(equivalence.function_ast(found['before']),
                                equivalence.function_ast(found['changed']))
            with self.assertRaisesRegex(TypeError, 'expected one lambda'):
                equivalence.function_ast(found['paired'])

    def test_moved_closures_compare_source_bindings_and_driver_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            common = ('FACTOR = 7\n\ndef helper(x):\n    return x + FACTOR\n\n'
                      'def inner_helper(x):\n    return x * 2\n\n')
            original = common + 'def moved(k):\n    return helper(inner_helper(k))\n'
            extracted = common + ('def register(*, helper):\n'
                                  '    def moved(k):\n        return helper(inner_helper(k))\n'
                                  '    return moved\n')
            modules = []
            for name, source in (('contract_diff', original), ('family', extracted)):
                path = root / (name + '.py')
                path.write_text(source)
                module_spec = importlib.util.spec_from_file_location(name, path)
                module = importlib.util.module_from_spec(module_spec)
                module_spec.loader.exec_module(module)
                modules.append(module)
            before, after = modules
            driver = types.ModuleType('contract_diff')
            driver.helper = after.helper
            reference = equivalence.function_shape(before.moved, before, self.sentinel, ('helper',), ())
            candidate = after.register(helper=driver.helper)
            self.assertEqual(reference, equivalence.function_shape(candidate, driver, self.sentinel,
                                                                    ('helper',), ('family',)))
            after.FACTOR = 8
            self.assertNotEqual(reference, equivalence.function_shape(candidate, driver, self.sentinel,
                                                                       ('helper',), ('family',)))
            after.FACTOR = 7
            with self.assertRaisesRegex(TypeError, 'not the driver binding'):
                equivalence.function_shape(after.register(helper=before.helper), driver, self.sentinel,
                                           ('helper',), ('family',))
            with self.assertRaisesRegex(TypeError, 'must be declared as injected'):
                equivalence.function_shape(candidate, driver, self.sentinel, (), ('family',))
            after.inner_helper = lambda x: x * 3
            self.assertNotEqual(reference, equivalence.function_shape(candidate, driver, self.sentinel,
                                                                       ('helper',), ('family',)))
            del after.inner_helper
            with self.assertRaisesRegex(TypeError, 'unresolved global'):
                equivalence.function_shape(candidate, driver, self.sentinel, ('helper',), ('family',))

    def test_unknown_bindings_and_recursive_helpers_fail_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'contract_diff.py'
            path.write_text('def reads(k):\n    return UNKNOWN\n\n'
                            'def recursive(k):\n    return recursive(k)\n')
            module_spec = importlib.util.spec_from_file_location('contract_diff', path)
            driver = importlib.util.module_from_spec(module_spec)
            module_spec.loader.exec_module(driver)
            driver.UNKNOWN = object()
            with self.assertRaisesRegex(TypeError, 'unsupported inventory value'):
                equivalence.function_shape(driver.reads, driver, self.sentinel, (), ())
            with self.assertRaisesRegex(TypeError, 'recursive helper graph'):
                equivalence.function_shape(driver.recursive, driver, self.sentinel, (), ())

    def test_the_injected_registry_is_compared_by_identity_with_the_driver_registry(self):
        driver = types.ModuleType('contract_diff')
        driver.CASES = {'a': {'fn': make_function(1)}}
        def reads(k):
            return CASES
        reads.__globals__['CASES'] = driver.CASES
        shape = equivalence.function_shape(reads, driver, self.sentinel, ('CASES',), ())
        self.assertEqual(shape['bindings']['CASES'], ['injected-registry'])
        driver.CASES['b'] = {'fn': make_function(2)}
        self.assertEqual(equivalence.function_shape(reads, driver, self.sentinel, ('CASES',), ()), shape)
        reads.__globals__['CASES'] = dict(driver.CASES)
        with self.assertRaisesRegex(TypeError, 'not the driver binding'):
            equivalence.function_shape(reads, driver, self.sentinel, ('CASES',), ())
        del reads.__globals__['CASES']

    def test_registration_parameters_are_the_keyword_only_parameters_of_register_functions(self):
        module = types.ModuleType('family_probe')
        exec('def register(case, *, CASES, helper):\n    pass\n\n'
             'def register_2(case, *, other):\n    pass\n\n'
             'def unrelated(*, hidden):\n    pass\n', module.__dict__)
        with patch.dict(sys.modules, family_probe=module):
            self.assertEqual(equivalence.registration_parameters(['family_probe']), {'CASES', 'helper', 'other'})

    def test_partial_wrapped_cases_compare_every_layer_of_bound_arguments(self):
        def run(k, blocked=None, *, boundary=None):
            return k
        base = functools.partial(functools.partial(run, blocked='user.bin'), boundary='save-marker')
        same = functools.partial(functools.partial(run, blocked='user.bin'), boundary='save-marker')
        other = functools.partial(functools.partial(run, blocked='user.db'), boundary='save-marker')
        self.assertEqual(equivalence.partial_layers(base, self.sentinel), equivalence.partial_layers(same, self.sentinel))
        self.assertNotEqual(equivalence.partial_layers(base, self.sentinel), equivalence.partial_layers(other, self.sentinel))
        self.assertEqual(equivalence.partial_layers(run, self.sentinel), [])
        with self.assertRaisesRegex(TypeError, 'wrapped'):
            equivalence.partial_layers(functools.partial(len), self.sentinel)

    def test_an_injected_path_compares_relative_to_the_driver_and_fails_outside_it(self):
        shapes = []
        with tempfile.TemporaryDirectory() as tmp, patch.dict(sys.modules):
            for index in range(2):
                root = Path(tmp) / str(index)
                driver = self.moved_helper_tree(root, 'from pathlib import Path\n'
                                                'WORKER = Path(__file__).resolve().with_name("contract-worker.py")\n'
                                                'OUTSIDE = Path(__file__).resolve().parents[3]\n'
                                                'def case(k):\n    return WORKER\n\n'
                                                'def outside(k):\n    return OUTSIDE\n')
                shapes.append(equivalence.function_shape(driver.case, driver, self.sentinel, ('WORKER',), ()))
                with self.assertRaisesRegex(TypeError, 'outside the driver directory'):
                    equivalence.function_shape(driver.outside, driver, self.sentinel, ('OUTSIDE',), ())
        self.assertEqual(shapes[0], shapes[1])
        self.assertEqual(shapes[0]['bindings']['WORKER'], ['injected-path', 'contract-worker.py'])

    def test_a_default_function_compares_by_source_whether_or_not_it_is_nested(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(sys.modules):
            base = self.moved_helper_tree(Path(tmp, 'base'), 'def case(k, mutate=(lambda found: found + 1,)):\n'
                                          '    return mutate\n')
            head = self.moved_helper_tree(Path(tmp, 'head'), 'import family\ncase = family.register()\n', {
                'family': 'def register():\n    def case(k, mutate=(lambda found: found + 1,)):\n'
                          '        return mutate\n    return case\n'})
            changed = self.moved_helper_tree(Path(tmp, 'changed'), 'import family\ncase = family.register()\n', {
                'family': 'def register():\n    def case(k, mutate=(lambda found: found + 2,)):\n'
                          '        return mutate\n    return case\n'})
            shape = lambda driver: equivalence.function_shape(driver.case, driver, self.sentinel, (), ('family',))
            self.assertEqual(shape(base)['defaults'], shape(head)['defaults'])
            self.assertNotEqual(shape(base)['defaults'], shape(changed)['defaults'])

    def test_injected_driver_class_compares_its_definition(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(sys.modules):
            shapes = []
            for index, marker in enumerate((1, 2)):
                root = Path(tmp) / str(index)
                path = root / 'tools/bisection/contract-diff.py'
                path.parent.mkdir(parents=True)
                path.write_text('class Rest:\n    marker = %d\n\n'
                                'def case(k):\n    return Rest\n' % marker)
                driver = equivalence.load_driver(root, 'bdb')
                shapes.append(equivalence.function_shape(driver.case, driver, self.sentinel, ('Rest',), ()))
            self.assertNotEqual(shapes[0], shapes[1])

    def test_module_counts_require_unique_positive_expectations(self):
        self.assertEqual(equivalence.module_counts(['one=1', 'two=2']), {'one': 1, 'two': 2})
        for specs in (['one'], ['one=0'], ['one=-1'], ['one=1', 'one=1'], ['bad-name=1']):
            with self.subTest(specs=specs), self.assertRaises(ValueError):
                equivalence.module_counts(specs)

    def test_retained_helper_specs_require_unique_modules_helpers_and_positive_counts(self):
        self.assertEqual(equivalence.retained_counts(['family:helper=2']), {'family:helper=2': ('family', 'helper', 2)})
        for specs in (['family=1'], ['family:helper=0'], ['bad-name:helper=1'],
                      ['family:helper=1', 'family:helper=1'], ['one:helper=1', 'two:helper=1']):
            with self.subTest(specs=specs), self.assertRaises(ValueError):
                equivalence.retained_counts(specs)

    def test_retained_helper_identifies_only_callbacks_created_from_its_code(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(sys.modules):
            root = Path(tmp)
            path = root / 'tools/bisection/contract-diff.py'
            path.parent.mkdir(parents=True)
            path.write_text('NO_ABORT = object()\n\n'
                            'def helper(value):\n    def run(k):\n        return value\n    return run\n\n'
                            'def other(k):\n    return 3\n\n'
                            'CASES = {"one": {"fn": helper(1)}, "two": {"fn": helper(2)},\n'
                            '         "other": {"fn": other}}\n')
            driver = equivalence.load_driver(root, 'bdb')
            result = equivalence.retained_helpers(driver, ['family:helper=2'])
            self.assertEqual(result['family:helper=2']['cases'], ['one', 'two'])
            with self.assertRaisesRegex(AssertionError, 'has 2 distinct retained cases, expected 1'):
                equivalence.retained_helpers(driver, ['family:helper=1'])
            driver.CASES['two']['fn'] = driver.other
            with self.assertRaisesRegex(AssertionError, 'has 1 distinct retained cases, expected 2'):
                equivalence.retained_helpers(driver, ['family:helper=2'])
            driver.helper.__module__ = 'elsewhere'
            with self.assertRaisesRegex(TypeError, 'not a driver function'):
                equivalence.retained_helpers(driver, ['family:helper=1'])

    def test_driver_file_binding_is_normalized_only_for_the_actual_driver(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(sys.modules):
            root = Path(tmp)
            path = root / 'tools/bisection/contract-diff.py'
            path.parent.mkdir(parents=True)
            path.write_text('def read_path(k):\n    return __file__\n')
            driver = equivalence.load_driver(root, 'bdb')
            result = equivalence.function_shape(driver.read_path, driver, self.sentinel, (), ())
            self.assertEqual(result['bindings']['__file__'], ['driver-file'])
            driver.__file__ = str(root / 'wrong.py')
            with self.assertRaisesRegex(TypeError, 'unrecognized __file__ binding'):
                equivalence.function_shape(driver.read_path, driver, self.sentinel, (), ())

    def moved_helper_tree(self, root, driver_source, helpers=None, extra=None):
        """Write a driver (and optional helper modules) and load the driver as contract_diff."""
        directory = root / 'tools/bisection'
        directory.mkdir(parents=True)
        (directory / 'contract-diff.py').write_text('NO_ABORT = object()\nCASES = {}\n' + driver_source)
        for name, source in (helpers or {}).items():
            (directory / (name + '.py')).write_text(source)
        for name in (helpers or {}):
            sys.modules.pop(name, None)
        return equivalence.load_driver(root, 'bdb')

    def test_a_helper_moved_unchanged_into_a_declared_module_compares_equal(self):
        helper = 'LIMIT = 3\n\ndef scale(x):\n    """Scale."""\n    return x * LIMIT\n'
        with tempfile.TemporaryDirectory() as tmp, patch.dict(sys.modules):
            base = self.moved_helper_tree(Path(tmp, 'base'), helper)
            before = equivalence.moved_helper_shapes(base, ('scale', 'LIMIT'), ('shared',), ())
            head = self.moved_helper_tree(Path(tmp, 'head'),
                                          'import shared\nscale = shared.scale\nLIMIT = shared.LIMIT\n',
                                          {'shared': helper})
            after = equivalence.moved_helper_shapes(head, ('scale', 'LIMIT'), ('shared',), ('shared',))
        self.assertEqual({name: entry['module'] for name, entry in before.items()},
                         {'scale': 'contract_diff', 'LIMIT': 'contract_diff'})
        self.assertEqual({name: entry['module'] for name, entry in after.items()}, {'scale': 'shared', 'LIMIT': 'shared'})
        equivalence.compare_moved_helpers(before, after, ('shared',))

    def test_a_changed_or_unmoved_helper_fails_closed(self):
        helper = 'LIMIT = 3\n\ndef scale(x):\n    return x * LIMIT\n'
        with tempfile.TemporaryDirectory() as tmp, patch.dict(sys.modules):
            base = self.moved_helper_tree(Path(tmp, 'base'), helper)
            before = equivalence.moved_helper_shapes(base, ('scale', 'LIMIT'), ('shared',), ())
            heads = {
                'body': ('import shared\nscale = shared.scale\nLIMIT = shared.LIMIT\n',
                         {'shared': helper.replace('x * LIMIT', 'x + LIMIT')}),
                'value': ('import shared\nscale = shared.scale\nLIMIT = shared.LIMIT\n',
                          {'shared': helper.replace('LIMIT = 3', 'LIMIT = 4')}),
                'unmoved': (helper, {'shared': helper}),
            }
            for label, (driver_source, modules) in heads.items():
                with self.subTest(label=label):
                    head = self.moved_helper_tree(Path(tmp, label), driver_source, modules)
                    after = equivalence.moved_helper_shapes(head, ('scale', 'LIMIT'), ('shared',), ('shared',))
                    with self.assertRaises(AssertionError):
                        equivalence.compare_moved_helpers(before, after, ('shared',))
            with self.assertRaises(AssertionError):
                equivalence.compare_moved_helpers(before, {'scale': before['scale']}, ('shared',))

    def test_a_helper_in_an_undeclared_or_ambiguous_module_fails_closed(self):
        helper = 'LIMIT = 3\n\ndef scale(x):\n    return x * LIMIT\n'
        with tempfile.TemporaryDirectory() as tmp, patch.dict(sys.modules):
            head = self.moved_helper_tree(Path(tmp, 'undeclared'), 'import other\nscale = other.scale\n',
                                          {'other': helper})
            with self.assertRaisesRegex(TypeError, 'not one declared helper module'):
                equivalence.moved_helper_shapes(head, ('scale',), ('shared',), ('shared',))
            twice = self.moved_helper_tree(
                Path(tmp, 'twice'), 'import first, second\nLIMIT = first.LIMIT\n',
                {'first': 'LIMIT = (3,)\n', 'second': 'import first\nLIMIT = first.LIMIT\n'})
            with self.assertRaisesRegex(TypeError, 'several helper modules'):
                equivalence.moved_helper_shapes(twice, ('LIMIT',), ('first', 'second'), ('first', 'second'))
            with self.assertRaisesRegex(TypeError, 'does not bind'):
                equivalence.moved_helper_shapes(twice, ('missing',), ('first',), ('first',))

    def test_discovery_fails_when_a_module_has_the_wrong_case_count(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(sys.modules):
            root = Path(tmp)
            path = root / 'tools/bisection/contract-diff.py'
            path.parent.mkdir(parents=True)
            path.write_text('def example(k):\n    pass\n'
                            'example.__module__ = "family"\n'
                            'CASES = {"case-one": {"fn": example}}\n')
            self.assertEqual(equivalence.discover(root, ['family=1']), {'family': ['case-one']})
            with self.assertRaisesRegex(AssertionError, 'has 1 cases, expected 2'):
                equivalence.discover(root, ['family=2'])
            with self.assertRaisesRegex(AssertionError, 'has 0 cases, expected 1'):
                equivalence.discover(root, ['missing=1'])


if __name__ == '__main__':
    unittest.main()
