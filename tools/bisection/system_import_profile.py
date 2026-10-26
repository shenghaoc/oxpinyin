"""Collect #599's import/save or cold-open profiles for the pin and the subject.

Called by run-system-import-profile.sh after it has checked the arms and built
the driver. Every side (pin, ox) is profiled three ways: Callgrind phase
instruction counts, the Massif heap peak and the Massif mapped-page peak. The
six collections are independent: each has its own user store, reads only
instruction and allocation counters, and shares no file with another. They run
in tools/dev/pool.py's admitted pool, sized by the heavy slot's CPU and memory admission, and their report
lines print in the fixed order pin then ox, callgrind, heap, pages, whatever
order they finish in. `--jobs 1` runs them in that order with no pool.

Within one collection the attempts stay sequential: a run that overlapped a
host suspend is discarded and the next attempt, in a fresh user store, decides.
Nothing here is timed, resident-set or teardown measurement; those stay serial.
Standard library only, plus tools.dev.
"""
import argparse
import os
from pathlib import Path
import re
import runpy
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
from tools.dev import pool, slot

SIDES = ('pin', 'ox')
MODES = ('callgrind', 'heap', 'pages')
ATTEMPTS = 6
# A boot-time minus monotonic-time drift over this many ns means the host slept.
SUSPEND_LIMIT_NS = 1_000_000
RUNNER = 'run-system-import-profile.sh'


class Failure(Exception):
    """A collection failed. Its lines print, then its stderr bytes, then the message."""
    def __init__(self, message, stderr=b'', lines=()):
        super().__init__(message)
        self.message, self.stderr, self.lines = message, stderr, list(lines)


def suspend_offset():
    """Return CLOCK_BOOTTIME minus CLOCK_MONOTONIC in ns; it grows while the host sleeps."""
    return (time.clock_gettime_ns(time.CLOCK_BOOTTIME)
            - time.clock_gettime_ns(time.CLOCK_MONOTONIC))


def load_average():
    """Return the one-minute load average as text, as /proc/loadavg gives it."""
    return Path('/proc/loadavg').read_text().split()[0]


def valgrind_options(mode):
    """Return the Valgrind tool options for one mode; the profile goes to stderr."""
    if mode == 'callgrind':
        # Combining dumps keeps import-mode client requests on the same
        # supported device, without creating .1/.2/.3 files.
        return ['--tool=callgrind', '--cache-sim=no', '--branch-sim=no',
                '--combine-dumps=yes', '--callgrind-out-file=/dev/stderr']
    options = ['--tool=massif', '--time-unit=i', '--heap=yes',
               '--stacks=no', '--massif-out-file=/dev/stderr']
    if mode == 'pages':
        options += ['--pages-as-heap=yes']
    return options


def report(side, mode, workload, stderr):
    """Validate one run's profile and return its report lines.

    Callgrind must hold one dump per phase; Massif must hold at least one heap
    snapshot. Anything else raises Failure.
    """
    if mode == 'callgrind':
        counts = [int(value) for value in re.findall(rb'^summary: (\d+)\s*$', stderr, re.M)]
        phases = ('cold-open',) if workload == '--cold-open' else ('open', 'import', 'save', 'close')
        if len(counts) != len(phases):
            raise Failure(f'{side}: expected {len(phases)} Callgrind dumps, got {len(counts)}')
        return [*(f'{side} {phase}: {count} instructions' for phase, count in zip(phases, counts)),
                f'{side} total: {sum(counts)} instructions']
    values = [int(value) for value in re.findall(rb'^mem_heap_B=(\d+)\s*$', stderr, re.M)]
    if not values:
        raise Failure(f'{side}: missing Massif heap snapshots')
    return [f'{side} peak {mode}: {max(values)} bytes']


def collect(side, mode, plan, environment, context):
    """Profile one side in one mode; return its report lines.

    The attempts are sequential and each uses a fresh user store of its own.
    A run that overlapped a host suspend is discarded and retried; any other
    failure, or six discards, ends the whole run. The child gets ENVIRONMENT
    as given, the same in every mode, and is launched and reaped by CONTEXT.
    """
    lines = []
    for attempt in range(1, ATTEMPTS + 1):
        user = plan['work'] / f'{side}-{mode}-user-{attempt}'
        before = suspend_offset()
        utc = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
        load = load_average()
        status, stdout, stderr = context.launch(
            ['valgrind', '--vgdb=no', '--log-fd=2', *valgrind_options(mode),
             str(plan['work'] / 'driver'), str(plan[side]), str(plan['data']), str(user), plan['workload']],
            env=environment)
        suspended = suspend_offset() - before
        if suspended > SUSPEND_LIMIT_NS:
            lines.append(f'DISCARD {side} {mode}: suspend_delta_ns={suspended}')
            continue
        if status or stdout or b'no-user-state' in stderr or not (user / 'user.conf').is_file():
            raise Failure(f'{side} {mode}: failed workload or user-store open', stderr, lines)
        lines.append(f'{side} {mode}: utc={utc} load1={load} '
                     f'suspend_delta_ns={suspended} exit=0 stdout_bytes=0 '
                     'user.conf=present no-user-state=absent')
        break
    else:
        raise Failure('repeated suspend-overlapping runs', lines=lines)
    try:
        return lines + report(side, mode, plan['workload'], stderr)
    except Failure as failure:
        failure.lines = lines
        raise


def run_collections(plan, jobs, emit):
    """Collect every side and mode; emit(side, mode, lines) runs in fixed order.

    The six collections are units of tools/dev/pool.py's admitted pool; --jobs 1
    limits the admitted pool to one worker. Emission is in this thread, pin then ox and
    callgrind, heap, pages, as each result becomes the next in that order. Every
    child gets the environment captured here, because Callgrind counts move with
    its size and serial and pooled runs must agree.
    """
    environment = dict(os.environ)
    units = [(f'{side}-{mode}', lambda context, side=side, mode=mode:
              collect(side, mode, plan, environment, context)) for side in SIDES for mode in MODES]
    keys = {f'{side}-{mode}': (side, mode) for side in SIDES for mode in MODES}
    return pool.run_admitted(units, 'system-import-profile collections', cancelled=slot.CANCELLED.is_set,
                             serial=jobs == 1, max_workers=jobs, emit=lambda name, lines: emit(*keys[name], lines),
                             cancel_error=slot.Cancelled)


def positive(text):
    """Parse a positive integer option value."""
    value = int(text)
    if value < 1:
        raise argparse.ArgumentTypeError('must be a positive integer')
    return value


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('pin', type=Path)
    parser.add_argument('ox', type=Path)
    parser.add_argument('data', type=Path)
    parser.add_argument('work', type=Path, help='directory holding the compiled driver; user stores go here')
    parser.add_argument('workload', help='iteration count, or --cold-open')
    parser.add_argument('--jobs', type=positive,
                        help='maximum concurrent collections, reduced by admission '
                             '(default: as many as admission allows; 1 runs them in order)')
    args = parser.parse_args(argv)
    check_tkrzw = runpy.run_path(str(ROOT / 'tools/tkrzw/runtime.py'))['check']
    plan = dict(pin=args.pin, ox=args.ox, data=args.data, work=args.work, workload=args.workload)
    # The sound-libtkrzw check precedes every collection, for both sides.
    for side in SIDES:
        check_tkrzw('auto', RUNNER, side, plan[side])

    def emit(side, mode, lines):
        for line in lines:
            print(line, flush=True)
    try:
        with slot.cancellation_handlers():
            run_collections(plan, args.jobs, emit)
    except Failure as failure:
        for line in failure.lines:
            print(line, flush=True)
        if failure.stderr:
            sys.stderr.buffer.write(failure.stderr)
            sys.stderr.buffer.flush()
        raise SystemExit(failure.message) from None
    except (OSError, ValueError) as error:
        # Admission refusals and cancellation, after every child is reaped.
        raise SystemExit(str(error)) from None
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
