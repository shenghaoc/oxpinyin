"""One CLI for the native tooling services. Standard library only."""
import argparse
import json
import sys

from . import check, doctor, host, registry, slot, test_runner


def _selected(args, *, required=False):
    return registry.select(registry.load(),
                           names=None if args.runners is None else registry.names(args.runners),
                           cells=None if args.cells is None else registry.names(args.cells),
                           issues=args.issue, kind=args.kind, require_selection=required)


def doctor_command(args):
    return doctor.main()


def host_command(args):
    for line in doctor.host_lines():
        print(line)
    return 0


def check_command(args):
    return check.run(args.backend)


def test_command(args):
    return test_runner.run(args.args, args.profile)


def test_doc_command(args):
    return test_runner.run_doc(args.args)


def list_command(args):
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


def build_parser():
    parser = argparse.ArgumentParser(prog='python -m tools.dev',
                                     description='Native tooling services: capabilities, registry and heavy slot.')
    sub = parser.add_subparsers(dest='command', required=True)

    p = sub.add_parser('doctor', help='report native capabilities and the interpreter identity')
    p.set_defaults(func=doctor_command)

    p = sub.add_parser('host', help='report host capabilities only')
    p.set_defaults(func=host_command)

    p = sub.add_parser('check', help='run one native setup check')
    p.add_argument('backend', nargs='?', default='bdb')
    p.set_defaults(func=check_command)

    p = sub.add_parser('test', help='run cargo nextest through the heavy slot')
    p.add_argument('--profile')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=test_command)

    p = sub.add_parser('test-doc', help='run cargo test --doc through the heavy slot')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=test_doc_command)

    p = sub.add_parser('list', help='list the registry, read-only')
    p.add_argument('--heavy', action='store_true', help='only rows marked heavy')
    _selection_arguments(p)
    p.set_defaults(func=list_command)

    p = sub.add_parser('plan', help='show the selected registry rows, read-only')
    _selection_arguments(p)
    p.set_defaults(func=plan_command)

    p = sub.add_parser('slot', help='inspect or run under the host-wide heavy slot')
    slot_sub = p.add_subparsers(dest='action', required=True)
    run = slot_sub.add_parser('run', help='run a command under one heavy slot')
    run.add_argument('command', nargs=argparse.REMAINDER)
    slot_sub.add_parser('status', help='show the lock directory and slot count')
    p.set_defaults(func=slot_command)
    return parser


def _selection_arguments(parser):
    parser.add_argument('--runners', help='comma-separated registry runner names')
    parser.add_argument('--cells', help='comma-separated registry cells (metadata only)')
    parser.add_argument('--issue', action='append', help='registry issue tag; repeat for alternatives')
    parser.add_argument('--kind', help='registry kind, such as gate or not-a-gate')


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
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
