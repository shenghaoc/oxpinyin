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
        if sys.platform != 'win32':
            raise OSError('WindowsJob is available only on Windows')
        import ctypes
        from ctypes import wintypes

        class LARGE_INTEGER(ctypes.Structure):
            _fields_ = [('QuadPart', ctypes.c_longlong)]

        class BASIC(ctypes.Structure):
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
            _fields_ = [
                ('ReadOperationCount', ctypes.c_ulonglong),
                ('WriteOperationCount', ctypes.c_ulonglong),
                ('OtherOperationCount', ctypes.c_ulonglong),
                ('ReadTransferCount', ctypes.c_ulonglong),
                ('WriteTransferCount', ctypes.c_ulonglong),
                ('OtherTransferCount', ctypes.c_ulonglong),
            ]

        class EXTENDED(ctypes.Structure):
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
        if not self._kernel32.AssignProcessToJobObject(
                self._wintypes.HANDLE(self.handle), self._wintypes.HANDLE(process_handle)):
            raise OSError(self._ctypes.get_last_error(), 'AssignProcessToJobObject failed')

    def terminate(self, code=1):
        if self.handle:
            self._kernel32.TerminateJobObject(self._wintypes.HANDLE(self.handle), code)

    def close(self):
        if self.handle:
            self._kernel32.CloseHandle(self._wintypes.HANDLE(self.handle))
            self.handle = None

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()
        return False


CANCELLED = threading.Event()


class Cancelled(ValueError):
    pass


def check_cancelled():
    if CANCELLED.is_set():
        raise Cancelled('cancelled; child process groups terminated and reaped')


@contextlib.contextmanager
def cancellation_handlers():
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
    the wait never fails or skips the work. `waited` is the seconds spent
    waiting, for the caller's timing row. `reclaimed` names a dead holder
    whose record this acquisition replaced, if any."""

    def __init__(self, command, out=sys.stderr, poll=0.5):
        self.command, self.out, self.poll = command, out, poll
        self.waited, self.reclaimed, self.slot, self._file = 0.0, None, None, None
        self.started, self.announced = None, None
        self.inherited = False
        self._handed_off = False
        self.holder_pid, self.inherited_fd = None, None

    def _try(self, index):
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
        """Try once. Do not block a worker while the slot is busy."""
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
        holder = dict(pid=self.holder_pid, worktree=str(ROOT), command=self.command,
                      since=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()))
        host.write_holder(self._file, holder)
        if self.waited >= self.poll:
            print(f'heavy slot {self.slot} acquired after {self.waited:.1f}s wait: {self.command}',
                  file=self.out, flush=True)
        return True

    def __enter__(self):
        while not self.try_acquire():
            time.sleep(self.poll)
        return self

    def pass_fds(self):
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
        # The child shares this open file description. Never unlock it here.
        # Keep our copy until the child is reaped. Descendants keep theirs.
        self._handed_off = True

    def __exit__(self, *exc):
        if self._file is None:
            return False
        try:
            if not self._handed_off:
                self._file.seek(0)
                self._file.truncate()
                self._file.flush()
        finally:
            self._file.close()
            self._file = None
            self._handed_off = False
        return False


def other_reservations(directory, own_slot):
    """Count live holders in other locked slots. Ignore dead records."""
    reserved = 0
    for path in directory.glob(f'{HEAVY_SLOT}-*'):
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
                reserved += estimate
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
    """One budget calculation for native heavy work and container transport."""
    resources = (read or host.resource_snapshot)()
    reserved = other_reservations(directory, own_slot)
    resources['notes'] = [*resources['notes'], f'other slot reservations={reserved} MiB']
    containers = container_reservations(directory, out=out)
    resources['notes'].append(f'container reservations={containers if containers is not None else "unknown"} MiB')
    if containers is None:
        resources['available_mib'] = None
    elif resources['available_mib'] is not None:
        resources['available_mib'] = max(0, resources['available_mib'] - reserved - containers)
    return resources


class ContainerReservation:
    """Native memory only: no heavy slot, PID ownership or inner lock state."""

    def __init__(self, runtime, kind, name, memory_mib, out=sys.stderr, poll=0.5):
        self.directory = host.host_lock_dir()
        token = uuid.uuid4().hex
        self.path = self.directory / f'container-memory-{token}.json'
        self.guard = None
        self.out, self.poll = out, poll
        self.record = dict(schema=1, runtime=runtime, kind=kind, name=name, token=token,
                           memory_mib=host.container_memory_mib(str(memory_mib)), phase='creating',
                           since=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()))

    def acquire(self):
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
                    if available is not None and available - host.HEADROOM_MIB >= estimate:
                        write_container_record(self.path, self.record)
                        print(f'container reservation: {self.path}; {estimate} MiB; '
                              + '; '.join(resources['notes']), file=self.out, flush=True)
                        return self
                now = time.monotonic()
                room = 'unknown' if available is None else max(0, available - host.HEADROOM_MIB)
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
        return (self.guard.fileno(),)

    def created(self):
        with admission_lock(self.directory):
            self.record['phase'] = 'created'
            write_container_record(self.path, self.record)

    def create_failed(self):
        # Persist that the client returned normally. A crash after this write
        # can recover from a complete token listing, unlike an in-flight create.
        with admission_lock(self.directory):
            self.record['phase'] = 'create-failed'
            write_container_record(self.path, self.record)
        return self.release()

    def release(self):
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
        if self.guard is not None:
            self.guard.close()
            self.guard = None
        if not self.path.exists():
            self.path.with_suffix('.guard').unlink(missing_ok=True)


def reserve_heavy_env(slot, env, *, runner=False, resource_snapshot=None, prepare_heavy_env=None):
    """Serialize the resource read, reservation check, and holder update."""
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


class HeavyAdmission:
    """Take the slot, then read resources before starting heavy work."""

    def __init__(self, command, env, out=sys.stderr, poll=0.5, *, runner=False,
                 resource_snapshot=None, prepare_heavy_env=None):
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
        resources = self.resource_snapshot()
        _, self.provisional, self.estimate = self.prepare_heavy_env(
            self.base_env, resources, runner=self.runner)
        self.available = resources['available_mib']
        self.headroom = host.memory_headroom(self.base_env, resources)
        print('resources (provisional): ' + '; '.join(resources['notes']), file=out, flush=True)
        host.plan_defaults(self.provisional, out, 'provisional defaults')

    def _blocked(self):
        if self.available is None:
            return host.ResourceBlocked('resource-blocked: available memory unknown')
        available = max(0, self.available - self.headroom)
        inherited = 'inherited slot; ' if self.slot.inherited else ''
        return host.ResourceBlocked(f'resource-blocked: {inherited}{self.estimate} MiB needed, {available} MiB available')

    def _finish_memory_wait(self, now):
        if self.memory_since is not None:
            self.memory_wait += now - self.memory_since
            self.memory_since = None

    def try_acquire(self):
        check_cancelled()
        now = time.monotonic()
        if self.started is None:
            self.started = now
        elif now - self.started >= self.limit:
            self._finish_memory_wait(now)
            self.close()
            raise self._blocked()
        if now < self.retry_at:
            return False
        self._finish_memory_wait(now)
        if not self.slot.try_acquire():
            return False
        self.slot_wait += self.slot.waited
        try:
            resources, child, chosen, self.estimate, admitted = reserve_heavy_env(
                self.slot, self.base_env, runner=self.runner,
                resource_snapshot=self.resource_snapshot,
                prepare_heavy_env=self.prepare_heavy_env)
            self.available = resources['available_mib']
            self.headroom = host.memory_headroom(self.base_env, resources)
            if not admitted:
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
        result['slot_wait_seconds'] = round(self.slot_wait + (self.slot.waited if self.env is None else 0), 3)
        result['memory_wait_seconds'] = round(result.get('memory_wait_seconds', 0.0) + self.memory_wait, 3)

    def __enter__(self):
        try:
            while not self.try_acquire():
                remaining = max(0, self.limit - (time.monotonic() - self.started))
                time.sleep(min(self.poll, remaining))
        except BaseException:
            self.close()
            raise
        return self

    def close(self):
        self.slot.__exit__()

    def __exit__(self, *exc):
        self.close()
        return False


def run_admitted(jobs, workers, run):
    """Submit only admitted jobs. Keep the pool at the requested size."""
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
    """Run COMMAND under one heavy slot, for the tools.dev slot command."""
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
