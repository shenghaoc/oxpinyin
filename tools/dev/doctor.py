"""Native capability report and interpreter acceptance. Standard library only.

Doctor reads capabilities; it does not provision, install or enter a
container. It rejects interpreter-selection overrides and a free-threaded
build, and it names the installation step for optional tools it cannot find.
"""
import os
import platform
import re
import shutil
import subprocess
import sys
import sysconfig
from pathlib import Path

OVERRIDES = (
    'UV_PYTHON',
    'UV_PYTHON_VERSION',
    'UV_PYTHON_IMPLEMENTATION',
    'UV_PYTHON_VARIANT',
    'UV_PYTHON_ARCH',
    'UV_PYTHON_BIN_DIR',
    'UV_PYTHON_PREFERENCE',
    'PYTHONHOME',
    'PYTHONPATH',
)
MANAGED_REQUEST = 'cpython@3.15.0+gil'
NEXTEST_MINIMUM = '0.9.115'


class DoctorError(RuntimeError):
    pass


def reject_overrides(environ=None):
    """Reject interpreter-selection settings; keep storage/cache settings."""
    environ = os.environ if environ is None else environ
    found = [name for name in OVERRIDES if environ.get(name)]
    if found:
        names = ', '.join(found)
        raise DoctorError(f'unsupported interpreter override(s): {names}; unset them before doctor')


def _run(command, environ):
    try:
        return subprocess.run(command, capture_output=True, text=True, env=environ, check=False)
    except OSError as exc:
        return None


def uv_version(environ=None):
    environ = os.environ if environ is None else environ
    result = _run(['uv', '--version'], environ)
    if result is None or result.returncode != 0:
        raise DoctorError('missing uv 0.13.1; install it from its documented host route')
    text = result.stdout.strip()
    if not text.startswith('uv 0.13.1 '):
        raise DoctorError(f'uv 0.13.1 required; found {text}')
    return text


def identity(environ=None):
    """Check the running interpreter and return its identity record."""
    environ = os.environ if environ is None else environ
    reject_overrides(environ)
    uv_version(environ)
    implementation = sys.implementation.name
    version = platform.python_version()
    if implementation != 'cpython':
        raise DoctorError(f'CPython required; found implementation {implementation}')
    if version != '3.15.0':
        raise DoctorError(f'CPython 3.15.0 required; found {version}')
    gil_disabled = sysconfig.get_config_var('Py_GIL_DISABLED')
    if gil_disabled:
        raise DoctorError('free-threaded CPython is not accepted; use the GIL-enabled build')
    gil_enabled = getattr(sys, '_is_gil_enabled', lambda: True)()
    if not gil_enabled:
        raise DoctorError('the running CPython has the GIL disabled')
    result = _run(['uv', 'python', 'find', '--managed-python', MANAGED_REQUEST], environ)
    if result is None or result.returncode != 0 or not result.stdout.strip():
        raise DoctorError('the running interpreter is not a uv-managed CPython 3.15.0+gil build')
    managed = Path(result.stdout.strip()).resolve()
    executable = Path(sys.executable).resolve()
    if managed != executable:
        raise DoctorError(f'the running interpreter is not the managed one: {executable} (managed: {managed})')
    return dict(implementation=implementation, version=version,
                gil_enabled=bool(gil_enabled), gil_disabled=bool(gil_disabled),
                executable=str(executable), managed=bool(managed == executable),
                uv=uv_version(environ))


def _first_line(result):
    if result is None:
        return None
    text = (result.stdout or result.stderr).strip()
    return text.splitlines()[0] if text else None


def nextest_version(environ=None):
    """Check an installed nextest; keep a missing optional tool report-only."""
    environ = os.environ if environ is None else environ
    executable = shutil.which('cargo-nextest', path=environ.get('PATH'))
    if executable is None:
        return None
    result = _run([executable, '--version'], environ)
    text = _first_line(result)
    version = re.match(r'^cargo-nextest (\d+)\.(\d+)\.(\d+)(?:\s|$)', text or '')
    minimum = tuple(map(int, NEXTEST_MINIMUM.split('.')))
    if (result is None or result.returncode != 0 or version is None
            or tuple(map(int, version.groups())) < minimum):
        raise DoctorError(f'cargo-nextest {NEXTEST_MINIMUM} or later required; '
                          f'found {text or "unreadable version"}')
    return text


def host_lines(environ=None):
    environ = os.environ if environ is None else environ
    lines = [f'os: {platform.system()} {platform.release()}',
             f'architecture: {platform.machine()}']
    rustup = shutil.which('rustup')
    rustc = shutil.which('rustc')
    if rustup:
        line = _first_line(_run(['rustup', 'show', 'active-toolchain'], environ))
        lines.append(f'rust toolchain: {line or "unknown"}')
    elif rustc:
        lines.append(f'rust toolchain: {_first_line(_run([rustc, "--version"], environ)) or "unknown"}')
    else:
        lines.append('rust toolchain: missing (install rustup: https://rustup.rs)')
    cc = shutil.which('cc')
    lines.append(f'compiler: {(_first_line(_run([cc, "--version"], environ)) if cc else "missing") or "unknown"}')
    backends = ['pkg-config', 'db5.3_dump', 'kchashmgr', 'tkrzw_dbm_util']
    found = [f'{name}={"yes" if shutil.which(name) else "no"}' for name in backends]
    lines.append('native backend utilities: ' + ', '.join(found))
    just = shutil.which('just')
    lines.append('just: ' + (just if just else 'missing (install: cargo install just --locked)'))
    nextest = shutil.which('cargo-nextest', path=environ.get('PATH'))
    lines.append('cargo-nextest: ' + (nextest if nextest else
                 f'missing (requires {NEXTEST_MINIMUM} or later; install: cargo install cargo-nextest --locked)'))
    runtimes = [name for name in ('container', 'podman', 'docker', 'wslc') if shutil.which(name)]
    lines.append('container runtime: ' + (', '.join(runtimes) if runtimes else 'none present'))
    if sys.platform.startswith('linux'):
        valgrind = shutil.which('valgrind')
        lines.append('valgrind: ' + (valgrind if valgrind else 'missing (install: apt-get install valgrind)'))
    return lines


def collect(environ=None):
    """Return (lines, status) without printing or provisioning."""
    environ = os.environ if environ is None else environ
    lines = host_lines(environ)
    status = 0
    try:
        info = identity(environ)
        lines.extend([
            f'uv: {info["uv"]}',
            f'interpreter: {info["implementation"]} {info["version"]}',
            f'managed: {"yes" if info["managed"] else "no"}',
            f'gil: {"enabled" if info["gil_enabled"] else "disabled"}',
            f'executable: {info["executable"]}',
        ])
    except DoctorError as exc:
        lines.append(f'FAIL: {exc}')
        status = 1
    try:
        version = nextest_version(environ)
        if version is not None:
            lines.append(f'cargo-nextest version: {version}')
    except DoctorError as exc:
        lines.append(f'FAIL: {exc}')
        status = 1
    return lines, status


def main(argv=None):
    lines, status = collect()
    for line in lines:
        print(line)
    return status
