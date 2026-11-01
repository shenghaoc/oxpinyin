"""Session-parse scaling: time, instructions and peak heap by input length (#1501).

`measure parse-scaling` runs the bisect harness's `--perf-parse` workload:
one pinyin_parse_more_full_pinyins call on a fresh instance over a generated
input of each shape and length (tools/bisection/parse_perf.h). Every arm
(`--arm NAME=LIBRARY`, for example base, branch and pin) is timed in ROUNDS
fresh processes pinned to one CPU, the arm order rotating each round (#736's
per-call method), then counted once under Callgrind (the harness's
bisect_parse_unit anchor) and once under the heap-counting harness build.
Each arm must give the same answer in every process. Per-byte costs divide
by the input length; ratios pair each round's samples, later arm over
earlier arm.
"""
import argparse
import itertools
import json
import os
from pathlib import Path
import re

from .. import slot
from ..runners import emit
from . import bench, identity, stats

ANCHOR = 'bisect_parse_unit'
SHAPES = ('ni', 'apostrophe')
HEAP_FLAGS = ('-DBISECT_HEAP_COUNTER', '-rdynamic')


def counts(text):
    values = [int(value) for value in text.split(',')]
    if not values or any(value < 4 or value > 1 << 30 for value in values):
        raise argparse.ArgumentTypeError('expected comma-separated lengths in 4..2^30')
    return values


def arm(text):
    name, separator, library = text.partition('=')
    if not separator or not re.fullmatch('[a-z][a-z0-9-]{0,15}', name) or not library:
        raise argparse.ArgumentTypeError('expected NAME=LIBRARY')
    return name, Path(library)


def arguments(argv):
    parser = argparse.ArgumentParser(prog='measure parse-scaling', description=__doc__)
    parser.add_argument('--cell', required=True, choices=('bdb', 'kc', 'tkrzw'))
    parser.add_argument('--arm', dest='arms', action='append', type=arm, required=True,
                        help='NAME=LIBRARY, repeatable; ratios are later arm over earlier arm')
    parser.add_argument('--system-dir', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--shapes', default=','.join(SHAPES))
    parser.add_argument('--lengths', type=counts, default=[4096, 32767, 65536, 262144])
    parser.add_argument('--rounds', type=int, default=7)
    parser.add_argument('--cpu', type=int, default=min(os.sched_getaffinity(0)))
    parser.add_argument('--no-callgrind', action='store_true', help='skip the instruction counts')
    args = parser.parse_args(argv)
    args.shapes = args.shapes.split(',')
    if any(shape not in SHAPES for shape in args.shapes):
        parser.error('--shapes takes ' + ','.join(SHAPES))
    if args.rounds < 2:
        parser.error('--rounds must be at least 2')
    names = [name for name, _ in args.arms]
    if len(set(names)) != len(names):
        parser.error('--arm names must differ')
    if args.cpu not in os.sched_getaffinity(0):
        parser.error('--cpu is outside this process affinity')
    args.arms = [(name, library.resolve(strict=True)) for name, library in args.arms]
    args.system_dir = args.system_dir.resolve(strict=True)
    args.out = args.out.absolute()
    if args.out.exists():
        parser.error('--out must not already exist')
    return args


def observation(raw, shape, length):
    """Validate one worker line; return it as a dictionary."""
    row = json.loads(raw)
    if (row.get('shape') != shape or row.get('length') != length or
            any(type(row.get(key)) is not int or row[key] < 0 for key in ('pid', 'parsed', 'stored', 'elapsed_ns'))):
        raise ValueError('invalid worker reply')
    if row['parsed'] != row['stored'] or not 0 < row['parsed'] <= length:
        raise ValueError('parse answer and stored length disagree')
    return row


def callgrind_total(text):
    """The Ir total of a Callgrind profile (its `totals:` or `summary:` line)."""
    for line in text.splitlines():
        if line.startswith(('totals:', 'summary:')):
            return int(line.split()[1])
    raise ValueError('profile has no totals')


def launch(args, env, harness, library, shape, length, prefix=()):
    command = [*prefix, harness, '--perf-parse', library, args.system_dir, shape, length]
    child = dict(env, LD_LIBRARY_PATH=bench.loader_path(library.parent, env.get('LD_LIBRARY_PATH')))
    side = 'pin' if library.name == 'libpinyin.so' else 'subject'
    bench.guard('parse-scaling', args.cell, side, library, command, env=child)
    done = bench.capture(bench.pinned(command, args.cpu), env=child)
    if done.status:
        bench.fail(f'{library}: {shape} {length}: worker failed ({done.status}): '
                   + done.stderr.decode(errors='replace')[-400:])
    try:
        return observation(done.stdout.decode().strip().splitlines()[-1], shape, length)
    except (ValueError, IndexError) as error:
        bench.fail(f'{library}: {shape} {length}: {error}', 3)


def measure(args, env, harness, heap_harness, shape, length):
    names = [name for name, _ in args.arms]
    libraries = dict(args.arms)
    samples = {name: [] for name in names}
    answers = {}
    for round_index in range(args.rounds):
        order = names[round_index % len(names):] + names[:round_index % len(names)]
        for name in order:
            row = launch(args, env, harness, libraries[name], shape, length)
            if answers.setdefault(name, row['parsed']) != row['parsed']:
                bench.fail(f'{name}: {shape} {length}: answers differ across processes', 3)
            samples[name].append(row['elapsed_ns'])
    rows = {}
    for name in names:
        heap = launch(args, env, heap_harness, libraries[name], shape, length)
        if heap['parsed'] != answers[name]:
            bench.fail(f'{name}: {shape} {length}: heap run answer differs', 3)
        instructions = None
        if not args.no_callgrind:
            profile = args.out / f'{name}.{shape}.{length}.callgrind'
            prefix = ['valgrind', '--tool=callgrind', '--collect-atstart=no', '--toggle-collect=' + ANCHOR,
                      '--cache-sim=no', '--branch-sim=no', '--callgrind-out-file=' + str(profile)]
            if launch(args, env, harness, libraries[name], shape, length, prefix)['parsed'] != answers[name]:
                bench.fail(f'{name}: {shape} {length}: Callgrind run answer differs', 3)
            instructions = callgrind_total(profile.read_text())
        median = stats.summary(samples[name])['median']
        rows[name] = dict(parsed=answers[name], ns=samples[name], median_ns=median,
                          ns_per_byte=median / length, instructions=instructions,
                          instructions_per_byte=None if instructions is None else instructions / length,
                          heap_peak_bytes=heap['heap_peak_bytes'],
                          heap_per_byte=heap['heap_peak_bytes'] / length)
    ratios = {f'{later}/{earlier}': stats.paired_ratio(samples[later], samples[earlier])
              for earlier, later in itertools.combinations(names, 2)}
    return dict(shape=shape, length=length, arms=rows, time_ratios=ratios)


def table(results, names):
    pairs = [f'{later}/{earlier}' for earlier, later in itertools.combinations(names, 2)]
    lines = ['| Input | Bytes | Arm | Parsed | Median ms | ns/byte | Ir | Ir/byte | Peak heap B | Heap B/byte |',
             '| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |']
    for result in results:
        for name in names:
            row = result['arms'][name]
            ir = '-' if row['instructions'] is None else f"{row['instructions']:,}"
            per = '-' if row['instructions'] is None else f"{row['instructions_per_byte']:.0f}"
            lines.append(f"| {result['shape']} | {result['length']} | {name} | {row['parsed']} | "
                         f"{row['median_ns'] / 1e6:.3f} | {row['ns_per_byte']:.1f} | {ir} | {per} | "
                         f"{row['heap_peak_bytes']:,} | {row['heap_per_byte']:.1f} |")
    lines += ['', '| Input | Bytes | ' + ' | '.join(f'{pair} time (median, 95% CI)' for pair in pairs) + ' |',
              '| --- | ---: |' + ' ---: |' * len(pairs)]
    for result in results:
        cells = [f"{ratio['median']:.3f} ({ratio['ci95'][0]:.3f}-{ratio['ci95'][1]:.3f})"
                 for ratio in (result['time_ratios'][pair] for pair in pairs)]
        lines.append(f"| {result['shape']} | {result['length']} | " + ' | '.join(cells) + ' |')
    return '\n'.join(lines) + '\n'


def campaign(args, env):
    for name, library in args.arms:
        backend = identity.library_backend(library)
        if backend != args.cell:
            bench.fail(f'{name}: expected {args.cell}, library requires {backend}')
    args.out.mkdir(parents=True)
    harness, heap_harness = args.out / 'bisect', args.out / 'bisect-heap'
    bench.run(bench.harness_build(harness, flags=(*bench.STRICT_FLAGS, '-g')), env=env)
    bench.run(bench.harness_build(heap_harness, flags=(*bench.STRICT_FLAGS, '-g', *HEAP_FLAGS)), env=env)
    results = [measure(args, env, harness, heap_harness, shape, length)
               for shape in args.shapes for length in args.lengths]
    names = [name for name, _ in args.arms]
    environment = dict(captured_utc=bench.utc_now(), cell=args.cell, cpu=args.cpu, rounds=args.rounds,
                       kernel=identity.uname('srvm'), cpu_model=identity.cpu_model(),
                       libraries={name: dict(path=str(library), sha256=identity.sha256(library))
                                  for name, library in args.arms})
    (args.out / 'result.json').write_text(json.dumps(dict(environment=environment, results=results), indent=2) + '\n')
    report = table(results, names)
    (args.out / 'table.md').write_text(report)
    emit(report)
    return 0


def run(argv, env):
    args = arguments(argv)
    with slot.cancellation_handlers():
        with slot.HeavyAdmission('parse-scaling measurement', dict(env), runner=True) as admission:
            return campaign(args, admission.env)


RECIPES = {'parse-scaling': run}
