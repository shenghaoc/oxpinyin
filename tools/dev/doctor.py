"""Native capability report, interpreter acceptance and the one check of tool minimums.

Standard library only. Doctor reads capabilities; it does not provision,
install or enter a container. It rejects interpreter-selection overrides and a
free-threaded build, and it checks every tool against its minimum version: a
newer tool passes, an older one fails with the minimum and the version found,
and an absent one is reported with its installation step. Each report line
names the resolved executable. Bash is required at the floor declared in
tools/lib/bash-floor.sh on Linux and macOS and reported as not applicable on
Windows, whose native recipes use PowerShell 7; ShellCheck is advisory at 0.11.0 because only the lint
job needs it. Each tool's version is
read in the format that tool prints and compared by that format's rules:
semantic versioning for most (a pre-release of the minimum is older than it),
the projects' own numbering for GNU ld, gcov and Valgrind. Runbooks and
setup.sh point here instead of repeating version lists.
"""
import os
import platform
import re
import shutil
import subprocess
import sys
import sysconfig
from collections.abc import Callable
from pathlib import Path
from typing import NamedTuple

from . import bash_floor

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
# The request .python-version states, the one place the interpreter version is
# written. The 3.15 series bounds below stay a deliberate step: the
# -X lazy_imports modes this tooling relies on are new in 3.15.
MANAGED_REQUEST = (Path(__file__).resolve().parents[2] / '.python-version').read_text().strip()
PYTHON_MINIMUM = '3.15.0'
PYTHON_BELOW = '3.16'
UV_MINIMUM = '0.13.1'


class DoctorError(RuntimeError):
    """Indicate an unsupported interpreter or an unacceptable tool version."""
    pass


class Version(NamedTuple):
    """A version as one tool prints it: KEY orders it, TEXT is what a report shows."""
    key: tuple
    text: str


class Format(NamedTuple):
    """How one tool writes its version. PARSE reads its output and returns a Version, or None
    when the output is not in this format; KEY turns a minimum into the same ordering."""
    parse: Callable
    key: Callable


_SEMVER = re.compile(r'(\d+)\.(\d+)\.(\d+)'
                     r'(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?'
                     r'(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?')


def semver_key(text):
    """Order a semantic version as semver.org 2.0.0 does (rule 11); ValueError when TEXT is not one.

    A pre-release is older than the release it precedes (1.59.0-beta.1 < 1.59.0), so it does not
    satisfy a minimum of that release, and newer than every lower release (1.60.0-beta.1 > 1.59.0).
    Pre-release identifiers compare field by field: numbers numerically and below names, names in
    ASCII order, a longer list above its own prefix. Build metadata (+...) does not take part.
    """
    match = _SEMVER.fullmatch(text)
    if match is None:
        raise ValueError(f'not a semantic version: {text!r}')
    core = tuple(int(part) for part in match.groups()[:3])
    prerelease = match.group(4)
    if prerelease is None:
        return core, 1, ()
    return core, 0, tuple((0, int(part), '') if part.isdigit() else (1, 0, part)
                          for part in prerelease.split('.'))


def _semver_format(program):
    """The format of a tool whose first line reads '<program> <semantic version> ...'."""
    line = re.compile(rf'{re.escape(program)} (\S+)')

    def parse(text):
        """Read the leading program/version pair, returning None for malformed versions."""
        match = line.match(text)
        if match is None:
            return None
        try:
            return Version(semver_key(match.group(1)), match.group(1))
        except ValueError:
            return None

    return Format(parse, semver_key)


def _dotted_key(text):
    """Order a dotted version by every numeric component: 2.43 == 2.43.0 < 2.43.0.1 < 2.43.1."""
    parts = [int(part) for part in text.split('.')]
    while parts and parts[-1] == 0:
        parts.pop()
    return tuple(parts)


# GNU ld, gcov and Valgrind do not use semantic versioning. Each is read in its own format, compared by
# number, and a development build counts as its number: it is never "older" for being one, and the
# hyphen of a distribution's package release is never a pre-release.
#
# GNU ld: "GNU ld (GNU Binutils for Debian) 2.47", "GNU ld version 2.43.1-15.fc42". Releases are 2.43
# and point releases such as 2.43.1; a distribution appends its package release after a hyphen
# (2.43.1-15.fc42, dropped here) or a snapshot date after a dot (2.45.1.20251027, kept as a component).
# Binutils numbers its development branch X.Y.50 once X.Y has branched, so a snapshot such as
# 2.43.50.20240101 is newer than 2.43 and counts as 2.43.50; 2.42.50.<date> is the road to 2.43 and
# does not reach it.
_GNU_LD = re.compile(r'GNU ld (?:\(.*?\)|version)\s+(\d+(?:\.\d+)+)')


def _parse_gnu_ld(text):
    """Parse GNU ld numeric version components, or return None for another format."""
    match = _GNU_LD.match(text)
    return None if match is None else Version(_dotted_key(match.group(1)), match.group(1))


# gcov: "gcov (Debian 16.2.0-3) 16.2.0", "gcov (GCC) 15.2.1 20260101 (Red Hat 15.2.1-7)". The package
# string sits in parentheses, then come GCC's own version, a date and a development phase. The version
# is the one after the package string; a hyphen release inside the parentheses is the package's. GCC
# marks trunk snapshots "(experimental)" (N.0.0 and N.0.1, before N.1.0) and release-branch snapshots
# "(prerelease)" (N.M.1, after N.M.0). A snapshot counts as its number: the minimum is a major version
# (14), so a snapshot of 14 counts as 14 and one of 15 as 15. gcov_measure.py checks the JSON
# format_version when it runs, which decides for a snapshot that predates the format. The phase is
# shown in the report.
_GCOV = re.compile(r'gcov \(.*?\)\s+(\d+(?:\.\d+)+)(.*)')


def _parse_gcov(text):
    """Read gcov numeric ordering and its development phase, or None for no match."""
    match = _GCOV.match(text)
    if match is None:
        return None
    version, rest = match.groups()
    phase = re.search(r'\((experimental|prerelease)\)', rest)
    return Version(_dotted_key(version), f'{version} {phase.group(1)}' if phase else version)


# Valgrind: "valgrind-3.27.1". A build from the git repository adds ".GIT" to the number of the release
# it is leading up to ("valgrind-3.22.0.GIT" is a snapshot on the way to 3.22.0); a distribution may
# append its package release after a hyphen (dropped here). A development build counts as its number:
# the minimum is the oldest release measured with, not a feature, and a git build of it is a fair
# stand-in. 3.21.0.GIT is still below 3.22.0, and 3.23.0.GIT is above it.
_VALGRIND = re.compile(r'valgrind-(\d+(?:\.\d+)*)(?:\.(GIT|SVN))?(?![\w.])')


def _parse_valgrind(text):
    """Read Valgrind numeric ordering and GIT/SVN display suffix, or None for no match."""
    match = _VALGRIND.match(text)
    if match is None:
        return None
    version, development = match.groups()
    return Version(_dotted_key(version), f'{version}.{development}' if development else version)


class Tool(NamedTuple):
    """An external tool and the oldest version the repository accepts."""
    name: str
    executable: str
    arguments: tuple
    format: Format      # how this tool writes its version
    minimum: str
    install: str
    platform: str = ''  # sys.platform prefix this check is for; empty means every platform
    identity: str = ''  # when set, any other program under this name is reported as absent
    optional: bool = False  # report the version but do not fail collect when it is old or missing
    applies_to: tuple = ()  # sys.platform prefixes it is checked on; elsewhere it is not applicable


UV = _semver_format('uv')

# Bash prints its version in a banner; ShellCheck puts it on the second line.
_BASH = re.compile(r'GNU bash, version (\d+(?:\.\d+)+)')
_SHELLCHECK = re.compile(r'^version:\s*(\d+(?:\.\d+)+)\s*$', re.MULTILINE)


def _parse_bash(text):
    """Read the number from 'GNU bash, version X.Y.Z(...)', or return None for another format."""
    match = _BASH.search(text)
    return None if match is None else Version(_dotted_key(match.group(1)), match.group(1))


def _parse_shellcheck(text):
    """Read ShellCheck's 'version: X.Y.Z' line, or return None for another format."""
    match = _SHELLCHECK.search(text)
    return None if match is None else Version(_dotted_key(match.group(1)), match.group(1))


# One row per tool. A minimum is the oldest version the repository has run
# or documents. Where the tool has its own configuration for it (the
# justfile, .config/nextest.toml, uv.toml, setup.sh) doctor.test.py keeps the
# two in step.
TOOLS = (
    Tool('bash', 'bash', ('--version',), Format(_parse_bash, _dotted_key), bash_floor.read_floor(),
         'brew install bash (macOS) and make env bash select it, or your distribution package',
         applies_to=('linux', 'darwin')),
    Tool('just', 'just', ('--version',), _semver_format('just'), '1.59.0',
         'brew install just, your distribution package, or cargo install just --locked'),
    Tool('cargo-nextest', 'cargo-nextest', ('--version',), _semver_format('cargo-nextest'), '0.9.115',
         'cargo install cargo-nextest --locked'),
    Tool('cargo-c', 'cargo-cinstall', ('--version',), _semver_format('cargo-c'), '0.10.24',
         'cargo install cargo-c --locked, or your distribution package'),
    Tool('cargo-fuzz', 'cargo-fuzz', ('--version',), _semver_format('cargo-fuzz'), '0.13.2',
         'cargo install cargo-fuzz --locked'),
    Tool('cargo-llvm-cov', 'cargo-llvm-cov', ('llvm-cov', '--version'), _semver_format('cargo-llvm-cov'),
         '0.9.0', 'cargo install cargo-llvm-cov --locked'),
    Tool('GNU ld', 'ld', ('--version',), Format(_parse_gnu_ld, _dotted_key), '2.43',
         'apt-get install binutils', platform='linux', identity=r'^GNU ld\b'),
    Tool('gcov', 'gcov', ('--version',), Format(_parse_gcov, _dotted_key), '14',
         'apt-get install gcc', platform='linux'),
    Tool('valgrind', 'valgrind', ('--version',), Format(_parse_valgrind, _dotted_key), '3.22.0',
         'apt-get install valgrind', platform='linux'),
    Tool('shellcheck', 'shellcheck', ('--version',), Format(_parse_shellcheck, _dotted_key), '0.11.0',
         'apt-get install shellcheck, or brew install shellcheck', optional=True),
)


def reject_overrides(environ=None):
    """Reject interpreter-selection settings; keep storage/cache settings."""
    environ = os.environ if environ is None else environ
    found = [name for name in OVERRIDES if environ.get(name)]
    if found:
        names = ', '.join(found)
        raise DoctorError(f'unsupported interpreter override(s): {names}; unset them before doctor')


def _run(command, environ):
    """Capture a command with the supplied environment; return None on spawn errors."""
    try:
        return subprocess.run(command, capture_output=True, text=True, env=environ, check=False)
    except OSError as exc:
        return None


def _numbers(text):
    """Convert dot-separated integer components to a tuple; invalid components raise."""
    return tuple(int(part) for part in text.split('.'))


def _at_least(found, minimum):
    """Compare dotted versions of any length; missing components count as zero."""
    width = max(len(found), len(minimum))
    pad = lambda numbers: numbers + (0,) * (width - len(numbers))
    return pad(found) >= pad(minimum)


def uv_version(environ=None):
    """Check uv against the minimum and return its stripped version output.

    Raise DoctorError when uv cannot run, prints an unknown format, or is too old.
    """
    environ = os.environ if environ is None else environ
    result = _run(['uv', '--version'], environ)
    if result is None or result.returncode != 0:
        raise DoctorError(f'missing uv {UV_MINIMUM} or newer; install it from its documented host route')
    text = result.stdout.strip()
    version = UV.parse(text)
    if version is None:
        raise DoctorError(f'unparseable uv version output: {text or "empty"}; '
                          f'uv {UV_MINIMUM} or newer required')
    if version.key < UV.key(UV_MINIMUM):
        raise DoctorError(f'uv {UV_MINIMUM} or newer required; found {text}')
    return text


def check_python(numbers=None, releaselevel=None):
    """Require a final version in the managed 3.15 series.

    Default to the running version and release level. Raise DoctorError for
    a prerelease, a version below 3.15.0, or a version at least 3.16; this
    helper does not check interpreter implementation or ownership.
    """
    numbers = tuple(sys.version_info[:3]) if numbers is None else tuple(numbers)
    releaselevel = sys.version_info.releaselevel if releaselevel is None else releaselevel
    if (releaselevel != 'final' or not _at_least(numbers, _numbers(PYTHON_MINIMUM))
            or _at_least(numbers, _numbers(PYTHON_BELOW))):
        found = '.'.join(map(str, numbers)) + ('' if releaselevel == 'final' else f' ({releaselevel})')
        raise DoctorError(f'CPython {PYTHON_MINIMUM} or newer, below {PYTHON_BELOW}, required; '
                          f'found {found}')


def identity(environ=None):
    """Validate the running managed CPython and return its identity record.

    Reject interpreter overrides, unsupported versions, disabled GIL, or a
    resolved executable differing from uv python find. Run uv subprocesses
    and raise DoctorError when these acceptance checks fail.
    """
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
    """Return the first nonblank output line, preferring stdout to stderr.

    Return None for an absent result or empty selected stream.
    """
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
    output = ((result.stdout or result.stderr) if result else '').strip()
    if tool.identity and not re.search(tool.identity, text or ''):
        return (f'{tool.name}: not found, {tool.executable} is {text or "unidentified"} at {executable} '
                f'(requires {tool.minimum} or later; install: {tool.install})')
    version = tool.format.parse(output)
    if (result is None or result.returncode != 0 or version is None
            or version.key < tool.format.key(tool.minimum)):
        found = version.text if version is not None else (text or 'unreadable version')
        raise DoctorError(f'{tool.name} {tool.minimum} or later required; found {found} at {executable}')
    return f'{tool.name}: {version.text} at {executable} (minimum {tool.minimum})'


def tool_lines(environ=None):
    """Return (lines, failures): every tool that applies here, against its minimum."""
    lines = []
    failures = []
    for tool in TOOLS:
        if tool.platform and not sys.platform.startswith(tool.platform):
            continue
        if tool.applies_to and not sys.platform.startswith(tool.applies_to):
            lines.append(f'{tool.name}: not applicable on {sys.platform}')
            continue
        try:
            line = check_tool(tool, environ)
        except DoctorError as exc:
            if tool.optional:
                lines.append(f'advisory: {exc}')
            else:
                failures.append(str(exc))
                lines.append(f'FAIL: {exc}')
        else:
            lines.append(line + (' (advisory)' if tool.optional else ''))
    return lines, failures


def host_lines(environ=None):
    """Collect OS, architecture, compiler, and available utility descriptions.

    Run version queries with environ; executable discovery uses the current
    process PATH. Return lines without printing or provisioning.
    """
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
        uv = UV.parse(info['uv'])
        lines.extend([
            f'uv: {uv.text if uv else info["uv"]} (minimum {UV_MINIMUM})',
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
    """Print the collected capability report and return its aggregate status.

    The argv parameter is unused.
    """
    lines, status = collect()
    for line in lines:
        print(line)
    return status
