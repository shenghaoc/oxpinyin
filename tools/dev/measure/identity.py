"""Host and artifact identity the measurement runners record beside their captures (#1290).

A record whose subject cannot be named is not a record, so the runners write
what the numbers depend on before taking any: the kernel, the CPU, the tool
versions and a digest of each artifact. These are the readers behind those
tables. Each takes the proc root and the command runner as parameters, so the
tables are built from fixtures on any platform; the defaults read the real host.
Standard library only.
"""
import hashlib
import os
from pathlib import Path
import re
from collections.abc import Callable, Sequence

from .. import host
from ..runners import Finished, write
from . import bench

PROC = Path('/proc')
FIXTURE_HOOKS = ('oxpinyin_init_for_fixtures', 'oxpinyin_test_set_user_bigram')
NEEDED = re.compile(r'Shared library: \[(.*)\]')
Run = Callable[..., 'bench.pool.Completed'] | None    # None: bench.capture, looked up at call time


def sha256(path) -> str:
    """The hex SHA-256 digest of the file at PATH."""
    digest = hashlib.sha256()
    with open(path, 'rb') as stream:
        for block in iter(lambda: stream.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def uname(flags: str = 'srm') -> str:
    """The fields `uname -FLAGS` prints, in the order of FLAGS (s, r, v, m)."""
    names = os.uname()
    fields = {'s': names.sysname, 'r': names.release, 'v': names.version, 'm': names.machine}
    return ' '.join(fields[flag] for flag in flags)


def processors(proc: Path = PROC, cgroup_root: Path = Path('/sys/fs/cgroup')) -> int:
    """The CPUs this process may use: its affinity set held to the smallest cgroup CPU quota, rounded up.

    The shell runners printed GNU `nproc`, which rounds a fractional cgroup v2 quota to the nearest CPU,
    reads no cgroup v1 quota and honours OMP_NUM_THREADS and OMP_THREAD_LIMIT. This count is the budget
    that heavy admission and cargo's job default use; the differences are recorded in
    docs/plans/tooling-python.md (U2).
    """
    return host.resource_snapshot(proc, cgroup_root)['cargo_cpus']


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


def needed(library, *, run: Run = None) -> list[str]:
    """The DT_NEEDED names of LIBRARY, from `readelf -d`; a failing readelf ends the run with its status."""
    done = (run or bench.capture)(['readelf', '-d', str(library)])
    if done.status:
        write(done.stderr, error=True)
        raise Finished(done.status)
    return [match[1] for line in done.stdout.decode(errors='replace').split('\n')
            if (match := NEEDED.search(line))]


def fixture_hooks(library, *, run: Run = None) -> int:
    """How many exported lines name a fixture hook (`nm -D … | grep -c`); a failing nm counts none."""
    done = (run or bench.capture)(['nm', '-D', str(library)])
    lines = done.stdout.decode(errors='replace').split('\n') if done.status == 0 else []
    return sum(any(hook in line for hook in FIXTURE_HOOKS) for line in lines)


def realpath(path) -> str:
    """The canonical path of PATH, as `readlink -f` prints it."""
    return os.path.realpath(path)
