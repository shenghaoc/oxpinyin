"""Host-wide heavy slot, admission and cancellation. Standard library only.

One implementation for every environment. The slot is a flock on a host-wide
lock file on Linux and macOS; the Windows path is specified in Layer 1 §5 and
uses LockFileEx through ctypes. The holder record is advisory: the kernel lock
is the authority, and a record naming a dead holder is reclaimed.
"""
import contextlib
import errno
try:
    import fcntl
except ImportError:
    fcntl = None
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time
import uuid

from . import host

HEAVY_SLOT = 'heavy-slot'
HEAVY_SLOT_HELD = 'OXPINYIN_HEAVY_SLOT_HELD'
HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

# fcntl is absent on Windows; these values are the POSIX constants used by the
# call sites, so the Windows branch can share the same call shape.
LOCK_EX = getattr(fcntl, 'LOCK_EX', 2)
LOCK_NB = getattr(fcntl, 'LOCK_NB', 4)
LOCKFILE_FAIL_IMMEDIATELY = 0x1
LOCKFILE_EXCLUSIVE_LOCK = 0x2
ERROR_LOCK_VIOLATION = 33
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000
JOB_OBJECT_EXTENDED_LIMIT_INFORMATION = 9
CREATE_SUSPENDED = 0x00000004


class WindowsJob:
    """Layer 1 §5 Windows Job Object: non-inheritable, kill on close.

    This path is written for Windows but is unverified on this host.
    """

    def __init__(self):
        """Create a non-inheritable Job Object configured to kill members on close.

        Raise OSError off Windows or when a required Windows API call fails.
        """
        if sys.platform != 'win32':
            raise OSError('WindowsJob is available only on Windows')
        import ctypes
        from ctypes import wintypes

        class LARGE_INTEGER(ctypes.Structure):
            """Represent a signed 64-bit value in the Windows job-limit layout."""
            _fields_ = [('QuadPart', ctypes.c_longlong)]

        class BASIC(ctypes.Structure):
            """Represent basic Job Object limits, including flags and working-set bounds."""
            _fields_ = [
                ('PerProcessUserTimeLimit', LARGE_INTEGER),
                ('PerJobUserTimeLimit', LARGE_INTEGER),
                ('LimitFlags', wintypes.DWORD),
                ('MinimumWorkingSetSize', ctypes.c_size_t),
                ('MaximumWorkingSetSize', ctypes.c_size_t),
                ('ActiveProcessLimit', wintypes.DWORD),
                ('Affinity', ctypes.c_size_t),
                ('PriorityClass', wintypes.DWORD),
                ('SchedulingClass', wintypes.DWORD),
            ]

        class IO_COUNTERS(ctypes.Structure):
            """Represent Job Object operation counts and transferred-byte counters."""
            _fields_ = [
                ('ReadOperationCount', ctypes.c_ulonglong),
                ('WriteOperationCount', ctypes.c_ulonglong),
                ('OtherOperationCount', ctypes.c_ulonglong),
                ('ReadTransferCount', ctypes.c_ulonglong),
                ('WriteTransferCount', ctypes.c_ulonglong),
                ('OtherTransferCount', ctypes.c_ulonglong),
            ]

        class EXTENDED(ctypes.Structure):
            """Represent extended Job Object limits and memory-use counters in bytes."""
            _fields_ = [
                ('BasicLimitInformation', BASIC),
                ('IoInfo', IO_COUNTERS),
                ('ProcessMemoryLimit', ctypes.c_size_t),
                ('JobMemoryLimit', ctypes.c_size_t),
                ('PeakProcessMemoryUsed', ctypes.c_size_t),
                ('PeakJobMemoryUsed', ctypes.c_size_t),
            ]

        self._ctypes = ctypes
        self._wintypes = wintypes
        self._kernel32 = ctypes.WinDLL('kernel32', use_last_error=True)
        self._kernel32.CreateJobObjectW.restype = wintypes.HANDLE
        self._kernel32.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        self._kernel32.SetHandleInformation.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.DWORD]
        self._kernel32.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int,
                                                           ctypes.c_void_p, wintypes.DWORD]
        self._kernel32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        self._kernel32.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
        self._kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
        self.handle = self._kernel32.CreateJobObjectW(None, None)
        if not self.handle:
            raise OSError(ctypes.get_last_error(), 'CreateJobObjectW failed')
        if not self._kernel32.SetHandleInformation(wintypes.HANDLE(self.handle), 1, 0):
            code = ctypes.get_last_error()
            self._kernel32.CloseHandle(wintypes.HANDLE(self.handle))
            raise OSError(code, 'SetHandleInformation(JobObject) failed')
        info = EXTENDED()
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not self._kernel32.SetInformationJobObject(
                wintypes.HANDLE(self.handle), JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                self._ctypes.byref(info), self._ctypes.sizeof(info)):
            code = ctypes.get_last_error()
            self._kernel32.CloseHandle(wintypes.HANDLE(self.handle))
            raise OSError(code, 'SetInformationJobObject failed')

    def assign(self, process_handle):
        """Add a process handle to this job; raise OSError if Windows refuses it."""
        if not self._kernel32.AssignProcessToJobObject(
                self._wintypes.HANDLE(self.handle), self._wintypes.HANDLE(process_handle)):
            raise OSError(self._ctypes.get_last_error(), 'AssignProcessToJobObject failed')

    def terminate(self, code=1):
        """Request termination of all job members with code when a handle is open."""
        if self.handle:
            self._kernel32.TerminateJobObject(self._wintypes.HANDLE(self.handle), code)

    def close(self):
        """Close this job handle once, triggering its kill-on-close policy."""
        if self.handle:
            self._kernel32.CloseHandle(self._wintypes.HANDLE(self.handle))
            self.handle = None

    def __enter__(self):
        """Return the already-created job for context-managed cleanup."""
        return self

    def __exit__(self, *exc):
        """Close the job without suppressing an exception from the context body."""
        self.close()
        return False


CANCELLED = threading.Event()


class Cancelled(ValueError):
    """Signal that a cooperative cancellation check observed a pending request."""
    pass


def check_cancelled():
    """Raise Cancelled when the shared cancellation event is set.

    This check itself does not terminate or reap processes.
    """
    if CANCELLED.is_set():
        raise Cancelled('cancelled; child process groups terminated and reaped')


@contextlib.contextmanager
def cancellation_handlers():
    """Clear cancellation state and temporarily handle termination signals.

    SIGTERM, SIGINT, and SIGHUP set the shared event where available. Restore
    previous handlers on exit; signal installation requires the main thread.
    """
    CANCELLED.clear()
    previous = {}
    for name in ('SIGTERM', 'SIGINT', 'SIGHUP'):
        signum = getattr(signal, name, None)
        if signum is not None:
            previous[signum] = signal.signal(signum, lambda *_: CANCELLED.set())
    try:
        yield
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)


def terminate_group(process):
    """Stop the group and reap its leader before the caller releases locks."""
    if sys.platform == 'win32':
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        return
    with contextlib.suppress(ProcessLookupError):
        os.killpg(process.pid, signal.SIGTERM)
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        pass
    # The leader can exit while a descendant still runs.
    with contextlib.suppress(ProcessLookupError):
        os.killpg(process.pid, signal.SIGKILL)
    process.wait()


def wait_child(process, timeout=None):
    """Wait for a child status while checking cancellation and an optional timeout.

    Timeout is in seconds. On any exception, terminate the process group and
    reap the leader before propagating the error.
    """
    deadline = None if timeout is None else time.monotonic() + timeout
    try:
        while True:
            check_cancelled()
            remaining = None if deadline is None else deadline - time.monotonic()
            if remaining is not None and remaining <= 0:
                raise subprocess.TimeoutExpired(process.args, timeout)
            try:
                rc = process.wait(timeout=min(0.1, remaining) if remaining is not None else 0.1)
                check_cancelled()
                return rc
            except subprocess.TimeoutExpired:
                continue
    except BaseException:
        terminate_group(process)
        raise


def _windows_lock(file, operation):
    """Lock one fixed byte beyond the holder record with LockFileEx."""
    import ctypes
    from ctypes import wintypes
    import msvcrt

    class OVERLAPPED(ctypes.Structure):
        """Represent the byte offset and event fields passed to LockFileEx."""
        _fields_ = [('Internal', ctypes.c_size_t), ('InternalHigh', ctypes.c_size_t),
                    ('Offset', wintypes.DWORD), ('OffsetHigh', wintypes.DWORD),
                    ('hEvent', wintypes.HANDLE)]

    kernel32 = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel32.LockFileEx.restype = wintypes.BOOL
    kernel32.LockFileEx.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.DWORD,
                                    wintypes.DWORD, wintypes.DWORD,
                                    ctypes.POINTER(OVERLAPPED)]
    handle = msvcrt.get_osfhandle(file.fileno())
    flags = LOCKFILE_EXCLUSIVE_LOCK
    if operation & LOCK_NB:
        flags |= LOCKFILE_FAIL_IMMEDIATELY
    overlapped = OVERLAPPED()
    overlapped.Offset = 0xFFFFFFFF
    overlapped.OffsetHigh = 0
    if not kernel32.LockFileEx(wintypes.HANDLE(handle), flags, 0, 1, 0,
                               ctypes.byref(overlapped)):
        code = ctypes.get_last_error()
        if code == ERROR_LOCK_VIOLATION:
            raise BlockingIOError(code, 'the slot is held', file.name)
        raise OSError(code, 'LockFileEx failed', file.name)


def lock_file(file, operation):
    """Retry interrupted locks. Keep other errors and the lock path."""
    if sys.platform == 'win32':
        _windows_lock(file, operation)
        return
    while True:
        try:
            fcntl.flock(file, operation)
            return
        except InterruptedError:
            continue
        except BlockingIOError:
            raise
        except OSError as exc:
            if exc.errno == errno.EINTR:
                continue
            raise OSError(exc.errno, exc.strerror, file.name) from exc


@contextlib.contextmanager
def admission_lock(directory):
    """Hold the directory's heavy-admission lock until the context exits.

    Open or create the file and block for exclusive ownership; closing the
    private descriptor releases it even if the body raises.
    """
    with (directory / 'heavy-admission').open('a+') as file:
        lock_file(file, LOCK_EX)
        yield
        # Closing this unshared descriptor releases the admission lock.


def inherited_slot_marker(directory, out=sys.stderr):
    """Validate the existing marker against this domain and a live ancestor holder."""
    marker = os.environ.get(HEAVY_SLOT_HELD)
    if marker is None:
        return None
    try:
        held = json.loads(marker)
        if (not isinstance(held, dict) or not {'lock_dir', 'slot', 'pid'} <= set(held)
                or set(held) - {'lock_dir', 'slot', 'pid', 'fd'}
                or not isinstance(held['lock_dir'], str)
                or not Path(held['lock_dir']).is_absolute()
                or type(held['slot']) is not int or held['slot'] < 0
                or type(held['pid']) is not int or held['pid'] <= 0
                or ('fd' in held and (type(held['fd']) is not int or held['fd'] < 0))):
            raise ValueError('invalid fields')
        marked_directory = Path(held['lock_dir']).absolute()
    except (ValueError, OSError, RuntimeError) as exc:
        raise ValueError(f'Invalid {HEAVY_SLOT_HELD}: expected a JSON object with '
                         'an absolute lock_dir, a nonnegative integer slot, and a positive holder pid') from exc
    if marked_directory == directory.resolve():
        holder = host.read_holder(marked_directory / f"{HEAVY_SLOT}-{held['slot']}")
        if (isinstance(holder, dict) and holder.get('pid') == held['pid']
                and host.pid_alive(held['pid']) and host.is_ancestor(held['pid'])):
            return held
        print(f'stale {HEAVY_SLOT_HELD}: pid {held["pid"]}, slot {held["slot"]}; '
              'acquire a slot normally', file=out, flush=True)
    else:
        print(f'stale {HEAVY_SLOT_HELD}: different lock directory; acquire a slot normally',
              file=out, flush=True)
    return None


class HeavySlot:
    """Hold one host-wide heavy slot for the duration of a `with` block.

    Acquisition tries every slot without blocking, then waits, polling, and
    prints who holds the slots once at the start of the wait and every 30 s;
    the bare wait has no deadline, but cancellation and lock errors raise.
    `waited` is the seconds spent
    waiting, for the caller's timing row. `reclaimed` names a dead holder
    whose record this acquisition replaced, if any. A step that ends with
    nothing holding the slot clears its record, handed off or not, so a record
    that outlives its holder's process means the holder was killed or left a
    descendant holding the slot."""

    def __init__(self, command, out=sys.stderr, poll=0.5):
        """Initialize acquisition state without taking a slot; poll is in seconds."""
        self.command, self.out, self.poll = command, out, poll
        self.waited, self.reclaimed, self.slot, self._file = 0.0, None, None, None
        self.started, self.announced = None, None
        self.inherited = False
        self._handed_off = False
        self.holder_pid, self.inherited_fd = None, None
        self.token = None

    def _try(self, index):
        """Open and nonblockingly lock a slot file, returning None when it is busy.

        Return the open locked file on success; close it before propagating errors.
        """
        file = (self.lock_dir / f'{HEAVY_SLOT}-{index}').open('a+')
        try:
            lock_file(file, LOCK_EX | LOCK_NB)
        except BlockingIOError:
            file.close()
            return None
        except BaseException:
            file.close()
            raise
        return file

    def _inherit(self):
        """Adopt a validated ancestor slot marker and report the inherited holder.

        Resolve/create the host lock directory. Return False when no valid
        marker remains; retain an optional descriptor only if it names the slot file.
        """
        self.lock_dir = host.host_lock_dir().absolute()
        held = inherited_slot_marker(self.lock_dir, self.out)
        if held is None:
            return False
        self.slot, self.holder_pid = held['slot'], held['pid']
        self.inherited = True
        fd = held.get('fd')
        if fd is not None:
            path = self.lock_dir / f'{HEAVY_SLOT}-{self.slot}'
            try:
                if os.path.samestat(os.fstat(fd), path.stat()):
                    self.inherited_fd = fd
            except OSError:
                pass
        print(f'heavy slot {self.slot} inherited from ancestor pid {self.holder_pid}: {self.command}',
              file=self.out, flush=True)
        return True

    def try_acquire(self):
        """Try to acquire or inherit a slot once without waiting for a busy lock.

        Return True when held and False when all slots are busy. Record the
        holder, report reclaimed dead records, and update waited in seconds;
        cancellation and lock errors propagate.
        """
        check_cancelled()
        if self._file is not None or self.inherited:
            return True
        if self.started is None:
            self.started = time.monotonic()
            if self._inherit():
                return True
        slots = host.heavy_slot_count()
        for index in range(slots):
            file = self._try(index)
            if file is not None:
                self.slot, self._file = index, file
                break
        self.waited = time.monotonic() - self.started
        if self._file is None:
            now = time.monotonic()
            if self.announced is None or now - self.announced >= 30:
                holders = [host.describe_holder(host.read_holder(self.lock_dir / f'{HEAVY_SLOT}-{i}')) for i in range(slots)]
                print(f'waiting for a heavy slot ({slots} on this host) for: {self.command}; held by: '
                      + '; '.join(holders), file=self.out, flush=True)
                self.announced = now
            return False
        path = Path(self._file.name)
        previous = host.read_holder(path)
        if isinstance(previous, dict) and previous.get('pid') and not host.pid_alive(int(previous['pid'])):
            self.reclaimed = previous
            print(f'reclaimed stale heavy slot {self.slot}: {host.describe_holder(previous)} (pid dead)',
                  file=self.out, flush=True)
        self.holder_pid = os.getpid()
        self.token = uuid.uuid4().hex
        holder = dict(pid=self.holder_pid, worktree=str(ROOT), command=self.command,
                      since=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()), token=self.token)
        host.write_holder(self._file, holder)
        if self.waited >= self.poll:
            print(f'heavy slot {self.slot} acquired after {self.waited:.1f}s wait: {self.command}',
                  file=self.out, flush=True)
        return True

    def __enter__(self):
        """Wait until a slot is acquired or inherited and return this holder.

        Check cancellation on each attempt; this bare slot wait has no deadline.
        """
        while not self.try_acquire():
            time.sleep(self.poll)
        return self

    def pass_fds(self):
        """Mark the held descriptor inheritable and return it for subprocess pass_fds.

        Return an empty tuple when an inherited marker has no usable descriptor;
        a locally acquired slot must already be held.
        """
        fd = self.inherited_fd if self.inherited else self._file.fileno()
        if fd is None:
            return ()
        os.set_inheritable(fd, True)
        return (fd,)

    def child_env(self, env):
        """Copy the environment and mark the slot for this child."""
        marker = dict(lock_dir=str(self.lock_dir.resolve()), slot=self.slot, pid=self.holder_pid)
        descriptors = self.pass_fds()
        if descriptors:
            marker['fd'] = descriptors[0]
        return dict(env, OXPINYIN_LOCK_DIR=str(self.lock_dir), **{HEAVY_SLOT_HELD: json.dumps(marker)})

    def handoff(self):
        """Mark the lock description as shared so cleanup preserves descendant ownership."""
        # The child shares this open file description. Never unlock it here.
        # Keep our copy until the child is reaped. Descendants keep theirs.
        self._handed_off = True

    def holders(self):
        """Each slot's recorded holder as (index, record), for the slots this host offers."""
        return [(index, host.read_holder(self.lock_dir / f'{HEAVY_SLOT}-{index}'))
                for index in range(host.heavy_slot_count())]

    def _clear_unheld(self, index, token):
        """After a hand-off, clear this holder's record if nothing holds the slot any more.

        The workload and its descendants share the open file description, so the
        lock outlives this process while any of them runs, and the record keeps
        naming the holder then. When this probe takes the lock none is left: the
        step ended normally, and its record must not read as a crashed holder's
        to the next acquisition. Another holder can take the slot after our
        descriptor closed and before the probe, write its record and die without
        releasing; the probe then holds a free lock over that holder's record. It
        clears a record only if it carries the token this holder wrote at
        acquire, which every rewrite of the record keeps. Any other record is
        left, so that holder is still reclaimed and reported. A holder killed
        before it got here never runs this, and a failed probe leaves the
        record, as a hand-off always did."""
        with contextlib.suppress(OSError):
            probe = self._try(index)
            if probe is not None:
                with probe:
                    record = host.read_holder(Path(probe.name))
                    if token is not None and isinstance(record, dict) and record.get('token') == token:
                        probe.seek(0)
                        probe.truncate()
                        probe.flush()

    def __exit__(self, *exc):
        """Close this holder's descriptor without explicitly unlocking shared ownership.

        Clear an unshared record immediately; after handoff, clear only this
        token's record if the slot is now unheld. Never suppress body exceptions.
        """
        if self._file is None:
            return False
        handed_off, index, token = self._handed_off, self.slot, self.token
        try:
            if not handed_off:
                self._file.seek(0)
                self._file.truncate()
                self._file.flush()
        finally:
            self._file.close()
            self._file = None
            self._handed_off = False
        if handed_off:
            self._clear_unheld(index, token)
        return False


def other_reservations(directory, own_slot, notes=None):
    """Charge live holders in other locked slots. Ignore dead records.

    Live memory already excludes what a holder's process tree uses, so a
    holder is charged only the rest of its estimate: max(0, estimate - use).
    A use that cannot be read is charged the full estimate.
    """
    reserved = 0
    for path in sorted(directory.glob(f'{HEAVY_SLOT}-*')):
        suffix = path.name.removeprefix(HEAVY_SLOT + '-')
        if not suffix.isdigit() or int(suffix) == own_slot:
            continue
        holder = host.read_holder(path)
        if not isinstance(holder, dict):
            continue
        pid, estimate = holder.get('pid'), holder.get('estimate_mib')
        if type(pid) is not int or pid <= 0 or not host.pid_alive(pid):
            continue
        if type(estimate) is not int or estimate <= 0:
            continue
        with path.open('a+') as probe:
            try:
                lock_file(probe, LOCK_EX | LOCK_NB)
            except BlockingIOError:
                use = host.process_tree_use_mib(pid)
                charge = estimate if use is None else max(0, estimate - use)
                reserved += charge
                if notes is not None:
                    notes.append(f'slot {suffix} pid {pid}: estimate {estimate} MiB, process tree use '
                                 f'{"unknown" if use is None else f"{use} MiB"}, charge {charge} MiB')
            # Close only this probe. Never unlock a shared description.
    return reserved


def write_container_record(path, record):
    """Atomic replacement, always called with heavy-admission held."""
    temporary = path.with_suffix('.tmp')
    with temporary.open('w') as file:
        json.dump(record, file)
        file.flush()
        os.fsync(file.fileno())
    temporary.replace(path)


def container_recovery_state(record):
    """Reconcile runtime state under admission, with no other launch client."""
    if record['phase'] == 'create-failed' and host.container_token_absent(record) is True:
        return 'gone'
    state = host.container_liveness(record)
    if state == 'created':
        # A token-labeled, never-started object settles even an interrupted
        # create. Delete it before freeing the budget. Guard holders prevent
        # this path from racing a client that could still issue start.
        if not host.remove_created_container(record):
            return 'unknown'
        if record['phase'] == 'create-failed':
            return 'gone' if host.container_token_absent(record) is True else 'unknown'
        return 'gone'
    if record['phase'] == 'creating' and state == 'gone':
        return 'creation outcome unknown'
    if record['phase'] == 'create-failed' and state == 'gone':
        # Name absence alone cannot exclude a differently named token holder.
        return 'unknown'
    return state


def container_reservations(directory, out=sys.stderr):
    """Count host budgets, recovering only settled, confirmed-gone containers.

    Caller holds heavy-admission. The per-record fence is inherited by runtime
    create/start clients. A dead launcher alone proves nothing about a container
    or a request still in flight. An interrupted create remains reserved even
    if its name is absent: a daemon may still complete that request later.
    """
    reserved = 0
    for path in directory.glob('container-memory-*.json'):
        record = host.read_holder(path)
        if (not isinstance(record, dict) or record.get('schema') != 1
                or type(record.get('memory_mib')) is not int or record['memory_mib'] <= 0
                or record.get('phase') not in ('creating', 'created', 'create-failed')
                or record.get('kind') not in ('docker', 'podman', 'container')
                or any(not isinstance(record.get(key), str) or not record[key]
                       for key in ('runtime', 'name', 'token'))
                or not Path(record['runtime']).is_absolute()):
            print(f'container reservation invalid: {path}; available memory unknown', file=out, flush=True)
            return None
        guard = path.with_suffix('.guard')
        with guard.open('a+') as file:
            try:
                lock_file(file, LOCK_EX | LOCK_NB)
            except BlockingIOError:
                state = 'launch/attached client active'
            else:
                state = container_recovery_state(record)
                if state == 'gone':
                    path.unlink()
                    guard.unlink()
                    print(f'recovered confirmed-gone container reservation: {record["name"]}',
                          file=out, flush=True)
                    continue
            reserved += record['memory_mib']
            if state not in ('live', 'launch/attached client active'):
                print(f'container reservation retained: {record["name"]}; '
                      f'liveness unknown ({state}); {record["memory_mib"]} MiB', file=out, flush=True)
    return reserved


def budget_resources(directory, own_slot=None, *, read=None, out=sys.stderr):
    """Read host resources and subtract other slot and container reservations in MiB.

    Return the resource mapping with diagnostic notes and reservation totals;
    invalid container records make available_mib unknown. The caller must
    hold heavy-admission because container recovery can remove records.
    """
    # Read the holders' use before live memory: growth between the two reads
    # is then counted in both, never credited.
    holders = []
    reserved = other_reservations(directory, own_slot, holders)
    resources = (read or host.resource_snapshot)()
    resources['notes'] = [*resources['notes'], *holders, f'other slot reservations={reserved} MiB']
    containers = container_reservations(directory, out=out)
    resources['notes'].append(f'container reservations={containers if containers is not None else "unknown"} MiB')
    # Kept beside the net figure so a refusal can print what the decision subtracted.
    resources.update(slot_reserved_mib=reserved, container_reserved_mib=containers)
    if containers is None:
        resources['available_mib'] = None
    elif resources['available_mib'] is not None:
        resources['available_mib'] = max(0, resources['available_mib'] - reserved - containers)
    return resources


class ContainerReservation:
    """Reserve a container memory cap in the native host budget.

    Keep a persistent record and an inherited creation guard until runtime
    exit is confirmed; this reservation does not take a heavy slot.
    """

    def __init__(self, runtime, kind, name, memory_mib, out=sys.stderr, poll=0.5):
        """Prepare a uniquely named reservation and validate its memory cap in MiB.

        Create the host lock directory but do not yet acquire the guard or budget.
        """
        self.directory = host.host_lock_dir()
        token = uuid.uuid4().hex
        self.path = self.directory / f'container-memory-{token}.json'
        self.guard = None
        self.out, self.poll = out, poll
        self.record = dict(schema=1, runtime=runtime, kind=kind, name=name, token=token,
                           memory_mib=host.container_memory_mib(str(memory_mib)), phase='creating',
                           since=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()))

    def acquire(self):
        """Hold the creation guard and wait for enough native memory to reserve.

        Serialize each budget decision with heavy-admission, then write the
        creating record and return self. Raise ResourceBlocked on wait expiry;
        close the guard on any failure. The native heavy-slot count is unchanged.
        """
        started, announced = time.monotonic(), None
        limit = host.resource_wait_limit(os.environ)
        self.guard = self.path.with_suffix('.guard').open('a+')
        lock_file(self.guard, LOCK_EX)
        try:
            while True:
                check_cancelled()
                with admission_lock(self.directory):
                    held = inherited_slot_marker(self.directory, self.out)
                    own_slot = held['slot'] if held is not None else None
                    resources = budget_resources(self.directory, own_slot, out=self.out)
                    available = resources['available_mib']
                    estimate = self.record['memory_mib']
                    headroom = host.desktop_headroom_mib(resources.get('total_mib'))
                    if available is not None and available - headroom >= estimate:
                        write_container_record(self.path, self.record)
                        print(f'container reservation: {self.path}; {estimate} MiB; '
                              + '; '.join(resources['notes']), file=self.out, flush=True)
                        return self
                now = time.monotonic()
                room = 'unknown' if available is None else max(0, available - headroom)
                if announced is None or now - announced >= 30:
                    print(f'waiting for memory: estimate {estimate} MiB, {room} MiB available '
                          'after headroom; no process started: container', file=self.out, flush=True)
                    announced = now
                if now - started >= limit:
                    reason = ('available memory unknown' if available is None else
                              f'{estimate} MiB needed, {room} MiB available')
                    raise host.ResourceBlocked(f'resource-blocked: {reason}')
                time.sleep(min(self.poll, max(0, limit - (now - started))))
        except BaseException:
            self.close()
            raise

    def pass_fds(self):
        """Return the acquired guard descriptor for runtime clients to inherit."""
        return (self.guard.fileno(),)

    def created(self):
        """Persist the settled created phase while holding the admission lock."""
        with admission_lock(self.directory):
            self.record['phase'] = 'created'
            write_container_record(self.path, self.record)

    def create_failed(self):
        """Record a completed failed-create request, then attempt confirmed release.

        Return whether recovery proved the reservation could be removed.
        """
        # Persist that the client returned normally. A crash after this write
        # can recover from a complete token listing, unlike an in-flight create.
        with admission_lock(self.directory):
            self.record['phase'] = 'create-failed'
            write_container_record(self.path, self.record)
        return self.release()

    def release(self):
        """Remove reservation and guard paths only after recovery confirms exit.

        Hold the admission lock, report the outcome, and return False while
        runtime state remains uncertain. The open guard is closed separately.
        """
        with admission_lock(self.directory):
            if container_recovery_state(self.record) != 'gone':
                print(f'container reservation retained: {self.record["name"]}; exit unconfirmed',
                      file=self.out, flush=True)
                return False
            self.path.unlink(missing_ok=True)
            self.path.with_suffix('.guard').unlink(missing_ok=True)
            print(f'container reservation released after confirmed exit: {self.record["name"]}',
                  file=self.out, flush=True)
            return True

    def close(self):
        """Close this launcher's guard without deleting an outstanding reservation.

        Remove the guard pathname only when no reservation record remains.
        """
        if self.guard is not None:
            self.guard.close()
            self.guard = None
        if not self.path.exists():
            self.path.with_suffix('.guard').unlink(missing_ok=True)


def reserve_heavy_env(slot, env, *, runner=False, resource_snapshot=None, prepare_heavy_env=None):
    """Serialize the resource read, reservation check, and holder estimate update.

    Return resources, child environment, chosen budgets, estimate in MiB,
    and the admission boolean. Update the holder only when admitted; an
    inherited holder retains the larger of its previous and new estimates.
    """
    read = resource_snapshot or host.resource_snapshot
    prepare = prepare_heavy_env or host.prepare_heavy_env
    with admission_lock(slot.lock_dir):
        resources = budget_resources(slot.lock_dir, slot.slot, read=read)
        child, chosen, estimate = prepare(env, resources, runner=runner)
        headroom = host.memory_headroom(env, resources)
        admitted = resources['available_mib'] is not None and resources['available_mib'] - headroom >= estimate
        if admitted:
            path = slot.lock_dir / f'{HEAVY_SLOT}-{slot.slot}'
            with path.open('r+') as record:
                holder = json.load(record)
                previous = holder.get('estimate_mib', 0)
                if type(previous) is not int or previous < 0:
                    previous = 0
                holder['estimate_mib'] = max(previous, estimate) if slot.inherited else estimate
                host.write_holder(record, holder)
        return resources, child, chosen, estimate, admitted


class SlotWaitExpired(host.ResourceBlocked):
    """The wait limit passed with every heavy slot held: a slot wait, not a memory refusal."""


def _mib(value):
    """Format a memory quantity with MiB, or unknown when the value is None."""
    return 'unknown' if value is None else f'{value} MiB'


class HeavyAdmission:
    """Take the slot, then read resources before starting heavy work.

    One wait limit covers both waits. When it passes, the error names what
    blocked the last attempt: a held slot (SlotWaitExpired), or the memory
    decision that attempt made (ResourceBlocked, with that decision's figures)."""

    def __init__(self, command, env, out=sys.stderr, poll=0.5, *, runner=False,
                 resource_snapshot=None, prepare_heavy_env=None):
        """Validate settings and print a provisional resource plan without taking a slot.

        Copy env, read resources, and initialize the combined wait limit in seconds;
        optional reader/preparer callbacks supply resource and budget policy.
        """
        self.resource_snapshot = resource_snapshot or host.resource_snapshot
        self.prepare_heavy_env = prepare_heavy_env or host.prepare_heavy_env
        host.validate_settings(env)
        self.command, self.out, self.poll = command, out, poll
        self.runner = runner
        self.base_env, self.env = env.copy(), None
        self.limit = host.resource_wait_limit(env)
        self.slot = HeavySlot(command, out=out, poll=poll)
        self.started = None
        self.slot_wait = self.memory_wait = 0.0
        self.memory_since = None
        self.retry_at = 0.0
        self.announced = None
        # What the last attempt met: 'slot' (held) or 'memory' (refused). The
        # figures below describe the last memory decision, and only that.
        self.blocked_by = None
        self.reservations = (None, None)
        resources = self.resource_snapshot()
        _, self.provisional, self.estimate = self.prepare_heavy_env(
            self.base_env, resources, runner=self.runner)
        self.available = resources['available_mib']
        self.headroom = host.memory_headroom(self.base_env, resources)
        print('resources (provisional): ' + '; '.join(resources['notes']), file=out, flush=True)
        host.plan_defaults(self.provisional, out, 'provisional defaults')

    def _blocked(self):
        """The memory refusal, from the figures of the decision that refused."""
        slots, containers = self.reservations
        print(f'memory refused: available {_mib(self.available)} after {_mib(slots)} for other slots '
              f'and {_mib(containers)} for containers; headroom {self.headroom} MiB; '
              f'estimate {self.estimate} MiB; no process started: {self.command}',
              file=self.out, flush=True)
        if self.available is None:
            return host.ResourceBlocked('resource-blocked: available memory unknown')
        available = max(0, self.available - self.headroom)
        inherited = 'inherited slot; ' if self.slot.inherited else ''
        return host.ResourceBlocked(f'resource-blocked: {inherited}{self.estimate} MiB needed, {available} MiB available')

    def _slot_wait_expired(self, now):
        """The slot wait that ran out the limit: who holds the slots, no memory figure."""
        held = '; '.join(f'slot {index}: {host.describe_holder(record)}'
                         for index, record in self.slot.holders())
        return SlotWaitExpired(f'slot-wait-expired: waited {now - self.started:.1f}s for a heavy slot '
                               f'(limit {self.limit:g}s); no process started; held by: {held}')

    def _expired(self, now):
        """Build the error for the last refusal, distinguishing slot and memory waits."""
        # Before the first memory decision the figures are the provisional reading,
        # which no decision used: a held slot must not be reported as a memory refusal.
        if self.blocked_by == 'slot':
            return self._slot_wait_expired(now)
        return self._blocked()

    def _finish_memory_wait(self, now):
        """Accumulate an active memory-wait interval in seconds and clear its start."""
        if self.memory_since is not None:
            self.memory_wait += now - self.memory_since
            self.memory_since = None

    def try_acquire(self):
        """Attempt slot acquisition and a serialized memory reservation without sleeping.

        Return True with the admitted child environment, or False while waiting.
        Release a newly acquired slot on memory refusal; inherited slots fail
        immediately. Expiry raises the last refusal, and cancellation propagates.
        """
        check_cancelled()
        now = time.monotonic()
        if self.started is None:
            self.started = now
        elif now - self.started >= self.limit:
            self._finish_memory_wait(now)
            self.close()
            raise self._expired(now)
        if now < self.retry_at:
            return False
        self._finish_memory_wait(now)
        if not self.slot.try_acquire():
            self.blocked_by = 'slot'
            return False
        self.slot_wait += self.slot.waited
        try:
            resources, child, chosen, self.estimate, admitted = reserve_heavy_env(
                self.slot, self.base_env, runner=self.runner,
                resource_snapshot=self.resource_snapshot,
                prepare_heavy_env=self.prepare_heavy_env)
            self.available = resources['available_mib']
            self.headroom = host.memory_headroom(self.base_env, resources)
            self.reservations = (resources.get('slot_reserved_mib'), resources.get('container_reserved_mib'))
            if not admitted:
                self.blocked_by = 'memory'
                if self.slot.inherited:
                    raise self._blocked()
                self.slot.__exit__()
                if not self.slot.inherited:
                    self.slot = HeavySlot(self.command, out=self.out, poll=self.poll)
                now = time.monotonic()
                self.memory_since, self.retry_at = now, now + self.poll
                if self.announced is None or now - self.announced >= 30:
                    print('resources: ' + '; '.join(resources['notes']), file=self.out, flush=True)
                    available = 'unknown' if self.available is None else max(0, self.available - self.headroom)
                    print(f'waiting for memory: estimate {self.estimate} MiB, '
                          f'{available} MiB available after headroom; '
                          f'no process started: {self.command}', file=self.out, flush=True)
                    self.announced = now
                if now - self.started >= self.limit:
                    raise self._blocked()
                return False
            print('resources (admitted): ' + '; '.join(resources['notes']), file=self.out, flush=True)
            before = {key: value[0] for key, value in self.provisional.items()}
            after = {key: value[0] for key, value in chosen.items()}
            if before != after:
                print(f'defaults changed after admission: {before} -> {after}', file=self.out, flush=True)
            host.plan_defaults(chosen, self.out, 'effective defaults')
            self.env = self.slot.child_env(child)
            return True
        except BaseException:
            self.close()
            raise

    def record(self, result):
        """Add rounded slot and memory wait seconds to a result mapping in place."""
        result['slot_wait_seconds'] = round(self.slot_wait + (self.slot.waited if self.env is None else 0), 3)
        result['memory_wait_seconds'] = round(result.get('memory_wait_seconds', 0.0) + self.memory_wait, 3)

    def __enter__(self):
        """Retry admission up to the shared wait limit and return the admitted context.

        Release local slot ownership before propagating any failure.
        """
        try:
            while not self.try_acquire():
                remaining = max(0, self.limit - (time.monotonic() - self.started))
                time.sleep(min(self.poll, remaining))
        except BaseException:
            self.close()
            raise
        return self

    def close(self):
        """Release this context's local slot ownership, preserving inherited ownership."""
        self.slot.__exit__()

    def __exit__(self, *exc):
        """Release local slot ownership without suppressing the body's exception."""
        self.close()
        return False


def run_admitted(jobs, workers, run):
    """Run admitted jobs in a fixed-size thread pool and return result mappings.

    A job supplies admission, result, and the data consumed by run(job).
    Admission failures become FAIL rows, while light jobs may bypass waiting
    heavy jobs. Record waits and close admissions after completion or errors;
    worker exceptions propagate.
    """
    import concurrent.futures
    pending, active, results = list(jobs), {}, []
    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
            while pending or active:
                check_cancelled()
                for future in list(active):
                    if future.done():
                        job = active.pop(future)
                        try:
                            result = future.result()
                            if job['admission']:
                                job['admission'].record(result)
                            results.append(result)
                        finally:
                            if job['admission']:
                                job['admission'].close()
                for job in list(pending):
                    if len(active) >= workers:
                        break
                    admission = job['admission']
                    try:
                        ready = admission is None or admission.try_acquire()
                    except (OSError, ValueError) as exc:
                        job['result'].update(status='FAIL', reason=str(exc))
                        admission.record(job['result'])
                        admission.close()
                        results.append(job['result'])
                        pending.remove(job)
                        continue
                    if ready:
                        pending.remove(job)
                        active[pool.submit(run, job)] = job
                if active:
                    concurrent.futures.wait(active, timeout=0.05,
                                            return_when=concurrent.futures.FIRST_COMPLETED)
                elif pending:
                    time.sleep(0.05)
    finally:
        for job in jobs:
            if job['admission']:
                job['admission'].close()
    return results


def run_command(command, env=None, out=sys.stderr, poll=0.5, *,
                prepare_heavy_env=None, on_admitted=None):
    """Run a child after heavy-slot and memory admission, returning its raw status.

    Copy the supplied environment or os.environ; invoke on_admitted with
    final settings before spawning. Install cancellation handlers, supervise
    the child group/job, and reap it before releasing local slot ownership.
    Admission, spawn, and cancellation errors propagate.
    """
    command = list(command)
    with cancellation_handlers():
        with HeavyAdmission(' '.join(command), dict(os.environ if env is None else env), out=out,
                            poll=poll, prepare_heavy_env=prepare_heavy_env) as admission:
            if on_admitted is not None:
                on_admitted(admission.env)
            if sys.platform == 'win32':
                import ctypes
                from ctypes import wintypes
                with WindowsJob() as job:
                    process = subprocess.Popen(command, env=admission.env,
                                               creationflags=CREATE_SUSPENDED)
                    try:
                        job.assign(process._handle)
                        ntdll = ctypes.WinDLL('ntdll', use_last_error=True)
                        ntdll.NtResumeProcess.restype = ctypes.c_long
                        ntdll.NtResumeProcess.argtypes = [wintypes.HANDLE]
                        if ntdll.NtResumeProcess(wintypes.HANDLE(process._handle)) != 0:
                            raise OSError(ctypes.get_last_error(), 'NtResumeProcess failed')
                        return wait_child(process)
                    except BaseException:
                        job.terminate()
                        process.kill()
                        process.wait()
                        raise
            with subprocess.Popen(command, env=admission.env, start_new_session=True,
                                  pass_fds=admission.slot.pass_fds()) as process:
                admission.slot.handoff()
                return wait_child(process)
