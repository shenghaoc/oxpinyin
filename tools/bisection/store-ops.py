#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Paired store-operation comparison of two builds of one backend.

Drives the `backend_matrix_<backend>` bench binary's `--op` mode
(`crates/oxpinyin-store/benches/support/ops.rs`) for a base build, a
same-bytes control (the base binary copied to another path) and a candidate
build, one fresh process per sample, rotating the order within each round.
The statistics are `teardown.py`'s, so the §7.5 rules
(`docs/findings/tooling-consolidation-python-just.md`) are applied by the
same code:

- timing: ns per store call; the control/base paired ratio and its whole
  95% bootstrap interval must lie in [0.95, 1.05], or the operation's
  candidate/base ratio is withheld (INCONCLUSIVE);
- residency: process VmHWM in KiB; control − base must lie within
  ±max(64 KiB, 1% of |median(base)|), or the candidate − base difference is
  withheld;
- Callgrind `Ir` inside `ops::measured`, and DHAT's whole-process totals
  minus a `--iters 0` run: control must equal base exactly, or the
  comparison is INVALID and no figures are printed.

Every sample's checksum must equal the base's first: the builds must do the
same work on the same bytes, or the operation is NOT COMPARABLE.

Writes one JSON record per line to --out (default stdout). Every record is
flushed, and fsynced when --out is a file; a failed write or flush stops the
run with the cause named and exit status 3, never a silent exit. Commits and
uploads nothing.

--base and --candidate must each be built from a tree that includes the
`--op` harness: a bench built without it has no `--op` or `--list-ops`. For
an adapter comparison, build --base from the harness commit itself, so that
the adapter code is the only difference between the two builds.

Usage:
    store-ops.py --base A --candidate B --data DIR --work DIR [--ops a,b]
                 [--rounds 30] [--warmups 3] [--cpu N] [--out FILE]
                 [--profile | --profile-only] [--keep-profiles]
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import teardown as td

SIDES = ('base', 'control', 'candidate')
WRITE_FAILED = 3
OUT = sys.stdout


class WriteFailed(Exception):
    pass


def record(kind, **values):
    line = json.dumps(dict(kind=kind, **values), sort_keys=True) + '\n'
    try:
        OUT.write(line)
        OUT.flush()
        if OUT is not sys.stdout:
            os.fsync(OUT.fileno())
    except OSError as error:
        raise WriteFailed(f'cannot write results to {getattr(OUT, "name", "stdout")}: '
                          f'{error.strerror or error} (errno {error.errno})') from error


def parse(text):
    values = dict(line.split('=', 1) for line in text.splitlines() if '=' in line)
    if values.get('vmhwm_kib') in (None, 'unavailable'):
        raise ValueError('worker printed no VmHWM')
    return dict(op=values['op'], iters=int(values['iters']), units=int(values['units']),
                ns=int(values['ns']), checksum=values['checksum'], vmhwm_kib=int(values['vmhwm_kib']))


def run(binary, op, iters, data, cpu, prefix=()):
    command = [*prefix, str(binary), '--op', op, '--iters', str(iters), '--data', str(data)]
    if cpu is not None:
        command = ['taskset', '-c', str(cpu), *command]
    done = subprocess.run(command, capture_output=True, text=True, check=False)
    if done.returncode:
        raise RuntimeError(f'{op} failed under {binary}: {done.stderr[-2000:]}')
    return done


def callgrind_ir(binary, op, iters, data, work, keep=None):
    out = keep or work / f'cg-{os.getpid()}.out'
    run(binary, op, iters, data, None, ('valgrind', '--tool=callgrind', '--toggle-collect=*ops::measured*',
                                        f'--callgrind-out-file={out}'))
    try:
        for line in out.read_text().splitlines():
            if line.startswith(('totals:', 'summary:')):
                return int(line.split()[1])
    finally:
        if keep is None:
            out.unlink(missing_ok=True)
    raise ValueError('callgrind printed no totals')


def dhat_totals(binary, op, iters, data, work):
    out = work / f'dhat-{os.getpid()}.json'
    run(binary, op, iters, data, None, ('valgrind', '--tool=dhat', f'--dhat-out-file={out}'))
    try:
        profile = json.loads(out.read_text())
    finally:
        out.unlink(missing_ok=True)
    points = profile['pps']
    return dict(blocks=sum(point['tbk'] for point in points), bytes=sum(point['tb'] for point in points),
                peak_bytes=sum(point['gb'] for point in points))


def compare_profiles(op, iters, units, values):
    """`values[side]` holds the exact counts per side; each must equal base's in the control."""
    control = {key: td.exact_control(values['base'][key], values['control'][key]) for key in values['base']}
    valid = all(check['verdict'] == 'VALID' for check in control.values())
    shown = dict(per_call={side: {key: value if key.startswith('peak') else value / units
                                  for key, value in values[side].items()} for side in SIDES})
    record('profile', op=op, iters=iters, units=units, verdict='VALID' if valid else 'INVALID',
           control=control, **(shown if valid else {}))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--base', type=Path, required=True)
    parser.add_argument('--candidate', type=Path, required=True)
    parser.add_argument('--data', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True, help='owned directory for copies and profiler output')
    parser.add_argument('--ops', help='comma-separated subset of --list-ops')
    parser.add_argument('--rounds', type=int, default=30)
    parser.add_argument('--warmups', type=int, default=3)
    parser.add_argument('--cpu', type=int)
    parser.add_argument('--profile', action='store_true', help='also run Callgrind and DHAT')
    parser.add_argument('--profile-only', action='store_true',
                        help='run only Callgrind and DHAT: no timing or residency samples')
    parser.add_argument('--keep-profiles', action='store_true',
                        help='keep each Callgrind output as <work>/callgrind/<op>.<side>.out')
    parser.add_argument('--out', type=Path, help='file for the JSON records (default stdout)')
    parser.add_argument('--profile-divisor', type=int, default=10,
                        help='profiler runs use the default iterations divided by this')
    args = parser.parse_args()
    global OUT
    if args.out:
        try:
            OUT = open(args.out, 'w', encoding='utf-8')
        except OSError as error:
            print(f'store-ops: cannot open {args.out}: {error.strerror} (errno {error.errno})', file=sys.stderr)
            return WRITE_FAILED
    try:
        status = compare(args, parser)
        if OUT is sys.stdout:
            sys.stdout.flush()
        else:
            OUT.close()
        return status
    except WriteFailed as error:
        try:
            print(f'store-ops: {error}', file=sys.stderr, flush=True)
        finally:
            # Skip interpreter shutdown: its flush of the unwritten buffer
            # would fail again and replace this status with 120.
            os._exit(WRITE_FAILED)


def compare(args, parser):
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    binaries = {}
    # Equal-length paths: the binary's own path is part of the process's
    # heap (argv, current_exe), and the exact-count control compares it.
    for index, (side, source) in enumerate((('base', args.base), ('control', args.base),
                                            ('candidate', args.candidate))):
        target = work / f'subject{index}' / 'backend_matrix'
        target.parent.mkdir(exist_ok=True)
        shutil.copyfile(source, target)
        target.chmod(0o755)
        binaries[side] = target
    listed = subprocess.run([str(binaries['base']), '--list-ops'], capture_output=True, text=True, check=True)
    defaults = dict(line.split() for line in listed.stdout.splitlines())
    ops = args.ops.split(',') if args.ops else list(defaults)
    if any(op not in defaults for op in ops):
        parser.error('unknown --ops entry')
    record('start', binaries={side: dict(path=str(path), sha256=td.digest_bytes(path.read_bytes()),
                                         bytes=path.stat().st_size) for side, path in binaries.items()},
           data=td.identity(td.file_set(args.data)), cpu=args.cpu, rounds=args.rounds, warmups=args.warmups,
           profile_only=args.profile_only, control_rule=list(td.CONTROL_INTERVAL),
           residency_rule=dict(floor_kib=td.RESIDENCY_FLOOR_KIB, fraction=td.RESIDENCY_FRACTION))
    for op in ops:
        iters = int(defaults[op])
        if args.profile_only:
            rows = {side: parse(run(binaries[side], op, iters, args.data, args.cpu).stdout) for side in SIDES}
            for side, row in rows.items():
                if (row['checksum'], row['units']) != (rows['base']['checksum'], rows['base']['units']):
                    record('equivalence', op=op, verdict='NOT COMPARABLE', side=side, sample=row, reference=rows['base'])
                    return 2
            profile(args, binaries, op, iters, rows['base']['units'], work)
            continue
        samples = {side: [] for side in SIDES}
        reference = None
        for index in range(args.rounds + args.warmups):
            order = list(SIDES[index % 3:] + SIDES[:index % 3])
            for side in order:
                row = parse(run(binaries[side], op, iters, args.data, args.cpu).stdout)
                reference = reference or row
                if (row['checksum'], row['units']) != (reference['checksum'], reference['units']):
                    record('equivalence', op=op, verdict='NOT COMPARABLE', side=side, sample=row, reference=reference)
                    return 2
                if index >= args.warmups:
                    samples[side].append(row)
        units = reference['units']
        per_call = {side: [row['ns'] / units for row in rows] for side, rows in samples.items()}
        control = td.stable_control(per_call['base'], per_call['control'])
        record('time', op=op, iters=iters, units=units, unit='ns per call',
               summaries={side: td.summary(values) for side, values in per_call.items()},
               control=control, verdict='STABLE' if control['stable'] else 'INCONCLUSIVE',
               candidate_base_ratio=td.paired_ratio(per_call['candidate'], per_call['base']) if control['stable'] else None)
        kib = {side: [row['vmhwm_kib'] for row in rows] for side, rows in samples.items()}
        residency = td.stable_residency(kib['base'], kib['control'])
        record('residency', op=op, unit='KiB', metric='VmHWM',
               summaries={side: td.summary(values) for side, values in kib.items()},
               control=residency, verdict='STABLE' if residency['stable'] else 'INCONCLUSIVE',
               candidate_base_difference=td.paired_difference(kib['candidate'], kib['base']) if residency['stable'] else None)
        if args.profile:
            profile(args, binaries, op, iters, units, work)
    return 0


def profile(args, binaries, op, iters, units, work):
    profiled = max(1, iters // args.profile_divisor)
    units_profiled = units * profiled // iters
    kept = work / 'callgrind'
    if args.keep_profiles:
        kept.mkdir(exist_ok=True)
    irs = {side: dict(ir=callgrind_ir(binaries[side], op, profiled, args.data, work,
                                      kept / f'{op}.{side}.out' if args.keep_profiles else None))
           for side in SIDES}
    compare_profiles(op + ':callgrind', profiled, units_profiled, irs)
    heaps = {}
    for side in SIDES:
        full = dhat_totals(binaries[side], op, profiled, args.data, work)
        setup = dhat_totals(binaries[side], op, 0, args.data, work)
        heaps[side] = dict(blocks=full['blocks'] - setup['blocks'], bytes=full['bytes'] - setup['bytes'],
                           peak_bytes_whole_process=full['peak_bytes'])
    compare_profiles(op + ':dhat', profiled, units_profiled, heaps)


if __name__ == '__main__':
    raise SystemExit(main())
