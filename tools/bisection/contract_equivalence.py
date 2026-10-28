#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Compare contract registrations and moved source in two checkout directories.

This is a light, source-only check. Each checkout is loaded in its own Python
process; no worker, shared library or differential is run. Specify case IDs
with --module and --case, or all cases in each module with --module-cases
MODULE=COUNT. --injected names the shared driver objects passed to registrations.
--retained-helper MODULE:HELPER=COUNT covers cases still created by a shared
driver helper. Helpers can also be named with --helper for source comparison.
--moved-helper NAME compares a driver definition that moved, unchanged, into one
of the --helper-module modules; those modules' functions are followed as driver
code on both sides. --registration-injected NAME is a driver binding a moved module's
registration code reads but no moved case function does (its effect is in the records);
it must be a keyword-only parameter of that module's register function.
"""
import argparse
import ast
import builtins
import dis
import functools
import hashlib
import importlib
import importlib.util
import inspect
import json
import os
from pathlib import Path, PurePath
import subprocess
import sys
import tempfile
import textwrap
import types


CELLS = ('bdb', 'kc', 'tkrzw')


def value_shape(value, sentinel):
    """Encode values without addresses, source locations or function modules.

    Unknown values fail closed. Functions include their code, defaults and
    closure recursively; partials include their bound arguments as well.
    """
    if value is sentinel:
        return ['no-abort']
    if value is None or type(value) in (bool, int, str):
        return [type(value).__name__, value]
    if type(value) is float:
        return ['float', value.hex()]
    if type(value) is complex:
        return ['complex', value.real.hex(), value.imag.hex()]
    if type(value) is bytes:
        return ['bytes', value.hex()]
    if value is Ellipsis:
        return ['ellipsis']
    if isinstance(value, slice):
        return ['slice', value_shape(value.start, sentinel), value_shape(value.stop, sentinel),
                value_shape(value.step, sentinel)]
    if isinstance(value, types.CodeType):
        return ['code', value.co_name, value.co_code.hex(), value.co_exceptiontable.hex(),
                value.co_argcount, value.co_posonlyargcount, value.co_kwonlyargcount, value.co_flags,
                value.co_stacksize, value_shape(value.co_consts, sentinel), list(value.co_names),
                list(value.co_varnames), list(value.co_freevars), list(value.co_cellvars)]
    if isinstance(value, types.FunctionType):
        return ['function', value.__qualname__, value_shape(value.__code__, sentinel),
                value_shape(value.__defaults__, sentinel), value_shape(value.__kwdefaults__, sentinel),
                [value_shape(cell.cell_contents, sentinel) for cell in value.__closure__ or ()]]
    if isinstance(value, functools.partial):
        return ['partial', value_shape(value.func, sentinel), value_shape(value.args, sentinel),
                value_shape(value.keywords, sentinel)]
    if isinstance(value, type):
        return ['type', value.__module__, value.__qualname__]
    if type(value) in (tuple, list):
        return [type(value).__name__, [value_shape(item, sentinel) for item in value]]
    if type(value) in (set, frozenset):
        items = [value_shape(item, sentinel) for item in value]
        return [type(value).__name__, sorted(items, key=canonical)]
    if type(value) is dict:
        return ['dict', [[value_shape(key, sentinel), value_shape(item, sentinel)] for key, item in value.items()]]
    raise TypeError('unsupported inventory value: %r (%s)' % (value, type(value).__name__))


def canonical(value):
    return json.dumps(value, ensure_ascii=True, separators=(',', ':'))


def digest(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def function_of(value):
    while isinstance(value, functools.partial):
        value = value.func
    if not isinstance(value, types.FunctionType):
        raise TypeError('case is not a Python function or partial: %r' % (value,))
    return value


def lambda_ast(fn):
    """The one lambda expression that starts on the function's first line of its file.

    inspect.getsource returns whole lines, which need not parse on their own.
    Two lambdas on one line cannot be told apart this way and fail closed."""
    tree = ast.parse(Path(fn.__code__.co_filename).read_text())
    nodes = [node for node in ast.walk(tree)
             if isinstance(node, ast.Lambda) and node.lineno == fn.__code__.co_firstlineno]
    if len(nodes) != 1:
        raise TypeError('expected one lambda on line %d for %s' % (fn.__code__.co_firstlineno, fn))
    return ast.dump(nodes[0], include_attributes=False)


def partial_layers(value, sentinel):
    """The bound arguments of each functools.partial around a case function, outermost first.

    Any other wrapper fails closed: only the function underneath is compared by source."""
    layers = []
    while isinstance(value, functools.partial):
        layers.append([value_shape(value.args, sentinel), value_shape(value.keywords, sentinel)])
        value = value.func
    if not isinstance(value, types.FunctionType):
        raise TypeError('moved case is wrapped: %r' % (value,))
    return layers


def function_ast(fn, injected=()):
    """Normalize only declared keyword-only state; keep decorators and bodies."""
    fn = function_of(fn)
    if fn.__name__ == '<lambda>':
        return lambda_ast(fn)
    node = ast.parse(textwrap.dedent(inspect.getsource(fn))).body[0]
    if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
        raise TypeError('expected a function definition for %s' % fn)
    kept = [(arg, default) for arg, default in zip(node.args.kwonlyargs, node.args.kw_defaults)
            if arg.arg not in injected]
    node.args.kwonlyargs = [arg for arg, _ in kept]
    node.args.kw_defaults = [default for _, default in kept]
    return ast.dump(node, include_attributes=False)


def global_reads(code):
    """Names read from globals, including reads in nested comprehensions."""
    names = {i.argval for i in dis.get_instructions(code) if i.opname == 'LOAD_GLOBAL'}
    for constant in code.co_consts:
        if isinstance(constant, types.CodeType):
            names.update(global_reads(constant))
    return names


def function_shape(fn, driver, sentinel, injected, moved_modules, active=(), used=None):
    """Compare source and actual bindings, allowing driver globals to become closures.

    Every injected value must be the driver's object. Unknown values, unresolved
    globals and recursive helper graphs fail closed rather than being omitted.
    """
    if not isinstance(fn, types.FunctionType):
        raise TypeError('expected a Python function, got %r' % (fn,))
    if fn in active:
        raise TypeError('recursive helper graph: %s' % fn.__qualname__)
    if fn.__builtins__ is not vars(builtins):
        raise TypeError('custom builtins in %s' % fn.__qualname__)
    if used is None:
        used = set()
    active = (*active, fn)
    closure = dict(zip(fn.__code__.co_freevars, (cell.cell_contents for cell in fn.__closure__ or ())))
    names = global_reads(fn.__code__) | set(closure)
    bindings = {}
    for name in sorted(names):
        if name in closure:
            value = closure[name]
            if name in global_reads(fn.__code__):
                raise TypeError('both closure and global binding for %s' % name)
            if moved_modules and isinstance(value, (types.FunctionType, type)) and hasattr(driver, name):
                if name not in injected:
                    raise TypeError('captured driver binding %s must be declared as injected' % name)
                if value is not getattr(driver, name):
                    raise TypeError('captured %s is not the driver binding' % name)
        elif name in fn.__globals__:
            value = fn.__globals__[name]
        elif hasattr(builtins, name):
            value = getattr(builtins, name)
        else:
            raise TypeError('unresolved global %s in %s' % (name, fn.__qualname__))
        if name == '__file__':
            if (fn.__globals__ is not vars(driver) or not isinstance(value, str) or
                    Path(value).resolve() != Path(driver.__spec__.origin).resolve()):
                raise TypeError('unrecognized __file__ binding in %s' % fn.__qualname__)
            bindings[name] = ['driver-file']
        elif name in injected:
            used.add(name)
            if not hasattr(driver, name) or value is not getattr(driver, name):
                raise TypeError('injected %s is not the driver binding' % name)
            if value is getattr(driver, 'CASES', None):
                # The registry itself: its entries are the cases, compared record by record.
                bindings[name] = ['injected-registry']
            elif isinstance(value, PurePath):
                # A path derived from the driver's location (WORKER): compare it relative to the driver.
                home = Path(driver.__spec__.origin).resolve().parent
                try:
                    bindings[name] = ['injected-path', Path(value).resolve().relative_to(home).as_posix()]
                except ValueError:
                    raise TypeError('injected path %s is outside the driver directory' % name) from None
            elif isinstance(value, type) and value.__module__ == driver.__name__:
                tree = ast.parse(textwrap.dedent(inspect.getsource(value)))
                if len(tree.body) != 1 or not isinstance(tree.body[0], ast.ClassDef):
                    raise TypeError('expected a class definition for %s' % name)
                bindings[name] = ['injected-class', ast.dump(tree.body[0], include_attributes=False)]
            elif isinstance(value, types.FunctionType):
                bindings[name] = ['injected-function', value_shape(value, sentinel),
                                  function_shape(value, driver, sentinel, injected, moved_modules, active, used)]
            else:
                bindings[name] = ['injected', value_shape(value, sentinel)]
        elif isinstance(value, types.ModuleType):
            bindings[name] = ['module', value.__name__]
        elif isinstance(value, (types.BuiltinFunctionType, types.BuiltinMethodType)):
            bindings[name] = ['builtin', value.__module__, value.__qualname__]
        elif isinstance(value, types.FunctionType) and value.__module__ in (driver.__name__, *moved_modules):
            bindings[name] = ['function', function_shape(value, driver, sentinel, injected, moved_modules, active, used)]
        else:
            bindings[name] = value_shape(value, sentinel)
    defaults = bound_values(fn.__defaults__, driver, sentinel, injected, moved_modules, active, used)
    kwdefaults = bound_values(fn.__kwdefaults__, driver, sentinel, injected, moved_modules, active, used)
    return {'ast': function_ast(fn), 'defaults': defaults, 'kwdefaults': kwdefaults, 'bindings': bindings}


def bound_values(value, driver, sentinel, injected, moved_modules, active, used):
    """Shape default values, comparing a driver or moved-module function by its source and bindings.

    A function defined inside a moved register function gains a qualname prefix and the
    nested-code flag; its source and bindings are what must agree. Every other value is
    fingerprinted as before."""
    if isinstance(value, types.FunctionType) and value.__module__ in (driver.__name__, *moved_modules):
        return ['function', function_shape(value, driver, sentinel, injected, moved_modules, active, used)]
    if type(value) in (tuple, list):
        return [type(value).__name__, [bound_values(item, driver, sentinel, injected, moved_modules, active, used)
                                       for item in value]]
    if type(value) is dict:
        return ['dict', [[value_shape(key, sentinel),
                          bound_values(item, driver, sentinel, injected, moved_modules, active, used)]
                         for key, item in value.items()]]
    return value_shape(value, sentinel)


def load_driver(root, cell):
    """Load one checkout's driver under the module name its objects expect."""
    path = root / 'tools/bisection/contract-diff.py'
    sys.path.insert(0, str(path.parent))
    os.environ['CONTRACT_DIFF_CELL'] = cell
    spec = importlib.util.spec_from_file_location('contract_diff', path)
    driver = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = driver
    spec.loader.exec_module(driver)
    return driver


def class_ast(value):
    tree = ast.parse(textwrap.dedent(inspect.getsource(value)))
    if len(tree.body) != 1 or not isinstance(tree.body[0], ast.ClassDef):
        raise TypeError('expected a class definition for %s' % value.__qualname__)
    return ast.dump(tree.body[0], include_attributes=False)


def moved_helper_shapes(driver, names, helper_modules, followed):
    """Each named driver binding with its owner and its source or value shape.

    The owner is the driver for its own definitions and otherwise the one helper
    module that binds the same object; an object bound by no or several helper
    modules, or by a helper module that was not declared, fails closed."""
    result = {}
    for name in names:
        if not hasattr(driver, name):
            raise TypeError('driver does not bind %s' % name)
        value = getattr(driver, name)
        owners = [module for module in helper_modules
                  if module in sys.modules and getattr(sys.modules[module], name, object()) is value]
        if isinstance(value, (types.FunctionType, type)):
            module = value.__module__
            if module != driver.__name__ and owners != [module]:
                raise TypeError('%s is defined in %s, which is not one declared helper module' % (name, module))
        else:
            if len(owners) > 1:
                raise TypeError('%s is bound by several helper modules: %s' % (name, owners))
            module = owners[0] if owners else driver.__name__
        if isinstance(value, types.FunctionType):
            shape = ['function', function_shape(value, driver, driver.NO_ABORT, (), followed)]
        elif isinstance(value, type):
            shape = ['class', class_ast(value)]
        else:
            shape = ['value', value_shape(value, driver.NO_ABORT)]
        result[name] = {'module': module, 'shape': shape}
    return result


def registration_parameters(modules):
    """Keyword-only parameters of every register* function in the named modules."""
    names = set()
    for module in modules:
        for attr, value in vars(sys.modules[module]).items():
            if attr.startswith('register') and isinstance(value, types.FunctionType):
                names.update(p.name for p in inspect.signature(value).parameters.values()
                             if p.kind is inspect.Parameter.KEYWORD_ONLY)
    return names


def snapshot(root, cell, moved, helpers, injected, source_modules=(), retained_specs=(), moved_helpers=(),
             helper_modules=(), registration_injected=()):
    """Read the entire registry, selections, plans and specified source ASTs."""
    driver = load_driver(root, cell)
    for module in helper_modules:
        if (root / 'tools/bisection' / (module + '.py')).exists():
            importlib.import_module(module)
    source_modules = (*source_modules, *helper_modules)
    sentinel = driver.NO_ABORT
    records = []
    for name, case in driver.CASES.items():
        # The case's source and bindings are checked separately below; retain
        # every other registration field, in order, for all moved cases.
        value = {key: item for key, item in case.items() if key != 'fn'} if name in moved else case
        records.append([name, digest(value_shape(value, sentinel))])

    tags = driver.tag_index()
    names = list(driver.CASES)
    scenarios = [('default', None, (), ()),
                 ('all-reversed-duplicate', ','.join(list(reversed(names)) * 2), (), ())]
    for tag in tags:
        if tag.startswith('family/'):
            scenarios.append((tag, None, (), (tag[len('family/'):],)))
        else:
            scenarios.append(('issue/' + tag, None, (tag,), ()))
    issue = next(tag for tag in tags if not tag.startswith('family/'))
    family = next(tag[len('family/'):] for tag in tags if tag.startswith('family/'))
    scenarios.append(('combined', names[0] + ',' + names[0], (issue, issue), (family,)))
    selections = []
    for label, cases, issues, families in scenarios:
        selected, reasons = driver.select_cases(cases, issues, families)
        plan = driver.plan_lines(selected, reasons, Path('/stub/subject.so'), Path('/stub/zhuyin.so'),
                                 Path('/stub/prefix'), cell)
        selections.append([label, len(selected), digest([selected, reasons, plan])])
    moved_source, used_injected = {}, set()
    for name in moved:
        fn = function_of(driver.CASES[name]['fn'])
        moved_source[name] = {'module': fn.__module__, 'partial': partial_layers(driver.CASES[name]['fn'], sentinel),
                              'shape': function_shape(fn, driver, sentinel, injected, source_modules,
                                                      used=used_injected)}
    unused = set(injected) - used_injected - set(registration_injected)
    if unused:
        raise TypeError('unused injected names: %s' % sorted(unused))
    case_modules = [module for module in source_modules if module not in helper_modules]
    if case_modules:
        missing = set(registration_injected) - registration_parameters(case_modules)
        if missing:
            raise TypeError('registration-injected names no moved register function takes: %s' % sorted(missing))
    helper_source = {}
    for name in helpers:
        owner = importlib.import_module(source_modules[0]) if source_modules else driver
        fn = getattr(owner, name)
        helper_source[name] = {'module': fn.__module__, 'ast': function_ast(fn, injected)}
    retained = retained_helpers(driver, retained_specs)
    shared = moved_helper_shapes(driver, moved_helpers, helper_modules, source_modules)
    return {'records': records, 'tags': digest(tags), 'skips': digest([driver.platform_skips(case)
            for case in driver.CASES.values()]), 'selections': selections, 'moved': moved_source,
            'helpers': helper_source, 'retained': retained, 'moved_helpers': shared}


def child_snapshot(root, cell, moved, helpers, injected, source_modules=(), retained_specs=(), moved_helpers=(),
                   helper_modules=(), registration_injected=()):
    args = [sys.executable, '-X', 'lazy_imports=normal', str(Path(__file__).resolve()),
            '--snapshot', str(root), '--cell', cell]
    for flag, values in (('--case', moved), ('--helper', helpers), ('--injected', injected)):
        for value in values:
            args.extend((flag, value))
    for module in source_modules:
        args.extend(('--source-module', module))
    for spec in retained_specs:
        args.extend(('--retained-helper', spec))
    for flag, values in (('--moved-helper', moved_helpers), ('--helper-module', helper_modules),
                         ('--registration-injected', registration_injected)):
        for value in values:
            args.extend((flag, value))
    result = subprocess.run(args, cwd=root, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError('snapshot failed for %s (%s):\n%s' % (root, cell, result.stderr))
    return json.loads(result.stdout)


def compare_snapshots(base, head, moved_module):
    for field in ('records', 'tags', 'skips', 'selections'):
        if base[field] != head[field]:
            if field == 'records':
                for index, pair in enumerate(zip(base[field], head[field])):
                    if pair[0] != pair[1]:
                        raise AssertionError('registration %d differs: %r != %r' % (index, *pair))
            raise AssertionError('%s differs' % field)
    if base.get('retained', {}) != head.get('retained', {}):
        raise AssertionError('retained driver helpers or their registrations differ')
    compare_moved_helpers(base.get('moved_helpers', {}), head.get('moved_helpers', {}),
                          head.get('helper_modules', ()))
    for field in ('moved', 'helpers'):
        if base[field].keys() != head[field].keys():
            raise AssertionError('%s names differ' % field)
        for name, before in base[field].items():
            after = head[field][name]
            if {key: value for key, value in before.items() if key != 'module'} != \
                    {key: value for key, value in after.items() if key != 'module'}:
                raise AssertionError('%s AST differs: %s' % (field, name))
            expected = moved_module[name] if isinstance(moved_module, dict) else moved_module
            if before['module'] != 'contract_diff' or after['module'] != expected:
                raise AssertionError('%s module did not move as expected: %s' % (field, name))


def compare_moved_helpers(base, head, helper_modules=()):
    """A moved helper keeps its shape, was the driver's own and is now one helper module's."""
    if base.keys() != head.keys():
        raise AssertionError('moved helper names differ')
    for name, before in base.items():
        after = head[name]
        if before['shape'] != after['shape']:
            raise AssertionError('moved helper differs: %s' % name)
        if before['module'] != 'contract_diff':
            raise AssertionError('moved helper was not defined by the base driver: %s' % name)
        if after['module'] == 'contract_diff' or (helper_modules and after['module'] not in helper_modules):
            raise AssertionError('moved helper is still the driver\'s or in an undeclared module: %s' % name)


def invocation(root, cell, args):
    env = {**os.environ, 'CONTRACT_DIFF_CELL': cell}
    return subprocess.run([sys.executable, '-X', 'lazy_imports=normal',
                           str(root / 'tools/bisection/contract-diff.py'), *args],
                          cwd=root, env=env, capture_output=True)


def cli_comparison(base, head, cell, moved_cases):
    before, after = (invocation(root, cell, ['--list-tags']) for root in (base, head))
    if (before.returncode, before.stdout, before.stderr) != (after.returncode, after.stdout, after.stderr):
        raise AssertionError('--list-tags differs')
    if before.returncode:
        raise AssertionError('--list-tags failed')
    tag_hash = hashlib.sha256(before.stdout).hexdigest()
    with tempfile.TemporaryDirectory() as temporary:
        prefix = Path(temporary) / 'prefix'
        lib = prefix / 'lib'
        data = lib / 'libpinyin/data'
        data.mkdir(parents=True)
        (prefix / 'oracle-pin.txt').write_text('libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99\n'
                                               '+dbm-' + cell + '\n')
        for path in (lib / 'libpinyin.so', data / 'bigram.db', prefix / 'subject.so', prefix / 'zhuyin.so'):
            path.touch()
        common = [cell, str(prefix), str(prefix / 'subject.so'), '--zhuyin-so', str(prefix / 'zhuyin.so'),
                  '--plan-only']
        scenarios = [([], 0), (['--cases', 'fini-twice,default-option-parse', '--issue', '532',
                               '--family', 'zero-key'], 0), (['--issue', '1347'], 0),
                     (['--cases', 'unknown-case'], 2)]
        scenarios.extend((['--cases', name], 0) for name in moved_cases)
        for selection, expected in scenarios:
            before, after = (invocation(root, cell, common + selection) for root in (base, head))
            if (before.returncode, before.stdout, before.stderr) != (after.returncode, after.stdout, after.stderr):
                raise AssertionError('--plan-only differs for %s' % selection)
            if before.returncode != expected:
                raise AssertionError('--plan-only exit %s, expected %s for %s' % (before.returncode, expected, selection))
    return tag_hash, len(scenarios)


def module_counts(specs):
    result = {}
    for spec in specs:
        name, separator, count = spec.partition('=')
        if not separator or not name.isidentifier() or not count.isdecimal() or int(count) < 1 or name in result:
            raise ValueError('expected a unique module=positive-count: %s' % spec)
        result[name] = int(count)
    return result


def retained_counts(specs):
    result, helpers = {}, set()
    for spec in specs:
        pair, equals, count = spec.partition('=')
        module, colon, helper = pair.partition(':')
        if (not equals or not colon or not module.isidentifier() or not helper.isidentifier() or
                not count.isdecimal() or int(count) < 1 or spec in result or helper in helpers):
            raise ValueError('expected a unique module:helper=positive-count: %s' % spec)
        result[spec] = (module, helper, int(count))
        helpers.add(helper)
    return result


def nested_codes(code):
    yield code
    for item in code.co_consts:
        if isinstance(item, types.CodeType):
            yield from nested_codes(item)


def retained_helpers(driver, specs):
    """Compare driver helpers and identify their unchanged registered callbacks."""
    result, seen = {}, set()
    for spec, (_, name, count) in retained_counts(specs).items():
        helper = getattr(driver, name)
        if not isinstance(helper, types.FunctionType) or helper.__module__ != driver.__name__:
            raise TypeError('%s is not a driver function' % name)
        codes = tuple(nested_codes(helper.__code__))
        names = [case_name for case_name, case in driver.CASES.items()
                 if (fn := function_of(case['fn'])).__module__ == driver.__name__
                 and any(fn.__code__ is code for code in codes)]
        if len(names) != count or seen.intersection(names):
            raise AssertionError('%s has %d distinct retained cases, expected %d' % (name, len(names), count))
        seen.update(names)
        result[spec] = {'cases': names, 'shape': function_shape(helper, driver, driver.NO_ABORT, (), ())}
    return result


def discover(root, specs):
    expected = module_counts(specs)
    driver = load_driver(root, CELLS[0])
    found = {module: [] for module in expected}
    for name, case in driver.CASES.items():
        module = function_of(case['fn']).__module__
        if module in found:
            found[module].append(name)
    for module, count in expected.items():
        if len(found[module]) != count:
            raise AssertionError('%s has %d cases, expected %d' % (module, len(found[module]), count))
    return found


def child_discover(root, specs):
    args = [sys.executable, '-X', 'lazy_imports=normal', str(Path(__file__).resolve()), '--discover', str(root)]
    for spec in specs:
        args.extend(('--module-cases', spec))
    result = subprocess.run(args, cwd=root, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError('module discovery failed for %s:\n%s' % (root, result.stderr))
    return json.loads(result.stdout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('base', nargs='?', type=Path, help='base checkout')
    parser.add_argument('--head', type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument('--module', help='module to which the cases and helpers moved')
    parser.add_argument('--module-cases', action='append', default=[], metavar='MODULE=COUNT',
                        help='compare every case owned by this module; repeat for a batch')
    parser.add_argument('--retained-helper', action='append', default=[], metavar='MODULE:HELPER=COUNT',
                        help='compare driver-owned cases created by a shared helper')
    parser.add_argument('--case', action='append', default=[], help='moved case ID (repeatable)')
    parser.add_argument('--helper', action='append', default=[], help='moved helper re-exported by driver')
    parser.add_argument('--injected', action='append', default=[], help='shared driver object passed to the module')
    parser.add_argument('--moved-helper', action='append', default=[], metavar='NAME',
                        help='driver definition moved unchanged into a --helper-module (repeatable)')
    parser.add_argument('--helper-module', action='append', default=[], metavar='MODULE',
                        help='module holding moved helpers; followed as driver code on both sides')
    parser.add_argument('--registration-injected', action='append', default=[], metavar='NAME',
                        help='injected driver binding read only by a moved module\'s registration code')
    parser.add_argument('--snapshot', type=Path, help=argparse.SUPPRESS)
    parser.add_argument('--discover', type=Path, help=argparse.SUPPRESS)
    parser.add_argument('--cell', choices=CELLS, help=argparse.SUPPRESS)
    parser.add_argument('--source-module', action='append', default=[], help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.discover:
        print(json.dumps(discover(args.discover.resolve(), args.module_cases)))
        return
    if args.snapshot:
        print(json.dumps(snapshot(args.snapshot.resolve(), args.cell, args.case, args.helper, args.injected,
                                  args.source_module, args.retained_helper, args.moved_helper,
                                  args.helper_module, args.registration_injected)))
        return
    if not args.base:
        parser.error('base is required')
    base, head = args.base.resolve(), args.head.resolve()
    if args.module_cases:
        if args.module or args.case or args.helper:
            parser.error('--module-cases cannot be combined with --module, --case or --helper')
        found = child_discover(head, args.module_cases)
        moved = {name: module for module, names in found.items() for name in names}
        if len(moved) != sum(map(len, found.values())):
            raise AssertionError('a case belongs to more than one module')
        modules = list(found)
        selected = [names[0] for names in found.values()]
    elif args.moved_helper and not (args.module or args.case or args.helper):
        moved, modules, selected = {}, [], []
    else:
        if not (args.module and args.case):
            parser.error('--module and at least one --case, --module-cases or --moved-helper are required')
        moved = {name: args.module for name in args.case}
        modules = [args.module]
        selected = [args.case[0]]
    for _, (module, _, _) in retained_counts(args.retained_helper).items():
        if module not in modules:
            parser.error('--retained-helper requires its module in the comparison')
    if not set(args.registration_injected) <= set(args.injected):
        parser.error('--registration-injected names must also be --injected')
    if args.moved_helper and not args.helper_module:
        parser.error('--moved-helper requires --helper-module')
    if len(set(args.moved_helper)) != len(args.moved_helper):
        parser.error('--moved-helper names must be unique')
    for cell in CELLS:
        before = child_snapshot(base, cell, moved, args.helper, args.injected, retained_specs=args.retained_helper,
                                moved_helpers=args.moved_helper, helper_modules=args.helper_module,
                                registration_injected=args.registration_injected)
        after = child_snapshot(head, cell, moved, args.helper, args.injected, modules, args.retained_helper,
                               args.moved_helper, args.helper_module, args.registration_injected)
        after['helper_modules'] = args.helper_module
        compare_snapshots(before, after, moved if not args.helper else args.module)
        retained = [name for group in after['retained'].values() for name in group['cases']]
        if set(retained).intersection(moved):
            raise AssertionError('a retained case also belongs to a moved module')
        plan_cases = selected + [group['cases'][0] for group in after['retained'].values()]
        tag_hash, plans = cli_comparison(base, head, cell, plan_cases)
        print('%s: %d registrations, default %d, %d selection scenarios, %d plans, %d modules / %d cases '
              '(%d module-owned, %d retained driver callbacks), %d moved helpers; '
              '--list-tags SHA-256 %s' %
              (cell, len(before['records']), before['selections'][0][1], len(before['selections']), plans,
               len(modules), len(moved) + len(retained), len(moved), len(retained), len(args.moved_helper),
               tag_hash))


if __name__ == '__main__':
    main()
