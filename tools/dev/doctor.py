"""Native capability report, interpreter acceptance and the one check of tool minimums.

Standard library only. Doctor reads capabilities; it does not provision,
install or enter a container. It rejects interpreter-selection overrides and a
free-threaded build, and it checks every tool against its minimum version: a
newer tool passes, an older one fails with the minimum and the version found,
and an absent one is reported with its installation step. Runbooks and
setup.sh point here instead of repeating version lists.
"""
import os
import platform
import re
import shutil
import subprocess
import sys
import sysconfig
from pathlib import Path
from typing import NamedTuple

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
# The managed 3.15 series, GIL build. 3.16 stays a deliberate step: the
# -X lazy_imports modes this tooling relies on are new in 3.15.
MANAGED_REQUEST = 'cpython@3.15+gil'
PYTHON_MINIMUM = '3.15.0'
PYTHON_BELOW = '3.16'
UV_MINIMUM = (0, 13, 1)


class DoctorError(RuntimeError):
    pass


class Tool(NamedTuple):
    """An external tool and the oldest version the repository accepts."""
    name: str
    executable: str
    arguments: tuple
    pattern: str        # applied to the first line of the version output
    minimum: str
    install: str
    platform: str = ''  # sys.platform prefix this check is for; empty means every platform
    identity: str = ''  # when set, any other program under this name is reported as absent


# One row per tool. A minimum is the oldest version the repository has run
# or documents. Where the tool has its own configuration for it (the
# justfile, .config/nextest.toml, uv.toml, setup.sh) doctor.test.py keeps the
# two in step.
TOOLS = (
    Tool('just', 'just', ('--version',),
         r'^just (\d+)\.(\d+)\.(\d+)', '1.59.0',
         'brew install just, your distribution package, or cargo install just --locked'),
    Tool('cargo-nextest', 'cargo-nextest', ('--version',),
         r'^cargo-nextest (\d+)\.(\d+)\.(\d+)', '0.9.115',
         'cargo install cargo-nextest --locked'),
    Tool('cargo-c', 'cargo-cinstall', ('--version',),
         r'^cargo-c (\d+)\.(\d+)\.(\d+)', '0.10.24',
         'cargo install cargo-c --locked, or your distribution package'),
    Tool('cargo-fuzz', 'cargo-fuzz', ('--version',),
         r'^cargo-fuzz (\d+)\.(\d+)\.(\d+)', '0.13.2',
         'cargo install cargo-fuzz --locked'),
    Tool('cargo-llvm-cov', 'cargo-llvm-cov', ('llvm-cov', '--version'),
         r'^cargo-llvm-cov (\d+)\.(\d+)\.(\d+)', '0.9.0',
         'cargo install cargo-llvm-cov --locked'),
    Tool('GNU ld', 'ld', ('--version',),
         r'^GNU ld .*?(\d+)\.(\d+)(?:\.(\d+))?', '2.43',
         'apt-get install binutils', platform='linux', identity=r'^GNU ld\b'),
    Tool('gcov', 'gcov', ('--version',),
         r'^gcov \(.*?\) (\d+)\.(\d+)\.(\d+)', '14',
         'apt-get install gcc', platform='linux'),
    Tool('valgrind', 'valgrind', ('--version',),
         r'^valgrind-(\d+)\.(\d+)(?:\.(\d+))?', '3.22.0',
         'apt-get install valgrind', platform='linux'),
)


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


def _numbers(text):
    return tuple(int(part) for part in text.split('.'))


def _at_least(found, minimum):
    """Compare dotted versions of any length; missing components count as zero."""
    width = max(len(found), len(minimum))
    pad = lambda numbers: numbers + (0,) * (width - len(numbers))
    return pad(found) >= pad(minimum)


def uv_version(environ=None):
    environ = os.environ if environ is None else environ
    result = _run(['uv', '--version'], environ)
    if result is None or result.returncode != 0:
        raise DoctorError('missing uv 0.13.1 or newer; install it from its documented host route')
    text = result.stdout.strip()
    version = re.match(r'^uv (\d+)\.(\d+)\.(\d+)(?:\s|$)', text)
    if version is None:
        raise DoctorError(f'unparseable uv version output: {text or "empty"}; '
                          'uv 0.13.1 or newer required')
    if tuple(map(int, version.groups())) < UV_MINIMUM:
        raise DoctorError(f'uv 0.13.1 or newer required; found {text}')
    return text


def check_python(numbers=None, releaselevel=None):
    """Accept a final CPython release of the managed series: 3.15.0 or newer, below 3.16."""
    numbers = tuple(sys.version_info[:3]) if numbers is None else tuple(numbers)
    releaselevel = sys.version_info.releaselevel if releaselevel is None else releaselevel
    if (releaselevel != 'final' or not _at_least(numbers, _numbers(PYTHON_MINIMUM))
            or _at_least(numbers, _numbers(PYTHON_BELOW))):
        found = '.'.join(map(str, numbers)) + ('' if releaselevel == 'final' else f' ({releaselevel})')
        raise DoctorError(f'CPython {PYTHON_MINIMUM} or newer, below {PYTHON_BELOW}, required; '
                          f'found {found}')


def identity(environ=None):
    """Check the running interpreter and return its identity record."""
    environ = os.environ if environ is None else environ
    reject_overrides(environ)
    uv_version(environ)
    implementation = sys.implementation.name
    version = platform.python_version()
    if implementation != 'cpython':
        raise DoctorError(f'CPython required; found implementation {implementation}')
    check_python()
    gil_disabled = sysconfig.get_config_var('Py_GIL_DISABLED')
    if gil_disabled:
        raise DoctorError('free-threaded CPython is not accepted; use the GIL-enabled build')
    gil_enabled = getattr(sys, '_is_gil_enabled', lambda: True)()
    if not gil_enabled:
        raise DoctorError('the running CPython has the GIL disabled')
    result = _run(['uv', 'python', 'find', '--managed-python', MANAGED_REQUEST], environ)
    if result is None or result.returncode != 0 or not result.stdout.strip():
        raise DoctorError(f'the running interpreter is not a uv-managed {MANAGED_REQUEST} build')
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


def check_tool(tool, environ=None):
    """Return the report line for TOOL, or raise DoctorError if it is older than its minimum.

    A tool that is not installed, or that is another program under the same
    name, is reported with its installation step and does not fail the check.
    """
    environ = os.environ if environ is None else environ
    executable = shutil.which(tool.executable, path=environ.get('PATH'))
    if executable is None:
        return (f'{tool.name}: missing (requires {tool.minimum} or later; '
                f'install: {tool.install})')
    result = _run([executable, *tool.arguments], environ)
    text = _first_line(result)
    if tool.identity and not re.search(tool.identity, text or ''):
        return (f'{tool.name}: not found, {tool.executable} is {text or "unidentified"} '
                f'(requires {tool.minimum} or later; install: {tool.install})')
    version = re.search(tool.pattern, text or '')
    found = None if version is None else tuple(int(part) for part in version.groups() if part is not None)
    if (result is None or result.returncode != 0 or found is None
            or not _at_least(found, _numbers(tool.minimum))):
        raise DoctorError(f'{tool.name} {tool.minimum} or later required; '
                          f'found {text or "unreadable version"}')
    return f'{tool.name}: {".".join(map(str, found))} (minimum {tool.minimum})'


def tool_lines(environ=None):
    """Return (lines, failures): every tool that applies here, against its minimum."""
    lines = []
    failures = []
    for tool in TOOLS:
        if tool.platform and not sys.platform.startswith(tool.platform):
            continue
        try:
            lines.append(check_tool(tool, environ))
        except DoctorError as exc:
            failures.append(str(exc))
            lines.append(f'FAIL: {exc}')
    return lines, failures


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
    runtimes = [name for name in ('container', 'podman', 'docker', 'wslc') if shutil.which(name)]
    lines.append('container runtime: ' + (', '.join(runtimes) if runtimes else 'none present'))
    return lines


def collect(environ=None):
    """Return (lines, status) without printing or provisioning."""
    environ = os.environ if environ is None else environ
    lines = host_lines(environ)
    status = 0
    try:
        info = identity(environ)
        uv = re.match(r'^uv (\d+\.\d+\.\d+)', info['uv'])
        lines.extend([
            f'uv: {uv.group(1) if uv else info["uv"]} (minimum {".".join(map(str, UV_MINIMUM))})',
            f'interpreter: {info["implementation"]} {info["version"]} '
            f'(minimum {PYTHON_MINIMUM}, below {PYTHON_BELOW})',
            f'managed: {"yes" if info["managed"] else "no"}',
            f'gil: {"enabled" if info["gil_enabled"] else "disabled"}',
            f'executable: {info["executable"]}',
        ])
    except DoctorError as exc:
        lines.append(f'FAIL: {exc}')
        status = 1
    tools, failures = tool_lines(environ)
    lines.extend(tools)
    if failures:
        status = 1
    return lines, status


def main(argv=None):
    lines, status = collect()
    for line in lines:
        print(line)
    return status
