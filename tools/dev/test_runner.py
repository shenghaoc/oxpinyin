"""Run the Rust sanitizer and ABI gates."""

from collections.abc import Mapping
from dataclasses import dataclass
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tomllib

from . import cell, native, slot

ROOT = Path(__file__).resolve().parents[2]

# The repository names its one nightly in this file. Fuzzing gets it from
# rustup. The sanitizer commands read the channel when they run, so no other
# tools.dev command needs the file.
SANITIZER_TOOLCHAIN_FILE = ROOT / 'fuzz' / 'rust-toolchain.toml'
SANITIZER_ALIASES = {'asan': 'address', 'tsan': 'thread', 'lsan': 'leak', 'msan': 'memory'}
SANITIZER_NAMES = ('address', 'thread', 'leak', 'memory')
SANITIZER_BACKENDS = cell.CELLS
SANITIZE_USAGE = ('usage: just sanitize [--check] <bdb|kc|tkrzw|all> '
                  '[address|thread|leak|memory]\n')
# Linux runs no tkrzw code (tools/dev/cell.py): `all` means the Linux cell there; the CI store
# job names its Berkeley DB gate explicitly.
TKRZW_NOT_LINUX = 'tkrzw is no Linux cell: Linux uses Kyoto Cabinet (docs/runbooks/setup.md#linux)'


@dataclass(frozen=True)
class SanitizerBackend:
    """One store backend's sanitizer data: feature, flags and display names."""

    name: str
    feature: str
    locked: bool
    target_from_nightly: bool
    flags: Mapping[str, str]
    display: Mapping[str, str]
    override_options: bool = False


_COMMON_FLAGS = {
    'thread': '-Zsanitizer=thread -Cdebuginfo=1 -Cunsafe-allow-abi-mismatch=sanitizer',
    'leak': '-Zsanitizer=leak -Cdebuginfo=1 -Cunsafe-allow-abi-mismatch=sanitizer',
}
_COMMON_DISPLAY = {'address': 'AddressSanitizer', 'thread': 'ThreadSanitizer',
                   'leak': 'LeakSanitizer'}
BACKEND_DATA = {
    'bdb': SanitizerBackend(
        'bdb', 'bdb', locked=False, target_from_nightly=False,
        flags={'address': '-Zsanitizer=address -Cdebuginfo=1', **_COMMON_FLAGS},
        display=_COMMON_DISPLAY),
    'kc': SanitizerBackend(
        'kc', 'kyotocabinet', locked=False, target_from_nightly=False,
        flags={'address': '-Zsanitizer=address',
               'thread': '-Zsanitizer=thread -Cunsafe-allow-abi-mismatch=sanitizer',
               'leak': '-Zsanitizer=leak -Cunsafe-allow-abi-mismatch=sanitizer'},
        display=_COMMON_DISPLAY),
    'tkrzw': SanitizerBackend(
        'tkrzw', 'tkrzw', locked=True, target_from_nightly=True,
        flags={'address': '-D warnings -Zsanitizer=address -Cdebuginfo=1',
               'thread': '-D warnings ' + _COMMON_FLAGS['thread'],
               'leak': '-D warnings ' + _COMMON_FLAGS['leak']},
        display=_COMMON_DISPLAY, override_options=True),
}


def sanitizer_nightly():
    """Return the nightly that SANITIZER_TOOLCHAIN_FILE names.

    Raise FileNotFoundError when the file is missing.
    """
    with SANITIZER_TOOLCHAIN_FILE.open('rb') as stream:
        return tomllib.load(stream)['toolchain']['channel']


def _toolchain_file_missing(command):
    """Return the diagnostic for a command that needs the missing toolchain file."""
    return f'{command}: {SANITIZER_TOOLCHAIN_FILE} is missing; it names the sanitizer nightly.'


def _nightly(environment):
    """Return the selected sanitizer nightly."""
    return environment.get('OXPINYIN_SANITIZER_TOOLCHAIN') or sanitizer_nightly()


def _toolchain_installed(tools, nightly, environment):
    """Return whether rustup lists the selected nightly."""
    try:
        result = tools.run(['rustup', 'toolchain', 'list'], environment=environment)
    except OSError:
        return False
    return any(line.startswith(nightly) for line in result.stdout.splitlines())


def _rustc_host(tools, environment, nightly=None):
    """Return the host triple rustc reports, with the nightly when given."""
    command = ['rustc'] + ([f'+{nightly}'] if nightly else []) + ['-vV']
    try:
        result = tools.run(command, environment=environment)
    except OSError:
        return ''
    for line in result.stdout.splitlines():
        if line.startswith('host: '):
            return line.removeprefix('host: ')
    return ''


def _toolchain_failure(backend, nightly, environment, out, err):
    """Print the missing-toolchain diagnostic on the stream the old gate used."""
    stream = out if backend.name == 'kc' else err
    print(f'{nightly} is not installed.', file=stream, flush=True)
    print(f'  install it with: rustup toolchain install {nightly} --profile minimal',
          file=stream, flush=True)
    if environment.get('OXPINYIN_SANITIZER_ALLOW_SKIP'):
        print('  SKIP: OXPINYIN_SANITIZER_ALLOW_SKIP is set; the gate did NOT run.',
              file=stream, flush=True)
        return 0
    print('  ERROR: the sanitizer gate cannot run. Install the toolchain, or set',
          file=stream, flush=True)
    print('  OXPINYIN_SANITIZER_ALLOW_SKIP=1 to allow skipping it.', file=stream, flush=True)
    return 1


def _sanitizer_options(backend, sanitizer, host_os, environment):
    """Return the child environment with the gate's sanitizer options applied."""
    child = dict(environment)
    if sanitizer == 'address':
        default = 'detect_leaks=0:abort_on_error=1'
        if backend.override_options or host_os == 'Darwin':
            child['ASAN_OPTIONS'] = default if backend.override_options else (
                child.get('ASAN_OPTIONS') or default)
    elif sanitizer == 'thread':
        if backend.override_options:
            child['TSAN_OPTIONS'] = 'abort_on_error=1'
        else:
            default = ('detect_deadlocks=0:abort_on_error=1'
                       if backend.name == 'kc' and host_os == 'Darwin' else 'abort_on_error=1')
            child['TSAN_OPTIONS'] = child.get('TSAN_OPTIONS') or default
    elif sanitizer == 'leak':
        child['LSAN_OPTIONS'] = ('abort_on_error=1' if backend.override_options else
                                 child.get('LSAN_OPTIONS') or 'abort_on_error=1')
    return child


def _launch(tools, argv, *, environment, err, **streams):
    """Run a required gate child; a launch failure has the shell's status, 127 or 126."""
    try:
        return tools.run(argv, environment=environment, **streams)
    except FileNotFoundError as error:
        print(f'{argv[0]}: command not found', file=err, flush=True)
        return subprocess.CompletedProcess([str(part) for part in argv],
                                           native.launch_status(error), '', '')
    except OSError as error:
        print(f'{argv[0]}: {error}', file=err, flush=True)
        return subprocess.CompletedProcess([str(part) for part in argv],
                                           native.launch_status(error), '', '')


def _cargo_test(backend, nightly, target, subcommand=('nextest', 'run')):
    """Return the backend's test argv, preserving the old argument order.

    Tests run under nextest: it runs each test in its own process and ends a
    hung test with the terminate-after limit of .config/nextest.toml.
    `subcommand=('test',)` gives the cargo test form, which only a
    `--no-run` build that lists the test binaries uses.
    """
    command = ['cargo', f'+{nightly}', *subcommand]
    if backend.locked:
        command += ['--locked', '--target', target, '-p', 'oxpinyin',
                    '--no-default-features', '--features', backend.feature]
    else:
        command += ['-p', 'oxpinyin', '--no-default-features',
                    '--features', backend.feature, '--target', target]
    command.append('--lib')
    if subcommand == ('nextest', 'run'):
        command += ['-E', 'test(/^store::/)']
    return command


def sanitize_abi(settings, environment):
    """Run the real-library ABI tests with the pinned Rust AddressSanitizer."""
    nightly = sanitizer_nightly()
    tools = native.Tools(environment=environment, root=settings.root)
    target = _rustc_host(tools, environment, nightly)
    if not target:
        print(f'ABI gate needs the sanitizer toolchain: rustup toolchain install {nightly}',
              file=sys.stderr)
        return 1
    child = dict(environment, RUSTFLAGS=BACKEND_DATA[settings.cell].flags['address'],
                 ASAN_OPTIONS='detect_leaks=1:abort_on_error=0', OXPINYIN_ABI_PYTHON=sys.executable)
    child.pop('CARGO_ENCODED_RUSTFLAGS', None)
    command = ['cargo', f'+{nightly}', 'nextest', 'run', '--locked', '--target', target,
               '-p', 'oxpinyin-abi-tests', *settings.feature_args(), '--features', 'abi-test',
               '--test', 'abi', '--fail-fast']
    return native.shell_status(slot.run_command(command, env=child, cwd=settings.root))


def _run_system_backend(backend, sanitizer, *, environment, out, err, tools, host_os, nightly, target):
    display = backend.display[sanitizer]
    child = _sanitizer_options(backend, sanitizer, host_os, environment)
    test_environment = dict(child, RUSTFLAGS=backend.flags[sanitizer])
    if sanitizer == 'address':
        suffix = ('(LeakSanitizer off on Darwin)' if host_os == 'Darwin'
                  else '(+ LeakSanitizer)')
        print(f'--- Rust under {display} {suffix} ---', file=out, flush=True)
    else:
        print(f'--- Rust under {display} ---', file=out, flush=True)
    result = _launch(tools, _cargo_test(backend, nightly, target),
                     environment=test_environment, err=err, stdout=None, stderr=None)
    if result.returncode:
        return native.shell_status(result.returncode)
    print('', file=out, flush=True)
    library = "libdb" if backend.name == "bdb" else "libkyotocabinet"
    print(f"sanitizers: clean ({library}'s own internals are NOT instrumented)",
          file=out, flush=True)
    return 0


def _run_tkrzw(backend, sanitizer, *, environment, out, err, tools, host_os, nightly, target):
    display = backend.display[sanitizer]
    if host_os != 'Darwin':
        # tkrzw is no Linux cell (tools/dev/cell.py): Linux runs no tkrzw code.
        print(f'ERROR: {TKRZW_NOT_LINUX}; the sanitizer gate did NOT run.', file=err, flush=True)
        return 1
    child = _sanitizer_options(backend, sanitizer, host_os, environment)
    # The just recipe's search paths serve Berkeley DB and Kyoto Cabinet. tkrzw
    # and its compression libraries are found through pkg-config, and the gate
    # certifies Homebrew's library by the loader of its utility.
    for name in ('LIBRARY_PATH', 'CPATH', 'PKG_CONFIG_PATH', 'PKG_CONFIG_LIBDIR',
                 'OXPINYIN_TKRZW_LIBRARY', 'OXPINYIN_TKRZW_PREFIX', 'OXPINYIN_TKRZW_PREFIXES'):
        child.pop(name, None)
    child['RUSTFLAGS'] = backend.flags[sanitizer]
    try:
        host_library = tools.tkrzw_host(environment=child)
    except native.NativeRefused:
        print('ERROR: no verified libtkrzw; the sanitizer gate did NOT run.', file=err, flush=True)
        print('  Install tkrzw with Homebrew.', file=err, flush=True)
        return 1
    child['OXPINYIN_TKRZW_LIBRARY'] = str(host_library)
    command = _cargo_test(backend, nightly, target)
    build = _cargo_test(backend, nightly, target, ('test',))
    print(f'--- building the tkrzw feature tests under {display} ---', file=out, flush=True)
    result = _launch(tools, [*build, '--no-run', '--message-format=json-render-diagnostics'],
                     environment=child, err=err, stdout=subprocess.PIPE, stderr=None)
    if result.returncode:
        return native.shell_status(result.returncode)
    executables = []
    for line in (result.stdout or '').splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(record, dict) and record.get('reason') == 'compiler-artifact':
            executable = record.get('executable')
            if executable is not None:
                executables.append(executable)
    checked = 0
    library = None
    for executable in executables:
        try:
            resolved = tools.tkrzw(Path(executable), runner='run-sanitizers', side='subject',
                                   cwd=ROOT, environment=child)
        except native.NativeRefused as error:
            print(f'REFUSED: {error}', file=err, flush=True)
            print('ERROR: the sanitizer gate did NOT run.', file=err, flush=True)
            return 1
        if resolved:
            library = resolved
            checked += 1
    if checked == 0:
        print('ERROR: no test binary loads libtkrzw; the sanitizer gate did NOT run.',
              file=err, flush=True)
        return 1
    print(f'libtkrzw: {checked} test binaries resolve it to {library}', file=out, flush=True)
    if sanitizer == 'address':
        print('', file=out, flush=True)
        print(f'--- Rust under {display} (LeakSanitizer off) ---', file=out, flush=True)
    else:
        print('', file=out, flush=True)
        print(f'--- Rust under {display} ---', file=out, flush=True)
    result = _launch(tools, command, environment=child, err=err,
                     stdout=None, stderr=None)
    if result.returncode:
        return native.shell_status(result.returncode)
    print('', file=out, flush=True)
    if sanitizer == 'address':
        print('--- C facade handles that outlive their context under AddressSanitizer ---',
              file=out, flush=True)
        lifetime = ['cargo', f'+{nightly}', 'nextest', 'run', '--locked', '--target', target,
                    '-p', 'oxpinyin-capi', '-p', 'oxzhuyin-capi',
                    '--no-default-features', '--features', backend.feature,
                    '--lib', '-E', 'test(outlives_context)']
        result = _launch(tools, lifetime, environment=child, err=err,
                         stdout=None, stderr=None)
        if result.returncode:
            return native.shell_status(result.returncode)
    print("sanitizers: clean (libtkrzw's own internals are NOT instrumented)", file=out, flush=True)
    return 0


def _run_sanitizer_backend(backend, sanitizer, *, environment, out, err, tools, host_os):
    """Run one backend's sanitizer gate and return its shell-style status."""
    if sanitizer not in backend.flags:
        print(f'unknown sanitizer: {sanitizer}', file=err, flush=True)
        return 2
    nightly = _nightly(environment)
    if not _toolchain_installed(tools, nightly, environment):
        return _toolchain_failure(backend, nightly, environment, out, err)
    if backend.target_from_nightly:
        target = environment.get('OXPINYIN_SANITIZER_TARGET') or _rustc_host(
            tools, environment, nightly)
    else:
        target = environment.get('OXPINYIN_SANITIZER_TARGET') or _rustc_host(tools, environment)
    if backend.name in ('bdb', 'kc'):
        return _run_system_backend(backend, sanitizer, environment=environment, out=out, err=err,
                                   tools=tools, host_os=host_os, nightly=nightly, target=target)
    return _run_tkrzw(backend, sanitizer, environment=environment, out=out, err=err,
                      tools=tools, host_os=host_os, nightly=nightly, target=target)


def sanitize(argv):
    """Select and run one sanitizer gate, preserving the old entry grammar.

    Return the backend's shell-style status; for `all`, 0 when every gate
    succeeded and 1 otherwise; 2 on a usage error. `--check` validates and
    prints the selection without running a gate.
    """
    arguments = list(argv)
    out, err = sys.stdout, sys.stderr
    environment = dict(os.environ)
    check = False
    if arguments[:1] == ['--check']:
        check = True
        arguments = arguments[1:]
    if not 1 <= len(arguments) <= 2:
        err.write(SANITIZE_USAGE)
        return 2
    # No backend named: the host's cell (tools/dev/cell.py). A named backend stays as named.
    backend_name = arguments[0] or cell.host_cell()
    sanitizer = arguments[1] if len(arguments) > 1 else 'address'
    sanitizer = SANITIZER_ALIASES.get(sanitizer, sanitizer)
    if backend_name not in (*SANITIZER_BACKENDS, 'all'):
        err.write(SANITIZE_USAGE)
        return 2
    if sanitizer not in SANITIZER_NAMES:
        err.write(SANITIZE_USAGE)
        return 2
    try:
        environment['OXPINYIN_SANITIZER_TOOLCHAIN'] = _nightly(environment)
    except FileNotFoundError:
        print(_toolchain_file_missing('sanitize'), file=err, flush=True)
        return 1
    host_os = platform.system()
    if sanitizer == 'memory':
        if host_os == 'Darwin':
            print('sanitize: MemorySanitizer is unsupported on macOS: rustc has no Darwin '
                  'MSan target.', file=err, flush=True)
        else:
            print('sanitize: MemorySanitizer has no in-tree wrapper; use address, thread '
                  'or leak.', file=err, flush=True)
        return 1
    gates = SANITIZER_BACKENDS if host_os == 'Darwin' else (cell.LOCAL_CELL,)
    if backend_name == 'tkrzw' and host_os != 'Darwin':
        print(f'sanitize: {TKRZW_NOT_LINUX}', file=err, flush=True)
        return 1
    if sanitizer == 'leak':
        if backend_name == 'tkrzw' or (backend_name == 'all' and 'tkrzw' in gates):
            print('sanitize: tkrzw LeakSanitizer is unsupported: the Rust harness keeps '
                  'process-lifetime allocations that LSan reports as leaks; use address '
                  'or thread.', file=err, flush=True)
            return 1
        if host_os == 'Darwin' and platform.machine() == 'arm64':
            print('sanitize: standalone LeakSanitizer is unsupported on macOS arm64; '
                  'use address (leak detection off on Darwin) or an x86_64 host.',
                  file=err, flush=True)
            return 1
    environment['OXPINYIN_SANITIZER'] = sanitizer
    if check:
        print(f'sanitize: check backend={backend_name} sanitizer={sanitizer} '
              f'host={host_os}/{platform.machine()}', file=out, flush=True)
        return 0
    tools = native.Tools(environment=environment)
    if backend_name != 'all':
        return _run_sanitizer_backend(
            BACKEND_DATA[backend_name], sanitizer, environment=environment,
            out=out, err=err, tools=tools, host_os=host_os)
    results = []
    failed = 0
    for name in gates:
        print(f'=== sanitize {name} ===', file=out, flush=True)
        code = native.shell_status(_run_sanitizer_backend(
            BACKEND_DATA[name], sanitizer, environment=environment,
            out=out, err=err, tools=tools, host_os=host_os))
        results.append(f'{name}: exit {code}')
        failed = failed or code != 0
        print('', file=out, flush=True)
    for result in results:
        print(f'sanitize all: {result}', file=out, flush=True)
    return 0 if not failed else 1
