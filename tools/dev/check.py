"""Native setup and lint checks.

These are setup and gate checks, not oracle or pin comparisons. The old
optional oracle-directory display is intentionally absent: the native boundary
does not probe an oracle installation or pass its variables to tests.
"""

if __name__ == '__main__':
    from .entry import require_normal_imports

    require_normal_imports()

import argparse
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys

from . import cell, native

ROOT = Path(__file__).resolve().parents[2]


def lookup(env, name):
    """NAME's value in ENV, ignoring case: os.environ upper-cases Windows names."""
    return next((value for key, value in env.items() if key.upper() == name.upper()), '')


def msvc_environment(env, *, capture=subprocess.run):
    """The Visual Studio developer environment: the caller's, or vcvars' via vswhere."""
    if lookup(env, 'VCToolsInstallDir'):
        return env
    root = lookup(env, 'ProgramFiles(x86)') or r'C:\Program Files (x86)'
    vswhere = Path(root) / 'Microsoft Visual Studio' / 'Installer' / 'vswhere.exe'
    found = capture([str(vswhere), '-latest', '-products', '*', '-requires',
                     'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'],
                    env=env, capture_output=True, text=True)
    installation = found.stdout.strip().splitlines()[0] if found.returncode == 0 and found.stdout.strip() else ''
    if not installation:
        raise ValueError('no Visual Studio with the C++ tools found; run from a Developer PowerShell for VS')
    script = 'vcvarsarm64.bat' if platform.machine().upper() == 'ARM64' else 'vcvars64.bat'
    vcvars = Path(installation) / 'VC' / 'Auxiliary' / 'Build' / script
    # cmd /s keeps the outer quotes literal, so the batch path may hold spaces.
    done = capture(f'cmd /d /s /c ""{vcvars}" >nul && set"', env=env, capture_output=True, text=True)
    if done.returncode:
        raise ValueError(f'{vcvars} failed with status {done.returncode}')
    # `set` also lists cmd's hidden per-drive "=C:=C:\\..." entries; skip them.
    loaded = dict(line.split('=', 1) for line in done.stdout.splitlines()
                  if '=' in line and not line.startswith('='))
    if not lookup(loaded, 'VCToolsInstallDir'):
        raise ValueError(f'{vcvars} did not set VCToolsInstallDir')
    return loaded


USAGE = 'usage: just check [' + '|'.join(cell.BACKENDS) + ']'


def windows_compiler(env, *, capture=subprocess.run, which=shutil.which):
    """Whether the MSVC compiler the Windows build uses is available through ENV.

    A developer shell has cl on PATH or has loaded the developer environment;
    otherwise the Visual Studio found through vswhere is used.
    The Rust and C++ bridge builds use MSVC, never cc.
    """
    if which('cl', path=lookup(env, 'PATH')):
        return True
    try:
        msvc_environment(env, capture=capture)
    except (ValueError, OSError):
        return False
    return True


def pkg_config_program(env):
    """The program that answers pkg-config queries: PKG_CONFIG on Windows, else pkg-config.

    Windows has no pkg-config; the setup names a pkgconf.exe in PKG_CONFIG.
    """
    if sys.platform == 'win32':
        return lookup(env, 'PKG_CONFIG') or 'pkg-config'
    return 'pkg-config'


def missing_tool(tools):
    """Name the first tool this host's checks need and TOOLS cannot find, or None.

    POSIX needs cargo, rustup, cc and pkg-config. Windows needs cargo, rustup,
    MSVC and the pkg-config program: cc is a Unix compiler driver, and the one
    call of it (the Berkeley DB header) has no Windows build.
    """
    windows = sys.platform == 'win32'
    for tool in ('cargo', 'rustup', 'msvc' if windows else 'cc', pkg_config_program(tools.environment)):
        if tool == 'msvc':
            if not windows_compiler(tools.environment):
                return 'MSVC (cl, or the Visual Studio C++ tools)'
        elif tools.which(tool) is None:
            return tool
    return None


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
    missing = missing_tool(tools)
    if missing is not None:
        print(f'missing {missing}; see docs/runbooks/setup.md', file=err, flush=True)
        return 1
    pkg_config = pkg_config_program(tools.environment)
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
    result = tools.run([pkg_config, '--modversion', 'libclang'])
    if result.returncode == 0:
        out.write(result.stdout)
        out.flush()
    else:
        print('libclang: discovered by bindgen during build (a .pc file is optional)', file=out, flush=True)
    if backend == 'bdb':
        if sys.platform == 'win32':
            print('bdb: reading db.h needs cc, which the MSVC build does not use; see docs/runbooks/setup.md',
                  file=err, flush=True)
            return 1
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
        result = tools.run([pkg_config, '--modversion', backend])
        out.write(result.stdout)
        out.flush()
        err.write(result.stderr)
        err.flush()
        if result.returncode:
            print(f'{backend}: pkg-config could not report a version', file=err, flush=True)
            return result.returncode
    args = ['--locked', *cell.feature_flags(backend), '-p', 'oxpinyin-store']
    if platform.system() == 'Linux':
        result = tools.run([pkg_config, '--modversion', 'glib-2.0'])
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


LINT_USAGE = ('usage: python -c "from tools.dev.check import lint_crate_cfg_placement; import sys; sys.exit(lint_crate_cfg_placement(sys.argv[1:]))" [file...]\n'
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

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='Run native setup checks.')
    parser.add_argument('backend', nargs='?')
    raise SystemExit(run(parser.parse_args().backend))
