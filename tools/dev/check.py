"""Native setup and lint checks.

These are setup and gate checks, not oracle or pin comparisons. The old
optional oracle-directory display is intentionally absent: the native boundary
does not probe an oracle installation or pass its variables to tests.
"""
import os
from pathlib import Path
import platform
import subprocess
import sys

from . import cell, native

ROOT = Path(__file__).resolve().parents[2]
USAGE = 'usage: tools/dev/check.sh [' + '|'.join(cell.BACKENDS) + ']'


def run(backend=None, out=sys.stdout, err=sys.stderr, env=None):
    """Check native prerequisites, then build and run the selected backend tests.

    Use the Cargo default and include the C API tests on Linux. Print tool
    and backend diagnostics; return 2 for an unknown backend, 1 for missing
    tools or launch errors, or the failing child status. The optional env is
    used only for the supervised Cargo child.
    """
    backend = backend or cell.declared_default()
    if backend not in cell.BACKENDS:
        print(USAGE, file=err, flush=True)
        return 2
    tools = native.Tools()
    for tool in ('cargo', 'rustup', 'cc', 'pkg-config'):
        if tools.which(tool) is None:
            print(f'missing {tool}; see docs/runbooks/setup.md', file=err, flush=True)
            return 1
    print(f'System: {platform.system()} {platform.machine()}', file=out, flush=True)
    result = tools.run(['rustup', 'show', 'active-toolchain'])
    out.write(result.stdout)
    out.flush()
    err.write(result.stderr)
    err.flush()
    if result.returncode:
        return result.returncode
    result = tools.run(['rustc', '-vV'])
    out.write(result.stdout)
    out.flush()
    err.write(result.stderr)
    err.flush()
    if result.returncode:
        return result.returncode
    target = os.environ.get('CARGO_TARGET_DIR') or 'target'
    print(f'Backend: {backend}; target directory: {target}', file=out, flush=True)
    result = tools.run(['pkg-config', '--modversion', 'libclang'])
    if result.returncode == 0:
        out.write(result.stdout)
        out.flush()
    else:
        print('libclang: discovered by bindgen during build (a .pc file is optional)', file=out, flush=True)
    if backend == 'bdb':
        command = ['cc']
        command += native.path_list_flags(os.environ, 'SYSTEM_DEPS_BDB_INCLUDE', '-I')
        command += ['-E', '-P', '-']
        result = tools.run(command, input='#include <db.h>\nDB_VERSION_STRING\n')
        if result.returncode:
            err.write(result.stderr)
            err.flush()
            return result.returncode
        lines = result.stdout.rstrip().splitlines()
        print('Berkeley DB header: ' + (lines[-1] if lines else ''), file=out, flush=True)
    else:
        result = tools.run(['pkg-config', '--modversion', backend])
        out.write(result.stdout)
        out.flush()
        err.write(result.stderr)
        err.flush()
        if result.returncode:
            print(f'{backend}: pkg-config could not report a version', file=err, flush=True)
            return result.returncode
    args = ['--locked', *cell.feature_flags(backend), '-p', 'oxpinyin-store']
    if platform.system() == 'Linux':
        result = tools.run(['pkg-config', '--modversion', 'glib-2.0'])
        out.write(result.stdout)
        out.flush()
        err.write(result.stderr)
        err.flush()
        if result.returncode:
            return result.returncode
        args += ['-p', 'oxpinyin-capi']
    try:
        rc = tools.supervised(['cargo', 'test', *args],
                              environment=dict(os.environ if env is None else env))
    except Exception as exc:
        print(f'FAIL: {exc}', file=err, flush=True)
        return 1
    if rc:
        return rc
    print('\nSetup check passed: native backend tests linked and ran. This is not oracle parity.', file=out, flush=True)
    return 0


LINT_USAGE = ('usage: lint-crate-cfg-placement.sh [file...]\n'
              '       with no arguments, lints every tracked *.rs file\n')
LINT_VIOLATION_HEADER = ('crate-cfg-placement: a crate-level `#![cfg(...)]` must sit BELOW '
                         'its `//!` docs.\n\n')
LINT_VIOLATION_FOOTER = (
    'A false crate-level `#![cfg(...)]` discards the crate attributes that follow\n'
    'it, so these docs vanish on the excluded platform and `missing_docs` fires\n'
    'there. Move the `#![cfg(...)]` below the `//!` block; the gate keeps its\n'
    'predicate and its whole-crate scope.\n')


def _placement_hit(path):
    """Return (gate line, doc line) for the first crate-level ///! after a gate."""
    gate = 0
    with path.open('rb') as source:
        for number, line in enumerate(source, 1):
            if gate == 0 and line.startswith(b'#![cfg('):
                gate = number
            elif gate and line.startswith(b'//!'):
                return gate, number
    return None


def lint_crate_cfg_placement(argv, *, out=sys.stdout, err=sys.stderr, root=ROOT,
                             tools=None):
    """Lint crate-level `#![cfg(...)]` placement and return 0, 1 or 2.

    No arguments scan every tracked Rust file; named paths must be readable
    regular files. Exit 2 is a usage or environment error, never a violation.
    """
    args = list(argv)
    if args[:1] in (['-h'], ['--help']):
        err.write(LINT_USAGE)
        return 2
    tools = native.Tools(root=root) if tools is None else tools
    if args:
        unreadable = [f'  {name}\n' for name in args if not Path(name).is_file()]
        if unreadable:
            err.write('crate-cfg-placement: not a readable regular file:\n' + ''.join(unreadable))
            return 2
        files = args
    else:
        try:
            result = tools.run(['git', 'ls-files', '--', '*.rs'], text=False, stdout=subprocess.PIPE)
        except OSError:
            result = None
        if result is None or result.returncode:
            err.write('crate-cfg-placement: not a git repository and no files given\n')
            return 2
        files = [os.fsdecode(name) for name in result.stdout.split(b'\n') if name]
    violations = []
    checked = 0
    named = bool(args)
    for name in files:
        path = Path(name) if named else Path(root) / name
        if not path.is_file():
            continue
        checked += 1
        hit = _placement_hit(path)
        if hit:
            gate, doc = hit
            violations.append(
                f'  {name}:{doc}: `//!` doc comment below the crate-level '
                f'`#![cfg(...)]` on line {gate}\n')
    if violations:
        err.write(LINT_VIOLATION_HEADER)
        err.write(''.join(violations))
        err.write('\n')
        err.write(LINT_VIOLATION_FOOTER)
        return 1
    print(f'crate-cfg-placement: {checked} file(s) checked, no violations', file=out)
    return 0
