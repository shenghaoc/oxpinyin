"""Native setup check selection and status, migrated from tools/dev/check.sh.

This is a setup check, not an oracle or pin comparison. The old optional
oracle-directory display is intentionally absent: the native boundary does
not probe an oracle installation or pass its variables to tests.
"""
import os
import platform
from pathlib import Path
import shutil
import subprocess
import sys

from . import slot

ROOT = Path(__file__).resolve().parents[2]
BACKENDS = ('bdb', 'tkrzw', 'kyotocabinet')
USAGE = 'usage: tools/dev/check.sh [bdb|tkrzw|kyotocabinet]'


def _run(command, *, input_text=None):
    return subprocess.run(command, input=input_text, text=True, capture_output=True, cwd=ROOT)


def run(backend=None, out=sys.stdout, err=sys.stderr, env=None):
    """Run one native setup check and return its exit status."""
    backend = backend or 'bdb'
    if backend not in BACKENDS:
        print(USAGE, file=err, flush=True)
        return 2
    for tool in ('cargo', 'rustup', 'cc', 'pkg-config'):
        if shutil.which(tool) is None:
            print(f'missing {tool}; see docs/runbooks/setup.md', file=err, flush=True)
            return 1
    print(f'System: {platform.system()} {platform.machine()}', file=out, flush=True)
    result = _run(['rustup', 'show', 'active-toolchain'])
    out.write(result.stdout)
    out.flush()
    err.write(result.stderr)
    err.flush()
    if result.returncode:
        return result.returncode
    result = _run(['rustc', '-vV'])
    out.write(result.stdout)
    out.flush()
    err.write(result.stderr)
    err.flush()
    if result.returncode:
        return result.returncode
    target = os.environ.get('CARGO_TARGET_DIR') or 'target'
    print(f'Backend: {backend}; target directory: {target}', file=out, flush=True)
    result = _run(['pkg-config', '--modversion', 'libclang'])
    if result.returncode == 0:
        out.write(result.stdout)
        out.flush()
    else:
        print('libclang: discovered by bindgen during build (a .pc file is optional)', file=out, flush=True)
    if backend == 'bdb':
        command = ['cc']
        include = os.environ.get('OXPINYIN_BDB_INCLUDE_DIR')
        if include:
            command.append(f'-I{include}')
        command += ['-E', '-P', '-']
        result = _run(command, input_text='#include <db.h>\nDB_VERSION_STRING\n')
        if result.returncode:
            err.write(result.stderr)
            err.flush()
            return result.returncode
        lines = result.stdout.rstrip().splitlines()
        print('Berkeley DB header: ' + (lines[-1] if lines else ''), file=out, flush=True)
    else:
        result = _run(['pkg-config', '--modversion', backend])
        out.write(result.stdout)
        out.flush()
        err.write(result.stderr)
        err.flush()
        if result.returncode:
            print(f'{backend}: pkg-config could not report a version', file=err, flush=True)
            return result.returncode
    args = ['--locked', '--no-default-features', '--features', backend, '-p', 'oxpinyin-store']
    if platform.system() == 'Linux':
        result = _run(['pkg-config', '--modversion', 'glib-2.0'])
        out.write(result.stdout)
        out.flush()
        err.write(result.stderr)
        err.flush()
        if result.returncode:
            return result.returncode
        args += ['-p', 'oxpinyin-capi']
    # Send slot diagnostics to stderr. Keep stdout aligned with check.sh.
    try:
        rc = slot.run_command(['cargo', 'test', *args], env=dict(os.environ if env is None else env),
                              out=err)
    except Exception as exc:
        print(f'FAIL: {exc}', file=err, flush=True)
        return 1
    if rc:
        return rc
    print('\nSetup check passed: native backend tests linked and ran. This is not oracle parity.', file=out, flush=True)
    return 0
