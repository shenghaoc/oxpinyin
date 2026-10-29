"""Sentence workloads of the existing bisect/Callgrind campaign (#1348).

The no-argument legacy campaign remains in valgrind.py. These explicitly
selected workloads compare main/PR/pin, with an exact same-bytes instruction
control for each Rust arm. They do not extend startup's RandomState exception,
except that the typing sequence reports a varying control as DIAGNOSTIC when
every varying function is a RANDOMSTATE_TYPING scope.
Optional native rounds time the same worker with plain shipping objects.
"""
import argparse
import copy
import json
import os
from pathlib import Path
import re
import shutil

from .. import slot
from ..runners import emit
from ...bisection.teardown import callgrind_costs
from . import bench, identity, instructions, neutrality, stats

ANCHORS = {'sentence': 'bisect_sentence_unit', 'typing-sequence': 'bisect_typing_unit'}
ARMS = ('main', 'pr__', 'pin_', 'mctl', 'pctl')
# Std HashSet/HashMap with the default RandomState (per-process SipHash keys)
# in these engine functions change probe and group-tag collision counts, hence
# their own Ir and the equality memcmp calls they make (PhraseText::eq).
RANDOMSTATE_TYPING = frozenset((
    'dedup_by_text_keep_first', 'dedup_ranked_by_text', 'order_window_tail',
    '__memcmp_avx2_movbe',
))


def positive(text):
    if not re.fullmatch(r'[0-9]+', text) or not 1 <= int(text) <= 100000:
        raise argparse.ArgumentTypeError('expected a decimal count in 1..100000')
    return int(text)


def arguments(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workload', required=True, choices=ANCHORS)
    parser.add_argument('--cell', required=True, choices=('bdb', 'kc', 'tkrzw'))
    for name in ('main', 'pr', 'pin', 'main-shipping', 'pr-shipping', 'system-dir', 'out'):
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--input', default='nihaozhongguo')
    parser.add_argument('--rounds', type=positive, default=3)
    parser.add_argument('--iterations', type=positive, default=1)
    parser.add_argument('--native-rounds', type=positive, default=0,
                        help='also time plain shipping libraries in rotating native rounds (minimum 20)')
    parser.add_argument('--native-iterations', type=positive, default=20,
                        help='invocations per native process; independent of Callgrind --iterations')
    parser.add_argument('--cpu', type=int, default=min(os.sched_getaffinity(0)))
    args = parser.parse_args(argv)
    if args.rounds < 2:
        parser.error('--rounds must be at least 2 (fresh-process controls)')
    if args.native_rounds and args.native_rounds < 20:
        parser.error('--native-rounds must be at least 20 (paired timing intervals)')
    if not re.fullmatch("[a-z']{1,255}", args.input):
        parser.error('--input must contain 1..255 lowercase ASCII pinyin letters/apostrophes')
    if args.cpu not in os.sched_getaffinity(0):
        parser.error('--cpu is outside this process affinity')
    for name in ('main', 'pr', 'pin', 'main_shipping', 'pr_shipping'):
        path = getattr(args, name).resolve(strict=True)
        if not path.is_file():
            parser.error(f'{name}: expected a library file')
        setattr(args, name, path)
    args.system_dir = args.system_dir.resolve(strict=True)
    if not args.system_dir.is_dir():
        parser.error('--system-dir must be a complete exported system directory')
    args.out = args.out.absolute()
    if args.out.exists():
        parser.error('--out must not already exist')
    return args


def observation(raw, args):
    """Validate a complete worker reply before using its PID or any counters."""
    row = json.loads(raw)
    # Zero is a valid elapsed_ns: macOS CLOCK_MONOTONIC has 1 microsecond resolution, and the
    # campaign reports Callgrind Ir, not this timing.
    if (type(row.get('pid')) is not int or row['pid'] <= 0 or
            row.get('workload') != args.workload or row.get('iterations') != args.iterations or
            type(row.get('elapsed_ns')) is not int or row['elapsed_ns'] < 0):
        raise ValueError('invalid worker identity/workload/count')
    expected = ([args.input[:i] for i in range(1, len(args.input) + 1)]
                if args.workload == 'typing-sequence' else [args.input])
    observations = row.get('observations', [])
    if len(observations) != args.iterations:
        raise ValueError('wrong iteration count')
    for steps in observations:
        if [step.get('input') for step in steps] != expected:
            raise ValueError('missing/reordered workload steps')
        for step in steps:
            if (type(step.get('parsed')) is not int or step['parsed'] <= 0 or
                    step.get('guess_sentence') is not True or step.get('get_sentence') is not True or
                    not isinstance(step.get('sentence'), str)):
                raise ValueError('sentence operation failed')
            if args.workload == 'typing-sequence':
                if step.get('guess_candidates') is not True or not isinstance(step.get('candidates'), list):
                    raise ValueError('candidate operation failed')
    return row


def instruction_controls(counts):
    """No tolerance: all fresh-process counts of each identical pair must agree."""
    return {arm: dict(valid=len(set(counts[arm] + counts[control])) == 1,
                      spread=max(counts[arm] + counts[control]) - min(counts[arm] + counts[control]))
            for arm, control in (('main', 'mctl'), ('pr__', 'pctl'))}


def checked_profile(raw, executable, pid, library):
    result = instructions.total_profile(raw, executable, pid)
    costs = callgrind_costs(result['raw'], library, preserve_objects=True)
    if costs['totals']['Ir'] != result['total'] or result['total'] <= 0:
        raise ValueError('exclusive Ir does not reconcile with a positive total')
    result['functions'] = {}
    for row in costs['functions']:
        result['functions'][row['function']] = result['functions'].get(row['function'], 0) + row['Ir']
    return result


def randomstate_scopes(profiles):
    """Per identical pair, the functions whose Ir varies across its rounds.

    The pairs are main with mctl and pr__ with pctl. A function's Ir is never
    compared across main and PR, so a deterministic main/PR change cannot
    count as variation. Exclusive Ir is keyed by function name, since each arm
    loads its own library copy. Return the sets per pair and the entries of
    either set outside RANDOMSTATE_TYPING, as (pair, function).
    """
    by_pair = {}
    for pair, arms in (('main', ('main', 'mctl')), ('pr__', ('pr__', 'pctl'))):
        tables = [table for arm in arms for table in profiles[arm]]
        by_pair[pair] = sorted(name for name in set().union(*tables)
                               if len({table.get(name, 0) for table in tables}) > 1)
    base = lambda name: re.sub(r'::h[0-9a-f]{16}$', '', name).removeprefix('oxpinyin_engine::session::')
    outside = [dict(pair=pair, function=name) for pair, names in by_pair.items() for name in names
               if base(name) not in RANDOMSTATE_TYPING]
    return by_pair, outside


def prepare(args, env):
    libraries = dict(main=args.main, pr__=args.pr, pin_=args.pin,
                     mctl=args.main, pctl=args.pr)
    for arm, library in libraries.items():
        backend = identity.library_backend(library)
        if backend != args.cell:
            bench.fail(f'{arm}: expected {args.cell}, library requires {backend}')
        bench.runtime_check('sentence campaign', args.cell, 'pin' if arm == 'pin_' else 'subject',
                            library, env=env)
    for arm, profile, shipping in (('main', args.main, args.main_shipping),
                                    ('pr', args.pr, args.pr_shipping)):
        info = bench.capture(['readelf', '--debug-dump=info', profile], env=env)
        if info.status or b'Compilation Unit @' not in info.stdout:
            bench.fail(arm + ': profiled library has no readable DWARF compilation units')
        code, report = neutrality.compare(profile, shipping)
        emit(report)
        if code:
            bench.fail(arm + ': debug-info neutrality failed')
    args.out.mkdir()
    copies = {}
    for arm, library in libraries.items():
        directory = args.out / arm
        directory.mkdir()
        copy = directory / 'libpinyin.so'
        shutil.copyfile(library, copy)
        if identity.sha256(copy) != identity.sha256(library):
            bench.fail(arm + ': library copy is not byte-identical')
        copies[arm] = copy
    return copies


def native_campaign(args, env, harness):
    """Time the same scoped worker natively, using plain shipping objects.

    Every round pairs base/PR and same-bytes controls, with the pin reported
    independently. No profiler prefix or Valgrind elapsed_ns is used. The
    shared statistics report intervals and control stability; no time-change
    tolerance is invented here.
    """
    directory = args.out / 'native'
    directory.mkdir()
    sources = dict(main=args.main_shipping, pr__=args.pr_shipping,
                   pin_=args.pin, mctl=args.main_shipping, pctl=args.pr_shipping)
    libraries = {}
    for arm, source in sources.items():
        destination = directory / arm / 'libpinyin.so'
        destination.parent.mkdir()
        shutil.copyfile(source, destination)
        if identity.sha256(source) != identity.sha256(destination):
            bench.fail(arm + ': native library copy is not byte-identical')
        libraries[arm] = destination
    observed = copy.copy(args)
    observed.iterations = args.native_iterations
    samples = {arm: [] for arm in ARMS}
    answers = {}
    for round_index in range(args.native_rounds):
        order = ARMS[round_index % len(ARMS):] + ARMS[:round_index % len(ARMS)]
        for arm in order:
            library = libraries[arm]
            command = [str(harness), '--perf-' + args.workload, str(library),
                       str(args.system_dir), args.input, str(args.native_iterations)]
            child_env = dict(env, LD_LIBRARY_PATH=bench.loader_path(library.parent,
                                                                  env.get('LD_LIBRARY_PATH')))
            bench.guard('native sentence campaign', args.cell, 'pin' if arm == 'pin_' else 'subject',
                        library, command, env=child_env)
            done = bench.capture(bench.pinned(command, args.cpu), env=child_env)
            stem = directory / f'{arm}.{round_index}'
            Path(str(stem) + '.stdout').write_bytes(done.stdout)
            Path(str(stem) + '.stderr').write_bytes(done.stderr)
            if done.status:
                bench.fail(f'{arm} native round {round_index}: worker failed ({done.status})')
            try:
                row = observation(done.stdout, observed)
            except (ValueError, KeyError, TypeError) as error:
                bench.fail(f'{arm}: invalid native observation: {error}', 3)
            if row['elapsed_ns'] <= 0:
                bench.fail(arm + ': native elapsed_ns must be positive', 3)
            if arm in answers and answers[arm] != row['observations']:
                bench.fail(arm + ': native fresh-process observations differ', 3)
            answers[arm] = row['observations']
            samples[arm].append(row['elapsed_ns'] / args.native_iterations)
    if not (answers['main'] == answers['mctl'] == answers['pr__'] == answers['pctl']):
        bench.fail('native base/PR/control observations differ', 3)
    controls = {arm: stats.stable_control(samples[arm], samples[control])
                for arm, control in (('main', 'mctl'), ('pr__', 'pctl'))}
    result = dict(verdict='VALID' if all(row['stable'] for row in controls.values()) else 'DIAGNOSTIC',
                  rounds=args.native_rounds, iterations=args.native_iterations,
                  ns_per_invocation=samples, summaries={arm: stats.summary(values) for arm, values in samples.items()},
                  pr_over_main=stats.paired_ratio(samples['pr__'], samples['main']), controls=controls,
                  main_pr_observations_equal=True, main_pin_observations_equal=answers['main'] == answers['pin_'])
    (args.out / 'native.json').write_text(json.dumps(result, indent=2) + '\n')
    emit(json.dumps(result, indent=2))
    return result


def campaign(args, env):
    libraries = prepare(args, env)
    harness = args.out / 'bisect'
    bench.run(bench.harness_build(harness, flags=(*bench.STRICT_FLAGS, '-g')), env=env)
    nm = bench.capture(['nm', harness], env=env)
    symbols = {fields[2]: fields[0] for line in nm.stdout.splitlines()
               if len(fields := line.decode().split()) == 3}
    addresses = [symbols.get(anchor) for anchor in ANCHORS.values()]
    if nm.status or None in addresses or len(set(addresses)) != len(addresses):
        bench.fail('sentence collection anchors missing or folded together')
    metadata = dict(workload=args.workload, input=args.input, cell=args.cell,
                    iterations=args.iterations, rounds=args.rounds, cpu=args.cpu,
                    native_rounds=getattr(args, 'native_rounds', 0),
                    native_iterations=getattr(args, 'native_iterations', 20),
                    captured_utc=bench.utc_now(), system_dir=str(args.system_dir),
                    kernel=identity.uname('srvm'), cpu_model=identity.cpu_model(),
                    libraries={arm: dict(path=str(path), sha256=identity.sha256(path))
                               for arm, path in libraries.items()},
                    sources=dict(main=str(args.main), pr=str(args.pr), pin=str(args.pin)),
                    shipping=dict(main=str(args.main_shipping), pr=str(args.pr_shipping)))
    (args.out / 'environment.json').write_text(json.dumps(metadata, indent=2) + '\n')
    if getattr(args, 'native_rounds', 0):
        native_campaign(args, env, harness)
    counts = {arm: [] for arm in ARMS}
    profiles = {arm: [] for arm in ARMS}
    answers = {}
    creators = set()
    for round_index in range(args.rounds):
        # Rotate serial launches; never overlap a collection with another arm.
        order = ARMS[round_index % len(ARMS):] + ARMS[:round_index % len(ARMS)]
        for arm in order:
            stem = args.out / f'{arm}.{round_index}'
            profile = Path(str(stem) + '.callgrind')
            command = ['valgrind', '--tool=callgrind', '--collect-atstart=no',
                       '--toggle-collect=' + ANCHORS[args.workload], '--cache-sim=no',
                       '--branch-sim=no', '--separate-threads=no', '--callgrind-out-file=' + str(profile),
                       str(harness), '--perf-' + args.workload, str(libraries[arm]),
                       str(args.system_dir), args.input, str(args.iterations)]
            child_env = dict(env, LD_LIBRARY_PATH=bench.loader_path(libraries[arm].parent,
                                                                  env.get('LD_LIBRARY_PATH')))
            bench.guard('sentence campaign', args.cell, 'pin' if arm == 'pin_' else 'subject',
                        libraries[arm], command, env=child_env)
            (Path(str(stem) + '.command.json')).write_text(json.dumps(command) + '\n')
            done = bench.capture(bench.pinned(command, args.cpu), env=child_env)
            Path(str(stem) + '.stdout').write_bytes(done.stdout)
            Path(str(stem) + '.stderr').write_bytes(done.stderr)
            if done.status:
                bench.fail(f'{arm} round {round_index}: worker failed ({done.status}); see {stem}.stderr')
            try:
                row = observation(done.stdout, args)
                parsed = checked_profile(profile.read_bytes(), harness, row['pid'], libraries[arm])
            except (ValueError, KeyError, TypeError, OSError) as error:
                bench.fail(f'{arm} round {round_index}: invalid collection: {error}', 3)
            if arm in answers and answers[arm] != row['observations']:
                bench.fail(f'{arm}: fresh-process observations differ', 3)
            answers[arm] = row['observations']
            counts[arm].append(parsed['total'])
            profiles[arm].append(parsed['functions'])
            creators.add(parsed['creator'])
            emit(f'{args.workload} {arm} round {round_index + 1}: {parsed["total"]} Ir')
    controls = instruction_controls(counts)
    same_answers = (answers['main'] == answers['mctl'] and answers['pr__'] == answers['pctl'])
    equal = answers['main'] == answers['pr__']
    exact = all(control['valid'] for control in controls.values())
    valid = exact and same_answers and equal and len(creators) == 1
    verdict = 'VALID' if valid else 'INVALID'
    extra = {}
    if args.workload == 'typing-sequence' and not exact and same_answers and equal and len(creators) == 1:
        # 2026-10-23 diagnostic ruling: a control that varies only inside the
        # RandomState scopes is reported as ranges, never as an exact match.
        by_pair, outside = randomstate_scopes(profiles)
        extra = dict(rule='randomstate', varying_by_pair=by_pair,
                     varying_functions=sorted(set().union(*by_pair.values())), outside_allowlist=outside)
        if not outside:
            valid, verdict = True, 'DIAGNOSTIC'
    result = dict(verdict=verdict, counts=counts, controls=controls, **extra,
                  control_observations_equal=same_answers,
                  main_pr_observations_equal=equal,
                  main_pin_observations_equal=answers['main'] == answers['pin_'],
                  pr_pin_observations_equal=answers['pr__'] == answers['pin_'])
    if valid:
        # Ir is per invocation (one sentence or one complete prefix sequence).
        result['Ir_ranges'] = {arm: [min(values), max(values)] for arm, values in counts.items()}
        result['Ir_per_invocation'] = {arm: [value / args.iterations for value in values]
                                       for arm, values in counts.items()}
        result['main_minus_pr_bounds'] = [(min(counts['main']) - max(counts['pr__'])) / args.iterations,
                                         (max(counts['main']) - min(counts['pr__'])) / args.iterations]
    (args.out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    emit(json.dumps(result, indent=2))
    return 0 if valid else 3


def run(argv, env):
    args = arguments(argv)
    with slot.cancellation_handlers():
        with slot.HeavyAdmission('sentence Callgrind campaign', dict(env), runner=True) as admission:
            return campaign(args, admission.env)
