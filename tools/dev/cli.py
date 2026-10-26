"""One CLI for the native tooling services. Standard library only."""
import argparse
import importlib.util
import json
import sys
from pathlib import Path

from . import abi, cell, check, coverage, doctor, host, pool, registry, slot, test_runner
from .measure import cold, recipes, startup


def _selected(args, *, required=False):
    """Load registry rows matching the CLI selectors.

    Raise RegistryError for invalid or empty selections; required also rejects
    an invocation without any selector.
    """
    return registry.select(registry.load(),
                           names=None if args.runners is None else registry.names(args.runners),
                           cells=None if args.cells is None else registry.names(args.cells),
                           issues=args.issue, kind=args.kind, require_selection=required)


def _oracle_versions():
    """Load the standalone oracle version checker shipped in tools/oracle."""
    path = Path(__file__).resolve().parents[1] / 'oracle' / 'versions.py'
    spec = importlib.util.spec_from_file_location('oracle_versions', path)
    if spec is None or spec.loader is None:
        raise SystemExit(f'cannot load {path}')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def oracle_versions_command(args):
    """Forward the oracle version check's options and return its status."""
    forwarded = []
    for name in ('manifest', 'probe', 'record'):
        value = getattr(args, name)
        if value is not None:
            forwarded += [f'--{name}', str(value)]
    if args.print_only:
        forwarded.append('--print')
    return _oracle_versions().main(forwarded)


def measure_command(args):
    """Dispatch one measure recipe and return its status."""
    if args.recipe == 'cold-qualify':
        return cold.main(args.args)
    if args.recipe == 'startup':
        return startup.main(args.args)
    return recipes.main(args.recipe, args.args)


def doctor_command(args):
    """Print capability and interpreter checks and return the doctor status."""
    return doctor.main()


def host_command(args):
    """Print host and tool capabilities; return 1 if a tool version check fails."""
    for line in doctor.host_lines():
        print(line)
    lines, failures = doctor.tool_lines()
    for line in lines:
        print(line)
    return 1 if failures else 0


def check_command(args):
    """Run native setup tests for the parsed backend and return their status."""
    return check.run(args.backend)


def lint_command(args):
    """Run the crate-configuration placement lint and return 0, 1 or 2."""
    return check.lint_crate_cfg_placement(args.files)


def backend_matrix_command(args):
    """Prove the exactly-one-backend invariant and return 0 or 1."""
    return check.backend_matrix()


def backend_probe_command(args):
    """Identify the system libpinyin's DBM and return its probe status."""
    return check.backend_probe()


def sanitize_command(args):
    """Select and run a sanitizer gate, forwarding its arguments unchanged."""
    return test_runner.sanitize(args.args)


def sanitize_toolchain_command(args):
    """Print the sanitizer nightly that the workflow environment selects."""
    return test_runner.sanitize_toolchain()


def test_command(args):
    """Forward test arguments and profile to nextest under resource admission."""
    return test_runner.run(args.args, args.profile)


def test_doc_command(args):
    """Forward doctest arguments through just heavy and return the child status."""
    return test_runner.run_doc(args.args)


def test_tools_command(args):
    """Run the same Python harness discovery and exclusions as CI."""
    return test_runner.run_tools(args.files, args.pattern, args.changed)


def coverage_command(args):
    """Run llvm-cov with disposable coverage state and return its shell status."""
    return coverage.run(args.args)


def list_command(args):
    """Print matching registry rows as tab-separated metadata.

    Apply the optional heavy-only filter after selection and raise
    RegistryError if it leaves no rows. Return zero after printing.
    """
    rows = _selected(args)
    if args.heavy:
        rows = [row for row in rows if registry.is_heavy(row)]
    for row in rows:
        print(f'{row["name"]}\t{row.get("kind", "")}\t{",".join(row["cells"])}\t'
              f'{"heavy" if registry.is_heavy(row) else "light"}\t'
              f'{row["implementation"]}')
    if not rows:
        raise registry.RegistryError('selection matched no registry rows')
    return 0


def plan_command(args):
    """Print selected command and environment templates without executing them.

    Require an explicit registry selection and show only requested cells;
    return zero after printing.
    """
    chosen = _selected(args, required=True)
    cells = None if args.cells is None else registry.names(args.cells)
    print('Registry description only; executes nothing. Cells and commands are compatibility metadata.')
    for row in chosen:
        inputs = '; '.join(row.get('required_inputs', []))
        selected_cells = [cell for cell in row['cells'] if cells is None or cell in cells]
        print(f'{row["name"]}: {"heavy" if registry.is_heavy(row) else "light"}; cells: '
              f'{",".join(selected_cells)}; '
              f'kind: {row.get("kind", "")}; inputs: {inputs or "none"}')
        print('  implementation: ' + row['implementation'])
        print('  command template: ' + json.dumps(row['command']))
        print('  environment template: ' + json.dumps(row.get('env', {}), sort_keys=True))
        if row.get('reason'):
            print('  state: ' + row['reason'])
    return 0


def slot_command(args):
    """Report slot configuration or execute a command under resource admission.

    Strip one leading -- from the child arguments. Report OS or value errors
    as status 1 and cancellation as 130; convert a negative child status to
    the corresponding shell signal status. A missing command raises SystemExit.
    """
    if args.action == 'status':
        print(f'lock_dir: {host.host_lock_dir()}')
        print(f'slots: {host.heavy_slot_count()}')
        return 0
    command = list(args.command)
    if command[:1] == ['--']:
        command = command[1:]
    if not command:
        raise SystemExit('slot run needs a command after --')
    try:
        rc = slot.run_command(command)
    except (OSError, ValueError) as exc:
        diagnostic = ' '.join(str(exc).splitlines())
        print(f'slot run: {diagnostic}', file=sys.stderr)
        return 130 if isinstance(exc, slot.Cancelled) else 1
    return 128 - rc if rc < 0 else rc


def pool_command(args):
    """Run a command per item within one CPU and memory budget; see tools/dev/pool.py."""
    if args.command[:1] == ['--']:
        args.command = args.command[1:]
    return pool.run_command(args)


def runner_command(args):
    """Call the shared driver recipe services; the registry still owns selection."""
    from . import runners
    return runners.main(args.args)


def build_parser():
    """Build the native service CLI with dispatch callbacks on each subcommand."""
    parser = argparse.ArgumentParser(prog='python -m tools.dev',
                                     description='Native tooling services: capabilities, registry and heavy slot.')
    sub = parser.add_subparsers(dest='command', required=True)

    p = sub.add_parser('doctor', help='report native capabilities and the interpreter identity')
    p.set_defaults(func=doctor_command)

    p = sub.add_parser('oracle-versions',
                       help='record or check the pin-reference image tool versions')
    p.add_argument('--manifest', type=Path)
    p.add_argument('--probe')
    p.add_argument('--record', type=Path)
    p.add_argument('--print', dest='print_only', action='store_true',
                   help='print installed versions without checking ranges')
    p.set_defaults(func=oracle_versions_command)

    p = sub.add_parser('host', help='report host capabilities only')
    p.set_defaults(func=host_command)

    p = sub.add_parser('check', help='run one native setup check')
    p.add_argument('backend', nargs='?', default='bdb')
    p.set_defaults(func=check_command)

    p = sub.add_parser('lint-crate-cfg-placement',
                       help='reject crate cfg attributes above crate documentation')
    p.add_argument('files', nargs='*')
    p.set_defaults(func=lint_command)

    p = sub.add_parser('backend-matrix', help='prove the exactly-one-backend invariant')
    p.set_defaults(func=backend_matrix_command)

    p = sub.add_parser('backend-probe', help='identify the system libpinyin backend')
    p.set_defaults(func=backend_probe_command)

    p = sub.add_parser('sanitize', help='run one store backend sanitizer gate')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=sanitize_command)

    p = sub.add_parser('sanitize-toolchain', help='print the sanitizer nightly')
    p.set_defaults(func=sanitize_toolchain_command)

    p = sub.add_parser('test', help='run cargo nextest through the heavy slot')
    p.add_argument('--profile')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=test_command)

    p = sub.add_parser('test-doc', help='run cargo test --doc through the heavy slot')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=test_doc_command)

    p = sub.add_parser('test-tools', help='run the CI Python harness suite')
    p.add_argument('files', nargs='*', metavar='FILE')
    p.add_argument('-k', dest='pattern', help='unittest name substring or wildcard pattern')
    p.add_argument('--changed', nargs='?', const='HEAD', metavar='BASE',
                   help='select tests for changes since BASE (default HEAD), including untracked files')
    p.set_defaults(func=test_tools_command)

    p = sub.add_parser('cell-artifacts', help='build or describe the selected cell artifacts')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=lambda args: cell.main(args.args))

    p = sub.add_parser('abi', help='check exports and allocation contracts through shared services')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=lambda args: abi.main(args.args))

    p = sub.add_parser('runner', help='execute one registry recipe through shared driver services')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=runner_command)

    p = sub.add_parser('coverage', help='run cargo llvm-cov with temporary per-invocation state')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=coverage_command)

    p = sub.add_parser('list', help='list the registry, read-only')
    p.add_argument('--heavy', action='store_true', help='only rows marked heavy')
    _selection_arguments(p)
    p.set_defaults(func=list_command)

    p = sub.add_parser('plan', help='show the selected registry rows, read-only')
    _selection_arguments(p)
    p.set_defaults(func=plan_command)

    p = sub.add_parser('measure', help='measurement and profile recipes')
    p.add_argument('recipe', choices=('startup', 'cold-qualify', *recipes.NAMES))
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=measure_command)

    p = sub.add_parser('slot', help='inspect or run under the host-wide heavy slot')
    slot_sub = p.add_subparsers(dest='action', required=True)
    run = slot_sub.add_parser('run', help='run a command under one heavy slot')
    run.add_argument('command', nargs=argparse.REMAINDER)
    slot_sub.add_parser('status', help='show the lock directory and slot count')
    p.set_defaults(func=slot_command)

    p = sub.add_parser('pool', help='run independent builds at once within one CPU and memory budget')
    pool.add_arguments(p)
    p.set_defaults(func=pool_command)
    return parser


def _selection_arguments(parser):
    """Add runner, cell, issue, and kind filters to an existing parser."""
    parser.add_argument('--runners', help='comma-separated registry runner names')
    parser.add_argument('--cells', help='comma-separated registry cells (metadata only)')
    parser.add_argument('--issue', action='append', help='registry issue tag; repeat for alternatives')
    parser.add_argument('--kind', help='registry kind, such as gate or not-a-gate')


def main(argv=None):
    """Dispatch native tooling arguments and return the selected command status.

    Default to sys.argv. Forward coverage and non-help test arguments directly
    to their runners; report registry selection errors on stderr with status 2.
    """
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv[:1] == ['cell-artifacts']:
        return cell.main(argv[1:])
    if argv[:1] == ['abi']:
        return abi.main(argv[1:])
    if argv[:1] == ['runner']:
        from . import runners
        return runners.main(argv[1:])
    if argv[:1] == ['coverage']:
        return coverage.run(argv[1:])
    if argv[:1] == ['lint-crate-cfg-placement']:
        return check.lint_crate_cfg_placement(argv[1:])
    if argv[:1] == ['backend-matrix']:
        return check.backend_matrix()
    if argv[:1] == ['backend-probe']:
        return check.backend_probe()
    if argv[:1] == ['sanitize']:
        return test_runner.sanitize(argv[1:])
    if argv[:1] == ['sanitize-toolchain']:
        return test_runner.sanitize_toolchain()
    if argv[:1] == ['test'] and argv[1:2] not in (['-h'], ['--help']):
        return test_runner.run(argv[1:])
    if argv[:1] == ['test-doc'] and argv[1:2] not in (['-h'], ['--help']):
        return test_runner.run_doc(argv[1:])
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except registry.RegistryError as exc:
        print(f'{args.command}: {exc}', file=sys.stderr)
        return 2
