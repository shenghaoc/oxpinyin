"""One CLI for the native tooling services. Standard library only."""
import argparse
import sys

from . import abi, cell, check, doctor, pool, test_runner
from .measure import recipes


def packaging_command(args):
    """Forward packaging options to the shared service."""
    from . import packaging
    return packaging.main(args.args)


def measure_command(args):
    """Dispatch one measure recipe and return its status."""
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


def sanitize_command(args):
    """Select and run a sanitizer gate, forwarding its arguments unchanged."""
    return test_runner.sanitize(args.args)


def sanitize_toolchain_command(args):
    """Print the sanitizer nightly that the workflow environment selects."""
    return test_runner.sanitize_toolchain()


def test_tools_command(args):
    """Run the same Python harness discovery and exclusions as CI."""
    return test_runner.run_tools(args.files, args.pattern, args.changed)


def pool_command(args):
    """Run a command per item within one CPU budget; see tools/dev/pool.py."""
    if args.command[:1] == ['--']:
        args.command = args.command[1:]
    return pool.run_command(args)


def cpp_smoke_command(args):
    """Run the C++ pinyin.h smoke gate."""
    from . import runners
    return runners.cpp_smoke(args.args)


def build_parser():
    """Build the native service CLI with dispatch callbacks on each subcommand."""
    parser = argparse.ArgumentParser(prog='python -m tools.dev',
                                     description='Native tooling services: capabilities and checks.')
    sub = parser.add_subparsers(dest='command', required=True)

    p = sub.add_parser('doctor', help='report native capabilities and the interpreter identity')
    p.set_defaults(func=doctor_command)


    p = sub.add_parser('host', help='report host capabilities only')
    p.set_defaults(func=host_command)

    p = sub.add_parser('check', help='run one native setup check')
    p.add_argument('backend', nargs='?')
    p.set_defaults(func=check_command)

    p = sub.add_parser('lint-crate-cfg-placement',
                       help='reject crate cfg attributes above crate documentation')
    p.add_argument('files', nargs='*')
    p.set_defaults(func=lint_command)


    p = sub.add_parser('sanitize', help='run one store backend sanitizer gate')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=sanitize_command)

    p = sub.add_parser('sanitize-toolchain', help='print the sanitizer nightly')
    p.set_defaults(func=sanitize_toolchain_command)

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

    p = sub.add_parser('cpp-smoke', help='compile and run the C++ pinyin.h smoke gate')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=cpp_smoke_command)

    p = sub.add_parser('rust-tables', help='check the pinned scripts2 Rust tables offline, or regenerate with --write')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=lambda args: __import__('tools.dev.rust_tables', fromlist=['main']).main(args.args))

    p = sub.add_parser('fetch-model', help='fetch and verify the pinned model20 archive into a build cache')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=lambda args: __import__('tools.dev.model', fromlist=['main']).main(args.args))

    p = sub.add_parser('fetch-contract-data',
                       help='fetch and verify the pinned libpinyin-data package: the contract differential system data')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=lambda args: __import__('tools.dev.contract_data', fromlist=['main']).main(args.args))

    p = sub.add_parser('provision', help='post-bootstrap cloud provisioning')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=lambda args: __import__('tools.dev.provision', fromlist=['main']).main(args.args))

    p = sub.add_parser('packaging', help='install, relink, metadata and package assembly')
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=packaging_command)

    p = sub.add_parser('measure', help='measurement and profile recipes')
    p.add_argument('recipe', choices=recipes.NAMES)
    p.add_argument('args', nargs=argparse.REMAINDER)
    p.set_defaults(func=measure_command)

    p = sub.add_parser('pool', help='run independent builds at once within one CPU budget')
    pool.add_arguments(p)
    p.set_defaults(func=pool_command)
    return parser


def main(argv=None):
    """Dispatch native tooling arguments and return the selected command status.

    Default to sys.argv. Forward non-help test arguments directly
    to their runners.
    """
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv[:1] == ['rust-tables']:
        from . import rust_tables
        return rust_tables.main(argv[1:])
    if argv[:1] == ['provision']:
        from . import provision
        return provision.main(argv[1:])
    if argv[:1] == ['fetch-model']:
        from . import model
        return model.main(argv[1:])
    if argv[:1] == ['fetch-contract-data']:
        from . import contract_data
        return contract_data.main(argv[1:])
    if argv[:1] == ['packaging']:
        from . import packaging
        return packaging.main(argv[1:])
    if argv[:1] == ['cell-artifacts']:
        return cell.main(argv[1:])
    if argv[:1] == ['abi']:
        return abi.main(argv[1:])
    if argv[:1] == ['cpp-smoke']:
        from . import runners
        return runners.cpp_smoke(argv[1:])
    if argv[:1] == ['lint-crate-cfg-placement']:
        return check.lint_crate_cfg_placement(argv[1:])
    if argv[:1] == ['sanitize']:
        return test_runner.sanitize(argv[1:])
    if argv[:1] == ['sanitize-toolchain']:
        return test_runner.sanitize_toolchain()
    args = build_parser().parse_args(argv)
    return args.func(args)
