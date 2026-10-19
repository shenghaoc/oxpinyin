"""One CLI for the native tooling services. Standard library only."""
import argparse
import sys

from . import check, doctor, host, registry, slot


def _names(value):
    return [name.strip() for name in (value or '').split(',') if name.strip()]


def doctor_command(args):
    return doctor.main()


def host_command(args):
    for line in doctor.host_lines():
        print(line)
    return 0


def check_command(args):
    return check.run(args.backend)


def list_command(args):
    rows = registry.load()
    if args.heavy:
        rows = [row for row in rows if registry.is_heavy(row)]
    for row in rows:
        print(f'{row["name"]}\t{row.get("kind", "")}\t{",".join(row["cells"])}\t'
              f'{"heavy" if registry.is_heavy(row) else "light"}')
    return 0


def plan_command(args):
    rows = registry.load()
    chosen = registry.select(rows, _names(args.runners) or None, _names(args.cells) or None)
    for row in chosen:
        inputs = '; '.join(row.get('required_inputs', []))
        print(f'{row["name"]}: {"heavy" if registry.is_heavy(row) else "light"}; cells: '
              f'{",".join(row["cells"])}; inputs: {inputs or "none"}')
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

    p = sub.add_parser('list', help='list the registry, read-only')
    p.add_argument('--heavy', action='store_true', help='only rows marked heavy')
    p.set_defaults(func=list_command)

    p = sub.add_parser('plan', help='show the selected registry rows, read-only')
    p.add_argument('--runners', help='comma-separated runner names')
    p.add_argument('--cells', help='comma-separated cells')
    p.set_defaults(func=plan_command)

    p = sub.add_parser('slot', help='inspect or run under the host-wide heavy slot')
    slot_sub = p.add_subparsers(dest='action', required=True)
    run = slot_sub.add_parser('run', help='run a command under one heavy slot')
    run.add_argument('command', nargs=argparse.REMAINDER)
    slot_sub.add_parser('status', help='show the lock directory and slot count')
    p.set_defaults(func=slot_command)
    return parser


def main(argv=None):
    args = build_parser().parse_args(argv)
    return args.func(args)
