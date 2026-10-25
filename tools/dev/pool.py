"""Bounded pool for independent builds under one CPU and memory budget (#1233, #1234).

Standard library only, and no syntax newer than Python 3.12: the image build
stage runs this module with the deps stage's python3, beside host.py, darwin.py
and records.py (tools/oracle/Dockerfile.oracle). Capacity comes from host.py's
readers; nothing here reads /proc or a cgroup.

`python -m tools.dev pool run ... -- ARGV...` runs ARGV once per item, at most
WORKERS at once, each with its share of the make jobs. By default the run holds
the heavy slot (tools/dev/slot.py, as `just heavy` does), and the plan is
computed from the budget admitted under that slot. `--admission none` sizes from
the container's own limits and takes no slot: the docker build stage has no
host slot.
"""
import argparse
import collections
import contextlib
import dataclasses
import os
import shlex
import shutil
import signal
import subprocess
import sys
import time
from collections.abc import Callable, Mapping, Sequence
from pathlib import Path
from typing import NamedTuple

from . import host

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
    cleanup: list[str] | None = None    # run once if the task was started and the pool stopped early
    remove: Path | None = None          # removed after the task succeeds


@dataclasses.dataclass
class Result:
    """What a pool run did: started tasks in order, failures, and whether it was cancelled."""
    started: list[str] = dataclasses.field(default_factory=list)
    failed: dict[str, int] = dataclasses.field(default_factory=dict)
    cancelled: bool = False

    @property
    def ok(self) -> bool:
        """True when every task ran and succeeded."""
        return not self.failed and not self.cancelled


class ProcessLauncher:
    """Start each task in its own session so the pool can stop and reap its whole tree."""

    def __init__(self, env: Mapping[str, str] | None = None, pass_fds: Sequence[int] = ()):
        """Remember the child environment and the slot descriptors children inherit."""
        self.env, self.pass_fds = env, tuple(pass_fds)

    def launch(self, task: Task):
        """Start TASK with its output in its log and return the process handle."""
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
             run_cleanup: Callable[[list[str]], object] | None = None, poll: float = 0.2,
             grace: float = 5.0, out=sys.stderr, sleep: Callable[[float], object] = time.sleep) -> Result:
    """Run TASKS, at most WORKERS at once, in order of submission.

    After the first failure, or once CANCELLED() is true, nothing more starts; each
    running task's group gets SIGTERM, then SIGKILL after GRACE seconds, and is
    reaped; and every started task's cleanup command runs. Results do not depend
    on completion order. Return what happened; the caller chooses the exit status.
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
        if stopped_early and run_cleanup is not None:
            by_name = {task.name: task for task in tasks}
            for name in result.started:
                if by_name[name].cleanup:
                    with contextlib.suppress(Exception):
                        run_cleanup(by_name[name].cleanup)
    return result


def _substitute(text: str, item: str, chosen: Plan) -> str:
    """Replace {item}, {jobs} and {workers}; other braces are left alone."""
    return text.replace('{item}', item).replace('{jobs}', str(chosen.jobs)).replace('{workers}', str(chosen.workers))


def build_tasks(args, chosen: Plan) -> list[Task]:
    """Expand the command template once per item."""
    tasks = []
    for item in args.items:
        def sub(text: str, item: str = item) -> str:
            return _substitute(text, item, chosen)
        tasks.append(Task(item, [sub(part) for part in args.command],
                          log=Path(sub(args.log)) if args.log else None,
                          cleanup=[sub(part) for part in shlex.split(args.cleanup)] if args.cleanup else None,
                          remove=Path(sub(args.remove)) if args.remove else None))
    return tasks


def _run_cleanup(argv: list[str]) -> None:
    """Run one cleanup command, ignoring its status."""
    subprocess.run(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL, check=False)


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
    if args.admission == 'none':
        resources = host.resource_snapshot()
        available = resources['available_mib']
        cpus = resources['cpus'] if args.cpus is None else min(resources['cpus'], args.cpus)
        chosen = plan(len(args.items), cpus, available, args.base_mib, args.job_mib)
        _announce(args, chosen, f'{cpus} CPUs, {"unknown" if available is None else available} MiB available', out)
        with _own_cancellation() as cancelled:
            result = run_pool(build_tasks(args, chosen), chosen.workers, launcher_factory(),
                              cancelled=cancelled, run_cleanup=_run_cleanup, out=out)
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
                result = run_pool(build_tasks(args, chosen), chosen.workers, launcher,
                                  cancelled=slot.CANCELLED.is_set, run_cleanup=_run_cleanup, out=out)
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
                     help='comma-separated item names; {item} in the command, log, cleanup and remove')
    run.add_argument('--cpus', type=int, help='cap the CPU budget (default: what the host or slot admits)')
    run.add_argument('--base-mib', type=int, default=BUILD_BASE_MIB)
    run.add_argument('--job-mib', type=int, default=BUILD_JOB_MIB)
    run.add_argument('--admission', choices=('slot', 'none'), default='slot',
                     help='slot: hold the host heavy slot; none: size from this container, no slot')
    run.add_argument('--log', help='log file per item; output of a failed item is tailed')
    run.add_argument('--cleanup', help='command (shell-quoted) run per started item after a failure or signal')
    run.add_argument('--remove', help='directory removed per item after it succeeds')
    run.add_argument('command', nargs=argparse.REMAINDER, help='-- COMMAND... ({item}, {jobs}, {workers})')


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
