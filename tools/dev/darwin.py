"""Darwin libproc observations shared by host admission and startup measurement.

The module owns the ctypes layouts for rusage_info_v4 and proc_taskinfo and
exposes one observation function. A call that fails leaves its fields None;
the functions never substitute zero for an unavailable kernel value. Byte
quantities stay bytes here; the startup adapter converts sizes to KiB.

Sources: <sys/resource.h> (rusage_info_v4), <sys/proc_info.h>
(proc_taskinfo and PROC_PIDTASKINFO), and <mach/task_info.h>
(MACH_TASK_BASIC_INFO resident_size_max). resident_size_max is only readable
with a task port, so a controller without one reports it unavailable instead
of using another metric. XNU's fill_task_rusage copies ri_instructions and
ri_cycles from task_power_info_extra, which is filled only under
CONFIG_PERVASIVE_CPI; a host probe requires each counter to advance and
reports an unpopulated one unavailable rather than zero.
"""
import ctypes
import os
import time

RUSAGE_INFO_V4 = 4
PROC_PIDTASKINFO = 4
MACH_TASK_BASIC_INFO = 20

OBSERVATION_FIELDS = (
    'resident_bytes',
    'resident_peak_bytes',
    'physical_footprint_bytes',
    'physical_footprint_interval_max_bytes',
    'physical_footprint_lifetime_max_bytes',
    'faults',
    'pageins',
    'disk_bytes_read',
    'disk_bytes_written',
    'instructions',
    'cycles',
    'instructions_available',
    'cycles_available',
)


class RusageInfoV4(ctypes.Structure):
    """Represent rusage_info_v4 from <sys/resource.h>."""
    _fields_ = [
        ('ri_uuid', ctypes.c_uint8 * 16),
        *[(name, ctypes.c_uint64) for name in (
            'ri_user_time', 'ri_system_time', 'ri_pkg_idle_wkups',
            'ri_interrupt_wkups', 'ri_pageins', 'ri_wired_size',
            'ri_resident_size', 'ri_phys_footprint',
            'ri_proc_start_abstime', 'ri_proc_exit_abstime',
            'ri_child_user_time', 'ri_child_system_time',
            'ri_child_pkg_idle_wkups', 'ri_child_interrupt_wkups',
            'ri_child_pageins', 'ri_child_elapsed_abstime',
            'ri_diskio_bytesread', 'ri_diskio_byteswritten',
            'ri_cpu_time_qos_default', 'ri_cpu_time_qos_maintenance',
            'ri_cpu_time_qos_background', 'ri_cpu_time_qos_utility',
            'ri_cpu_time_qos_legacy', 'ri_cpu_time_qos_user_initiated',
            'ri_cpu_time_qos_user_interactive',
            'ri_billed_system_time', 'ri_serviced_system_time',
            'ri_logical_writes', 'ri_lifetime_max_phys_footprint',
            'ri_instructions', 'ri_cycles', 'ri_billed_energy',
            'ri_serviced_energy', 'ri_interval_max_phys_footprint',
            'ri_runnable_time')],
    ]


class TaskInfo(ctypes.Structure):
    """Represent proc_taskinfo from <sys/proc_info.h>."""
    _fields_ = [
        ('pti_virtual_size', ctypes.c_uint64),
        ('pti_resident_size', ctypes.c_uint64),
        ('pti_total_user', ctypes.c_uint64),
        ('pti_total_system', ctypes.c_uint64),
        ('pti_threads_user', ctypes.c_uint64),
        ('pti_threads_system', ctypes.c_uint64),
        ('pti_policy', ctypes.c_int32),
        ('pti_faults', ctypes.c_int32),
        ('pti_pageins', ctypes.c_int32),
        ('pti_cow_faults', ctypes.c_int32),
        ('pti_messages_sent', ctypes.c_int32),
        ('pti_messages_received', ctypes.c_int32),
        ('pti_syscalls_mach', ctypes.c_int32),
        ('pti_syscalls_unix', ctypes.c_int32),
        ('pti_csw', ctypes.c_int32),
        ('pti_threadnum', ctypes.c_int32),
        ('pti_numrunning', ctypes.c_int32),
        ('pti_priority', ctypes.c_int32),
    ]


class TimeValue(ctypes.Structure):
    """Represent Mach time_value_t, whose two fields are 32-bit integers."""
    _fields_ = [('seconds', ctypes.c_int32), ('microseconds', ctypes.c_int32)]


class MachTaskBasicInfo(ctypes.Structure):
    """Represent mach_task_basic_info from <mach/task_info.h>."""
    _fields_ = [
        ('virtual_size', ctypes.c_uint64),
        ('resident_size', ctypes.c_uint64),
        ('resident_size_max', ctypes.c_uint64),
        ('user_time', TimeValue),
        ('system_time', TimeValue),
        ('policy', ctypes.c_int32),
        ('suspend_count', ctypes.c_int32),
    ]


_libc = None
_counter_availability = None


def _system():
    """Return the configured libSystem handle, loading it once."""
    global _libc
    if _libc is None:
        libc = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)
        libc.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
        libc.proc_pid_rusage.restype = ctypes.c_int
        libc.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64,
                                      ctypes.c_void_p, ctypes.c_int]
        libc.proc_pidinfo.restype = ctypes.c_int
        libc.proc_listchildpids.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_int]
        libc.proc_listchildpids.restype = ctypes.c_int
        libc.mach_task_self.restype = ctypes.c_uint
        libc.task_for_pid.argtypes = [ctypes.c_uint, ctypes.c_int, ctypes.POINTER(ctypes.c_uint)]
        libc.task_for_pid.restype = ctypes.c_int
        libc.task_info.argtypes = [ctypes.c_uint, ctypes.c_int, ctypes.c_void_p,
                                   ctypes.POINTER(ctypes.c_uint)]
        libc.task_info.restype = ctypes.c_int
        libc.mach_port_deallocate.argtypes = [ctypes.c_uint, ctypes.c_uint]
        libc.mach_port_deallocate.restype = ctypes.c_int
        _libc = libc
    return _libc


def rusage(pid):
    """Return rusage_info_v4 for one PID, or raise OSError when unavailable."""
    info = RusageInfoV4()
    if _system().proc_pid_rusage(pid, RUSAGE_INFO_V4, ctypes.byref(info)) != 0:
        raise OSError(ctypes.get_errno(), f'proc_pid_rusage failed for pid {pid}')
    return info


def taskinfo(pid):
    """Return proc_taskinfo for one PID, or raise OSError when unavailable."""
    info = TaskInfo()
    if _system().proc_pidinfo(pid, PROC_PIDTASKINFO, 0, ctypes.byref(info),
                              ctypes.sizeof(info)) != ctypes.sizeof(info):
        raise OSError(ctypes.get_errno(), f'proc_pidinfo failed for pid {pid}')
    return info


def children(pid):
    """Return direct child PIDs from proc_listchildpids, or raise OSError."""
    libc = _system()
    capacity = libc.proc_listchildpids(pid, None, 0)
    if capacity < 0:
        raise OSError(ctypes.get_errno(), 'proc_listchildpids failed')
    buffer = (ctypes.c_int * max(1, capacity))()
    count = libc.proc_listchildpids(pid, buffer, ctypes.sizeof(buffer))
    if count < 0:
        raise OSError(ctypes.get_errno(), 'proc_listchildpids failed')
    return buffer[:count]


def footprint(pid):
    """Return one process's physical footprint in bytes, or raise OSError."""
    return int(rusage(pid).ri_phys_footprint)


def resident_peak(pid):
    """Return resident_size_max in bytes, or None when no task port is granted.

    macOS keeps this high-water mark in mach_task_basic_info, which requires
    task_for_pid. The controller normally lacks that privilege, so this returns
    None rather than substituting the physical-footprint peak.
    """
    libc = _system()
    task = ctypes.c_uint()
    if libc.task_for_pid(libc.mach_task_self(), pid, ctypes.byref(task)) != 0:
        return None
    try:
        info = MachTaskBasicInfo()
        count = ctypes.c_uint(ctypes.sizeof(info) // ctypes.sizeof(ctypes.c_int))
        if libc.task_info(task, MACH_TASK_BASIC_INFO, ctypes.byref(info),
                          ctypes.byref(count)) != 0:
            return None
        return int(info.resident_size_max)
    finally:
        libc.mach_port_deallocate(libc.mach_task_self(), task)


def _probe_counters():
    """Return whether instructions and cycles advance across a forced context switch.

    XNU's fill_task_rusage copies ri_instructions and ri_cycles from
    task_power_info_extra, and task_power_info_locked fills that extra only
    under CONFIG_PERVASIVE_CPI; otherwise the zero-initialised extra leaves
    both counters at zero. The fields carry no separate "unsupported" marker.
    A process that executes code must advance a backed counter, and Recount
    publishes instructions and cycles at a context switch, so require an
    increase across real work and a sleep. Both counters are probed
    separately; an unchanged zero is unavailable, not a valid zero.
    """
    try:
        before = rusage(os.getpid())
        sum(range(20000))
        time.sleep(0.02)
        after = rusage(os.getpid())
    except (OSError, AttributeError):
        return (False, False)
    return (int(after.ri_instructions) > int(before.ri_instructions),
            int(after.ri_cycles) > int(before.ri_cycles))


def counter_availability():
    """Return cached (instructions, cycles) availability for this host."""
    global _counter_availability
    if _counter_availability is None:
        _counter_availability = _probe_counters()
    return _counter_availability


def observation(pid):
    """Return every available Darwin observation, with None for a failed call.

    rusage_info_v4 supplies resident and footprint sizes, disk I/O, the
    footprint maxima, and the instruction/cycle counters. proc_taskinfo
    supplies the fault and page-in counts. Counter availability is probed once
    per host; a failed source leaves only its own fields unavailable, and no
    zero is manufactured for a missing value.
    """
    result = dict.fromkeys(OBSERVATION_FIELDS)
    instructions_available, cycles_available = counter_availability()
    result['instructions_available'] = False
    result['cycles_available'] = False
    try:
        usage = rusage(pid)
    except (OSError, AttributeError):
        usage = None
    if usage is not None:
        result.update(
            resident_bytes=int(usage.ri_resident_size),
            physical_footprint_bytes=int(usage.ri_phys_footprint),
            physical_footprint_interval_max_bytes=int(usage.ri_interval_max_phys_footprint),
            physical_footprint_lifetime_max_bytes=int(usage.ri_lifetime_max_phys_footprint),
            disk_bytes_read=int(usage.ri_diskio_bytesread),
            disk_bytes_written=int(usage.ri_diskio_byteswritten),
        )
        if instructions_available:
            result['instructions'] = int(usage.ri_instructions)
            result['instructions_available'] = True
        if cycles_available:
            result['cycles'] = int(usage.ri_cycles)
            result['cycles_available'] = True
    try:
        info = taskinfo(pid)
    except (OSError, AttributeError):
        info = None
    if info is not None:
        result.update(faults=int(info.pti_faults), pageins=int(info.pti_pageins))
    try:
        result['resident_peak_bytes'] = resident_peak(pid)
    except (OSError, AttributeError):
        pass
    return result
