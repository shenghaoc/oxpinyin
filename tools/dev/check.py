"""Native setup, lint, backend-matrix and host-backend checks.

These are setup and gate checks, not oracle or pin comparisons. The old
optional oracle-directory display is intentionally absent: the native boundary
does not probe an oracle installation or pass its variables to tests.
"""
import os
from pathlib import Path
import platform
import re
import subprocess
import sys

from . import native

ROOT = Path(__file__).resolve().parents[2]
BACKENDS = ('bdb', 'tkrzw', 'kyotocabinet')
USAGE = 'usage: tools/dev/check.sh [bdb|tkrzw|kyotocabinet]'


def run(backend=None, out=sys.stdout, err=sys.stderr, env=None):
    """Check native prerequisites, then build and run the selected backend tests.

    Default to Berkeley DB and include the C API tests on Linux. Print tool
    and backend diagnostics; return 2 for an unknown backend, 1 for missing
    tools or slot errors, or the failing child status. The optional env is
    used only for the Cargo child admitted through the heavy slot.
    """
    backend = backend or 'bdb'
    if backend not in BACKENDS:
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
        include = os.environ.get('OXPINYIN_BDB_INCLUDE_DIR')
        if include:
            command.append(f'-I{include}')
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
    args = ['--locked', '--no-default-features', '--features', backend, '-p', 'oxpinyin-store']
    if platform.system() == 'Linux':
        result = tools.run(['pkg-config', '--modversion', 'glib-2.0'])
        out.write(result.stdout)
        out.flush()
        err.write(result.stderr)
        err.flush()
        if result.returncode:
            return result.returncode
        args += ['-p', 'oxpinyin-capi']
    # Send slot diagnostics to stderr. Keep stdout aligned with check.sh.
    try:
        rc = tools.admitted(['cargo', 'test', *args],
                            environment=dict(os.environ if env is None else env), out=err)
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


VALID_MATRIX = (
    ('', 'default (bdb)'),
    ('--no-default-features --features kyotocabinet', '--no-default-features --features kyotocabinet'),
    ('--no-default-features --features tkrzw', '--no-default-features --features tkrzw'),
    ('--no-default-features --features bdb', '--no-default-features --features bdb'),
)
INVALID_MATRIX = (
    'kyotocabinet,tkrzw',
    'kyotocabinet,bdb',
    'tkrzw,bdb',
    'kyotocabinet,tkrzw,bdb',
)


def _matrix_result(tools, args):
    """Run one cargo check and return its merged output and status."""
    argv = ['cargo', 'check', '--locked', '-p', 'oxpinyin-store', *args]
    try:
        result = tools.run(argv, stderr=subprocess.STDOUT, stdout=subprocess.PIPE)
        return result.returncode, result.stdout or ''
    except FileNotFoundError as error:
        return native.launch_status(error), f'{argv[0]}: command not found\n'
    except OSError as error:
        return native.launch_status(error), f'{error}\n'


def _indent(lines, width=5):
    return ''.join(' ' * width + line + '\n' for line in lines)


def backend_matrix(*, out=sys.stdout, err=sys.stderr, root=ROOT, tools=None):
    """Prove the exactly-one-backend invariant and return 0 or 1.

    Valid selections must compile; every multi-backend and zero-backend
    selection must be refused by the store crate's own guard.
    """
    tools = native.Tools(root=root) if tools is None else tools
    passed = failed = 0
    for peer, label in VALID_MATRIX:
        args = peer.split() if peer else []
        print(f'\u2500\u2500 valid: {label}', file=out, flush=True)
        code, log = _matrix_result(tools, args)
        if code == 0:
            print('   PASS', file=out, flush=True)
            passed += 1
        else:
            print('   FAIL \u2014 expected a clean build; tail:', file=out, flush=True)
            out.write(_indent(log.splitlines()[-20:]))
            out.flush()
            failed += 1
    for combo in INVALID_MATRIX:
        print(f'\u2500\u2500 invalid: --features {combo}', file=out, flush=True)
        code, log = _matrix_result(tools, ['--no-default-features', '--features', combo])
        if code == 0:
            print('   FAIL \u2014 the guard did not fire, build succeeded', file=out, flush=True)
            failed += 1
        elif 'more than one store backend selected' in log:
            print('   PASS (refused by the exactly-one-backend guard)', file=out, flush=True)
            passed += 1
        else:
            print('   FAIL \u2014 build refused for another reason:', file=out, flush=True)
            lines = [line for line in log.splitlines()
                     if 'compile_error' in line or line.startswith('error')][:3]
            out.write(_indent(lines))
            out.flush()
            failed += 1
    print('\u2500\u2500 invalid: --no-default-features (zero backends)', file=out, flush=True)
    code, log = _matrix_result(tools, ['--no-default-features'])
    if code == 0:
        print('   FAIL \u2014 the guard did not fire, build succeeded', file=out, flush=True)
        failed += 1
    elif 'no store backend selected' in log:
        print('   PASS (refused by the exactly-one-backend guard)', file=out, flush=True)
        passed += 1
    else:
        print('   FAIL \u2014 build refused for another reason:', file=out, flush=True)
        lines = [line for line in log.splitlines()
                 if 'compile_error' in line or line.startswith('error')][:3]
        out.write(_indent(lines))
        out.flush()
        failed += 1
    print(f'\nsummary: {passed} passed, {failed} failed', file=out, flush=True)
    return 0 if failed == 0 else 1


def _os_release(environment):
    """Return the PRETTY_NAME /etc/os-release exports, or the inherited value."""
    value = environment.get('PRETTY_NAME', '')
    try:
        text = Path('/etc/os-release').read_text(errors='replace')
    except OSError:
        return value or 'unknown'
    for line in text.splitlines():
        line = line.strip()
        if line.startswith('PRETTY_NAME='):
            candidate = line.partition('=')[2]
            if len(candidate) >= 2 and candidate[0] == candidate[-1] and candidate[0] in '"\'':
                candidate = candidate[1:-1]
            return candidate
    return value or 'unknown'


def _probe_find_library(tools):
    """Resolve libpinyin.so the way the shell probe did: cache first, then find."""
    try:
        result = tools.run(['ldconfig', '-p'])
    except OSError:
        result = None
    if result is not None:
        for line in result.stdout.splitlines():
            match = re.search(r'=> (.*libpinyin\.so[^ ]*)', line)
            if match:
                return match.group(1)
    try:
        result = tools.run(['find', '/usr', '/lib', '/lib64', '-name', 'libpinyin.so*'])
    except OSError:
        return ''
    lines = result.stdout.splitlines()
    return lines[0] if lines else ''


def _probe_version(tools):
    """Return the first package version the distro queries report, or ?."""
    commands = (
        ['rpm', '-q', '--qf', '%{VERSION}-%{RELEASE}\n', 'libpinyin'],
        ['dpkg-query', '-W', '-f=${Version}\n', 'libpinyin[0-9]*'],
        ['pacman', '-Q', 'libpinyin'],
    )
    for command in commands:
        try:
            result = tools.run(command)
        except OSError:
            continue
        lines = [line for line in result.stdout.splitlines() if line]
        if lines:
            return lines[0]
    return '?'


def _probe_needed(tools, library):
    """Return the direct DT_NEEDED entries, or a diagnostic status.

    readelf is authoritative when present; objdump is the fallback.  A failed
    inspection is an error, never an unknown classification.
    """
    if tools.which('readelf') is not None:
        try:
            result = tools.run(['readelf', '-dW', library])
        except OSError:
            result = None
        if result is None or result.returncode:
            return None, f'backend : unknown (readelf could not read {library})'
        needed = []
        for line in result.stdout.splitlines():
            match = re.search(r'\(NEEDED\).*\[(.*)\]', line)
            if match:
                needed.append(match.group(1))
        return needed, None
    if tools.which('objdump') is not None:
        try:
            result = tools.run(['objdump', '-p', library])
        except OSError:
            result = None
        if result is None or result.returncode:
            return None, f'backend : unknown (objdump could not read {library})'
        needed = [fields[1] for fields in (line.split() for line in result.stdout.splitlines())
                  if fields[:1] == ['NEEDED'] and len(fields) > 1]
        return needed, None
    return None, 'backend : unknown (need readelf or objdump; install binutils)'


def backend_probe(*, out=sys.stdout, err=sys.stderr, root=ROOT, tools=None,
                  environment=None):
    """Identify the DBM linked directly into the system libpinyin.

    Return 0 when a known DBM was identified, 1 when libpinyin was inspected
    but no known DBM was found, 2 when it is not installed, and 3 when its
    ELF dependencies could not be read.
    """
    environment = dict(os.environ if environment is None else environment)
    tools = native.Tools(root=root, environment=environment) if tools is None else tools
    print(f'distro  : {_os_release(environment)}', file=out, flush=True)
    library = _probe_find_library(tools)
    if not library:
        print('libpinyin: not found (package not installed)', file=out, flush=True)
        return 2
    print(f'so      : {library}', file=out, flush=True)
    print(f'version : {_probe_version(tools)}', file=out, flush=True)
    needed, diagnostic = _probe_needed(tools, library)
    if diagnostic is not None:
        print(diagnostic, file=out, flush=True)
        return 3
    hit = next((entry for entry in needed
                if re.search(r'libtkrzw|kyotocabinet|libdb-|libdb5|libdb\.so', entry, re.I)), '')
    if 'tkrzw' in hit.lower():
        backend = 'Tkrzw'
    elif 'kyotocabinet' in hit.lower():
        backend = 'KyotoCabinet'
    elif 'libdb' in hit.lower():
        backend = 'BerkeleyDB'
    else:
        backend = 'unknown (no known DBM library in DT_NEEDED)'
    print(f'backend : {backend}', file=out, flush=True)
    if hit:
        print(f'needed  : {hit}', file=out, flush=True)
        return 0
    # The shell's final `[ -n "$hit" ] && echo` made an unknown DBM status 1.
    return 1
