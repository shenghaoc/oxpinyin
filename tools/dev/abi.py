"""Verify both public C ABIs and run the pin-backed allocation failure audit."""

if __name__ == '__main__':
    from .entry import require_utf8_mode

    require_utf8_mode()

from collections.abc import Sequence
from dataclasses import replace
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
from . import native, pool, slot
from .packaging import Finished, Tools, crate_dir, install, linux_exports, version_exports


def driver_services(settings: Settings) -> DriverServices:
    context = pool.UnitContext('ABI gate', slot.CANCELLED, environment=dict(settings.environment))
    return DriverServices(settings, context)


def darwin_exported_symbols(text: str) -> set[str]:
    """Darwin nm -gU already selects every external defined symbol."""
    result = set()
    for line in text.splitlines():
        fields = line.split()
        if len(fields) >= 3:
            result.add(fields[2].removeprefix('_'))
    return result


def darwin_export_trie(text: str) -> set[str]:
    """Read dyld_info -exports, failing closed on diagnostics or unknown rows."""
    result = set()
    for line in text.splitlines():
        row = line.strip()
        if not row or row.endswith(':') or row.split() == ['offset', 'symbol']:
            continue
        match = re.fullmatch(r'(?:0x[0-9a-fA-F]+|\[re-export\])\s+(\S+)(?:\s+.*)?', row)
        if not match:
            raise ValueError(f'unrecognized dyld_info export row: {row}')
        result.add(match[1].removeprefix('_'))
    return result


def shipped_library(settings: Settings, name: str, stage: Path) -> Path:
    """Stage NAME's release library with `just packaging install` and return the relinked dylib.

    On macOS the gate checks what ships. The cargo-built cdylib exports every `#[export_name]`
    symbol of the dependency graph, and the packaging relink is the one link that excludes them
    (docs/packaging.md#export-gate), so the gate stages the library as the nightly job does.
    """
    arguments = [name, '--prefix=/usr/local', f'--destdir={stage}', '--', '--release', '--locked',
                 *settings.feature_args()]
    try:
        install(arguments, Tools(env=settings.environment, root=settings.root))
    except Finished as error:
        raise CellError(f'FAIL: staging {name}', error.status) from None
    return stage / 'usr/local/lib' / f'{name}.15.0.0.dylib'


def check_exports(settings: Settings, system: str) -> int:
    if system not in ('Linux', 'Darwin'):
        print(f'fatal: unsupported host {system}', file=sys.stderr)
        return 2
    with tempfile.TemporaryDirectory(prefix='abi-exports.') as scratch:
        return check_export_sets(settings, system, Path(scratch))


def check_export_sets(settings: Settings, system: str, scratch: Path) -> int:
    extension = 'so' if system == 'Linux' else 'dylib'
    libraries = [('libpinyin', 'OXPINYIN_CAPI_SO', f'liboxpinyin_capi.{extension}', 'oxpinyin-capi',
                  settings.root / 'capi/libpinyin.ver'),
                 ('libzhuyin', 'OXPINYIN_ZHUYIN_SO', f'liboxzhuyin_capi.{extension}', 'oxzhuyin-capi',
                  settings.root / 'zhuyin-capi/libzhuyin.ver')]
    status = 0
    for name, variable, filename, package, version in libraries:
        if system == 'Darwin' and not settings.environment.get(variable):
            library = shipped_library(settings, name, scratch / 'stage')
        else:
            library = settings.artifact(variable, filename, package)
        print(f'--- {name}: {library} against {version.relative_to(settings.root)} ---')
        flags = ['-D', '--defined-only'] if system == 'Linux' else ['-gU']
        result = subprocess.run(['nm', *flags, str(library)], env=settings.environment,
                                capture_output=True, text=True)
        if result.returncode:
            sys.stderr.write(result.stderr)
            return result.returncode
        node, names = version_exports(version)
        expected = set(names)
        if system == 'Linux':
            symbols = set(linux_exports(result.stdout, node))
        else:
            symbols = darwin_exported_symbols(result.stdout)
            result = subprocess.run(['dyld_info', '-exports', str(library)], env=settings.environment,
                                    capture_output=True, text=True)
            if result.returncode:
                sys.stderr.write(result.stderr)
                return result.returncode
            try:
                trie = darwin_export_trie(result.stdout)
            except ValueError as error:
                print(str(error), file=sys.stderr)
                return 1
            if trie != symbols:
                print(f'FAIL: nm and export trie differ: {sorted(trie ^ symbols)}', file=sys.stderr)
                status = 1
            symbols |= trie
        missing, extra = expected - symbols, symbols - expected
        print(f'exported: {len(symbols)}, in .ver: {len(expected)}')
        if missing:
            print('MISSING (in .ver, not exported):')
            for symbol in sorted(missing):
                print('  ' + symbol)
        for symbol in sorted(extra):
            print('LEAKED non-ABI export: ' + symbol)
        if missing or extra:
            status = 1
        else:
            print('OK: exactly the .ver set')
    print("FAIL: the exported symbol set is not the pin's version script" if status else
          "PASS: both libraries export exactly their version script's set")
    return status


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
            user = work / ('user-' + mode)
            user.mkdir()
            data = str(Path(prefix) / 'lib/libpinyin/data')
            # Both sides allocate returned strings through GLib, so the probe fails
            # them by their GLib or libc caller; no symbol or build detail is read here.
            subject_args = [subject, data, mode, 'subject', str(user)]
            for args in (subject_args, [str(Path(prefix) / f'lib/lib{mode}.so'), data, mode, 'pin', '']):
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
        if not argv:
            status = check_abi(settings, platform.system())
        elif argv[0] == 'alloc-failure':
            status = check_alloc_failure(settings, argv[1:])
        else:
            print('usage: just abi [alloc-failure PIN_PREFIX PINYIN_SO ZHUYIN_SO]', file=sys.stderr)
            return 2
    except CellError as error:
        print(error, file=sys.stderr)
        return error.status
    except subprocess.CalledProcessError as error:
        status = error.returncode
    return native.shell_status(status)


VERIFIED = re.compile(r'(pin-probe:[A-Za-z0-9_.-]+|source:[A-Za-z0-9_./-]+:[0-9]+(-[0-9]+)?|unverified|unexecuted|n/a)')


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


def check_static(settings: Settings, abi: str, header: str) -> int:
    from collections import Counter
    root = crate_dir(settings.root, abi.removeprefix('lib'))
    header_file = root / header
    text = (root / (abi + '.alloc')).read_text()
    slots = header_slots(header_file.read_text())
    rows = sorted(tuple(fields[:4]) for _, fields in register_rows(text) if len(fields) >= 4)
    _, exports = version_exports(root / (abi + '.ver'))
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
    for symbol, slot_name, shape in slots:
        classes = '\n'.join(fields[2] for _, fields in register_rows(text)
                            if len(fields) > 2 and fields[:2] == [symbol, slot_name])
        if not classes:
            continue
        valid = (classes == 'borrowed' or classes.startswith('handle:')) if shape == 'ptr-return' else classes == allowed[shape]
        if not valid:
            fail(f"{symbol}/{slot_name} is '{shape}' in the header but '{classes}' in the register")
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


def check_abi(settings: Settings, system: str) -> int:
    for name in ('pinyin', 'zhuyin'):
        status = check_static(settings, 'lib' + name, name + '.h')
        if status:
            return status
    if system == 'Windows':
        print('PASS: allocation registers match the headers and version scripts (Windows).')
        return 0
    if system != 'Linux':
        return check_exports(settings, system)

    from .abi_headers import check_headers
    from .test_runner import sanitize_abi

    # Resolve each development artifact once, so the export, link and ownership
    # checks all observe the same library, including caller-supplied artifacts.
    pinyin = settings.artifact('OXPINYIN_CAPI_SO', 'liboxpinyin_capi.so', 'oxpinyin-capi')
    zhuyin = settings.artifact('OXPINYIN_ZHUYIN_SO', 'liboxzhuyin_capi.so', 'oxzhuyin-capi')
    env = dict(settings.environment, OXPINYIN_CAPI_SO=str(pinyin), OXPINYIN_ZHUYIN_SO=str(zhuyin))
    status = check_exports(replace(settings, environment=env), system)
    if status:
        return status
    with tempfile.TemporaryDirectory(prefix='abi-') as directory:
        work = Path(directory)
        check_headers(settings, work, {'pinyin': pinyin, 'zhuyin': zhuyin})
        fixture = work / 'sys'
        driver_services(settings).mini_fixture(fixture)
        return sanitize_abi(settings, dict(env, OXPINYIN_ABI_FIXTURE=str(fixture)))


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


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
