"""Host and artifact identity the measurement runners record beside their captures (#1290).

A record whose subject cannot be named is not a record, so the runners write
what the numbers depend on before taking any: the kernel, the CPU, the tool
versions and a digest of each artifact. These are the readers behind those
tables. Each takes the proc root and the command runner as parameters, so the
tables are built from fixtures on any platform; the defaults read the real host.
Standard library only.
"""
import hashlib
from pathlib import Path
import platform
import re
from collections.abc import Callable, Sequence

from .. import host
from . import bench

PROC = Path('/proc')
Run = Callable[..., 'bench.pool.Completed'] | None    # None: bench.capture, looked up at call time


def library_backend(library, *, run: Run = None):
    """Identify the linked object's backend, not the caller's environment."""
    result = (run or bench.capture)(['readelf', '-dW', library])
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


def sha256(path) -> str:
    """The hex SHA-256 digest of the file at PATH."""
    digest = hashlib.sha256()
    with open(path, 'rb') as stream:
        for block in iter(lambda: stream.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def uname(flags: str = 'srm') -> str:
    """The fields `uname -FLAGS` prints, in the order of FLAGS (s, r, v, m)."""
    names = platform.uname()    # os.uname() on POSIX; Windows has no os.uname
    fields = {'s': names.system, 'r': names.release, 'v': names.version, 'm': names.machine}
    return ' '.join(fields[flag] for flag in flags)


def processors(proc: Path = PROC, cgroup_root: Path = Path('/sys/fs/cgroup')) -> int:
    """The CPUs this process may use: its affinity set held to the smallest cgroup CPU quota, rounded up.

    The shell runners printed GNU `nproc`, which rounds a fractional cgroup v2 quota to the nearest CPU,
    reads no cgroup v1 quota and honours OMP_NUM_THREADS and OMP_THREAD_LIMIT. This count is the budget
    that cargo's job default uses.
    """
    return host.cpu_snapshot(proc, cgroup_root)['cargo_cpus']


def _first_line(path: Path, needle: str) -> str | None:
    """The first line of PATH that contains NEEDLE (`grep -m1`), or None when there is none or no file."""
    try:
        for line in path.read_text(errors='replace').split('\n'):
            if needle in line:
                return line
    except OSError:
        pass
    return None


def cpu_model(proc: Path = PROC) -> str:
    """The first `model name` of /proc/cpuinfo without its label; empty where it has none (arm64)."""
    line = _first_line(proc / 'cpuinfo', 'model name')
    return '' if line is None else line.partition(':')[2].lstrip(' ')


def memory_total(proc: Path = PROC) -> str:
    """The MemTotal line of /proc/meminfo as it reads; empty where there is none."""
    return _first_line(proc / 'meminfo', 'MemTotal') or ''


def loadavg(proc: Path = PROC, fields: int | None = None) -> str:
    """/proc/loadavg without its final newline, or its first FIELDS space-separated fields."""
    text = (proc / 'loadavg').read_text().rstrip('\n')
    return text if fields is None else ' '.join(text.split(' ')[:fields])


def perf_event_paranoid(proc: Path = PROC) -> str:
    """The kernel's perf_event_paranoid setting, or `unknown` where it cannot be read."""
    try:
        return (proc / 'sys/kernel/perf_event_paranoid').read_text().rstrip('\n')
    except OSError:
        return 'unknown'


def first_output_line(command: Sequence, *, run: Run = None) -> str:
    """The first line COMMAND prints (`$(COMMAND | head -1)`); empty when it cannot run."""
    try:
        done = (run or bench.capture)(command)
    except OSError:
        return ''
    return done.stdout.decode(errors='replace').split('\n')[0]


def package_version(package: str, *, run: Run = None) -> str:
    """`pkg-config --modversion PACKAGE`, or `unknown` when pkg-config fails or is absent."""
    try:
        done = (run or bench.capture)(['pkg-config', '--modversion', package])
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


def macho_uuid(path, *, run=None):
    """Read exactly one native slice UUID, for a dylib or its dSYM."""
    done = (run or bench.capture)(['xcrun', 'dwarfdump', '--uuid', str(path)])
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
            for a, b in zip(observed, expected)):
        raise ValueError('runtime module files differ')
    return observed


def windows_preflight(arms, data, env, installed):
    """Resolve each implicitly linked arm before the measured schedule."""
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
        for name in ('tkrzw.dll', 'glib-2.0-0.dll'):
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


def expected_cow_commit(data, *, page_size=None):
    """Input-derived MemoryChunk views: default SYSTEM_FILE rows, no addon loads.

    Init maps these whole files; the typing corpus never calls load_phrase_library.
    DBM files belong to tkrzw and are not memmap2 copy-on-write views.
    """
    if page_size is None:
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
