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
HEADROOM_MIB = 2048         # left to the desktop: the browser, the editor, the page cache


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


def total_memory_mib():
    """Total physical memory in MiB, or None."""
    try:
        if sys.platform.startswith('linux'):
            for line in Path('/proc/meminfo').read_text().splitlines():
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


def resource_snapshot(proc=Path('/proc'), cgroup_root=Path('/sys/fs/cgroup')):
    """Read known limits. Report each unreadable limit as unknown."""
    notes, memories, cpus = [], [], []
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
                    current = int((directory / 'memory.current').read_text().strip())
                    if maximum < 0 or current < 0:
                        raise ValueError('negative memory')
                    room = max(0, maximum - current) // (1024 * 1024)
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
        if total is not None:
            notes.append(f'total memory={total} MiB')
    if not cpus:
        count = os.cpu_count()
        notes.append(f'CPU count={count if count is not None else "unknown; fallback 1"}')
        cpus.append(count or 1)
    return dict(available_mib=min(memories) if memories else None,
                cpus=max(1, math.floor(min(cpus))),
                cargo_cpus=max(1, math.ceil(min(cpus))), notes=notes)


def fit(available_mib, per_process_mib, ceiling):
    """Return zero when no process fits after the headroom."""
    return max(0, min(ceiling, (available_mib - HEADROOM_MIB) // per_process_mib))


READ_HOST = object()


def validate_settings(env):
    build = env.get('CARGO_BUILD_JOBS')
    if build is not None and build != 'default':
        if not re.fullmatch(r'[+-]?[0-9]+', build) or int(build) == 0 or not -(2**31) <= int(build) < 2**31:
            raise ValueError('Invalid CARGO_BUILD_JOBS: use a nonzero integer or default')
    threads = env.get('RUST_TEST_THREADS')
    if threads is not None and (not re.fullmatch(r'[+]?[0-9]+', threads) or int(threads) <= 0):
        raise ValueError('Invalid RUST_TEST_THREADS: use a positive integer')


def memory_defaults(env, explicit_jobs, cpus=None, available_mib=READ_HOST):
    """Keep caller values. Choose only unset values from current resources."""
    validate_settings(env)
    if available_mib is READ_HOST:
        resources = resource_snapshot()
        available_mib = resources['available_mib']
        cpus = cpus or resources['cpus']
    cpus = cpus or os.cpu_count() or 1
    chosen = {}
    def choose(name, explicit, per_process, ceiling, unit):
        if explicit is not None:
            chosen[name] = (explicit, 'set by the caller', True)
        elif available_mib is None:
            chosen[name] = (0, 'available memory unknown; no capacity admitted', False)
        else:
            value = fit(available_mib, per_process, ceiling)
            chosen[name] = (value, f'{available_mib} MiB available - {HEADROOM_MIB} MiB headroom, '
                                   f'{per_process} MiB per {unit}, at most {ceiling}', False)
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
    chosen = memory_defaults(env, None, cpus=resources['cpus'], available_mib=resources['available_mib'])
    chosen.pop('--jobs')
    child = dict(env, **{name: str(value[0]) for name, value in chosen.items()})
    build = child['CARGO_BUILD_JOBS']
    count = resources['cargo_cpus'] if build == 'default' else int(build)
    if count < 0:
        count = max(1, resources['cargo_cpus'] + count)
    estimate = max(RUNNER_PEAK_MIB if runner else RUSTC_PEAK_MIB, count * RUSTC_PEAK_MIB,
                   int(child['RUST_TEST_THREADS']) * TEST_THREAD_PEAK_MIB)
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


def heavy_slot_count():
    """How many heavy slots the host offers: $OXPINYIN_HEAVY_SLOTS, default 1."""
    try:
        return max(1, int(os.environ.get('OXPINYIN_HEAVY_SLOTS', '1')))
    except ValueError:
        return 1


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


def _windows_parent(pid):
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
        return None
    entry = ENTRY()
    entry.dwSize = ctypes.sizeof(ENTRY)
    try:
        more = kernel32.Process32FirstW(snapshot, ctypes.byref(entry))
        while more:
            if entry.th32ProcessID == pid:
                return int(entry.th32ParentProcessID)
            more = kernel32.Process32NextW(snapshot, ctypes.byref(entry))
    finally:
        kernel32.CloseHandle(snapshot)
    return None


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
