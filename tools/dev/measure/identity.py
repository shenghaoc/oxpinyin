"""Host and artifact identity the measurement runners record beside their captures (#1290).

A record whose subject cannot be named is not a record, so the runners write
what the numbers depend on before taking any: the kernel, the CPU, the tool
versions and a digest of each artifact. These are the readers behind those
tables. Standard library only.
"""
from pathlib import Path
import platform
import re
from collections.abc import Sequence

from . import bench
from tools.oracle.build import file_sha256 as sha256

PROC = Path('/proc')


def library_backend(library):
    """Identify the linked object's backend, not the caller's environment."""
    result = bench.capture(['readelf', '-dW', library])
    if result.status:
        raise ValueError('cannot read library backend: ' + result.stderr.decode(errors='replace'))
    needed = re.findall(r'\(NEEDED\).*\[([^]]+)\]', result.stdout.decode())
    patterns = {'bdb': r'libdb(?:-[0-9.]+)?\.so(?:\.[0-9.]+)?',
                'kc': r'libkyotocabinet\.so(?:\.[0-9.]+)?',
                'tkrzw': r'libtkrzw\.so(?:\.[0-9.]+)?'}
    backends = [backend for name in needed for backend, pattern in patterns.items()
                if re.fullmatch(pattern, name)]
    if len(backends) != 1:
        raise ValueError('expected exactly one backend in DT_NEEDED: ' + ', '.join(needed))
    return backends[0]


def kernel() -> str:
    """System, release, version and machine in the recorded kernel field."""
    names = platform.uname()
    return f'{names.system} {names.release} {names.version} {names.machine}'


def _first_line(path: Path, needle: str) -> str | None:
    """The first line of PATH that contains NEEDLE (`grep -m1`), or None when there is none or no file."""
    try:
        for line in path.read_text(errors='replace').split('\n'):
            if needle in line:
                return line
    except OSError:
        pass
    return None


def cpu_model() -> str:
    """The first `model name` of /proc/cpuinfo without its label; empty where it has none (arm64)."""
    line = _first_line(PROC / 'cpuinfo', 'model name')
    return '' if line is None else line.partition(':')[2].lstrip(' ')


def memory_total() -> str:
    """The MemTotal line of /proc/meminfo as it reads; empty where there is none."""
    return _first_line(PROC / 'meminfo', 'MemTotal') or ''


def perf_event_paranoid() -> str:
    """The kernel's perf_event_paranoid setting, or `unknown` where it cannot be read."""
    try:
        return (PROC / 'sys/kernel/perf_event_paranoid').read_text().rstrip('\n')
    except OSError:
        return 'unknown'


def first_output_line(command: Sequence, *, env=None) -> str:
    """The first line COMMAND prints (`$(COMMAND | head -1)`); empty when it cannot run."""
    try:
        done = bench.capture(command, env=env)
    except OSError:
        return ''
    return done.stdout.decode(errors='replace').split('\n')[0]


def package_version(package: str, *, env) -> str:
    """`pkg-config --modversion PACKAGE`, or `unknown` when pkg-config fails or is absent."""
    try:
        done = bench.capture(['pkg-config', '--modversion', package], env=env)
    except OSError:
        return 'unknown'
    return done.stdout.decode(errors='replace').rstrip('\n') if done.status == 0 else 'unknown'


def pc_version(path) -> str:
    """The `Version:` lines of a .pc file as `grep -h` prints them, or `unknown` when there are none."""
    try:
        lines = [line for line in Path(path).read_text(errors='replace').split('\n')
                 if line.startswith('Version:')]
    except OSError:
        return 'unknown'
    return '\n'.join(lines) if lines else 'unknown'


def macho_uuid(path):
    """Read exactly one native slice UUID, for a dylib or its dSYM."""
    done = bench.capture(['xcrun', 'dwarfdump', '--uuid', str(path)])
    ids = re.findall(r'UUID: ([0-9A-Fa-f-]{36}) \(arm64\)', done.stdout.decode())
    if done.status or len(ids) != 1:
        raise ValueError('expected one arm64 UUID: ' + str(path))
    return ids[0].replace('-', '').lower()


def same_file(left, right):
    return Path(left).samefile(right)


def module_inventory(paths):
    """Name actual loaded files, not import names or a guessed loader search."""
    if not paths or not all(isinstance(path, str) for path in paths):
        raise ValueError('missing runtime module inventory')
    inventory = sorted((dict(path=str(Path(path).resolve(strict=True)), sha256=sha256(path))
                        for path in paths), key=lambda item: item['path'].casefold())
    if len({item['path'].casefold() for item in inventory}) != len(inventory):
        raise ValueError('duplicate runtime module')
    return inventory


def verify_modules(paths, expected):
    observed = module_inventory(paths)
    if len(observed) != len(expected) or any(
            not same_file(a['path'], b['path']) or a['sha256'] != b['sha256']
            for a, b in zip(observed, expected, strict=True)):
        raise ValueError('runtime module files differ')
    return observed


def windows_preflight(arms, data, env, installed):
    """Load each selected arm before the measured schedule."""
    import json
    expected = None
    for arm in arms.values():
        done = bench.capture([arm['driver'], '--measure', arm['dylib'], arm['uuid'], data],
                             env={**env, 'MEASURE_PREFLIGHT': '1'})
        if done.status:
            raise ValueError('Windows dependency preflight failed: ' + done.stderr.decode(errors='replace'))
        row = json.loads(done.stdout)
        if not same_file(row['library'], arm['dylib']):
            raise ValueError('wrong loaded library in preflight')
        inventory = module_inventory(row['modules'])
        names = {Path(item['path']).name.lower(): item for item in inventory}
        # tkrzw is linked statically into the arm; GLib is the one vcpkg module.
        name = 'glib-2.0-0.dll'
        if name not in names or not same_file(names[name]['path'], installed / 'bin' / name):
            raise ValueError('wrong vcpkg runtime: ' + name)
        if not any(name.startswith('vcruntime') for name in names) or 'ucrtbase.dll' not in names:
            raise ValueError('missing CRT identity')
        if expected is not None:
            verify_modules(row['modules'], expected)
        else:
            expected = inventory
        arm['modules'] = expected
    return expected


def expected_cow_commit(data):
    """Input-derived MemoryChunk views: default SYSTEM_FILE rows, no addon loads.

    Init maps these whole files; the typing corpus never calls load_phrase_library.
    DBM files belong to tkrzw and are not memmap2 copy-on-write views.
    """
    import mmap
    page_size = mmap.PAGESIZE
    rows = {}
    for line in (Path(data) / 'table.conf').read_text().splitlines():
        fields = line.split()
        if not fields or fields[0] != 'default':
            continue
        if len(fields) != 6:
            raise ValueError('cannot identify mapped inputs from table.conf')
        index = 'GB_DICTIONARY' if fields[1] == 'TSI_DICTIONARY' else fields[1]
        rows[index] = fields
    files = []
    for fields in rows.values():
        if fields[5] != 'SYSTEM_FILE':
            continue
        path = Path(data) / fields[3]
        if path.is_file():
            size = path.stat().st_size
            files.append(dict(name=fields[3], file_bytes=size,
                              page_rounded_bytes=((size + page_size - 1) // page_size) * page_size))
    return dict(label='expected commit from copy-on-write views', page_size_bytes=page_size,
                bytes=sum(item['page_rounded_bytes'] for item in files), files=files)
