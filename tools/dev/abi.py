"""Shared C ABI symbol and allocator gates; standard library only."""
from collections.abc import Sequence
from dataclasses import dataclass, replace
from pathlib import Path
import os
import platform
import re
import shlex
import subprocess
import sys
import tempfile

from .cell import CellError, Settings
from .drivers import DriverServices
from . import pool, slot


def driver_services(settings: Settings) -> DriverServices:
    context = pool.UnitContext('ABI gate', slot.CANCELLED, environment=dict(settings.environment),
                               fds=slot.inherited_fds('ABI gate'))
    return DriverServices(settings, context)


@dataclass(frozen=True)
class HostFacts:
    system: str

    @classmethod
    def current(cls) -> 'HostFacts':
        return cls(platform.system())


def version_globals(text: str) -> set[str]:
    active = False
    result = set()
    for line in text.splitlines():
        if 'global:' in line:
            active = True
            continue
        if 'local:' in line:
            active = False
        if active and ';' in line:
            result.add(re.sub(r'[\s;]', '', line))
    return result


def exported_symbols(text: str, facts: HostFacts) -> set[str]:
    """Parse supplied nm output; the parser runs on every native platform."""
    result = set()
    for line in text.splitlines():
        fields = line.split()
        if len(fields) < 3:
            continue
        if facts.system == 'Linux' and fields[1] in 'TtWw':
            result.add(fields[2])
        elif facts.system == 'Darwin' and fields[1] == 'T':
            result.add(fields[2].removeprefix('_'))
    return result


def check_exports(settings: Settings, argv: Sequence[str], facts: HostFacts) -> int:
    if facts.system not in ('Linux', 'Darwin'):
        print(f'fatal: unsupported host {facts.system}', file=sys.stderr)
        return 2
    shipped = bool(argv) and argv[0] == '--shipped'
    extension = 'so' if facts.system == 'Linux' else 'dylib'
    pinyin = replace(settings, feature=settings.feature + ',shipped') if shipped else settings
    libraries = [('libpinyin', pinyin.artifact('OXPINYIN_CAPI_SO', f'libpinyin_capi.{extension}', 'oxpinyin-capi'),
                  settings.root / 'crates/oxpinyin-capi/libpinyin.ver', 'pinyin',
                  set() if shipped else {'oxpinyin_init_for_fixtures', 'oxpinyin_test_set_user_bigram'}),
                 ('libzhuyin', settings.artifact('OXPINYIN_ZHUYIN_SO', f'libzhuyin_capi.{extension}', 'oxpinyin-zhuyin-capi'),
                  settings.root / 'crates/oxpinyin-zhuyin-capi/libzhuyin.ver', 'zhuyin', set())]
    status = 0
    for name, library, version, prefix, hooks in libraries:
        print(f'--- {name}: {library} against {version.relative_to(settings.root)} ---')
        flags = ['-D', '--defined-only'] if facts.system == 'Linux' else ['-gU']
        result = subprocess.run(['nm', *flags, str(library)], env=settings.environment,
                                capture_output=True, text=True)
        if result.returncode:
            sys.stderr.write(result.stderr)
            return result.returncode
        symbols = exported_symbols(result.stdout, facts)
        expected = version_globals(version.read_text())
        actual = {symbol for symbol in symbols if symbol.startswith(prefix + '_')}
        missing, extra = expected - actual, actual - expected
        print(f'{prefix}_* exported: {len(actual)}, in .ver: {len(expected)}')
        for label, names in [('MISSING (in .ver, not exported):', missing),
                             ('EXTRA (exported, not in .ver):', extra)]:
            if names:
                print(label)
                for symbol in sorted(names):
                    print('  ' + symbol)
                status = 1
        leaked = False
        for symbol in sorted(symbols - actual):
            if symbol in hooks:
                print('allowed non-ABI export: ' + symbol)
            else:
                print('LEAKED non-ABI export: ' + symbol)
                leaked = True
                status = 1
        if not missing and not extra and not leaked:
            print('OK: exactly the .ver set')
    print("FAIL: the exported symbol set is not the pin's version script" if status else
          "PASS: both libraries export exactly their version script's set")
    return status


def allocation_ranges(text: str) -> tuple[str, str, str, str, str]:
    """Recover exactly the adapter ranges the injected allocation driver needs."""
    strings, lists, buffers = [], [], []
    for line in text.splitlines():
        fields = line.split()
        if len(fields) < 4 or fields[2] != 't':
            continue
        address, size, _, name = fields[:4]
        if re.search(r'^_RNv.*3ffi10owned_cstr$', name):
            strings.append((address, size))
        if re.search(r'^_RINvNt.*3ffi15owned_cstr_list', name):
            lists.append((address, size))
        if re.search(r'^_RINvNt.*3ffi(16malloc_cstr_with|23malloc_cstr_with_policy|24malloc_cstr_with_warning)', name):
            buffers.append(address + ':' + size)
    if len(strings) != 1 or len(lists) != 1:
        raise CellError('missing unique adapter symbols', 2)
    if not buffers:
        raise CellError('missing byte-buffer adapter symbols', 2)
    return (*strings[0], *lists[0], ','.join(buffers))


def check_alloc_failure(settings: Settings, argv: Sequence[str]) -> int:
    if len(argv) != 3:
        print('expected pin prefix and two subject shared libraries', file=sys.stderr)
        return 2
    prefix, *subjects = argv
    services = driver_services(settings)
    with tempfile.TemporaryDirectory() as directory:
        work = Path(directory)
        flags = subprocess.run(['pkg-config', '--cflags', '--libs', 'glib-2.0'],
                               capture_output=True, text=True, check=True, env=settings.environment)
        common = ['g++', '-std=c++17', '-O0', '-g', '-Wall', '-Wextra', '-Werror', '-fno-builtin', '-rdynamic']
        for name, defines in [('probe', []), ('control', ['-DOXPINYIN_ALLOC_FAILURE_DISABLED'])]:
            result = services.command([*common, *defines, 'tools/abi/alloc-failure.cc',
                                       *shlex.split(flags.stdout), '-ldl', '-o', str(work / name)])
            if result.returncode:
                return result.returncode
        status = 0
        for mode, subject in zip(('pinyin', 'zhuyin'), subjects):
            result = subprocess.run(['nm', '-S', '-a', subject], capture_output=True, text=True,
                                    env=settings.environment)
            if result.returncode:
                sys.stderr.write(result.stderr)
                return result.returncode
            try:
                ranges = allocation_ranges(result.stdout)
            except CellError as error:
                print(f'{error}: {subject}', file=sys.stderr)
                return error.status
            user = work / ('user-' + mode)
            user.mkdir()
            data = str(Path(prefix) / 'lib/libpinyin/data')
            subject_args = [subject, data, mode, 'subject', *ranges, str(user)]
            for args in (subject_args, [str(Path(prefix) / f'lib/lib{mode}.so'), data, mode, 'pin',
                                        '0', '0', '0', '0', '', '']):
                if services.command([str(work / 'probe'), *args]).returncode:
                    status = 1
            control = services.command([str(work / 'control'), *subject_args], capture=True, merged=True)
            if control.returncode == 0:
                print(f'failure-disabled control unexpectedly passed: {mode}', file=sys.stderr)
                return 1
            print(f'{mode} injection-disabled negative control: rejected')
        return status


def dispatch(argv: Sequence[str]) -> int:
    try:
        settings = Settings.read(os.environ)
        if argv and argv[0] == 'exports':
            status = check_exports(settings, argv[1:], HostFacts.current())
        elif argv and argv[0] == 'alloc-failure':
            status = check_alloc_failure(settings, argv[1:])
        elif argv and argv[0] == 'alloc-pairing':
            status = check_alloc_pairing(settings, argv[1:], HostFacts.current())
        else:
            print('usage: abi exports|alloc-failure|alloc-pairing [ARG ...]', file=sys.stderr)
            return 2
        return 128 - status if status < 0 else status
    except CellError as error:
        print(error, file=sys.stderr)
        return error.status
    except subprocess.CalledProcessError as error:
        return error.returncode


VERIFIED = re.compile(r'(pin-probe:[A-Za-z0-9_.-]+|pin-aborts:row[0-9]+[a-d]?|source:[A-Za-z0-9_./-]+:[0-9]+(-[0-9]+)?|unverified|unexecuted|n/a)')


def header_slots(text: str) -> list[tuple[str, str, str]]:
    """Flatten the frozen C header and recover pointer returns/out parameters."""
    text = re.sub(r'//[^\n]*', '', text)
    text = re.sub(r'/\*.*?\*/', '', text, flags=re.S)
    lines, disabled, nested = [], False, 0
    for line in text.splitlines():
        if re.fullmatch(r'\s*#\s*if\s+0\s*', line):
            disabled = True
            continue
        if disabled:
            if re.match(r'\s*#\s*(if|ifdef|ifndef)', line):
                nested += 1
            elif re.match(r'\s*#\s*endif', line):
                if nested:
                    nested -= 1
                else:
                    disabled = False
            continue
        if not re.match(r'\s*#', line):
            lines.append(line)
    slots = []
    for statement in ' '.join(lines).split(';'):
        declaration = re.sub(r'[ \t]+', ' ', statement).strip()
        if not re.match(r'^[A-Za-z_][A-Za-z0-9_ ]*\*? *[a-z_]+ *\(', declaration):
            continue
        signature, parameters = declaration.split('(', 1)
        symbol = re.search(r'([A-Za-z_][A-Za-z0-9_]*) *$', signature).group(1)
        if '*' in signature:
            slots.append((symbol, 'return', 'ptr-return'))
        for parameter in parameters.removesuffix(')').split(','):
            parameter = parameter.strip()
            match = re.search(r'([A-Za-z_][A-Za-z0-9_]*) *$', parameter)
            if not match:
                continue
            shape = ('strv' if '***' in parameter else
                     'const-pp' if parameter.startswith('const ') and '**' in parameter else
                     'char-pp' if 'char **' in parameter else
                     'opaque-pp' if '**' in parameter else None)
            if shape:
                slots.append((symbol, match.group(1), shape))
    return sorted(slots)


def register_rows(text: str) -> list[tuple[int, list[str]]]:
    return [(index, line.split()) for index, line in enumerate(text.splitlines(), 1)
            if line.strip() and not line.lstrip().startswith('#')]


def register_shape(text: str) -> list[str]:
    errors = []
    for index, fields in register_rows(text):
        if len(fields) != 5:
            errors.append(f'    line {index}: {len(fields)} fields, not 5')
        elif not VERIFIED.fullmatch(fields[4]):
            errors.append(f'    line {index}: <verified> {fields[4]} is not a documented kind')
    return errors


def check_static(settings: Settings, abi: str, crate: str, header: str) -> int:
    from collections import Counter
    root = settings.root / 'crates' / crate
    header_file = root / header
    text = (root / (abi + '.alloc')).read_text()
    slots = header_slots(header_file.read_text())
    rows = sorted(tuple(fields[:4]) for _, fields in register_rows(text) if len(fields) >= 4)
    exports = version_globals((root / (abi + '.ver')).read_text())
    print(f'--- {abi}: register against {header} ---')
    print(f'  header slots: {len(slots)}, register entries: {len(rows)}')
    status = 0

    def fail(message: str) -> None:
        nonlocal status
        print('  FAIL: ' + message)
        status = 1

    declared = Counter(' '.join(row[:2]) for row in slots)
    registered = Counter(' '.join(row[:2]) for row in rows)
    for heading, names, reason in (
            ('  slots the header declares but the register does not classify:',
             sorted((declared - registered).elements()), 'unclassified ABI slot'),
            ('  register entries with no matching slot in the header:',
             sorted((registered - declared).elements()), 'stale register entry'),
            ('  duplicate register entries:', sorted(key for key, count in registered.items() if count > 1),
             'duplicate register entry')):
        if names:
            print(heading)
            for name in names:
                print('    ' + name)
            fail(reason)
    allowed = {'strv': 'g_strfreev', 'const-pp': 'borrowed', 'char-pp': 'g_free', 'opaque-pp': 'borrowed'}
    for symbol, slot, shape in slots:
        classes = '\n'.join(fields[2] for _, fields in register_rows(text)
                            if len(fields) > 2 and fields[:2] == [symbol, slot])
        if not classes:
            continue
        valid = (classes == 'borrowed' or classes.startswith('handle:')) if shape == 'ptr-return' else classes == allowed[shape]
        if not valid:
            fail(f"{symbol}/{slot} is '{shape}' in the header but '{classes}' in the register")
    for symbol, _, ownership, _ in rows:
        if symbol not in exports:
            fail(f'{symbol} is in the register but not in {abi}.ver')
        if ownership.startswith('handle:'):
            destructor = ownership.removeprefix('handle:')
            if destructor not in exports:
                fail(f"{symbol} declares destructor '{destructor}', which {abi}.ver does not export")
    if not status:
        print('  OK: every header slot classified, every class and destructor sound')
    errors = register_shape(text)
    if errors:
        print('  register lines with the wrong shape:')
        print('\n'.join(errors))
        fail('register line is not <symbol> <slot> <class> <notes> <verified>')
    shape_ok = not errors
    lines = text.splitlines()
    for label, fields in [('four-fields', 4), ('misspelled-kind', 5)]:
        broken = lines.copy()
        entries = register_rows(text)
        if entries:
            index = entries[0][0] - 1
            row = broken[index].split()[:fields]
            if fields == 5:
                # awk assigns a missing fifth field too, so malformed inputs
                # remain a shape failure rather than raising IndexError.
                row += [''] * (5 - len(row))
                row[4] = 'unverifed'
            broken[index] = ' '.join(row)
        if not register_shape('\n'.join(broken)):
            fail(f'shape control did not fire: a register with a {label} line passes')
            shape_ok = False
    if shape_ok:
        print('  OK: five fields and a documented <verified> kind on every line; shape control fired')
    return status


def check_alloc_pairing(settings: Settings, argv: Sequence[str], facts: HostFacts) -> int:
    services = driver_services(settings)
    static_only = bool(argv) and argv[0] == '--static-only'
    if facts.system != 'Linux' and not static_only:
        print('fatal: the dynamic half needs Linux (LeakSanitizer); re-run with --static-only', file=sys.stderr)
        return 2
    gates = [('libpinyin', 'oxpinyin-capi', 'pinyin.h', 'pinyin_capi'),
             ('libzhuyin', 'oxpinyin-zhuyin-capi', 'zhuyin.h', 'zhuyin_capi')]
    status = 0
    for abi, crate, header, _ in gates:
        status |= check_static(settings, abi, crate, header)
    if static_only:
        print('FAIL: the allocator-pairing register does not match the frozen ABI' if status else
              'PASS: static half only (--static-only); the dynamic half was not run')
        return status
    with tempfile.TemporaryDirectory() as directory:
        work = Path(directory)
        print('--- building oxpinyin-capi and oxpinyin-zhuyin-capi ---')
        library_directory = work / 'lib'
        library_directory.mkdir()
        for abi, crate, _, library in gates:
            variable = 'OXPINYIN_CAPI_SO' if abi == 'libpinyin' else 'OXPINYIN_ZHUYIN_SO'
            artifact = settings.artifact(variable, f'lib{library}.so', crate)
            (library_directory / f'lib{library}.so').symlink_to(artifact)
            (library_directory / f'{abi}.so.15').symlink_to(f'lib{library}.so')
        services.mini_fixture(work / 'sys')
        flags = settings.environment.get('GLIB_CFLAGS') or subprocess.run(
            ['pkg-config', '--cflags', 'glib-2.0'], env=settings.environment,
            capture_output=True, text=True, check=True).stdout
        libraries = subprocess.run(['pkg-config', '--libs', 'glib-2.0'], env=settings.environment,
                                   capture_output=True, text=True, check=True).stdout
        # The normal run, coverage and deliberately leaking control share a
        # fixture and library identity. These steps must remain ordered.
        for abi, crate, _, library in gates:
            before = status
            source = settings.root / 'tools/abi' / ('alloc-pairing-' + abi.removeprefix('lib') + '.cc')
            register = settings.root / 'crates' / crate / (abi + '.alloc')
            rows = sorted(' '.join(fields[:4]) for _, fields in register_rows(register.read_text()) if len(fields) >= 4)
            print(f'--- {abi}: {source.name} under AddressSanitizer/LeakSanitizer ---')
            user = work / (abi + '-user')
            user.mkdir()

            def compile_driver(output: Path, leak: bool) -> int:
                return services.command(['g++', '-std=c++17', '-Wall', '-Wextra', '-Werror', '-O1', '-g',
                                         '-fno-omit-frame-pointer', '-fsanitize=address', *shlex.split(flags),
                                         '-I' + str(settings.root / 'crates' / crate), str(source),
                                         '-L' + str(library_directory), '-Wl,-rpath,' + str(library_directory),
                                         '-l' + library, *shlex.split(libraries),
                                         *(['-DOXPINYIN_ALLOC_PAIRING_LEAK'] if leak else []),
                                         '-o', str(output)]).returncode

            driver = work / (abi + '-driver')
            result = compile_driver(driver, False)
            if result:
                return result
            environment = dict(settings.environment, ASAN_OPTIONS='detect_leaks=1:abort_on_error=0')
            result = services.run([str(driver), str(work / 'sys'), str(user), '--coverage'], driver,
                                  runner='alloc-pairing', side='subject', cwd=settings.root,
                                  environment=environment, merged=True)
            output = result.stdout.decode(errors='replace').rstrip('\n')
            if result.returncode:
                print('\n'.join('    ' + line for line in output.splitlines()))
                print(f'  FAIL: the sanitized run exited {result.returncode} (0 expected)')
                status = 1
                continue
            coverage = sorted(line.split()[1:7] for line in output.splitlines() if line.startswith('SLOT '))
            for heading, items, reason in (
                    ('  slots the driver never reached with a live pointer:',
                     [' '.join(row[:2]) for row in coverage if row[4] != 'hit'],
                     'unexercised slot — the pairing was not actually tested'),
                    ('  false-return contracts the driver never probed:',
                     [f'{row[0]} {row[1]} ({row[3]})' for row in coverage if row[5] == 'unchecked'],
                     'unprobed false-return contract')):
                if items:
                    print(heading)
                    for item in items:
                        print('    ' + item)
                    print('  FAIL: ' + reason)
                    status = 1
            actual = sorted(' '.join(row[:4]) for row in coverage)
            if actual != rows:
                print('  driver slot table and register disagree (<driver only / register only>):')
                from collections import Counter
                left, right = Counter(actual), Counter(rows)
                for item in sorted((left - right).elements()):
                    print('    ' + item)
                for item in sorted((right - left).elements()):
                    print('    \t' + item)
                print('  FAIL: driver/register drift')
                status = 1
            if status == before:
                print(f'  OK: {len(coverage)} slots exercised and released, no leak')
            import shutil
            shutil.rmtree(user)
            user.mkdir()
            leak = work / (abi + '-driver-leak')
            result = compile_driver(leak, True)
            if result:
                return result
            result = services.command([str(leak), str(work / 'sys'), str(user)],
                                      environment=environment, capture=True, merged=True)
            if result.returncode == 0 or b'LeakSanitizer' not in result.stdout:
                print(f'  the deliberately-leaking build exited {result.returncode} without a LeakSanitizer report')
                print('  FAIL: negative control did not fire — the leak instrument is not working')
                status = 1
            else:
                print(f'  OK: negative control fired (exit {result.returncode}, LeakSanitizer reported)')
    print('FAIL: the allocator-pairing contract is not held' if status else
          'PASS: every ABI allocation is classified, exercised and released by its declared deallocator')
    return status


def main(argv: Sequence[str]) -> int:
    with slot.cancellation_handlers():
        try:
            return dispatch(argv)
        except (pool.UnitStopped, slot.Cancelled) as error:
            print(error, file=sys.stderr)
            return slot.cancellation_status()
        except FileNotFoundError as error:
            print(error, file=sys.stderr)
            return 127
        except (OSError, ValueError) as error:
            print(error, file=sys.stderr)
            return 1
