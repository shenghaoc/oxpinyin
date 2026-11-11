"""Cancellation and child-process supervision. Standard library only.

A shared cancellation event set by SIGTERM, SIGINT and SIGHUP, process-group
termination, and a supervised child runner. On Windows every supervised child
runs in a Job Object that ends the child tree when it closes or is terminated.
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
JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION = 1
JOB_OBJECT_BASIC_PROCESS_ID_LIST = 3
SYNCHRONIZE = 0x00100000
ERROR_MORE_DATA = 234
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

        class ACCOUNTING(ctypes.Structure):
            """Represent Job Object accounting; ActiveProcesses counts the members still running."""
            _fields_ = [
                ('TotalUserTime', LARGE_INTEGER),
                ('TotalKernelTime', LARGE_INTEGER),
                ('ThisPeriodTotalUserTime', LARGE_INTEGER),
                ('ThisPeriodTotalKernelTime', LARGE_INTEGER),
                ('TotalPageFaultCount', wintypes.DWORD),
                ('TotalProcesses', wintypes.DWORD),
                ('ActiveProcesses', wintypes.DWORD),
                ('TotalTerminatedProcesses', wintypes.DWORD),
            ]

        self._accounting = ACCOUNTING
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
        self._kernel32.QueryInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p,
                                                             wintypes.DWORD, ctypes.c_void_p]
        self._kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
        self._kernel32.OpenProcess.restype = wintypes.HANDLE
        self._kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self._kernel32.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
        self._members = []
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

    def _remember_members(self):
        """Open a handle on every process in the job now, so wait_empty can wait for each one to end.

        A handle keeps its process object, so no other process can take over its id meanwhile.
        """
        ctypes, wintypes = self._ctypes, self._wintypes
        count = 64
        while True:
            class ID_LIST(ctypes.Structure):
                """Represent the ids of the processes in a job."""
                _fields_ = [('NumberOfAssignedProcesses', wintypes.DWORD),
                            ('NumberOfProcessIdsInList', wintypes.DWORD),
                            ('ProcessIdList', ctypes.c_size_t * count)]
            info = ID_LIST()
            if self._kernel32.QueryInformationJobObject(
                    wintypes.HANDLE(self.handle), JOB_OBJECT_BASIC_PROCESS_ID_LIST,
                    ctypes.byref(info), ctypes.sizeof(info), None):
                break
            if ctypes.get_last_error() != ERROR_MORE_DATA or info.NumberOfAssignedProcesses <= count:
                return
            count = info.NumberOfAssignedProcesses + 16
        for index in range(info.NumberOfProcessIdsInList):
            handle = self._kernel32.OpenProcess(SYNCHRONIZE, False, info.ProcessIdList[index])
            if handle:    # none: the process has ended already
                self._members.append(handle)

    def terminate(self, code=1):
        """Request termination of all job members with code when a handle is open.

        The members are noted first: wait_empty waits for each of them to end.
        """
        if self.handle:
            self._remember_members()
            self._kernel32.TerminateJobObject(self._wintypes.HANDLE(self.handle), code)

    def wait_empty(self, timeout):
        """Wait up to timeout seconds for every job member to end; return whether none is left.

        TerminateJobObject returns before the members are gone, and a terminated
        process already reports its exit code to Popen. A member is gone when the job
        counts no active process and the process object of each member that terminate
        noted is signalled: until then a member can still hold its working directory.
        """
        if self.handle is None:
            return True    # closed: the kill-on-close policy has already run
        deadline = time.monotonic() + timeout
        while True:
            info = self._accounting()
            if not self._kernel32.QueryInformationJobObject(
                    self._wintypes.HANDLE(self.handle), JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION,
                    self._ctypes.byref(info), self._ctypes.sizeof(info), None):
                raise OSError(self._ctypes.get_last_error(), 'QueryInformationJobObject failed')
            if not info.ActiveProcesses and all(
                    self._kernel32.WaitForSingleObject(self._wintypes.HANDLE(member), 0) == 0
                    for member in self._members):
                return True
            if time.monotonic() >= deadline:
                return False
            time.sleep(.005)

    def close(self):
        """Close this job handle once, triggering its kill-on-close policy, and the member handles."""
        if self.handle:
            self._kernel32.CloseHandle(self._wintypes.HANDLE(self.handle))
            self.handle = None
        while self._members:
            self._kernel32.CloseHandle(self._wintypes.HANDLE(self._members.pop()))

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


def prove_ended(job, process):
    """Wait until no member of JOB, the Job Object of PROCESS, is left; the caller terminated it.

    Raise RuntimeError when a member outlives five seconds, as terminate_group
    does for a POSIX group it cannot prove gone.
    """
    if not job.wait_empty(5):
        raise RuntimeError(f'job of process {process.pid} termination could not be proved')


def terminate_group(process):
    """Stop the group and reap its leader before the caller releases locks.

    On Windows the group is the Job Object that spawn gave the process: ending it
    ends every descendant, one that outlives its leader too.
    """
    if sys.platform == 'win32':
        job = getattr(process, 'job', None)
        # A console Ctrl-C reaches every process on the console, this child too. Give it the
        # POSIX path's 5 s grace to finish its own cleanup (the pool removes the containers it
        # owns) before terminating it; any other cancellation or failure terminates at once.
        if CANCELLED.is_set() and CANCELLED_SIGNAL == signal.SIGINT:
            try:
                process.wait(timeout=5)
                if job is not None:
                    job.terminate()
                    prove_ended(job, process)
                return
            except subprocess.TimeoutExpired:
                pass
        # Terminate the leader first: once the job has ended it, Popen reports an exit code
        # before the process handle signals, and wait would return at once.
        process.terminate()
        if job is not None:
            job.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        if job is not None:
            prove_ended(job, process)
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


def spawn(command, **options):
    """Start COMMAND as the leader of a tree that terminate_group ends whole; return its Popen.

    OPTIONS go to subprocess.Popen. On POSIX the child leads a new session and
    process group. On Windows it starts suspended, joins a new kill-on-close
    WindowsJob (kept as process.job) and then resumes, so no descendant escapes
    the job. A failure to start, assign or resume leaves nothing running and
    propagates.
    """
    if sys.platform != 'win32':
        return subprocess.Popen(command, start_new_session=True, **options)
    import ctypes
    from ctypes import wintypes
    job = WindowsJob()
    try:
        process = subprocess.Popen(command, creationflags=CREATE_SUSPENDED, **options)
    except BaseException:
        job.close()
        raise
    try:
        job.assign(process._handle)
        ntdll = ctypes.WinDLL('ntdll', use_last_error=True)
        ntdll.NtResumeProcess.restype = ctypes.c_long
        ntdll.NtResumeProcess.argtypes = [wintypes.HANDLE]
        if ntdll.NtResumeProcess(wintypes.HANDLE(process._handle)) != 0:
            raise OSError(ctypes.get_last_error(), 'NtResumeProcess failed')
    except BaseException:
        job.terminate()
        process.kill()
        process.wait()
        job.close()
        raise
    process.job = job
    return process


def end_job(job, process):
    """End every member of JOB, the Job Object of PROCESS, prove that none is left, then close it.

    This is the path cancellation takes, for a leader that exited by itself too: a descendant
    that is still running is ended, and the caller may remove what it held (a working
    directory, a file) as soon as this returns. Closing the job alone kills its members
    asynchronously. The job is closed even when the proof fails.
    """
    try:
        job.terminate()
        prove_ended(job, process)
    finally:
        job.close()


def release(process):
    """End the Job Object of a process that spawn started (a no-op elsewhere).

    Call this once the leader is reaped. Any member still running is ended and
    proved gone before it returns.
    """
    job = getattr(process, 'job', None)
    if job is not None:
        end_job(job, process)


@contextlib.contextmanager
def supervised(command, **options):
    """Run `with spawn(COMMAND, **OPTIONS) as process`, then release the Job Object."""
    process = spawn(command, **options)
    try:
        with process:
            yield process
    finally:
        release(process)


def wait_child(process, timeout=None, *, usage=False):
    """Wait for a child status while checking cancellation and an optional timeout.

    Timeout is in seconds. On any exception, terminate the process group and
    reap the leader before propagating the error.
    """
    if usage and not hasattr(os, "wait4"):
        raise ValueError("per-child rusage requires wait4")
    deadline = None if timeout is None else time.monotonic() + timeout
    try:
        while True:
            check_cancelled()
            remaining = None if deadline is None else deadline - time.monotonic()
            if remaining is not None and remaining <= 0:
                raise subprocess.TimeoutExpired(process.args, timeout)
            if usage:
                # This is the sole reaper. Do not call poll/wait before wait4.
                pid, status, rusage = os.wait4(process.pid, os.WNOHANG)
                if pid:
                    process.returncode = os.waitstatus_to_exitcode(status)
                    process.rusage = rusage
                    check_cancelled()
                    return process.returncode
                CANCELLED.wait(min(0.01, remaining) if remaining is not None else 0.01)
                continue
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
        with supervised(command, env=child_env, cwd=cwd, stdout=stdout) as process:
            return wait_child(process)
