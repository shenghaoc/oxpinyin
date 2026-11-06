"""Cancellation and child-process supervision. Standard library only.

A shared cancellation event set by SIGTERM, SIGINT and SIGHUP, process-group
termination, and a supervised child runner. On Windows a Job Object kills the
child tree when it closes.
"""
import contextlib
import os
import signal
import subprocess
import sys
import threading
import time

from . import host

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
CANCELLED_SIGNAL = None
# Open cancellation_handlers contexts; only the outermost clears the state.
_HANDLER_DEPTH = 0


def cancellation_status():
    """The shell status of the signal recorded by cancellation_handlers."""
    return 128 + CANCELLED_SIGNAL if CANCELLED_SIGNAL is not None else 130


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
    A nested context, such as run_command's inside a runner's, keeps the
    state: a signal the outer handlers recorded still cancels the inner work.
    """
    global CANCELLED_SIGNAL, _HANDLER_DEPTH
    if not _HANDLER_DEPTH:
        CANCELLED.clear()
        CANCELLED_SIGNAL = None
    def cancel(signum, frame):
        global CANCELLED_SIGNAL
        CANCELLED_SIGNAL = signum
        CANCELLED.set()
    previous = {}
    for name in ('SIGTERM', 'SIGINT', 'SIGHUP'):
        signum = getattr(signal, name, None)
        if signum is not None:
            previous[signum] = signal.signal(signum, cancel)
    _HANDLER_DEPTH += 1
    try:
        yield
    finally:
        _HANDLER_DEPTH -= 1
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
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    except PermissionError:
        if sys.platform != 'darwin':
            raise
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        pass
    # The leader can exit while a descendant still runs.
    deadline = time.monotonic() + 5
    group_gone = False
    while True:
        try:
            os.killpg(process.pid, signal.SIGKILL)
            break
        except ProcessLookupError:
            group_gone = True
            break
        except PermissionError:
            # XNU killpg1 filters zombie group members and can return EPERM
            # until the group disappears. Retry without hiding a lasting denial.
            if sys.platform != 'darwin' or time.monotonic() >= deadline:
                raise
            time.sleep(.01)
    process.wait()
    # killpg returning only means the signal was queued. Wait for executing
    # descendants to disappear before admission can be released. A container's
    # PID 1 may retain zombies indefinitely; those have no work or descriptors.
    if not group_gone:
        deadline = time.monotonic() + 5
        while host.process_group_live(process.pid) is not False:
            if time.monotonic() >= deadline:
                raise RuntimeError(f'process group {process.pid} termination could not be proved')
            time.sleep(.01)


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


def run_command(command, env=None, *, cwd=None, stdout=None):
    """Run a child under cancellation handlers and return its raw status.

    Copy the supplied environment or os.environ. Install cancellation
    handlers, refuse to spawn when a cancellation is already pending,
    supervise the child group or job, and reap it before returning.
    Spawn and cancellation errors propagate. On every platform, cwd is the
    child's working directory and stdout redirects its standard output to a
    file or descriptor (default: inherited; never PIPE, which nothing here
    drains).
    """
    command = list(command)
    child_env = dict(os.environ if env is None else env)
    with cancellation_handlers():
        check_cancelled()
        if sys.platform == 'win32':
            import ctypes
            from ctypes import wintypes
            with WindowsJob() as job:
                process = subprocess.Popen(command, env=child_env, cwd=cwd, stdout=stdout,
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
        with subprocess.Popen(command, env=child_env, cwd=cwd, stdout=stdout,
                              start_new_session=True) as process:
            return wait_child(process)
