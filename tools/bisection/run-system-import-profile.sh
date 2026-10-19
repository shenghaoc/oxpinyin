#!/usr/bin/env bash
# #599: instruction counts (including phase breakdown), peak heap, and mapped
# pages for import/save or cold open. Profiling output stays in memory;
# the new work directory contains only the driver and fresh user profiles.
# Usage: <pin-so> <ox-so> <system-data-dir> <new-work-dir> [iterations|--cold-open]
set -euo pipefail
pin=$(realpath "${1:?pin-so}")
ox=$(realpath "${2:?ox-so}")
data=$(realpath "${3:?system-data-dir}")
out=${4:?new-output-dir}
iterations=${5:-1000}
mkdir "$out"
out=$(realpath "$out")
root=$(cd "$(dirname "$0")" && pwd)
cc -std=gnu11 -Wall -Wextra -Werror -O2 "$root/system-import-profile.c" -ldl -o "$out/driver"
uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal - "$pin" "$ox" "$data" "$out" "$iterations" <<'PY'
from pathlib import Path
import re
import subprocess
import sys
import time

pin, ox, data, work, workload = sys.argv[1:]
root = Path(work)
cold = workload == '--cold-open'


def suspend_offset():
    return (time.clock_gettime_ns(time.CLOCK_BOOTTIME)
            - time.clock_gettime_ns(time.CLOCK_MONOTONIC))


for side, so in (('pin', pin), ('ox', ox)):
    for mode in ('callgrind', 'heap', 'pages'):
        if mode == 'callgrind':
            # Combining dumps keeps import-mode client requests on the same
            # supported device, without creating .1/.2/.3 files.
            options = ['--tool=callgrind', '--cache-sim=no', '--branch-sim=no',
                       '--combine-dumps=yes', '--callgrind-out-file=/dev/stderr']
        else:
            options = ['--tool=massif', '--time-unit=i', '--heap=yes',
                       '--stacks=no', '--massif-out-file=/dev/stderr']
            if mode == 'pages':
                options += ['--pages-as-heap=yes']
        for attempt in range(1, 7):
            user = root / f'{side}-{mode}-user-{attempt}'
            before = suspend_offset()
            utc = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
            load = Path('/proc/loadavg').read_text().split()[0]
            run = subprocess.run(
                ['valgrind', '--vgdb=no', '--log-fd=2', *options,
                 str(root / 'driver'), so, data, str(user), workload],
                input=b'', stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            suspended = suspend_offset() - before
            if suspended > 1_000_000:
                print(f'DISCARD {side} {mode}: suspend_delta_ns={suspended}', flush=True)
                continue
            if (run.returncode or run.stdout or
                    b'no-user-state' in run.stderr or
                    not (user / 'user.conf').is_file()):
                sys.stderr.buffer.write(run.stderr)
                raise SystemExit(f'{side} {mode}: failed workload or user-store open')
            print(f'{side} {mode}: utc={utc} load1={load} '
                  f'suspend_delta_ns={suspended} exit=0 stdout_bytes=0 '
                  'user.conf=present no-user-state=absent', flush=True)
            break
        else:
            raise SystemExit('repeated suspend-overlapping runs')
        if mode == 'callgrind':
            counts = [int(value) for value in re.findall(rb'^summary: (\d+)\s*$', run.stderr, re.M)]
            phases = ('cold-open',) if cold else ('open', 'import', 'save', 'close')
            if len(counts) != len(phases):
                raise SystemExit(f'{side}: expected {len(phases)} Callgrind dumps, got {len(counts)}')
            for phase, count in zip(phases, counts):
                print(f'{side} {phase}: {count} instructions')
            print(f'{side} total: {sum(counts)} instructions', flush=True)
        else:
            values = [int(value) for value in re.findall(rb'^mem_heap_B=(\d+)\s*$', run.stderr, re.M)]
            if not values:
                raise SystemExit(f'{side}: missing Massif heap snapshots')
            print(f'{side} peak {mode}: {max(values)} bytes', flush=True)
PY
