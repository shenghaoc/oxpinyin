"""Bounded pool for independent builds under one CPU and memory budget (#1233).

Standard library only. Capacity comes from host.py's readers; nothing here reads
/proc or a cgroup.

`python -m tools.dev pool run ... -- ARGV...` runs ARGV once per item, at most
WORKERS at once, each with its share of the make jobs. By default the run holds
the heavy slot (tools/dev/slot.py, as `just heavy` does), and the plan is
computed from the budget admitted under that slot. `--admission none` sizes from
the host's own readers and takes no slot, for callers with no host slot to hold.

CPU shares are per pool run (and, in run-all.py, per invocation): workers times
make jobs stays within the CPUs this run was admitted or measured, but those CPUs
are not reserved against other, independent slot holders, which plan their own
share the same way. Memory is different: the slot admits it host-wide and counts
other holders' reservations. There is no global CPU scheduler.

Units: a task may carry a callable instead of an argv (`unit()`). `run_units` and
`run_admitted` run such callables in worker threads under the same loop, so
in-process steps and launched processes get one stop rule: the first failure or a
cancellation starts nothing more, stops the running units, and joins every one (a
unit reaps its own children, `UnitContext.launch` does) before the admission is
released. Results and emission follow unit order, never completion order. A unit
must stop when `UnitContext.check()` raises; a thread cannot be killed.

Containers: with `--container-runtime`, each item's command may use {token} (a
random value for this invocation) and {cidfile} (a private file per item) in
`--name`, `--label` and `--cidfile` flags. Cleanup after a failure or a signal
removes a container only when the runtime reports an ID equal to the one recorded
in that item's cidfile and the label `--container-label` equal to this token. An
item whose run never created a container (empty cidfile) leaves nothing to
remove, and a container this run did not create is never touched, whatever its name.
"""
import argparse
import collections
import contextlib
import dataclasses
import functools
import os
import queue
import secrets
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
from collections.abc import Callable, Mapping, Sequence
from pathlib import Path
from typing import NamedTuple

from . import host, slot

# Planning figures for one library build or data regeneration, in MiB: the
# largest cgroup peaks measured for build-oracle.sh on linux/amd64 (Docker, 4
# CPUs, 2026-10-25 UTC). --libs-only: bdb 361 at -j1 and 432 at -j4, kc 528 at
# -j1 and 1,312 at -j4 (about 270 per make job on about 270 fixed), tkrzw 367
# and 691. --regen-data on bdb at -j4: 606. Rounded up to cover every point.
BUILD_BASE_MIB = 300
BUILD_JOB_MIB = 275
# Overlap only while each build keeps two make jobs: a build's compile scales
# with make -j, its configure and link steps do not, and three builds at -j1 on
# four CPUs ran 16% slower than one at a time at -j4 (#1233).
MIN_OVERLAP_JOBS = 2

POOL_WORKERS = 'OXPINYIN_POOL_WORKERS'
POOL_JOBS = 'OXPINYIN_POOL_JOBS'
CANCELLED_STATUS = 130
TAIL_LINES = 40


class Plan(NamedTuple):
    """How many builds overlap and the make jobs each one gets."""
    workers: int
    jobs: int


def plan(tasks: int, cpus: int, memory_mib: int | None,
         base_mib: int = BUILD_BASE_MIB, job_mib: int = BUILD_JOB_MIB) -> Plan:
    """Divide a CPU and memory budget among TASKS independent builds.

    Workers times jobs never exceeds CPUS, so the nested make -j is counted; every
    overlapped build keeps MIN_OVERLAP_JOBS jobs; and workers x (BASE_MIB + jobs x
    JOB_MIB) fits MEMORY_MIB. Unknown memory (None) admits no overlap: one build
    with every CPU. A single build's jobs are held below the CPU count when
    memory is short, never below one.
    """
    cpus, tasks = max(1, cpus), max(1, tasks)
    if memory_mib is None:
        return Plan(1, cpus)
    workers = max(1, min(tasks, cpus // MIN_OVERLAP_JOBS))
    while workers > 1 and workers * (base_mib + (cpus // workers) * job_mib) > memory_mib:
        workers -= 1
    jobs = max(1, cpus // workers)
    if workers == 1:
        jobs = max(1, min(jobs, (memory_mib - base_mib) // job_mib))
    return Plan(workers, jobs)


def estimate_mib(chosen: Plan, base_mib: int = BUILD_BASE_MIB, job_mib: int = BUILD_JOB_MIB) -> int:
    """Memory the plan needs, in MiB: every worker's fixed part plus its make jobs."""
    return chosen.workers * (base_mib + chosen.jobs * job_mib)


def admission_preparer(tasks: int, cpu_cap: int | None, base_mib: int = BUILD_BASE_MIB,
                       job_mib: int = BUILD_JOB_MIB):
    """Return the heavy-admission callback that sizes the pool from the admitted budget.

    slot.HeavyAdmission calls it with the resource snapshot read under the
    admission lock, after other slots' reservations are subtracted. It chooses
    the plan from that room and reserves exactly the plan's estimate, so the pool
    cannot use capacity the slot did not admit.
    """
    def prepare(env: Mapping[str, str], resources, *, runner: bool = False):
        cpus = resources['cpus'] if cpu_cap is None else min(resources['cpus'], cpu_cap)
        available = resources['available_mib']
        headroom = host.memory_headroom(env, resources)
        room = None if available is None else max(0, available - headroom)
        chosen = plan(tasks, cpus, room, base_mib, job_mib)
        why = (f'{tasks} builds, {cpus} CPUs, '
               + ('memory unknown, no overlap' if room is None else f'{room} MiB after {headroom} MiB headroom')
               + f', {base_mib} MiB + {job_mib} MiB per make job')
        return (dict(env, **{POOL_WORKERS: str(chosen.workers), POOL_JOBS: str(chosen.jobs)}),
                {POOL_WORKERS: (chosen.workers, why, False), POOL_JOBS: (chosen.jobs, why, False)},
                estimate_mib(chosen, base_mib, job_mib))
    return prepare


@dataclasses.dataclass
class Task:
    """One item of work: its command, log file and what to do afterwards."""
    name: str
    argv: list[str]
    log: Path | None = None
    cleanup: Callable[[], object] | None = None    # run once if the task was started and the pool stopped early
    remove: Path | None = None          # removed after the task succeeds
    run: Callable[['UnitContext'], object] | None = None    # a unit: run in a worker thread instead of launching ARGV


@dataclasses.dataclass
class Result:
    """What a pool run did: started tasks in order, failures, and whether it was cancelled."""
    started: list[str] = dataclasses.field(default_factory=list)
    failed: dict[str, int] = dataclasses.field(default_factory=dict)
    cancelled: bool = False
    errors: dict[str, BaseException] = dataclasses.field(default_factory=dict)    # units that raised

    @property
    def ok(self) -> bool:
        """True when every task ran and succeeded."""
        return not self.failed and not self.cancelled


class ProcessLauncher:
    """Start each task in its own session so the pool can stop and reap its whole tree."""

    def __init__(self, env: Mapping[str, str] | None = None, pass_fds: Sequence[int] = (),
                 cpus: Sequence[int] = (), wake: threading.Event | None = None,
                 cancelled: Callable[[], bool] | None = None):
        """Remember the child environment and the slot descriptors children inherit.

        CPUS are leased one per running unit; WAKE is set whenever a unit ends; units
        also see CANCELLED() at their checks.
        """
        self.env, self.pass_fds, self.cancelled = env, tuple(pass_fds), cancelled
        self.leases: queue.Queue = queue.Queue()
        for cpu in cpus:
            self.leases.put(cpu)
        self.wake = wake or threading.Event()
        self.units: dict[str, 'UnitHandle'] = {}    # the handles of the units launched, by name

    def launch(self, task: Task):
        """Start TASK and return its handle: a worker thread for a unit, else a process.

        A process runs ARGV with its output in its log.
        """
        if task.run is not None:
            try:
                cpu = self.leases.get_nowait()
            except queue.Empty:
                cpu = None
            context = UnitContext(task.name, threading.Event(), cpu,
                                  {} if self.env is None else dict(self.env), self.pass_fds, self.cancelled)
            self.units[task.name] = UnitHandle(task, context, self.wake, self.leases)
            return self.units[task.name]
        log = None
        if task.log is not None:
            task.log.parent.mkdir(parents=True, exist_ok=True)
            log = task.log.open('wb')
        try:
            process = subprocess.Popen(task.argv, env=None if self.env is None else dict(self.env),
                                       stdin=subprocess.DEVNULL, stdout=log or subprocess.DEVNULL,
                                       stderr=subprocess.STDOUT if log else subprocess.DEVNULL,
                                       start_new_session=True, pass_fds=self.pass_fds)
        finally:
            if log is not None:
                log.close()
        return Handle(process)


class Handle:
    """The pool's view of a started task; tests substitute a fake with the same methods."""

    def __init__(self, process: subprocess.Popen):
        """Wrap PROCESS, the leader of its own process group."""
        self.process = process

    def poll(self) -> int | None:
        """Return the exit status, or None while the task runs."""
        return self.process.poll()

    def terminate(self) -> None:
        """Send SIGTERM to the task's whole process group."""
        with contextlib.suppress(ProcessLookupError):
            os.killpg(self.process.pid, signal.SIGTERM)

    def kill(self) -> None:
        """Send SIGKILL to the task's whole process group."""
        with contextlib.suppress(ProcessLookupError):
            os.killpg(self.process.pid, signal.SIGKILL)

    def wait(self, timeout: float | None = None) -> int | None:
        """Wait for the leader; return None if TIMEOUT seconds pass first."""
        try:
            return self.process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            return None


def _tail(path: Path | None, out) -> None:
    """Print the last lines of a failed task's log."""
    if path is None or not path.exists():
        return
    lines = path.read_text(errors='replace').splitlines()[-TAIL_LINES:]
    print(f'last lines of {path}:', file=out, flush=True)
    for line in lines:
        print(f'  {line}', file=out, flush=True)


def run_pool(tasks: Sequence[Task], workers: int, launcher, *, cancelled: Callable[[], bool],
             poll: float = 0.2, grace: float = 5.0, out=sys.stderr, sleep: Callable[[float], object] = time.sleep,
             progress: Callable[[], object] | None = None) -> Result:
    """Run TASKS, at most WORKERS at once, in order of submission.

    After the first failure, or once CANCELLED() is true, nothing more starts; each
    running task's group gets SIGTERM, then SIGKILL after GRACE seconds, and is
    reaped; and every started task's cleanup command runs. Results do not depend
    on completion order. Return what happened; the caller chooses the exit status.
    PROGRESS, if given, runs in this thread after every reap and once more after
    the running tasks are stopped and joined.
    """
    pending = collections.deque(tasks)
    running: dict[str, tuple[Task, object]] = {}
    result = Result()

    def reap() -> None:
        for name, (task, handle) in list(running.items()):
            status = handle.poll()
            if status is None:
                continue
            del running[name]
            if status:
                result.failed[name] = status
                error = getattr(handle, 'error', None)
                if error is not None:
                    result.errors[name] = error
                print(f'{name}: failed with status {status}', file=out, flush=True)
                _tail(task.log, out)
            elif task.remove is not None:
                shutil.rmtree(task.remove, ignore_errors=True)

    try:
        while pending or running:
            if cancelled():
                result.cancelled = True
                break
            while pending and len(running) < workers and not result.failed and not cancelled():
                task = pending.popleft()
                running[task.name] = (task, launcher.launch(task))
                result.started.append(task.name)
            sleep(poll)
            reap()
            if progress is not None:
                progress()
            if result.failed:
                break
    finally:
        for _, handle in running.values():
            handle.terminate()
        deadline = time.monotonic() + grace
        for _, handle in running.values():
            remaining = max(0.0, deadline - time.monotonic())
            if handle.wait(remaining) is None:
                handle.kill()
                handle.wait()
        stopped_early = bool(result.failed) or result.cancelled or bool(running)
        running.clear()
        if progress is not None:
            progress()
        if stopped_early:
            by_name = {task.name: task for task in tasks}
            for name in result.started:
                if by_name[name].cleanup is not None:
                    with contextlib.suppress(Exception):
                        by_name[name].cleanup()
    return result


class UnitStopped(Exception):
    """Raised inside a unit when its pool has stopped: a sibling failed, or the run was cancelled."""


class UnitsCancelled(Exception):
    """The run was cancelled (the default error of `run_units`)."""


class Completed(NamedTuple):
    """What `UnitContext.launch` returns: the exit status and the captured output."""
    status: int
    stdout: bytes
    stderr: bytes


class StoppableDriver:
    """A driver as one unit sees it: its own checks, plus the pool's stop request.

    Every other attribute is the shared driver's, so a unit's launches terminate and
    reap their process groups through the driver's own code.
    """

    def __init__(self, driver, context: 'UnitContext'):
        self.driver, self.context = driver, context

    def check_cancelled(self):
        """Run the driver's check, then raise UnitStopped if the pool has stopped."""
        self.driver.check_cancelled()
        self.context.check()

    def __getattr__(self, name):
        return getattr(self.driver, name)


@dataclasses.dataclass
class UnitContext:
    """What a running unit sees: its stop request, leased CPU, environment and slot descriptors."""
    name: str
    stop: threading.Event
    cpu: int | None = None
    environment: dict = dataclasses.field(default_factory=dict)
    fds: tuple = ()
    cancelled: Callable[[], bool] | None = None

    def check(self) -> None:
        """Raise UnitStopped once the pool has stopped or the run is cancelled; units call this at every wait."""
        if self.stop.is_set():
            raise UnitStopped(f'{self.name}: the pool stopped')
        if self.cancelled is not None and self.cancelled():
            raise UnitStopped(f'{self.name}: cancelled')

    def driver(self, base) -> StoppableDriver:
        """Wrap BASE, a driver with check_cancelled, so its checks also see the pool's stop."""
        return StoppableDriver(base, self)

    def launch(self, argv: Sequence[str], *, env: Mapping[str, str] | None = None,
               pass_fds: Sequence[int] | None = None, poll: float = 0.1,
               cwd: Path | None = None, merge_stderr: bool = False, umask: int = -1) -> Completed:
        """Run ARGV in its own session and return its status and captured output.

        stdin is closed at once. Every POLL seconds the unit checks for a stop; on a
        stop, a timeout of the caller's making, or any other error the group gets
        SIGTERM, then SIGKILL through slot.terminate_group, and its leader is reaped before the
        error propagates. ENV defaults to the unit's environment, PASS_FDS to its slot
        descriptors.
        """
        with subprocess.Popen(list(argv), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT if merge_stderr else subprocess.PIPE, cwd=cwd, umask=umask,
                              env=dict(self.environment if env is None else env), start_new_session=True,
                              pass_fds=tuple(self.fds if pass_fds is None else pass_fds)) as process:
            try:
                first = True
                while True:
                    self.check()
                    try:
                        stdout, stderr = process.communicate(b'' if first else None, timeout=poll)
                        break
                    except subprocess.TimeoutExpired:
                        first = False
            except BaseException:
                slot.terminate_group(process)
                raise
        return Completed(process.returncode, stdout, stderr or b'')


class UnitHandle:
    """The pool's view of a unit running in a worker thread (the same methods as Handle)."""

    def __init__(self, task: Task, context: UnitContext, wake: threading.Event, leases: queue.Queue):
        self.task, self.context, self.wake, self.leases = task, context, wake, leases
        self.value, self.error = None, None
        self.thread = threading.Thread(target=self.work, name=task.name, daemon=True)
        self.thread.start()

    def work(self) -> None:
        """Run the unit; keep its value or error, return the CPU lease, wake the pool's loop."""
        try:
            self.value = self.task.run(self.context)
        except BaseException as error:
            self.error = error
        finally:
            if self.context.cpu is not None:
                self.leases.put(self.context.cpu)
            self.wake.set()

    def poll(self) -> int | None:
        """None while the unit runs; 0 if it returned, -SIGTERM if it stopped, else 1."""
        if self.thread.is_alive():
            return None
        if self.error is None:
            return 0
        return -signal.SIGTERM if isinstance(self.error, UnitStopped) else 1

    def terminate(self) -> None:
        """Ask the unit to stop: the next UnitContext.check() raises."""
        self.context.stop.set()

    def kill(self) -> None:
        """A thread cannot be killed; this repeats the stop request."""
        self.context.stop.set()

    def wait(self, timeout: float | None = None) -> int | None:
        """Join the unit's thread; return None if TIMEOUT seconds pass first."""
        self.thread.join(timeout)
        return self.poll()


def cancelled_from(check: Callable[[], object]) -> Callable[[], bool]:
    """Turn CHECK, which raises when the run is cancelled, into a flag the pool polls."""
    def flag() -> bool:
        try:
            check()
        except Exception:
            return True
        return False
    return flag


def run_units(units: Sequence[tuple[str, Callable[[UnitContext], object]]], workers: int, *,
              cancelled: Callable[[], bool], launcher: ProcessLauncher | None = None,
              emit: Callable[[str, object], object] | None = None,
              cancel_error: Callable[[str], BaseException] = UnitsCancelled,
              poll: float = 0.05, grace: float = 5.0, out=sys.stderr) -> list:
    """Run UNITS, (name, function) pairs, at most WORKERS at once; return their values in order.

    This is run_pool with callables: the same stop rule, with every thread joined
    before return. EMIT(name, value) runs in this thread in unit order as the next
    value becomes available, so output never depends on completion order; units
    finished before a failure are still emitted. A failure raises the earliest
    unit's own error (never a sibling's UnitStopped); a cancellation raises
    CANCEL_ERROR('cancelled').
    """
    launcher = launcher or ProcessLauncher()
    names = [name for name, _ in units]
    if len(set(names)) != len(names):
        raise ValueError('run_units needs distinct unit names')
    handles = launcher.units
    emitted = 0

    def progress() -> None:
        nonlocal emitted
        while emitted < len(names) and names[emitted] in handles:
            handle = handles[names[emitted]]
            if handle.thread.is_alive() or handle.error is not None:
                return
            emitted += 1
            if emit is not None:
                emit(names[emitted - 1], handle.value)
    result = run_pool([Task(name, [], run=function) for name, function in units], workers, launcher,
                      cancelled=cancelled, poll=poll, grace=grace, out=out,
                      sleep=lambda seconds: (launcher.wake.wait(seconds), launcher.wake.clear()),
                      progress=progress)
    if result.cancelled:
        raise cancel_error('cancelled')
    for name in names:
        error = handles[name].error if name in handles else None
        if error is not None and not isinstance(error, UnitStopped):
            raise error
    if cancelled():
        raise cancel_error('cancelled')
    if not result.ok:
        raise UnitsCancelled('the run stopped before every unit finished')
    return [handles[name].value for name in names]


def unit_preparer(units: int, cpu_cap: int | None = None, unit_mib: int = host.RUNNER_PEAK_MIB):
    """Return the heavy-admission callback that sizes a pool of UNITS callables.

    Like admission_preparer, it chooses from the room the slot admits after other
    holders' reservations and headroom, and reserves exactly what it chose: one
    UNIT_MIB allowance per worker (the heavy-runner allowance by default), at most
    UNITS workers and the admitted (or capped) CPUs. When not even one worker fits,
    it reserves one worker's allowance and the slot refuses it.
    """
    def prepare(env: Mapping[str, str], resources, *, runner: bool = False):
        cpus = resources['cpus'] if cpu_cap is None else min(resources['cpus'], cpu_cap)
        available = resources['available_mib']
        headroom = host.memory_headroom(env, resources)
        workers = 0 if available is None else host.fit(available, unit_mib, min(units, cpus), headroom)
        why = (f'{units} units, {cpus} CPUs, '
               + ('memory unknown' if available is None else f'{max(0, available - headroom)} MiB after {headroom} MiB headroom')
               + f', {unit_mib} MiB per unit')
        return (dict(env, **{POOL_WORKERS: str(max(1, workers))}),
                {POOL_WORKERS: (workers, why, False)}, max(1, workers) * unit_mib)
    return prepare


class Budget(NamedTuple):
    """What admission granted a pool: its worker count, environment and slot descriptors."""
    workers: int
    env: dict
    fds: tuple


@contextlib.contextmanager
def admitted(units: int, label: str, *, cancelled: Callable[[], bool], cpu_cap: int | None = None,
             unit_mib: int = host.RUNNER_PEAK_MIB, cancel_error: Callable[[str], BaseException] = UnitsCancelled,
             out=sys.stderr):
    """Admit a pool of at most UNITS callables before any starts, and yield its Budget.

    Take or inherit the heavy slot and reserve the pool's memory (unit_preparer). While
    waiting, CANCELLED() is polled and CANCEL_ERROR raised; the slot is released on any error. The caller joins
    its threads and reaps their children before leaving the block, which releases it.
    """
    with contextlib.ExitStack() as stack:
        admission = slot.HeavyAdmission(label, dict(os.environ), out=out, runner=True,
                                        prepare_heavy_env=unit_preparer(units, cpu_cap, unit_mib))
        stack.callback(admission.close)
        while not admission.try_acquire():
            if cancelled():
                raise cancel_error('cancelled while waiting for admission')
            time.sleep(0.05)
        if cancelled():
            raise cancel_error('cancelled while waiting for admission')
        budget = Budget(int(admission.env[POOL_WORKERS]), dict(admission.env), tuple(admission.slot.pass_fds()))
        admission.slot.handoff()
        print(f'{label}: requested {units}, admitted {budget.workers}', file=out, flush=True)
        yield budget


def run_admitted(units: Sequence[tuple[str, Callable[[UnitContext], object]]], label: str, *,
                 cancelled: Callable[[], bool], serial: bool = False, cpus: Sequence[int] | None = None,
                 max_workers: int | None = None,
                 emit: Callable[[str, object], object] | None = None,
                 cancel_error: Callable[[str], BaseException] = UnitsCancelled,
                 unit_mib: int = host.RUNNER_PEAK_MIB, out=sys.stderr) -> list:
    """Run independent UNITS in a pool sized by the heavy slot's admission; return values in order.

    SERIAL, or a single unit, runs them in order in this thread with no admission: the
    serial reference. Otherwise the pool is admitted (`admitted`) and sized by it, never
    above MAX_WORKERS, the unit count or CPUS (the CPU ids to lease, one per running unit; none means
    no limit and no lease); see `run_units` for the stop and ordering rules. Every unit
    sees the admitted environment and slot descriptors.
    """
    if max_workers is not None and max_workers < 1:
        raise ValueError('max_workers must be positive')
    if serial or len(units) < 2:
        values = []
        for name, function in units:
            try:
                values.append(function(UnitContext(name, threading.Event(), cancelled=cancelled)))
            except UnitStopped:
                if cancelled():
                    raise cancel_error('cancelled') from None
                raise
            if emit is not None:
                emit(name, values[-1])
        return values
    cpu_cap = None if cpus is None else len(cpus)
    if max_workers is not None:
        cpu_cap = max_workers if cpu_cap is None else min(cpu_cap, max_workers)
    with admitted(len(units), label, cancelled=cancelled, cpu_cap=cpu_cap,
                  unit_mib=unit_mib, cancel_error=cancel_error, out=out) as budget:
        workers = max(1, min(budget.workers, len(units), len(cpus) if cpus else len(units),
                             max_workers if max_workers is not None else len(units)))
        launcher = ProcessLauncher(env=budget.env, pass_fds=budget.fds, cpus=list(cpus or [])[:workers],
                                   cancelled=cancelled)
        return run_units(units, workers, cancelled=cancelled, launcher=launcher, emit=emit,
                         cancel_error=cancel_error, out=out)


@dataclasses.dataclass(frozen=True)
class OwnedContainer:
    """A container this invocation asked RUNTIME to create: where its ID is recorded, and the label and token to match."""
    runtime: tuple[str, ...]
    cidfile: Path
    label: str
    token: str


def _capture(argv: Sequence[str]) -> subprocess.CompletedProcess:
    """Run one runtime command and capture its output; the pool never waits on a hung runtime for long."""
    return subprocess.run(list(argv), stdin=subprocess.DEVNULL, capture_output=True, text=True,
                          check=False, timeout=60)


def remove_owned(owned: OwnedContainer, run: Callable[[Sequence[str]], subprocess.CompletedProcess] = _capture,
                 out=sys.stderr) -> bool:
    """Remove the container OWNED recorded, only if it is the one this invocation created.

    The ID comes from the cidfile the runtime wrote for this run's `run` command. An empty or
    missing file means no container was created. The runtime must report the same ID and carry
    this run's token in the label; anything else is left alone. Return True when a removal ran.
    """
    try:
        recorded = owned.cidfile.read_text().strip()
    except OSError:
        return False
    if not recorded:
        return False
    template = '{{.Id}} {{index .Config.Labels "%s"}}' % owned.label
    found = run([*owned.runtime, 'container', 'inspect', '--format', template, recorded])
    if found.returncode:
        return False
    identity, _, label = found.stdout.strip().partition(' ')
    if identity != recorded or label != owned.token:
        print(f'left container {recorded[:12]} alone: it was not created by this run', file=out, flush=True)
        return False
    run([*owned.runtime, 'container', 'rm', '-f', recorded])
    return True


def _substitute(text: str, item: str, chosen: Plan, token: str = '', cidfile: str = '') -> str:
    """Replace {item}, {jobs}, {workers}, {token} and {cidfile}; other braces are left alone."""
    return (text.replace('{item}', item).replace('{jobs}', str(chosen.jobs))
            .replace('{workers}', str(chosen.workers)).replace('{token}', token).replace('{cidfile}', cidfile))


def build_tasks(args, chosen: Plan, token: str = '', cid_dir: Path | None = None) -> list[Task]:
    """Expand the command template once per item.

    With --container-runtime, TOKEN and a cidfile under CID_DIR belong to each item: its cleanup
    removes only the container the runtime recorded there, and only if its label carries TOKEN.
    """
    tasks = []
    for item in args.items:
        cidfile = None if cid_dir is None else cid_dir / f'{item}.cid'

        def sub(text: str, item: str = item, cidfile: Path | None = cidfile) -> str:
            return _substitute(text, item, chosen, token, '' if cidfile is None else str(cidfile))
        cleanup = None
        if args.container_runtime and cidfile is not None:
            owned = OwnedContainer(tuple(shlex.split(args.container_runtime)), cidfile, args.container_label, token)
            cleanup = functools.partial(remove_owned, owned)
        tasks.append(Task(item, [sub(part) for part in args.command],
                          log=Path(sub(args.log)) if args.log else None,
                          cleanup=cleanup, remove=Path(sub(args.remove)) if args.remove else None))
    return tasks


def _announce(args, chosen: Plan, budget: str, out) -> None:
    """Print the plan the way the other effective settings are reported."""
    print(f'{args.label}: {" ".join(args.items)} with {chosen.workers} at once, make -j{chosen.jobs} each '
          f'({budget})', file=out, flush=True)


@contextlib.contextmanager
def _own_cancellation():
    """Turn SIGINT, SIGTERM and SIGHUP into a flag the pool polls (no slot module here)."""
    import threading
    event = threading.Event()
    previous = {}
    for name in ('SIGTERM', 'SIGINT', 'SIGHUP'):
        signum = getattr(signal, name, None)
        if signum is not None:
            previous[signum] = signal.signal(signum, lambda *_: event.set())
    try:
        yield event.is_set
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)


def _finish(result: Result, out) -> int:
    """Map a pool result to an exit status: 0, 1 for a failure, 130 for cancellation."""
    if result.cancelled:
        print('cancelled; child process groups terminated and reaped', file=out, flush=True)
        return CANCELLED_STATUS
    return 0 if result.ok else 1


def run_command(args, out=sys.stderr, launcher_factory=ProcessLauncher) -> int:
    """Run the pool for parsed ARGS and return its exit status."""
    if not args.items or not args.command:
        raise SystemExit('pool run needs --items and a command after --')
    if len(set(args.items)) != len(args.items):
        raise SystemExit('pool run needs distinct --items')
    token = secrets.token_hex(8)
    with tempfile.TemporaryDirectory(prefix='oxpinyin-pool-') as cid_directory:
        cid_dir = Path(cid_directory)

        def tasks_for(chosen: Plan) -> list[Task]:
            return build_tasks(args, chosen, token, cid_dir)
        if args.admission == 'none':
            resources = host.resource_snapshot()
            available = resources['available_mib']
            cpus = resources['cpus'] if args.cpus is None else min(resources['cpus'], args.cpus)
            chosen = plan(len(args.items), cpus, available, args.base_mib, args.job_mib)
            _announce(args, chosen, f'{cpus} CPUs, {"unknown" if available is None else available} MiB available', out)
            with _own_cancellation() as cancelled:
                result = run_pool(tasks_for(chosen), chosen.workers, launcher_factory(),
                                  cancelled=cancelled, out=out)
            return _finish(result, out)
        from . import slot
        try:
            with slot.cancellation_handlers():
                prepare = admission_preparer(len(args.items), args.cpus, args.base_mib, args.job_mib)
                with slot.HeavyAdmission(f'{args.label}: {" ".join(args.items)}', dict(os.environ),
                                         out=out, prepare_heavy_env=prepare) as admission:
                    chosen = Plan(int(admission.env[POOL_WORKERS]), int(admission.env[POOL_JOBS]))
                    _announce(args, chosen, f'admitted, {estimate_mib(chosen, args.base_mib, args.job_mib)} MiB reserved', out)
                    launcher = launcher_factory(env=admission.env, pass_fds=admission.slot.pass_fds())
                    admission.slot.handoff()
                    result = run_pool(tasks_for(chosen), chosen.workers, launcher,
                                      cancelled=slot.CANCELLED.is_set, out=out)
        except slot.Cancelled:
            return _finish(Result(cancelled=True), out)
        except (OSError, ValueError) as exc:
            print(f'pool run: {" ".join(str(exc).splitlines())}', file=out, flush=True)
            return 1
        return _finish(result, out)


def add_arguments(parser: argparse.ArgumentParser) -> None:
    """Define the `pool run` options on PARSER (shared by `python -m tools.dev pool` and this module)."""
    sub = parser.add_subparsers(dest='action', required=True)
    run = sub.add_parser('run', help='run a command per item, within one CPU and memory budget')
    run.add_argument('--label', default='pool')
    run.add_argument('--items', type=lambda value: [part for part in value.split(',') if part], default=[],
                     help='comma-separated item names; {item} in the command, log and remove')
    run.add_argument('--cpus', type=int, help='cap the CPU budget (default: what the host or slot admits)')
    run.add_argument('--base-mib', type=int, default=BUILD_BASE_MIB)
    run.add_argument('--job-mib', type=int, default=BUILD_JOB_MIB)
    run.add_argument('--admission', choices=('slot', 'none'), default='slot',
                     help='slot: hold the host heavy slot; none: size from this container, no slot')
    run.add_argument('--log', help='log file per item; output of a failed item is tailed')
    run.add_argument('--container-runtime', help='container command (shell-quoted); with it, items may use {token} and '
                     '{cidfile}, and a failure or signal removes only the containers this run recorded and labelled')
    run.add_argument('--container-label', default='oxpinyin.pool',
                     help='label key whose value is the run token on each container the command creates')
    run.add_argument('--remove', help='directory removed per item after it succeeds')
    run.add_argument('command', nargs=argparse.REMAINDER, help='-- COMMAND... ({item}, {jobs}, {workers}, {token}, {cidfile})')


def main(argv: Sequence[str] | None = None) -> int:
    """Entry point of `python -m tools.dev.pool`."""
    parser = argparse.ArgumentParser(prog='python -m tools.dev.pool')
    add_arguments(parser)
    args = parser.parse_args(argv)
    if args.command[:1] == ['--']:
        args.command = args.command[1:]
    return run_command(args)


if __name__ == '__main__':
    sys.exit(main())
