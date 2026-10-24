"""Host resource reads and heavy-work estimates. Standard library only.

The module owns the native capacity questions the heavy slot needs: known
memory, CPU ceilings, host-wide lock location and process ancestry. It never
spawns a shell tool when a system call or a standard-library read can answer.
"""
import ctypes
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys

# The Linux cgroup reader and its tests use os.sched_getaffinity. macOS has no
# such syscall; the all-CPU set is the equivalent native answer, and it keeps
# the Linux-shaped reader testable on the development host.
if not hasattr(os, 'sched_getaffinity'):
    def _sched_getaffinity_fallback(_pid):
        return set(range(os.cpu_count() or 1))
    os.sched_getaffinity = _sched_getaffinity_fallback

# These estimates use the existing Step 0 peaks from 2026-10-19 UTC.
RUSTC_PEAK_MIB = 600        # rustc 555 MiB (test build), 492 (cold cell build), 470 (incremental)
TEST_THREAD_PEAK_MIB = 64   # the crates' test binaries peak at 6-11 MiB; rustdoc beside them 91 MiB
RUNNER_PEAK_MIB = 1024      # one gate-set runner job (contract-diff worker pairs and their fixtures)
HEADROOM_MIB = 2048         # at most left to the desktop: the browser, the editor, the page cache
HEADROOM_PERCENT = 10       # of total memory, capped at HEADROOM_MIB
# Total memory from which the host offers a second heavy slot: 15 GiB, because
# a 16 GiB machine's Linux MemTotal is below 16 GiB.
TWO_SLOT_TOTAL_MIB = 15 * 1024
# BDB cpp-smoke, two jobs, rootless Podman, 2026-10-21 UTC: cold-build peak
# 2,232,991,744 bytes (ceil 2130 MiB). Add 20% (426 MiB), round up to 256 MiB;
# also covers two RUNNER_PEAK_MIB (2048 MiB) and two RUSTC_PEAK_MIB (1200 MiB).
CONTAINER_MEMORY_MIB = 2560


def container_memory_mib(value):
    if value is None:
        return CONTAINER_MEMORY_MIB
    if not isinstance(value, str) or not re.fullmatch(r'[1-9][0-9]*', value):
        raise ValueError('container memory must be a positive integer in MiB')
    return int(value)


def inspect_container(record):
    """Read runtime metadata; CLI/daemon errors are unknown, never exit proof."""
    try:
        result = subprocess.run([record['runtime'], 'inspect', record['name']],
                                capture_output=True, text=True, timeout=5)
        if result.returncode:
            return None
        rows = json.loads(result.stdout)
        if not isinstance(rows, list) or len(rows) != 1 or not isinstance(rows[0], dict):
            return None
        return rows[0]
    except (OSError, ValueError, subprocess.SubprocessError):
        return None


def container_liveness(record):
    """Read live, created (never started), gone, or unknown runtime state.

    Names carry a random ownership label; a different owner is uncertain.
    Pending creates are handled by the admission code, never by PID death.
    """
    row = inspect_container(record)
    try:
        if row is not None:
            if record['kind'] == 'container':
                labels = row['configuration']['labels']
                status = row['status']
                # Observed container 1.5.0 uses an object with state,
                # startedDate and networks; documented versions use a scalar.
                state = status.get('state') if isinstance(status, dict) else status
                if not isinstance(state, str):
                    return 'unknown'
                stopped = state == 'stopped'
            else:
                labels = row['Config']['Labels']
                state = row['State']['Status']
                stopped = state in ('exited', 'dead')
            if not isinstance(labels, dict) or labels.get('io.oxpinyin.memory-reservation') != record['token']:
                return 'unknown'
            if stopped:
                return 'gone'
            if state == 'created':
                return 'created'
            return 'live' if state in ('running', 'paused', 'restarting', 'stopping') else 'unknown'
        # Do not turn an inspect error, including "not found", into exit proof.
        apple = record['kind'] == 'container'
        rows = list_containers(record)
        if rows is None:
            return 'unknown'
        for item in rows:
            names = [item['id']] if apple else item['Names']
            if isinstance(names, str):
                names = names.split(',')
            if not isinstance(names, list):
                return 'unknown'
            if record['name'] in names:
                return 'unknown'
        return 'gone'
    except (KeyError, TypeError, OSError, ValueError, subprocess.SubprocessError):
        return 'unknown'


def list_containers(record, *, token_only=False):
    """A completed all-container listing, or None for an uncertain result."""
    try:
        apple = record['kind'] == 'container'
        args = (['list', '--all', '--format', 'json'] if apple else
                ['ps', '--all', '--no-trunc', '--format', '{{json .}}'])
        if token_only and not apple:
            args += ['--filter', f'label=io.oxpinyin.memory-reservation={record["token"]}']
        result = subprocess.run([record['runtime'], *args], capture_output=True,
                                text=True, timeout=5)
        if result.returncode:
            return None
        data = result.stdout.strip()
        # Docker emits JSON lines; Apple and some Podman versions emit arrays.
        rows = (json.loads(data) if apple or data.startswith('[') else
                [json.loads(line) for line in data.splitlines()])
        if not isinstance(rows, list) or any(not isinstance(row, dict) for row in rows):
            return None
        return rows
    except (KeyError, OSError, ValueError, subprocess.SubprocessError):
        return None


def container_token_absent(record):
    """True only after a complete listing proves no token-labeled container."""
    rows = list_containers(record, token_only=True)
    if rows is None:
        return None
    if record['kind'] != 'container':
        # A supported Docker/Podman label filter covers every container state.
        return not rows
    try:
        for row in rows:
            labels = row['configuration']['labels']
            if not isinstance(labels, dict):
                return None
            if labels.get('io.oxpinyin.memory-reservation') == record['token']:
                return False
        return True
    except (KeyError, TypeError):
        return None


def remove_created_container(record):
    """Remove only this token's inert object. Caller owns admission and guard.

    No force: if something starts it between inspect and removal, the runtime
    refuses deletion. Confirm absence/exit again before releasing any budget.
    """
    if container_liveness(record) != 'created':
        return False
    try:
        result = subprocess.run([record['runtime'], 'rm', record['name']],
                                capture_output=True, text=True, timeout=5)
        return result.returncode == 0 and container_liveness(record) == 'gone'
    except (OSError, subprocess.SubprocessError):
        return False


def _macos_available_mib():
    """Free + inactive + speculative pages from Mach host_statistics64."""
    try:
        libc = ctypes.CDLL('/usr/lib/libSystem.B.dylib')
        libc.mach_host_self.restype = ctypes.c_uint
        host = libc.mach_host_self()

        class VMStatistics64(ctypes.Structure):
            _fields_ = [
                ('free_count', ctypes.c_uint),
                ('active_count', ctypes.c_uint),
                ('inactive_count', ctypes.c_uint),
                ('wire_count', ctypes.c_uint),
                ('zero_fill_count', ctypes.c_ulonglong),
                ('reactivations', ctypes.c_ulonglong),
                ('pageins', ctypes.c_ulonglong),
                ('pageouts', ctypes.c_ulonglong),
                ('faults', ctypes.c_ulonglong),
                ('cow_faults', ctypes.c_ulonglong),
                ('lookups', ctypes.c_ulonglong),
                ('hits', ctypes.c_ulonglong),
                ('purges', ctypes.c_ulonglong),
                ('purgeable_count', ctypes.c_uint),
                ('speculative_count', ctypes.c_uint),
                ('decompressions', ctypes.c_ulonglong),
                ('compressions', ctypes.c_ulonglong),
                ('swapins', ctypes.c_ulonglong),
                ('swapouts', ctypes.c_ulonglong),
                ('compressor_page_count', ctypes.c_uint),
                ('throttled_count', ctypes.c_uint),
                ('external_page_count', ctypes.c_uint),
                ('internal_page_count', ctypes.c_uint),
                ('total_uncompressed_pages_in_compressor', ctypes.c_ulonglong),
            ]

        stats = VMStatistics64()
        count = ctypes.c_uint(ctypes.sizeof(stats) // ctypes.sizeof(ctypes.c_int))
        libc.host_statistics64.argtypes = [ctypes.c_uint, ctypes.c_int,
                                           ctypes.POINTER(VMStatistics64),
                                           ctypes.POINTER(ctypes.c_uint)]
        libc.host_statistics64.restype = ctypes.c_int
        if libc.host_statistics64(host, 4, ctypes.byref(stats), ctypes.byref(count)) != 0:
            return None
        page_size = ctypes.c_ulong()
        libc.host_page_size.argtypes = [ctypes.c_uint, ctypes.POINTER(ctypes.c_ulong)]
        libc.host_page_size.restype = ctypes.c_int
        if libc.host_page_size(host, ctypes.byref(page_size)) != 0:
            return None
        pages = stats.free_count + stats.inactive_count + stats.speculative_count
        return pages * page_size.value // (1024 * 1024)
    except (OSError, ValueError, AttributeError):
        return None


def _macos_total_mib():
    try:
        libc = ctypes.CDLL('/usr/lib/libSystem.B.dylib')
        value = ctypes.c_ulonglong()
        length = ctypes.c_size_t(ctypes.sizeof(value))
        libc.sysctlbyname.argtypes = [ctypes.c_char_p, ctypes.c_void_p,
                                      ctypes.POINTER(ctypes.c_size_t), ctypes.c_void_p,
                                      ctypes.c_size_t]
        libc.sysctlbyname.restype = ctypes.c_int
        if libc.sysctlbyname(b'hw.memsize', ctypes.byref(value), ctypes.byref(length),
                             None, 0) == 0:
            return int(value.value) // (1024 * 1024)
    except (OSError, ValueError, AttributeError):
        return None
    return None


def total_memory_mib(proc=Path('/proc')):
    """Total physical memory in MiB, or None. Linux: MemTotal from /proc/meminfo."""
    try:
        if sys.platform.startswith('linux'):
            for line in (proc / 'meminfo').read_text().splitlines():
                if line.startswith('MemTotal:'):
                    return int(line.split()[1]) // 1024
        elif sys.platform == 'darwin':
            return _macos_total_mib()
        elif sys.platform == 'win32':
            class Status(ctypes.Structure):
                _fields_ = [('dwLength', ctypes.c_ulong), ('dwMemoryLoad', ctypes.c_ulong),
                            ('ullTotalPhys', ctypes.c_ulonglong), ('ullAvailPhys', ctypes.c_ulonglong),
                            ('ullTotalPageFile', ctypes.c_ulonglong), ('ullAvailPageFile', ctypes.c_ulonglong),
                            ('ullTotalVirtual', ctypes.c_ulonglong), ('ullAvailVirtual', ctypes.c_ulonglong),
                            ('ullAvailExtendedVirtual', ctypes.c_ulonglong)]
            status = Status(dwLength=ctypes.sizeof(Status))
            if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
                return int(status.ullTotalPhys) // (1024 * 1024)
    except (OSError, ValueError, AttributeError):
        return None
    return None


def memory_available_mib():
    """Memory the host can give new processes now, in MiB, or None.
    Linux: MemAvailable from /proc/meminfo. macOS: Mach host_statistics64
    free + inactive + speculative pages. Windows: GlobalMemoryStatusEx."""
    try:
        if sys.platform.startswith('linux'):
            for line in Path('/proc/meminfo').read_text().splitlines():
                if line.startswith('MemAvailable:'):
                    return int(line.split()[1]) // 1024
        elif sys.platform == 'darwin':
            return _macos_available_mib()
        elif sys.platform == 'win32':
            class Status(ctypes.Structure):
                _fields_ = [('dwLength', ctypes.c_ulong), ('dwMemoryLoad', ctypes.c_ulong),
                            ('ullTotalPhys', ctypes.c_ulonglong), ('ullAvailPhys', ctypes.c_ulonglong),
                            ('ullTotalPageFile', ctypes.c_ulonglong), ('ullAvailPageFile', ctypes.c_ulonglong),
                            ('ullTotalVirtual', ctypes.c_ulonglong), ('ullAvailVirtual', ctypes.c_ulonglong),
                            ('ullAvailExtendedVirtual', ctypes.c_ulonglong)]
            status = Status(dwLength=ctypes.sizeof(Status))
            if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
                return int(status.ullAvailPhys) // (1024 * 1024)
    except (OSError, ValueError, AttributeError, subprocess.SubprocessError):
        return None
    return None


def linux_cgroup_dirs(proc=Path('/proc'), root=Path('/sys/fs/cgroup')):
    """Find the current cgroup and its visible ancestors."""
    entry = next(line[3:] for line in (proc / 'self/cgroup').read_text().splitlines()
                 if line.startswith('0::'))
    relative = Path(entry)
    if not relative.is_absolute() or '..' in relative.parts:
        raise ValueError('invalid cgroup path')
    current = root / entry.lstrip('/')
    return [current, *list(current.parents)[:len(current.relative_to(root).parts)]]


def _memory_min(directory):
    """Read a level's hard memory protection, defaulting to the kernel's zero.

    memory.min is hard protection that the kernel does not reclaim, so a
    positive floor keeps this level on the current-based calculation. A
    missing or unreadable file is the kernel default, zero:
    https://docs.kernel.org/admin-guide/cgroup-v2.html#memory-interface-files
    """
    try:
        minimum = int((directory / 'memory.min').read_text().strip())
        if minimum < 0:
            raise ValueError('negative memory.min')
        return minimum, f'memory.min={minimum}'
    except FileNotFoundError:
        return 0, 'memory.min=0 (kernel default; missing)'
    except (OSError, ValueError):
        return 0, 'memory.min=0 (kernel default; unreadable)'


def _cgroup_memory_used(directory, current):
    """Retain charges other than clean file cache.

    Reclaimable slab is not free room: the kernel defines it as memory that
    only might be reclaimed, and referenced slab objects stay charged in
    kernel. It remains inside the kernel aggregate and is never subtracted.
    Field meanings and slab semantics:
    https://docs.kernel.org/admin-guide/cgroup-v2.html#memory-interface-files
    The kernel aggregate includes its component counters and both slab types;
    never add those components again. Socket buffers are charged separately:
    Linux v6.18 mm/memcontrol.c, memory_stats and mem_cgroup_sk_charge:
    https://github.com/torvalds/linux/blob/v6.18/mm/memcontrol.c
    """
    required = ('anon', 'file', 'kernel', 'sock', 'shmem', 'active_file',
                'inactive_file', 'file_dirty', 'file_writeback')
    optional = ('slab_reclaimable',)
    stats = {}
    for line in (directory / 'memory.stat').read_text().splitlines():
        fields = line.split()
        if len(fields) != 2:
            raise ValueError('malformed memory.stat row')
        key, value = fields
        if key in required or key in optional:
            if key in stats or not re.fullmatch(r'[0-9]+', value):
                raise ValueError(f'invalid memory.stat {key}')
            stats[key] = int(value)
    missing = [key for key in required if key not in stats]
    if missing:
        raise ValueError('missing memory.stat fields: ' + ', '.join(missing))
    for key in ('shmem', 'file_dirty', 'file_writeback'):
        if stats[key] > stats['file']:
            raise ValueError(f'memory.stat {key} exceeds file')
    if stats.get('slab_reclaimable', 0) > stats['kernel']:
        raise ValueError('memory.stat slab_reclaimable exceeds kernel')

    # file already includes shmem, whose swap-backed pages are not clean file
    # cache. LRU counters describe lists, not types; cap them by file - shmem
    # instead of assuming they equal file. Both active and inactive clean
    # cache can be reclaimed, including the build output blocking the runner.
    candidates = min(stats['file'] - stats['shmem'],
                     stats['active_file'] + stats['inactive_file'])
    # Dirty and writeback pages may overlap after redirtying. Their bounded
    # sum conservatively reserves both disjoint sets; it is not an exact
    # union and can overreserve overlapping pages. max(dirty, writeback)
    # would underreserve disjoint pages. Never subtract more than candidates.
    reserved = min(candidates, stats['file_dirty'] + stats['file_writeback'])
    clean = candidates - reserved
    # Keep unclassified charges and use the larger total across non-atomic
    # current/stat reads. shmem and kernel components are already included.
    total = max(current, stats['anon'] + stats['file'] + stats['kernel'] + stats['sock'])
    return total - clean, stats


def resource_snapshot(proc=Path('/proc'), cgroup_root=Path('/sys/fs/cgroup')):
    """Read known limits. Report each unreadable limit as unknown."""
    notes, memories, limits, cpus = [], [], [], []
    if sys.platform.startswith('linux'):
        try:
            line = next(line for line in (proc / 'meminfo').read_text().splitlines()
                        if line.startswith('MemAvailable:'))
            memory = int(line.split()[1]) // 1024
            if memory < 0:
                raise ValueError('negative memory')
            memories.append(memory)
            notes.append(f'MemAvailable={memory} MiB')
        except (OSError, ValueError, IndexError, StopIteration):
            notes.append('MemAvailable=unknown')
        total = total_memory_mib(proc)
        notes.append(f'MemTotal={total} MiB' if total is not None else 'MemTotal=unknown')
        try:
            affinity = len(os.sched_getaffinity(0))
            if not affinity:
                raise ValueError('empty CPU affinity')
            cpus.append(affinity)
            notes.append(f'CPU affinity={affinity}')
        except (OSError, ValueError, AttributeError):
            notes.append('CPU affinity=unknown')
            count = os.cpu_count()
            if count:
                cpus.append(count)
                notes.append(f'CPU count={count}')
        try:
            directories = linux_cgroup_dirs(proc, cgroup_root)
        except (OSError, ValueError, StopIteration):
            directories = []
            notes.append('cgroup v2 memory and CPU limits=unknown')
        for directory in directories:
            label = '/' if directory == cgroup_root else '/' + str(directory.relative_to(cgroup_root))
            try:
                maximum = (directory / 'memory.max').read_text().strip()
                if maximum == 'max':
                    notes.append(f'cgroup {label} memory.max=max')
                else:
                    maximum = int(maximum)
                    if maximum < 0:
                        raise ValueError('negative memory limit')
                    limits.append(maximum)
                    current = int((directory / 'memory.current').read_text().strip())
                    if current < 0:
                        raise ValueError('negative memory')
                    minimum, minimum_note = _memory_min(directory)
                    if minimum > 0:
                        # Protected memory is not reclaimable, so this level
                        # keeps today's max - current room even when memory.stat
                        # is readable:
                        # https://docs.kernel.org/admin-guide/cgroup-v2.html#memory-interface-files
                        used = current
                        notes.append(f'cgroup {label} {minimum_note}: protected memory; '
                                     f'fallback to memory.current={current}')
                    else:
                        try:
                            used, stats = _cgroup_memory_used(directory, current)
                            fields = ' '.join(f'{key}={value}' for key, value in stats.items())
                            notes.append(f'cgroup {label} memory.stat bytes: {fields}; '
                                         f'memory.current={current}; {minimum_note}; used={used}')
                        except (OSError, ValueError) as exc:
                            # Missing/unsupported stats retain today's stricter room.
                            # An unreadable max/current still skips this level below.
                            used = current
                            notes.append(f'cgroup {label} memory.stat unusable; '
                                         f'fallback to memory.current={current} ({exc}); '
                                         f'{minimum_note}')
                    room = max(0, maximum - used) // (1024 * 1024)
                    memories.append(room)
                    notes.append(f'cgroup {label} memory room={room} MiB')
            except (OSError, ValueError):
                notes.append(f'cgroup {label} memory room=unknown')
            try:
                quota, period = (directory / 'cpu.max').read_text().split()
                period = int(period)
                if period <= 0:
                    raise ValueError('invalid CPU period')
                if quota == 'max':
                    notes.append(f'cgroup {label} cpu.max=max')
                else:
                    quota = int(quota)
                    if quota <= 0:
                        raise ValueError('invalid CPU quota')
                    cpus.append(quota / period)
                    notes.append(f'cgroup {label} CPU quota={quota}/{period}')
            except (OSError, ValueError):
                notes.append(f'cgroup {label} CPU quota=unknown')
    else:
        memory = memory_available_mib()
        if memory is not None:
            memories.append(memory)
        notes.append(f'available memory={memory if memory is not None else "unknown"} MiB')
        total = total_memory_mib()
        notes.append(f'total memory={total if total is not None else "unknown"} MiB')
    if not cpus:
        count = os.cpu_count()
        notes.append(f'CPU count={count if count is not None else "unknown; fallback 1"}')
        cpus.append(count or 1)
    return dict(available_mib=min(memories) if memories else None,
                memory_limit_mib=min(limits) // (1024 * 1024) if limits else None,
                total_mib=total,
                cpus=max(1, math.floor(min(cpus))),
                cargo_cpus=max(1, math.ceil(min(cpus))), notes=notes)


def desktop_headroom_mib(total_mib):
    """10% of total memory, at most HEADROOM_MIB; HEADROOM_MIB when the total is unknown."""
    if total_mib is None:
        return HEADROOM_MIB
    return min(HEADROOM_MIB, total_mib * HEADROOM_PERCENT // 100)


def memory_headroom(env, resources):
    """The host keeps desktop room; a hard-limited compatibility guest does not."""
    if env.get('OXPINYIN_COMPAT_CONTAINER') == '1' and resources.get('memory_limit_mib') is not None:
        return 0
    return desktop_headroom_mib(resources.get('total_mib'))


def fit(available_mib, per_process_mib, ceiling, headroom_mib=HEADROOM_MIB):
    """Return zero when no process fits after the headroom."""
    return max(0, min(ceiling, (available_mib - headroom_mib) // per_process_mib))


READ_HOST = object()


def validate_settings(env):
    build = env.get('CARGO_BUILD_JOBS')
    if build is not None and build != 'default':
        if not re.fullmatch(r'[+-]?[0-9]+', build) or int(build) == 0 or not -(2**31) <= int(build) < 2**31:
            raise ValueError('Invalid CARGO_BUILD_JOBS: use a nonzero integer or default')
    threads = env.get('RUST_TEST_THREADS')
    if threads is not None and (not re.fullmatch(r'[+]?[0-9]+', threads) or int(threads) <= 0):
        raise ValueError('Invalid RUST_TEST_THREADS: use a positive integer')
    cases = env.get('OXPINYIN_CONTRACT_JOBS')
    if cases is not None and (not re.fullmatch(r'[+]?[0-9]+', cases) or int(cases) <= 0):
        raise ValueError('Invalid OXPINYIN_CONTRACT_JOBS: use a positive integer')


def memory_defaults(env, explicit_jobs, cpus=None, available_mib=READ_HOST, *, memory_limit_mib=None,
                    total_mib=None):
    """Keep caller values. Choose only unset values from current resources.

    Each heavy slot gets an equal share of the room after the headroom. One
    process is the minimum while it fits the whole room; admission decides.
    """
    validate_settings(env)
    if available_mib is READ_HOST:
        resources = resource_snapshot()
        available_mib = resources['available_mib']
        memory_limit_mib = resources.get('memory_limit_mib')
        total_mib = resources.get('total_mib')
        cpus = cpus or resources['cpus']
    cpus = cpus or os.cpu_count() or 1
    headroom = memory_headroom(env, dict(memory_limit_mib=memory_limit_mib, total_mib=total_mib))
    slots = heavy_slot_count(total_mib)
    chosen = {}
    def choose(name, explicit, per_process, ceiling, unit):
        if explicit is not None:
            chosen[name] = (explicit, 'set by the caller', True)
        elif available_mib is None:
            chosen[name] = (0, 'available memory unknown; no capacity admitted', False)
        else:
            share = max(0, available_mib - headroom) // slots
            value = min(ceiling, share // per_process)
            reason = (f'{available_mib} MiB available - {headroom} MiB headroom, a 1/{slots} share, '
                      f'{per_process} MiB per {unit}, at most {ceiling}')
            if not value and fit(available_mib, per_process, ceiling, headroom):
                value = 1
                reason += f'; one {unit}, which fits the whole room'
            chosen[name] = (value, reason, False)
    choose('CARGO_BUILD_JOBS', env.get('CARGO_BUILD_JOBS'), RUSTC_PEAK_MIB,
           cpus, 'rustc')
    choose('RUST_TEST_THREADS', env.get('RUST_TEST_THREADS'), TEST_THREAD_PEAK_MIB,
           cpus, 'test thread')
    choose('--jobs', explicit_jobs, RUNNER_PEAK_MIB, 2, 'runner job')
    return chosen


def plan_defaults(chosen, out=sys.stderr, phase='defaults'):
    for name, (value, reason, explicit) in chosen.items():
        print(f"{phase}: {name}={value} ({'explicit: ' if explicit else ''}{reason})", file=out, flush=True)


def prepare_heavy_env(env, resources, *, runner=False):
    """Prepare one child environment and its admission estimate."""
    chosen = memory_defaults(env, None, cpus=resources['cpus'], available_mib=resources['available_mib'],
                             memory_limit_mib=resources.get('memory_limit_mib'),
                             total_mib=resources.get('total_mib'))
    chosen.pop('--jobs')
    child = dict(env, **{name: str(value[0]) for name, value in chosen.items()})
    build = child['CARGO_BUILD_JOBS']
    count = resources['cargo_cpus'] if build == 'default' else int(build)
    if count < 0:
        count = max(1, resources['cargo_cpus'] + count)
    estimate = max(RUNNER_PEAK_MIB if runner else RUSTC_PEAK_MIB, count * RUSTC_PEAK_MIB,
                   int(child['RUST_TEST_THREADS']) * TEST_THREAD_PEAK_MIB)
    if 'OXPINYIN_CONTRACT_JOBS' in env:
        # A case runs pin then subject, so at most one worker per active
        # case is live. Charge the existing runner estimate for EACH case.
        # This explicit setting is a requested maximum: reduce it against
        # the room left after other live reservations and headroom, before
        # the admission lock publishes the estimate or any worker starts.
        requested = int(env['OXPINYIN_CONTRACT_JOBS'])
        available = resources['available_mib']
        headroom = memory_headroom(env, resources)
        workers = 0 if available is None else fit(available, RUNNER_PEAK_MIB, requested, headroom)
        reason = (f'requested maximum {requested}; available memory unknown' if available is None else
                  f'requested maximum {requested}; {available} MiB available - {headroom} MiB headroom; '
                  f'{RUNNER_PEAK_MIB} MiB per contract case')
        chosen['OXPINYIN_CONTRACT_JOBS'] = (workers, reason, False)
        # Zero capacity still needs one worker's estimate. Admission refuses
        # it; no zero-sized pool or unaccounted worker can be launched.
        child['OXPINYIN_CONTRACT_JOBS'] = str(max(1, workers))
        estimate = max(estimate, max(1, workers) * RUNNER_PEAK_MIB)
    return child, chosen, estimate


def resource_wait_limit(env):
    try:
        limit = float(env.get('OXPINYIN_RESOURCE_WAIT_SECONDS', '1800'))
        if not math.isfinite(limit) or limit < 0:
            raise ValueError('invalid wait')
        return limit
    except ValueError as exc:
        raise ValueError('Invalid OXPINYIN_RESOURCE_WAIT_SECONDS: use finite nonnegative seconds') from exc


class ResourceBlocked(ValueError):
    pass


def host_lock_dir():
    """Where the host-wide locks live: $OXPINYIN_LOCK_DIR, else
    $XDG_CACHE_HOME/oxpinyin/locks, else ~/.cache/oxpinyin/locks (the cache
    tools/cloud/setup.sh already uses). Shared by every worktree and agent."""
    root = os.environ.get('OXPINYIN_LOCK_DIR')
    if not root:
        if sys.platform == 'win32' and os.environ.get('LOCALAPPDATA'):
            root = str(Path(os.environ['LOCALAPPDATA']) / 'oxpinyin/locks')
        else:
            cache = os.environ.get('XDG_CACHE_HOME') or str(Path.home() / '.cache')
            root = str(Path(cache) / 'oxpinyin/locks')
    path = Path(root)
    if os.environ.get('OXPINYIN_LOCK_DIR') and not path.is_absolute():
        raise ValueError('OXPINYIN_LOCK_DIR must be an absolute path')
    path = path.absolute()
    path.mkdir(parents=True, exist_ok=True)
    return path


def heavy_slot_count(total_mib=READ_HOST):
    """How many heavy slots the host offers: $OXPINYIN_HEAVY_SLOTS, else two
    with at least 15 GiB of total memory; a 16 GiB machine's Linux MemTotal is
    below 16 GiB. One below that or when the total is unknown."""
    if 'OXPINYIN_HEAVY_SLOTS' in os.environ:
        try:
            return max(1, int(os.environ['OXPINYIN_HEAVY_SLOTS']))
        except ValueError:
            return 1
    if total_mib is READ_HOST:
        total_mib = total_memory_mib()
    return 2 if total_mib is not None and total_mib >= TWO_SLOT_TOTAL_MIB else 1


def pid_alive(pid):
    if sys.platform == 'win32':
        import ctypes
        from ctypes import wintypes
        kernel32 = ctypes.WinDLL('kernel32', use_last_error=True)
        kernel32.OpenProcess.restype = wintypes.HANDLE
        kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        kernel32.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
        kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
        handle = kernel32.OpenProcess(0x1000, False, pid)
        if not handle:
            return ctypes.get_last_error() == 5  # access denied: the process exists
        code = wintypes.DWORD()
        ok = kernel32.GetExitCodeProcess(handle, ctypes.byref(code))
        kernel32.CloseHandle(handle)
        return bool(ok) and code.value == 259  # STILL_ACTIVE
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def read_holder(path):
    try:
        return json.loads(path.read_text() or 'null')
    except (OSError, ValueError):
        return None


def describe_holder(holder):
    if not isinstance(holder, dict):
        return 'no holder recorded'
    return (f"pid {holder.get('pid')} in {holder.get('worktree')} since {holder.get('since')}: "
            f"{holder.get('command')}")


def write_holder(file, holder):
    file.seek(0)
    file.truncate()
    file.write(json.dumps(holder))
    file.flush()


def _windows_processes():
    """(pid, parent pid) for every process in one Toolhelp snapshot."""
    import ctypes
    from ctypes import wintypes

    class ENTRY(ctypes.Structure):
        _fields_ = [('dwSize', wintypes.DWORD), ('cntUsage', wintypes.DWORD),
                    ('th32ProcessID', wintypes.DWORD), ('th32DefaultHeapID', ctypes.c_size_t),
                    ('th32ModuleID', wintypes.DWORD), ('cntThreads', wintypes.DWORD),
                    ('th32ParentProcessID', wintypes.DWORD), ('pcPriClassBase', wintypes.LONG),
                    ('dwFlags', wintypes.DWORD), ('szExeFile', wintypes.WCHAR * 260)]

    kernel32 = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel32.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
    kernel32.Process32FirstW.argtypes = [wintypes.HANDLE, ctypes.POINTER(ENTRY)]
    kernel32.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(ENTRY)]
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    snapshot = kernel32.CreateToolhelp32Snapshot(0x2, 0)
    if snapshot in (None, wintypes.HANDLE(-1).value):
        raise OSError(ctypes.get_last_error(), 'CreateToolhelp32Snapshot failed')
    entry = ENTRY()
    entry.dwSize = ctypes.sizeof(ENTRY)
    processes = []
    try:
        more = kernel32.Process32FirstW(snapshot, ctypes.byref(entry))
        while more:
            processes.append((int(entry.th32ProcessID), int(entry.th32ParentProcessID)))
            more = kernel32.Process32NextW(snapshot, ctypes.byref(entry))
    finally:
        kernel32.CloseHandle(snapshot)
    return processes


def _windows_parent(pid):
    return next((parent for child, parent in _windows_processes() if child == pid), None)


def process_parent(pid):
    try:
        if sys.platform == 'win32':
            return _windows_parent(pid)
        if sys.platform.startswith('linux'):
            return int(Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[1])
        result = subprocess.run(['ps', '-o', 'ppid=', '-p', str(pid)],
                                capture_output=True, text=True, check=True)
        return int(result.stdout.strip())
    except (OSError, ValueError, IndexError, subprocess.SubprocessError):
        return None


def is_ancestor(pid):
    current, seen = os.getppid(), set()
    while current and current not in seen:
        if current == pid:
            return True
        seen.add(current)
        current = process_parent(current)
    return False


def _linux_tree_pss(pid, proc):
    """Pss bytes of PID and its descendants, from each /proc/<pid>/smaps_rollup.

    Pss divides a shared page among the processes that map it, so pages that
    several processes share (rustc's libraries) count once. A process whose
    Pss cannot be read raises, which makes the use unknown.
    """
    children, listed = {}, set()
    for entry in proc.iterdir():
        if not entry.name.isdigit():
            continue
        try:
            fields = (entry / 'stat').read_text().rsplit(')', 1)[1].split()
        except OSError:
            continue  # exited during the walk, or another user's process
        children.setdefault(int(fields[1]), []).append(int(entry.name))
        listed.add(int(entry.name))
    if pid not in listed:
        return None
    total, pending, seen = 0, [pid], set()
    while pending:
        current = pending.pop()
        if current not in seen:
            seen.add(current)
            rows = [line.split() for line in (proc / str(current) / 'smaps_rollup').read_text().splitlines()
                    if line.startswith('Pss:')]
            if len(rows) != 1 or len(rows[0]) != 3 or rows[0][2] != 'kB':
                raise ValueError(f'no Pss row in /proc/{current}/smaps_rollup')
            total += int(rows[0][1]) * 1024
            pending.extend(children.get(current, ()))
    return total


def _macos_tree_footprint(pid):
    """Physical footprint bytes of PID and its descendants, from libproc.

    proc_pid_rusage's ri_phys_footprint leaves out shared file-backed pages,
    so pages that several processes map (rustc's libraries) are not counted
    in each. A zombie reports zero. A process whose footprint cannot be read
    raises, which makes the use unknown.
    """
    libc = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)
    libc.proc_listchildpids.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_int]
    libc.proc_listchildpids.restype = ctypes.c_int
    libc.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
    libc.proc_pid_rusage.restype = ctypes.c_int

    class RusageInfo(ctypes.Structure):
        # struct rusage_info_v0, <sys/resource.h>
        _fields_ = [('ri_uuid', ctypes.c_uint8 * 16),
                    *[(name, ctypes.c_uint64) for name in (
                        'ri_user_time', 'ri_system_time', 'ri_pkg_idle_wkups',
                        'ri_interrupt_wkups', 'ri_pageins', 'ri_wired_size',
                        'ri_resident_size', 'ri_phys_footprint',
                        'ri_proc_start_abstime', 'ri_proc_exit_abstime')]]

    def footprint(target):
        info = RusageInfo()
        if libc.proc_pid_rusage(target, 0, ctypes.byref(info)) != 0:  # RUSAGE_INFO_V0
            raise OSError(ctypes.get_errno(), f'proc_pid_rusage failed for pid {target}')
        return info.ri_phys_footprint

    def children(target):
        capacity = libc.proc_listchildpids(target, None, 0)
        if capacity < 0:
            raise OSError(ctypes.get_errno(), 'proc_listchildpids failed')
        buffer = (ctypes.c_int * max(1, capacity))()
        count = libc.proc_listchildpids(target, buffer, ctypes.sizeof(buffer))
        if count < 0:
            raise OSError(ctypes.get_errno(), 'proc_listchildpids failed')
        return buffer[:count]

    total, pending, seen = 0, [pid], set()
    while pending:
        current = pending.pop()
        if current not in seen:
            seen.add(current)
            total += footprint(current)
            pending.extend(children(current))
    return total


def _windows_tree_working_set(pid):
    """Working sets of PID and its descendants, from the Toolhelp process walk.

    A working set counts a shared page in each process that maps it. A
    process whose working set cannot be read makes the use unknown.
    """
    import ctypes
    from ctypes import wintypes

    class COUNTERS(ctypes.Structure):
        _fields_ = [('cb', wintypes.DWORD), ('PageFaultCount', wintypes.DWORD),
                    *[(name, ctypes.c_size_t) for name in (
                        'PeakWorkingSetSize', 'WorkingSetSize', 'QuotaPeakPagedPoolUsage',
                        'QuotaPagedPoolUsage', 'QuotaPeakNonPagedPoolUsage',
                        'QuotaNonPagedPoolUsage', 'PagefileUsage', 'PeakPagefileUsage')]]

    kernel32 = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel32.GetProcessTimes.argtypes = [wintypes.HANDLE, *[ctypes.POINTER(wintypes.FILETIME)] * 4]
    kernel32.K32GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(COUNTERS), wintypes.DWORD]
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]

    def read(target):
        """(creation time, working set bytes), or None when they cannot be read."""
        handle = kernel32.OpenProcess(0x1000, False, target)  # PROCESS_QUERY_LIMITED_INFORMATION
        if not handle:
            return None
        try:
            times = [wintypes.FILETIME() for _ in range(4)]
            counters = COUNTERS(cb=ctypes.sizeof(COUNTERS))
            if (not kernel32.GetProcessTimes(handle, *[ctypes.byref(value) for value in times])
                    or not kernel32.K32GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb)):
                return None
            return times[0].dwHighDateTime << 32 | times[0].dwLowDateTime, counters.WorkingSetSize
        finally:
            kernel32.CloseHandle(handle)

    children = {}
    for child, parent in _windows_processes():
        children.setdefault(parent, []).append(child)
    root = read(pid)
    if root is None:
        return None
    total, pending, seen = 0, [(pid, root)], {pid}
    while pending:
        current, (created, size) = pending.pop()
        total += size
        for child in children.get(current, ()):
            if child in seen:
                continue
            info = read(child)
            if info is None:
                return None
            # A reused parent PID can name a process older than its "child".
            if info[0] >= created:
                seen.add(child)
                pending.append((child, info))
    return total


def process_tree_use_mib(pid, proc=Path('/proc')):
    """Memory use of PID and its descendants in MiB, or None when unknown.

    Linux sums Pss from /proc and macOS the physical footprint from libproc,
    so pages shared within the tree count once. Windows sums the working
    sets from the Toolhelp walk, which count a shared page in each process.
    None means the use cannot be read: PID or any process in its tree cannot
    be read, or the walk failed.
    """
    try:
        if sys.platform.startswith('linux'):
            total = _linux_tree_pss(pid, proc)
        elif sys.platform == 'darwin':
            total = _macos_tree_footprint(pid)
        elif sys.platform == 'win32':
            total = _windows_tree_working_set(pid)
        else:
            return None
    except (OSError, ValueError, IndexError, AttributeError):
        return None
    return None if total is None else total // (1024 * 1024)
