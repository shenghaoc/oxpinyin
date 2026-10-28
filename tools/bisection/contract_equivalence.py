#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Compare contract registrations and moved source in two checkout directories.

This is a light, source-only check. Each checkout is loaded in its own Python
process; no worker, shared library or differential is run. Specify case IDs
with --module and --case, or all cases in each module with --module-cases
MODULE=COUNT. --injected names the shared driver objects passed to registrations.
Helpers can also be named with --helper for source comparison.
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
from pathlib import Path
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
        if name in injected:
            used.add(name)
            if not hasattr(driver, name) or value is not getattr(driver, name):
                raise TypeError('injected %s is not the driver binding' % name)
            if isinstance(value, type) and value.__module__ == driver.__name__:
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
    return {'ast': function_ast(fn), 'defaults': value_shape(fn.__defaults__, sentinel),
            'kwdefaults': value_shape(fn.__kwdefaults__, sentinel), 'bindings': bindings}


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


def snapshot(root, cell, moved, helpers, injected, source_modules=()):
    """Read the entire registry, selections, plans and specified source ASTs."""
    driver = load_driver(root, cell)
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
        if driver.CASES[name]['fn'] is not fn:
            raise TypeError('moved case is wrapped: %s' % name)
        moved_source[name] = {'module': fn.__module__, 'shape': function_shape(
            fn, driver, sentinel, injected, source_modules, used=used_injected)}
    if set(injected) != used_injected:
        raise TypeError('unused injected names: %s' % sorted(set(injected) - used_injected))
    helper_source = {}
    for name in helpers:
        owner = importlib.import_module(source_modules[0]) if source_modules else driver
        fn = getattr(owner, name)
        helper_source[name] = {'module': fn.__module__, 'ast': function_ast(fn, injected)}
    return {'records': records, 'tags': digest(tags), 'skips': digest([driver.platform_skips(case)
            for case in driver.CASES.values()]), 'selections': selections, 'moved': moved_source,
            'helpers': helper_source}


def child_snapshot(root, cell, moved, helpers, injected, source_modules=()):
    args = [sys.executable, '-X', 'lazy_imports=normal', str(Path(__file__).resolve()),
            '--snapshot', str(root), '--cell', cell]
    for flag, values in (('--case', moved), ('--helper', helpers), ('--injected', injected)):
        for value in values:
            args.extend((flag, value))
    for module in source_modules:
        args.extend(('--source-module', module))
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
    parser.add_argument('--case', action='append', default=[], help='moved case ID (repeatable)')
    parser.add_argument('--helper', action='append', default=[], help='moved helper re-exported by driver')
    parser.add_argument('--injected', action='append', default=[], help='shared driver object passed to the module')
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
                                  args.source_module)))
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
    else:
        if not (args.module and args.case):
            parser.error('--module and at least one --case, or --module-cases, are required')
        moved = {name: args.module for name in args.case}
        modules = [args.module]
        selected = [args.case[0]]
    for cell in CELLS:
        before = child_snapshot(base, cell, moved, args.helper, args.injected)
        after = child_snapshot(head, cell, moved, args.helper, args.injected, modules)
        compare_snapshots(before, after, moved if not args.helper else args.module)
        tag_hash, plans = cli_comparison(base, head, cell, selected)
        print('%s: %d registrations, default %d, %d selection scenarios, %d plans, %d modules / %d cases; '
              '--list-tags SHA-256 %s' %
              (cell, len(before['records']), before['selections'][0][1], len(before['selections']), plans,
               len(modules), len(moved), tag_hash))


if __name__ == '__main__':
    main()
