"""Bounded pool of independent units and tasks, with one stop rule.

`python -m tools.dev.pool run ... -- ARGV...` runs ARGV once per item, at most
WORKERS at once, each with its share of the make jobs: workers times jobs stays
within the allowed CPUs.

Standard library only. A task is an argv or a callable (a unit). `run_units`
and `run_leased` run callables in worker threads under the same loop, so
in-process steps and launched processes get one stop rule: the first failure or a
cancellation starts nothing more, stops the running units, and joins every one (a
unit reaps its own children, `UnitContext.launch` does). Results and emission
follow unit order, never completion order. A unit must stop when
`UnitContext.check()` raises; a thread cannot be killed.

Containers: `remove_owned` removes a container only when the runtime reports an ID
equal to the one recorded in the unit's cidfile and the label equal to the run's
token. The runtime writes the cidfile after it creates the container, so a stop
in between leaves a container the cidfile does not name. `remove_labelled` then
removes each container that carries the run's token, which only this run's
containers carry. A container this run did not create is never touched, whatever
its name.
"""

if __name__ == '__main__':
    from .entry import require_normal_imports

    require_normal_imports()

import argparse
import collections
import contextlib
import dataclasses
import functools
import os
import queue
import secrets
import shlex
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

TAIL_LINES = 40
CANCELLED_STATUS = 130


@dataclasses.dataclass
class Task:
    """One item of work: its command, log file and what to do afterwards."""
    name: str
    argv: list[str]
    log: Path | None = None
    cleanup: Callable[[], object] | None = None    # run once if the task was started and the pool stopped early
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
    """Start each task through slot.spawn so the pool can stop and reap its whole tree.

    That is a session of its own on POSIX and a Job Object on Windows.
    """

    def __init__(self, env: Mapping[str, str] | None = None,
                 cpus: Sequence[int] = (), wake: threading.Event | None = None,
                 cancelled: Callable[[], bool] | None = None):
        """Remember the child environment.

        CPUS are leased one per running unit; WAKE is set whenever a unit ends; units
        also see CANCELLED() at their checks.
        """
        self.env, self.cancelled = env, cancelled
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
                                  {} if self.env is None else dict(self.env),
                                  cancelled=self.cancelled)
            self.units[task.name] = UnitHandle(task, context, self.wake, self.leases)
            return self.units[task.name]
        log = None
        if task.log is not None:
            task.log.parent.mkdir(parents=True, exist_ok=True)
            log = task.log.open('wb')
        try:
            process = slot.spawn(task.argv, env=None if self.env is None else dict(self.env),
                                 stdin=subprocess.DEVNULL, stdout=log or subprocess.DEVNULL,
                                 stderr=subprocess.STDOUT if log else subprocess.DEVNULL)
        finally:
            if log is not None:
                log.close()
        return Handle(process, getattr(process, 'job', None))


class Handle:
    """The pool's view of a started task; tests substitute a fake with the same methods."""

    def __init__(self, process: subprocess.Popen, job=None):
        """Wrap PROCESS, the leader of its own process group, or JOB, the Windows Job Object it runs in."""
        self.process, self.job = process, job

    def _release(self, status: int | None) -> int | None:
        """End the job once the leader has ended, descendants included, and pass STATUS on."""
        if status is not None and self.job is not None:
            job, self.job = self.job, None
            slot.end_job(job, self.process)
        return status

    def poll(self) -> int | None:
        """Return the exit status, or None while the task runs."""
        return self._release(self.process.poll())

    def terminate(self) -> None:
        """Send SIGTERM to the task's whole process group; Windows ends the whole job at once."""
        if sys.platform == 'win32':
            self.process.terminate()
            if self.job is not None:
                self.job.terminate()
                slot.prove_ended(self.job, self.process)
            return
        with contextlib.suppress(ProcessLookupError):
            os.killpg(self.process.pid, signal.SIGTERM)

    def kill(self) -> None:
        """Send SIGKILL to the task's whole process group; Windows ends the whole job."""
        if sys.platform == 'win32':
            self.process.kill()
            if self.job is not None:
                self.job.terminate()
                slot.prove_ended(self.job, self.process)
            return
        with contextlib.suppress(ProcessLookupError):
            os.killpg(self.process.pid, signal.SIGKILL)

    def wait(self, timeout: float | None = None) -> int | None:
        """Wait for the leader; return None if TIMEOUT seconds pass first."""
        try:
            return self._release(self.process.wait(timeout=timeout))
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


@dataclasses.dataclass
class UnitContext:
    """What a running unit sees: its stop request, leased CPU and environment."""
    name: str
    stop: threading.Event
    cpu: int | None = None
    environment: dict = dataclasses.field(default_factory=dict)
    cancelled: Callable[[], bool] | None = None

    def check(self) -> None:
        """Raise UnitStopped once the pool has stopped or the run is cancelled; units call this at every wait."""
        if self.stop.is_set():
            raise UnitStopped(f'{self.name}: the pool stopped')
        if self.cancelled is not None and self.cancelled():
            raise UnitStopped(f'{self.name}: cancelled')

    def launch(self, argv: Sequence[str], *, env: Mapping[str, str] | None = None,
               poll: float = 0.1,
               cwd: Path | None = None, merge_stderr: bool = False, umask: int = -1,
               input: bytes = b'') -> Completed:
        """Run ARGV in its own session (a Job Object on Windows) and return its status and captured output.

        INPUT is written to stdin, then stdin is closed (empty by default).
        Every POLL seconds the unit checks for a stop; on a
        stop, a timeout of the caller's making, or any other error the group gets
        SIGTERM, then SIGKILL through slot.terminate_group, and its leader is reaped before the
        error propagates. ENV defaults to the unit's environment.
        """
        with slot.supervised(list(argv), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT if merge_stderr else subprocess.PIPE, cwd=cwd, umask=umask,
                             env=dict(self.environment if env is None else env)) as process:
            try:
                first = True
                while True:
                    self.check()
                    try:
                        stdout, stderr = process.communicate(input if first else None, timeout=poll)
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
    from tools.oracle import image
    found = image.container_owner(owned.runtime, recorded, owned.label, run=run)
    if found is None:
        return False
    identity, _, label = found.partition(' ')
    if identity != recorded or label != owned.token:
        print(f'left container {recorded[:12]} alone: it was not created by this run', file=out, flush=True)
        return False
    run(image.runtime_command([*owned.runtime, 'container', 'rm', '-f', recorded]))
    return True


def remove_labelled(owned: OwnedContainer, run: Callable[[Sequence[str]], subprocess.CompletedProcess] = _capture) -> int:
    """Remove every container that carries this run's token, whatever the cidfile holds.

    The runtime creates the container and then writes its ID to the cidfile. A pool that stops
    the runtime between the two leaves a container no cidfile names. The token is this
    invocation's alone, so the label still does. Each candidate is inspected first and removed
    only when the runtime reports its ID and this token. Return how many were removed.
    """
    from tools.oracle import image
    removed = 0
    for candidate in image.container_ids(owned.runtime, owned.label, owned.token, run=run):
        found = image.container_owner(owned.runtime, candidate, owned.label, run=run)
        if found is None:
            continue
        identity, _, label = found.partition(' ')
        if identity != candidate or label != owned.token:
            continue
        run(image.runtime_command([*owned.runtime, 'container', 'rm', '-f', candidate]))
        removed += 1
    return removed


def remove_run_containers(owned: OwnedContainer, run: Callable[[Sequence[str]], subprocess.CompletedProcess] = _capture,
                          out=sys.stderr) -> bool:
    """Remove the container OWNED recorded, then any container of this run the cidfile never named."""
    recorded = remove_owned(owned, run, out)
    return remove_labelled(owned, run) > 0 or recorded


def run_leased(units: Sequence[tuple[str, Callable[[UnitContext], object]]], *,
               cancelled: Callable[[], bool], serial: bool = False, cpus: Sequence[int] | None = None,
               max_workers: int | None = None,
               emit: Callable[[str, object], object] | None = None,
               cancel_error: Callable[[str], BaseException] = UnitsCancelled,
               env: Mapping[str, str] | None = None, out=sys.stderr) -> list:
    """Run independent UNITS, one CPU leased per running unit; return values in order.

    SERIAL caps execution at one worker. The pool never exceeds MAX_WORKERS, the
    unit count or CPUS (the CPU ids to lease; defaults to the host's allowed set).
    See `run_units` for the stop and ordering rules. Every unit sees ENV
    (default os.environ) as its environment.
    """
    if max_workers is not None and max_workers < 1:
        raise ValueError('max_workers must be positive')
    if not units:
        return []
    cpus = sorted(host.allowed_cpus()) if cpus is None else list(cpus)
    if not cpus:
        raise ValueError('cpus must not be empty')
    if serial:
        max_workers = 1
    workers = max(1, min(len(units), len(cpus), max_workers if max_workers is not None else len(units)))
    launcher = ProcessLauncher(env=dict(os.environ if env is None else env), cpus=cpus[:workers],
                               cancelled=cancelled)
    return run_units(units, workers, cancelled=cancelled, launcher=launcher, emit=emit,
                     cancel_error=cancel_error, out=out)


class Plan(NamedTuple):
    """How many builds overlap and the make jobs each one gets."""
    workers: int
    jobs: int


# Overlap only while each build keeps two make jobs: a build's compile scales
# with make -j, its configure and link steps do not, and three builds at -j1 on
# four CPUs ran 16% slower than one at a time at -j4 (#1233).
MIN_OVERLAP_JOBS = 2


def plan(tasks: int, cpus: int) -> Plan:
    """Divide a CPU budget among TASKS independent builds.

    Workers times jobs never exceeds CPUS, so the nested make -j is counted, and
    every overlapped build keeps MIN_OVERLAP_JOBS jobs.
    """
    cpus, tasks = max(1, cpus), max(1, tasks)
    workers = max(1, min(tasks, cpus // MIN_OVERLAP_JOBS))
    return Plan(workers, max(1, cpus // workers))


def _substitute(text: str, item: str, chosen: Plan, token: str = '', cidfile: str = '') -> str:
    """Replace {item}, {jobs}, {workers}, {token} and {cidfile}; other braces are left alone."""
    return (text.replace('{item}', item).replace('{jobs}', str(chosen.jobs))
            .replace('{workers}', str(chosen.workers)).replace('{token}', token).replace('{cidfile}', cidfile))


def build_tasks(args, chosen: Plan, token: str = '', cid_dir: Path | None = None) -> list[Task]:
    """Expand the command template once per item.

    With --container-runtime, TOKEN and a cidfile under CID_DIR belong to each item: its cleanup
    removes the container the runtime recorded there and any other container whose label carries TOKEN.
    """
    tasks = []
    for item in args.items:
        cidfile = None if cid_dir is None else cid_dir / f'{item}.cid'

        def sub(text: str, item: str = item, cidfile: Path | None = cidfile) -> str:
            return _substitute(text, item, chosen, token, '' if cidfile is None else str(cidfile))
        cleanup = None
        if args.container_runtime and cidfile is not None:
            owned = OwnedContainer(tuple(shlex.split(args.container_runtime)), cidfile, args.container_label, token)
            cleanup = functools.partial(remove_run_containers, owned)
        tasks.append(Task(item, [sub(part) for part in args.command],
                          log=Path(sub(args.log)) if args.log else None,
                          cleanup=cleanup))
    return tasks


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
    allowed = len(host.allowed_cpus())
    cpus = allowed if args.cpus is None else min(allowed, args.cpus)
    chosen = plan(len(args.items), cpus)
    print(f'{args.label}: {" ".join(args.items)} with {chosen.workers} at once, make -j{chosen.jobs} each '
          f'({cpus} CPUs)', file=out, flush=True)
    with tempfile.TemporaryDirectory(prefix='oxpinyin-pool-') as cid_directory:
        try:
            with slot.cancellation_handlers():
                result = run_pool(build_tasks(args, chosen, token, Path(cid_directory)), chosen.workers,
                                  launcher_factory(), cancelled=slot.CANCELLED.is_set, out=out)
        except slot.Cancelled:
            return _finish(Result(cancelled=True), out)
    return _finish(result, out)


def add_arguments(parser: argparse.ArgumentParser) -> None:
    """Define the pool operation and its options on PARSER."""
    sub = parser.add_subparsers(dest='action', required=True)
    run = sub.add_parser('run', help='run a command per item, within one CPU budget')
    run.add_argument('--label', default='pool')
    run.add_argument('--items', type=lambda value: [part for part in value.split(',') if part], default=[],
                     help='comma-separated item names; {item} in the command and log')
    run.add_argument('--cpus', type=int, help='cap the CPU budget (default: the allowed CPUs)')
    run.add_argument('--log', help='log file per item; output of a failed item is tailed')
    run.add_argument('--container-runtime', help='container command (shell-quoted); with it, items may use {token} and '
                     '{cidfile}, and a failure or signal removes only the containers this run recorded and labelled')
    run.add_argument('--container-label', default='oxpinyin.pool',
                     help='label key whose value is the run token on each container the command creates')
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
