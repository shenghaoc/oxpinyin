"""Validate the retained shell launcher's declared host inputs before runtime access.

This is not a launcher or a second runner catalog. Native state paths are never
container inputs. The native admission directory/marker remain host-side only.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from .. import doctor, host, registry
from .identity import digest_file, unredirected

HOST_SETTINGS = frozenset({
    'OXPINYIN_CONTAINER', 'OXPINYIN_CONTAINER_MEMORY_MIB',
    'OXPINYIN_PROXY_CA', 'OXPINYIN_COMPAT_NETWORK',
    'OXPINYIN_LOCK_DIR', 'OXPINYIN_HEAVY_SLOT_HELD',
    'OXPINYIN_HEAVY_SLOTS', 'OXPINYIN_RESOURCE_WAIT_SECONDS',
})
STATE_SETTINGS = frozenset({
    'CARGO_HOME', 'CARGO_TARGET_DIR', 'CARGO_BUILD_TARGET_DIR', 'RUSTUP_HOME',
    'UV_CACHE_DIR', 'UV_PYTHON_INSTALL_DIR', 'UV_INSTALL_DIR',
    # Intentional: container runtimes must not inherit host loader paths.
    'PYTHONHOME', 'PYTHONPATH', 'LD_LIBRARY_PATH', 'DYLD_LIBRARY_PATH',
    'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
    'PKG_CONFIG_PATH', 'PKG_CONFIG_LIBDIR',
})


def validate_environment(environ):
    """Reject host settings that could select or redirect container state.

    Ignore empty values. Raise ValueError for interpreter overrides, forbidden
    state paths, undeclared prefixed settings, a relative lock directory, or
    a missing/empty proxy CA file. Filesystem errors can propagate.
    """
    try:
        doctor.reject_overrides(environ)
    except doctor.DoctorError as exc:
        raise ValueError(str(exc)) from exc
    for name, value in environ.items():
        if not value:
            continue
        if name in STATE_SETTINGS or (name.startswith(('OXPINYIN_', 'PINYIN_', 'ORACLE_', 'UV_PYTHON_'))
                                     and name not in HOST_SETTINGS):
            raise ValueError(f'unsupported container input/override: {name}; '
                             'native paths and artifacts cannot select container state')
    lock = environ.get('OXPINYIN_LOCK_DIR')
    if lock and not Path(lock).is_absolute():
        raise ValueError('OXPINYIN_LOCK_DIR must be absolute (native budget state only)')
    ca = environ.get('OXPINYIN_PROXY_CA')
    if ca and (not Path(ca).is_file() or Path(ca).stat().st_size == 0):
        raise ValueError('OXPINYIN_PROXY_CA must name a nonempty CA file')


def positive(value):
    """Parse a positive decimal integer without leading zeros for argparse.

    Raise ArgumentTypeError for every other spelling.
    """
    if not re.fullmatch(r'[1-9][0-9]*', value):
        raise argparse.ArgumentTypeError('expected a positive integer')
    return int(value)


def contract_issues():
    """Use the existing contract catalog's read-only tag listing."""
    script = Path(__file__).resolve().parents[2] / 'bisection/contract-diff.py'
    result = subprocess.run([sys.executable, str(script), '--list-tags'],
                            capture_output=True, text=True, check=False)
    if result.returncode:
        raise ValueError(result.stderr.strip() or 'cannot read contract issue tags')
    return set(json.loads(result.stdout)['tags'])


def validate(argv, environ=None):
    """Parse and validate launcher inputs without accessing a container runtime.

    Check environment, result namespace, gate selection, timeouts, and any
    model archive digest against tools/oracle/oracle-pin.txt. ``--cases``
    needs ``--runners`` naming contract-diff; run-all.py checks the case IDs
    against the contract index inside the container. Return the parsed
    namespace. Read the registry and, when needed, run the contract tag
    listing; reject invalid inputs with parser exits or value errors.
    """
    environ = os.environ if environ is None else environ
    validate_environment(environ)
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument('--runtime')
    parser.add_argument('--platform', choices=('linux/amd64', 'linux/arm64'))
    parser.add_argument('--cell', choices=('bdb', 'kc', 'tkrzw'), default='bdb')
    parser.add_argument('--archive', type=Path)
    parser.add_argument('--jobs', type=positive, default=2)
    parser.add_argument('--memory-mib', type=positive,
                        default=environ.get('OXPINYIN_CONTAINER_MEMORY_MIB', '2560'))
    parser.add_argument('--memory-smoke', action='store_true')
    parser.add_argument('--list', action='store_true')
    parser.add_argument('--runners')
    parser.add_argument('--issue', action='append')
    parser.add_argument('--cases')
    parser.add_argument('--timeout', type=positive)
    parser.add_argument('--runner-timeout', action='append', default=[])
    args = parser.parse_args(argv)
    runtime = args.runtime or environ.get('OXPINYIN_CONTAINER')
    if runtime and Path(runtime).name not in ('podman', 'docker', 'container', 'wslc'):
        raise ValueError(f'unknown container runtime: {runtime}')
    result_root = Path.cwd() / '.oxpinyin-container'
    unredirected(result_root, Path.cwd(),
                 message='container result namespace must not redirect through a symlink')
    host.container_memory_mib(str(args.memory_mib))
    if args.cases is not None and (args.runners is None
                                   or 'contract-diff' not in registry.names(args.runners)):
        raise ValueError('--cases requires --runners contract-diff')
    if args.memory_smoke or args.list:
        if args.runners is not None or args.issue is not None or args.archive is not None or args.timeout or args.runner_timeout:
            raise ValueError('list/memory smoke cannot select task inputs or runners')
        if args.memory_smoke and args.list:
            raise ValueError('list and memory smoke are mutually exclusive')
        return args
    if args.runners is None and args.issue is None:
        raise ValueError('select --runners or --issue')
    rows = registry.select(registry.load(),
                           names=None if args.runners is None else registry.names(args.runners),
                           cells=[args.cell])
    if args.issue is not None:
        requested = {registry.normal_issue(value) for value in args.issue}
        tags = contract_issues() if any(row['name'] == 'contract-diff' for row in rows) else set()
        known = {registry.normal_issue(tag) for row in rows for tag in row.get('issues', [])} | tags
        if requested - known:
            raise ValueError('unknown issue(s) for selected work: ' + ', '.join(sorted(requested - known)))
        rows = [row for row in rows if requested.intersection(
            registry.normal_issue(tag) for tag in row.get('issues', [])) or
            row['name'] == 'contract-diff' and requested.intersection(tags)]
    if args.runners is not None and any(row.get('kind') != 'gate' for row in rows):
        raise ValueError('measurement recipes cannot be launched as compatibility gates')
    rows = [row for row in rows if row.get('kind') == 'gate']
    if not rows:
        raise ValueError('selection contains no gates')
    selected = {row['name'] for row in rows}
    for value in args.runner_timeout:
        name, separator, seconds = value.partition('=')
        if not separator or name not in selected:
            raise ValueError(f'runner timeout must name a selected runner: {value}')
        positive(seconds)
    if args.archive is not None:
        pin = Path(__file__).resolve().parents[2] / 'oracle/oracle-pin.txt'
        expected = dict(line.split('=', 1) for line in pin.read_text().splitlines()
                        if line and not line.startswith('#'))['model_archive_sha256']
        if digest_file(args.archive) != expected:
            raise ValueError('model archive checksum mismatch')
    return args


def main(argv=None):
    """Validate launcher arguments and return zero, or report a failure as status 2."""
    try:
        validate(sys.argv[1:] if argv is None else argv)
    except (OSError, ValueError, doctor.DoctorError, argparse.ArgumentTypeError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
