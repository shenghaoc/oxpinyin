"""Teardown workload for contract-diff.py and bisect.c. Standard library only.

The controller never runs in the measured native child. Observations, smaps and
profiler output remain in memory. Only private fixture inputs/manifests and the
native executable are materialized, under the caller's temporary work directory.
"""
import collections
import contextlib
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import random
import re
import selectors
import shutil
import statistics
import subprocess
import threading
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
FIELDS = ('Rss', 'Pss', 'Anonymous', 'Private_Clean', 'Private_Dirty', 'Shared_Clean')
PROFILES = ('empty', 'modest', 'phrases', 'journals')
CASES = (*PROFILES, 'modest-unsaved', 'modest-saved')
# Accepted before the first sample. Do not tune these from campaign output.
CONTROL_INTERVAL = (0.95, 1.05)
TIMING_ROUNDS, TIMING_WARMUPS = 60, 3
MEMORY_ROUNDS, MEMORY_WARMUPS = 30, 2


def sibling(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), HERE / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SMAPS = sibling('rss-smaps')


def add_arguments(parser):
    parser.add_argument('--teardown', action='store_true', help='native teardown measurement workload')
    parser.add_argument('--teardown-stage', choices=('provision', 'verify', 'pilot', 'confirm', 'repeat', 'multi', 'attribute'),
                        default='verify')
    parser.add_argument('--teardown-work-dir', type=Path, help='owned temporary ext4 directory; caller cleans it')
    parser.add_argument('--teardown-cases', default=','.join(CASES))
    parser.add_argument('--teardown-facades', choices=('both', 'pinyin', 'zhuyin'), default='both')
    parser.add_argument('--teardown-cpu', type=int)
    parser.add_argument('--teardown-attribution-case', choices=CASES, default='journals')


def digest_bytes(value):
    return hashlib.sha256(value).hexdigest()


def file_set(directory):
    return {str(path.relative_to(directory)): (path.stat().st_size, digest_bytes(path.read_bytes()))
            for path in sorted(Path(directory).rglob('*')) if path.is_file()}


def identity(files):
    return digest_bytes(json.dumps(files, sort_keys=True, separators=(',', ':')).encode())


def exports(facade):
    crate, version = ('oxpinyin-capi', 'libpinyin.ver') if facade == 'pinyin' else ('oxpinyin-zhuyin-capi', 'libzhuyin.ver')
    return re.findall(r'\b' + facade + r'_\w+(?=\s*;)', (ROOT / 'crates' / crate / version).read_text())


def build_worker(work):
    executable = work / 'bisect-teardown'
    command = ['cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2', '-g',
               '-o', str(executable), str(HERE / 'bisect.c'), '-ldl']
    subprocess.run(command, check=True)
    return executable, dict(command=command, sha256=digest_bytes(executable.read_bytes()),
                            bytes=executable.stat().st_size,
                            sources={name: digest_bytes((HERE / name).read_bytes())
                                     for name in ('bisect.c', 'teardown.h')})


def mapping_group(row, library, data):
    name, perms = row['name'], row['perms']
    if not name or name.startswith('[anon') or name in ('[heap]', '[stack]'):
        return 'ANON:' + (name or '[anonymous]')
    access = 'text' if 'x' in perms else 'writable' if 'w' in perms else 'readonly'
    path = name.removesuffix(' (deleted)')
    if path == str(library):
        return 'SUBJECT:' + access
    if path.startswith(str(data) + '/'):
        return 'DATA:' + str(Path(path).relative_to(data)) + ':' + access
    if '.so' in Path(path).name:
        return 'LIB:' + Path(path).name + ':' + access
    return 'OTHER:' + name + ':' + access


def read_memory(pid, library, data):
    proc = Path('/proc') / str(pid)
    with (proc / 'smaps').open() as handle:
        rows = SMAPS.parse_lines(handle, fields=FIELDS)
    totals = {field: sum(row[field] for row in rows) for field in FIELDS}
    rollup = {}
    for line in (proc / 'smaps_rollup').read_text().splitlines():
        key, _, value = line.partition(':')
        if key in FIELDS:
            rollup[key] = int(value.split()[0])
    # Pss is rounded per mapping; the rollup rounds after summation.
    for field in FIELDS:
        if field != 'Pss' and totals[field] != rollup.get(field):
            raise ValueError('smaps/rollup disagreement for ' + field)
    groups = {}
    for row in rows:
        group = groups.setdefault(mapping_group(row, library, data), dict.fromkeys(FIELDS, 0))
        for field in FIELDS:
            group[field] += row[field]
    return dict(totals=rollup, mapping_totals=totals, groups=groups,
                mappings=len(rows), fds=len(list((proc / 'fd').iterdir())))


def signed_delta(a, b):
    """a minus b, keeping negative values, including disappearing groups."""
    return {key: a.get(key, 0) - b.get(key, 0) for key in sorted(set(a) | set(b))}


def memory_deltas(checkpoints):
    result = {}
    for label, a, b in (('M1-M2', 'M1', 'M2'), ('M2-M3', 'M2', 'M3'), ('M3-M0', 'M3', 'M0')):
        left, right = checkpoints[a], checkpoints[b]
        groups = {key: signed_delta(left['groups'].get(key, {}), right['groups'].get(key, {}))
                  for key in sorted(set(left['groups']) | set(right['groups']))}
        result[label] = dict(totals=signed_delta(left['totals'], right['totals']), groups=groups,
                             fds=left['fds'] - right['fds'], mappings=left['mappings'] - right['mappings'])
        if left['allocator'] is not None and right['allocator'] is not None:
            result[label]['allocator'] = signed_delta(left['allocator'], right['allocator'])
    return result


def percentile(values, fraction):
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lo, hi = math.floor(position), math.ceil(position)
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (position - lo)


def summary(values):
    if not values or any(not math.isfinite(value) for value in values):
        raise ValueError('missing or nonfinite observations')
    middle = statistics.median(values)
    return dict(n=len(values), median=middle, minimum=min(values), maximum=max(values),
                p25=percentile(values, .25), p75=percentile(values, .75),
                range_over_median=(max(values) - min(values)) / abs(middle) if middle else None)


def paired_ratio(numerator, denominator):
    if len(numerator) != len(denominator) or not numerator or any(value <= 0 for value in denominator):
        raise ValueError('paired ratios require equal, positive-denominator samples')
    ratios = [a / b for a, b in zip(numerator, denominator)]
    generator = random.Random(0x074A2219)
    resamples = [statistics.median(generator.choices(ratios, k=len(ratios))) for _ in range(4000)]
    return dict(median=statistics.median(ratios), ci95=[percentile(resamples, .025), percentile(resamples, .975)])


def stable_control(ox, control):
    ratio = paired_ratio(control, ox)
    return dict(ratio=ratio, stable=CONTROL_INTERVAL[0] <= ratio['ci95'][0] <= ratio['median'] <=
                ratio['ci95'][1] <= CONTROL_INTERVAL[1], rule=list(CONTROL_INTERVAL))


def select_rows(exported, balanced=False):
    seen, selected, counts = set(), [], [0, 0, 0, 0]
    quotas = (3000, 3000, 3000, 1000)
    for library, phrase, pinyin, _ in exported:
        if phrase in seen or (balanced and counts[library - 1] == quotas[library - 1]):
            continue
        seen.add(phrase)
        counts[library - 1] += 1
        selected.append((library, phrase, pinyin, 20000))
        if len(selected) == 10000:
            break
    if len(selected) != 10000 or (balanced and tuple(counts) != quotas):
        raise ValueError('export cannot populate recovered fixture recipe')
    # Balanced selection joins the library groups in order, as the recipe does.
    return sorted(selected, key=lambda row: row[0]) if balanced else selected


def modest_rows():
    return [(library, phrase, pinyin, 5) for library in range(1, 5) for _ in range(10)
            for phrase, pinyin in (('测试', "ce4'shi4"), ('测侧', "ce4'ce4"))]


def row_text(rows):
    return ''.join(f'{library}\t{phrase}\t{pinyin}\t{count}\n' for library, phrase, pinyin, count in rows)


def witness_inputs(rows):
    found = []
    for _, _, pronunciation, _ in rows:
        value = re.sub(r'[0-9]', '', pronunciation)
        if value and len(value.encode()) <= 24 and value not in found:
            found.append(value)
            if len(found) == 400:
                break
    return found


class Campaign:
    def __init__(self, worker, data, work, cpu, driver):
        self.worker, self.data, self.work, self.cpu, self.driver = worker, data.resolve(), work, cpu, driver
        self.serial = 0

    def run(self, facade, library, profile, *, mode='time', save=False, scenario='first-candidate',
            rows=None, instances=1, cycles=1, inputs=None, reopen=False, profiler=None):
        self.serial += 1
        scratch = self.work / ('sample-%06d' % self.serial)
        scratch.mkdir()
        user = scratch / 'user'
        shutil.copytree(profile, user)
        expected_source = file_set(profile)
        checkpoints, samples, closed = {}, [], None
        command = ['taskset', '-c', str(self.cpu)]
        if profiler:
            command += ['valgrind', '--tool=' + profiler]
            if profiler == 'callgrind':
                command += ['--collect-atstart=no', '--cache-sim=no', '--branch-sim=no',
                            '--read-inline-info=yes', '--callgrind-out-file=/dev/stderr',
                            '--toggle-collect=bisect_teardown_free_unit',
                            '--toggle-collect=bisect_teardown_fini_unit']
            elif profiler == 'dhat':
                command += ['--dhat-out-file=/dev/stderr']
            else:
                raise ValueError('unknown profiler')
        command += [str(self.worker), '--teardown', facade, str(library), str(self.data), str(user),
                    mode, str(int(save)), str(instances), str(cycles), scenario, str(rows) if rows else '-']
        env = dict(os.environ, OXPINYIN_TEARDOWN_EXPORTS=','.join(exports(facade)))
        if inputs is not None:
            env['OXPINYIN_TEARDOWN_INPUTS'] = str(inputs)
        errors, documents, boundaries = bytearray(), [], []
        self.driver.check_cancelled()
        try:
            with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  env=env, start_new_session=True) as process:
                def drain():
                    while block := process.stderr.read(65536):
                        errors.extend(block)
                reader = threading.Thread(target=drain)
                reader.start()
                selector = selectors.DefaultSelector()
                selector.register(process.stdout, selectors.EVENT_READ)
                buffered = bytearray()
                deadline = time.monotonic() + (1800 if profiler else 120)
                try:
                    while True:
                        self.driver.check_cancelled()
                        if time.monotonic() >= deadline:
                            raise subprocess.TimeoutExpired(command, deadline)
                        if not selector.select(.1):
                            continue
                        block = os.read(process.stdout.fileno(), 65536)
                        if not block:
                            if buffered:
                                raise ValueError('unterminated native response')
                            break
                        buffered.extend(block)
                        while b'\n' in buffered:
                            line, _, rest = buffered.partition(b'\n')
                            buffered = bytearray(rest)
                            if mode == 'export':
                                documents.append(line.decode())
                                continue
                            document = json.loads(line)
                            if 'checkpoint' in document:
                                point, cycle = document['checkpoint'], document['cycle']
                                boundaries.append((point, cycle))
                                if point == 'reset':
                                    shutil.rmtree(user)
                                    shutil.copytree(profile, user)
                                elif cycles == 1 or cycle in (0, 1, 10, 50, 100):
                                    memory = read_memory(process.pid, library.resolve(), self.data)
                                    memory['allocator'] = document['allocator']
                                    checkpoints.setdefault(cycle, {})[point] = memory
                                process.stdin.write(b'\n')
                                process.stdin.flush()
                            elif document.get('teardown') == 'sample':
                                samples.append(document)
                            elif document.get('teardown') == 'closed':
                                closed = document
                            else:
                                documents.append(document)
                    status = process.wait(timeout=10)
                except BaseException:
                    self.driver.terminate_group(process)
                    raise
                finally:
                    selector.close()
                    process.stdin.close()
                    reader.join()
            self.driver.check_cancelled()
            if status:
                raise ValueError(f'native teardown failed ({status}): {bytes(errors)[-3000:]!r}')
            if file_set(profile) != expected_source:
                raise ValueError('source profile was changed')
            stderr = errors.decode('utf-8', 'surrogateescape').replace(str(user), '$USER')
            observation = dict(samples=[{key: row[key] for key in ('options', 'saved', 'observations')} for row in samples],
                               stderr=stderr, userfiles=file_set(user), documents=documents)
            if mode not in ('export', 'populate'):
                if len(samples) != cycles or closed is None or closed['status'] != 0:
                    raise ValueError('native lifecycle did not complete')
                if mode == 'memory':
                    expected = [('M0', 0)]
                    for cycle in range(1, cycles + 1):
                        expected.extend((point, cycle) for point in ('M1', 'M2', 'M3'))
                        if cycle < cycles:
                            expected.append(('reset', cycle))
                    if boundaries != expected:
                        raise ValueError('native checkpoint boundaries are out of order')
                    initial = checkpoints[0]['M0']
                    for points in checkpoints.values():
                        if 'M3' in points:
                            if set(points) != {'M1', 'M2', 'M3'}:
                                raise ValueError('native checkpoint boundaries are incomplete')
                            points['M0'] = initial
                for row in samples:
                    frees = [value['call'] for value in row['free_instance_ns']]
                    if len(frees) != instances:
                        raise ValueError('not every instance was freed')
                    for clock in ('wall_ns', 'cpu_ns'):
                        total = sum(value[clock] for value in frees)
                        if total != row['times']['free_all_instances_ns'][clock] or total + row['times']['fini_ns'][clock] != row['times']['teardown_total_ns'][clock]:
                            raise ValueError('native teardown totals are inconsistent')
                samples[-1]['times'].update(closed['times'])
            if reopen:
                observation['reopen'] = self.run(facade, library, user, mode='observe', inputs=inputs)['observation']
            return dict(observation=observation, samples=samples, checkpoints=checkpoints, command=command)
        finally:
            shutil.rmtree(scratch)


def print_record(kind, **values):
    print(json.dumps(dict(teardown=kind, **values), sort_keys=True, ensure_ascii=True), flush=True)


def provision(campaign, pin, root):
    if root.exists():
        profiles = {name: root / name / 'user' for name in PROFILES}
        for name, user in profiles.items():
            lines = (user.parent / 'profile.manifest').read_text().splitlines()
            if lines[:1] != ['oxpinyin-startup-profile-v1']:
                raise ValueError('profile manifest schema mismatch')
            recorded = {line.split('\t', 2)[2]: (int(line.split('\t', 2)[0]), line.split('\t', 2)[1]) for line in lines[1:]}
            if recorded != file_set(user):
                raise ValueError('profile manifest mismatch: ' + name)
        return profiles
    root.mkdir()
    export_user = root / 'export-user'
    export_user.mkdir()
    export = campaign.run('pinyin', pin, export_user, mode='export')['observation']['documents']
    exported = []
    for line in export:
        library, phrase, pinyin, count = line.split('\t')
        exported.append((int(library), phrase, pinyin, int(count)))
    shutil.rmtree(export_user)
    recipes = dict(empty=[], modest=modest_rows(), phrases=select_rows(exported), journals=select_rows(exported, True))
    profiles = {}
    for name, rows in recipes.items():
        destination = root / name
        destination.mkdir()
        user = destination / 'user'
        user.mkdir()
        path = destination / 'rows.tsv'
        path.write_text(row_text(rows))
        if rows:
            # Populate through a private working copy, then move its resulting
            # profile into place. run() intentionally never mutates its input.
            populate_user(campaign, pin, user, path)
        files = file_set(user)
        (destination / 'profile.manifest').write_text('\n'.join(
            ['oxpinyin-startup-profile-v1', *[f'{size}\t{sha}\t{filename}' for filename, (size, sha) in files.items()]]) + '\n')
        (destination / 'witness.txt').write_text('')
        profiles[name] = user
        print_record('fixture', profile=name, files=files, identity=identity(files),
                     rows=len(rows), recipe_sha256=digest_bytes(path.read_bytes()), historical_byte_identity=False)
    return profiles


def populate_user(campaign, pin, user, rows):
    # Fixture generation is untimed. This is the same native executable and
    # facade machinery, with no dependency on the historical initbench.
    command = [str(campaign.worker), '--teardown', 'pinyin', str(pin), str(campaign.data), str(user),
               'populate', '1', '1', '1', 'first-candidate', str(rows)]
    env = dict(os.environ, OXPINYIN_TEARDOWN_EXPORTS=','.join(exports('pinyin')))
    result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=300)
    if result.returncode or json.loads(result.stdout) != dict(populated=True, saved=True):
        raise ValueError('fixture population failed: ' + result.stderr)


def run(args, driver, parser):
    if args.cell != 'bdb' or args.per_call or args.cases or args.issue or args.family or args.expect_parent or args.observations or args.plan_only or args.axis_c_replacement or args.axis_c_mutation or args.axis_c_snapshot_check or args.regenerate_sentence_ties:
        parser.error('--teardown is a separate BDB workload; contract/profiler modes cannot be combined')
    if not args.teardown_work_dir or not args.teardown_work_dir.is_dir():
        parser.error('--teardown-work-dir must name an owned temporary directory')
    cases = tuple(args.teardown_cases.split(','))
    if not cases or len(set(cases)) != len(cases) or any(name not in CASES for name in cases):
        parser.error('unknown or repeated --teardown-cases')
    cpu = args.teardown_cpu if args.teardown_cpu is not None else min(os.sched_getaffinity(0))
    if cpu not in os.sched_getaffinity(0):
        parser.error('--teardown-cpu is outside this process affinity')
    work = args.teardown_work_dir.resolve()
    fs = subprocess.check_output(['stat', '-f', '-c', '%T', str(work)], text=True).strip()
    if fs != 'ext2/ext3':
        parser.error('teardown work and measured libraries must reside on ext4 (stat reported ' + fs + ')')
    facades = ('pinyin', 'zhuyin') if args.teardown_facades == 'both' else (args.teardown_facades,)
    subjects = {'ox': dict(pinyin=args.pinyin_so, zhuyin=args.zhuyin_so),
                'pin': {facade: args.prefix / 'lib' / ('lib' + facade + '.so') for facade in facades}}
    for paths in subjects.values():
        for facade in facades:
            if paths.get(facade) is None or not paths[facade].is_file():
                parser.error('missing ' + facade + ' library')
    # Copy all measured bytes onto ext4, including the same-bytes control.
    libraries = work / 'libraries'
    libraries.mkdir(exist_ok=True)
    subjects['control'] = {}
    provenance = {}
    for subject in ('ox', 'pin', 'control'):
        directory = libraries / subject
        directory.mkdir(exist_ok=True)
        provenance[subject] = {}
        for facade in facades:
            source = subjects['ox'][facade] if subject == 'control' else subjects[subject][facade]
            target = directory / ('lib' + facade + '.so')
            if source.resolve() != target.resolve():
                shutil.copyfile(source, target)
            subjects[subject][facade] = target
            provenance[subject][facade] = dict(path=str(target), sha256=digest_bytes(target.read_bytes()), bytes=target.stat().st_size)
    worker, worker_identity = build_worker(work)
    data = args.prefix / 'lib/libpinyin/data'
    campaign = Campaign(worker, data, work, cpu, driver)
    print_record('start', stage=args.teardown_stage, cpu=cpu, filesystem=fs, libraries=provenance,
                 worker=worker_identity, model_identity=identity(file_set(data)), control_noise_rule=list(CONTROL_INTERVAL))
    profiles = provision(campaign, subjects['pin']['pinyin'], work / 'profiles')
    if args.teardown_stage == 'provision':
        return 0
    return execute(campaign, args, subjects, profiles, cases, facades, driver)


def execute(campaign, args, subjects, profiles, cases, facades, driver):
    dirty_rows = campaign.work / 'dirty.tsv'
    dirty_rows.write_text(row_text([(1, '测试', "ce4'shi4", 20000)]))
    witnesses = prepare_witnesses(campaign, subjects['pin']['pinyin'], profiles)
    comparable, references = {}, {}
    for facade in facades:
        for case in cases:
            options = case_options(case, profiles, dirty_rows)
            readings = {}
            for side in ('pin', 'ox', 'control'):
                readings[side] = campaign.run(facade, subjects[side][facade], mode='observe',
                                              inputs=witnesses[case.split('-')[0]], reopen=True, **options)
            reference = readings['pin']['observation']
            matches = all(row['observation'] == reference for row in readings.values())
            comparable[facade, case] = matches
            references[facade, case] = {side: workload_observation(row) for side, row in readings.items()}
            print_record('equivalence', facade=facade, case=case,
                         verdict='COMPARABLE' if matches else 'NOT COMPARABLE',
                         digests={side: identity(row['observation']) for side, row in readings.items()},
                         differences={} if matches else {side: first_difference(reference, row['observation'])
                                                        for side, row in readings.items() if row['observation'] != reference})
    # Report a safe checkpoint before starting any ratio campaign. A mismatch
    # is an observation to report, never permission to repair shipping behavior.
    if not all(comparable.values()):
        print_record('checkpoint', status='equivalence-blocked', comparable=[list(key) for key, value in comparable.items() if value])
        return 2
    if args.teardown_stage == 'verify':
        return 0
    if args.teardown_stage in ('pilot', 'confirm'):
        pilot = args.teardown_stage == 'pilot'
        for facade in facades:
            for case in cases:
                options = case_options(case, profiles, dirty_rows)
                for mode, rounds, warmups in (('time', 5 if pilot else TIMING_ROUNDS, 1 if pilot else TIMING_WARMUPS),
                                              ('memory', 3 if pilot else MEMORY_ROUNDS, 1 if pilot else MEMORY_WARMUPS)):
                    samples = {side: [] for side in subjects}
                    for index in range(rounds + warmups):
                        order = ['ox', 'control', 'pin']
                        order = order[index % 3:] + order[:index % 3]
                        for side in order:
                            row = campaign.run(facade, subjects[side][facade], mode=mode, **options)
                            if workload_observation(row) != references[facade, case][side]:
                                raise ValueError(f'timed workload observation changed: {facade} {case} {side}')
                            if index >= warmups:
                                samples[side].append(row)
                    if mode == 'time':
                        report_times(facade, case, samples, pilot)
                    else:
                        report_memory(facade, case, samples, pilot)
        return 0
    if args.teardown_stage in ('repeat', 'multi'):
        for facade in facades:
            for case in ('empty', args.teardown_attribution_case):
                options = case_options(case, profiles, dirty_rows)
                for side in ('ox', 'control', 'pin'):
                    repeated = args.teardown_stage == 'repeat'
                    row = campaign.run(facade, subjects[side][facade], mode='memory', cycles=100 if repeated else 1,
                                       instances=1 if repeated else 2,
                                       **dict(options, scenario='first-candidate' if repeated else 'multi'))
                    points = {str(cycle): value for cycle, value in row['checkpoints'].items() if cycle}
                    print_record(args.teardown_stage, facade=facade, case=case, side=side, checkpoints=points,
                                 observations_equal=all(value == row['observation']['samples'][0] for value in row['observation']['samples']))
        return 0
    if args.teardown_stage == 'attribute':
        for facade in facades:
            options = case_options(args.teardown_attribution_case, profiles, dirty_rows)
            for side in ('ox', 'control', 'pin'):
                row = campaign.run(facade, subjects[side][facade], profiler='callgrind', **options)
                profile = driver.per_call_profile(row['observation']['stderr'])
                shown = driver.per_call_attribution(profile, 1, 25)
                print_record('callgrind', facade=facade, case=args.teardown_attribution_case, side=side,
                             profiler_only=True, **shown)
                heap = campaign.run(facade, subjects[side][facade], profiler='dhat', **options)
                text = heap['observation']['stderr']
                position = text.find('{"dhatFileVersion"')
                if position < 0:
                    position = text.find('{\n "dhatFileVersion"')
                if position < 0:
                    raise ValueError('missing in-memory DHAT document')
                document, _ = json.JSONDecoder().raw_decode(text[position:])
                print_record('dhat', facade=facade, case=args.teardown_attribution_case, side=side,
                             profiler_only=True, totals={key: document.get(key) for key in ('te', 'tg', 'tuth')},
                             sites=document.get('pps', []))
        return 0
    raise ValueError('unknown teardown stage')


def case_options(case, profiles, dirty_rows):
    return dict(profile=profiles[case.split('-')[0]], scenario='dirty' if '-' in case else 'first-candidate',
                rows=dirty_rows if '-' in case else None, save=case.endswith('-saved'))


def workload_observation(row):
    return {key: row['observation'][key] for key in ('samples', 'stderr', 'userfiles')}


def first_difference(left, right, path='$'):
    if type(left) is not type(right):
        return dict(path=path, pin=left, subject=right)
    if isinstance(left, dict):
        if left.keys() != right.keys():
            return dict(path=path, pin_keys=sorted(left), subject_keys=sorted(right))
        for key in left:
            if left[key] != right[key]:
                return first_difference(left[key], right[key], path + '.' + str(key))
    elif isinstance(left, (list, tuple)):
        for index, (a, b) in enumerate(zip(left, right)):
            if a != b:
                return first_difference(a, b, path + '[' + str(index) + ']')
        if len(left) != len(right):
            return dict(path=path + '.length', pin=len(left), subject=len(right))
    elif left != right:
        return dict(path=path, pin=left, subject=right)
    return None


def prepare_witnesses(campaign, pin, profiles):
    result = {}
    for name in PROFILES:
        path = profiles[name].parent / 'witness.txt'
        if name == 'empty':
            result[name] = path
            continue
        if not path.read_text():
            rows = []
            for line in (path.parent / 'rows.tsv').read_text().splitlines():
                library, phrase, pronunciation, count = line.split('\t')
                rows.append((int(library), phrase, pronunciation, int(count)))
            candidates = witness_inputs(rows)
            candidate_file = path.parent / 'candidate-inputs.txt'
            candidate_file.write_text(''.join(value + '\n' for value in candidates))
            empty = campaign.run('pinyin', pin, profiles['empty'], mode='observe', inputs=candidate_file)
            populated = campaign.run('pinyin', pin, profiles[name], mode='observe', inputs=candidate_file)
            def by_input(row):
                return {value['transcript']: value for value in row['observation']['documents']
                        if value.get('parser') == 'full'}
            a, b = by_input(empty), by_input(populated)
            chosen = [value for value in sorted(candidates) if a.get(value) != b.get(value)][:40]
            if not chosen:
                raise ValueError('populated profile changes no witness answer: ' + name)
            path.write_text(''.join(value + '\n' for value in chosen))
        result[name] = path
        print_record('witness', profile=name, inputs=path.read_text().splitlines(), sha256=digest_bytes(path.read_bytes()))
    return result


def report_times(facade, case, samples, pilot):
    for metric in samples['ox'][0]['samples'][0]['times']:
        for clock in ('wall_ns', 'cpu_ns'):
            values = {side: [row['samples'][0]['times'][metric][clock] for row in rows]
                      for side, rows in samples.items()}
            control = stable_control(values['ox'], values['control'])
            print_record('time', facade=facade, case=case, metric=metric, clock=clock, pilot=pilot,
                         summaries={side: summary(value) for side, value in values.items()}, control=control,
                         verdict='STABLE' if control['stable'] else 'INCONCLUSIVE',
                         ox_pin_ratio=paired_ratio(values['ox'], values['pin']),
                         control_pin_ratio=paired_ratio(values['control'], values['pin']))


def report_memory(facade, case, samples, pilot):
    # Absolute checkpoint levels have meaningful positive-denominator ratios.
    # Signed release/residual deltas are reported in KiB, never divided by a
    # negative/zero baseline. No VmHWM observation enters this path.
    page_kib = os.sysconf('SC_PAGE_SIZE') // 1024
    for point in ('M0', 'M1', 'M2', 'M3'):
        for metric in FIELDS:
            values = {side: [row['checkpoints'][0 if point == 'M0' else 1][point]['totals'][metric] for row in rows]
                      for side, rows in samples.items()}
            control_differences = [b - a for a, b in zip(values['ox'], values['control'])]
            print_record('memory', facade=facade, case=case, point=point, metric=metric, unit='KiB', pilot=pilot,
                         summaries={side: summary(value) for side, value in values.items()},
                         control_paired_difference=summary(control_differences),
                         control_within_one_page=all(abs(value) <= page_kib for value in control_differences),
                         ox_pin_ratio=paired_ratio(values['ox'], values['pin']) if all(value > 0 for value in values['pin']) else None)
    for side, rows in samples.items():
        deltas = [memory_deltas(dict(row['checkpoints'][1], M0=row['checkpoints'][0]['M0'])) for row in rows]
        shown = {label: {metric: summary([delta[label]['totals'][metric] for delta in deltas]) for metric in FIELDS}
                 for label in ('M1-M2', 'M2-M3', 'M3-M0')}
        print_record('memory-deltas', facade=facade, case=case, side=side, pilot=pilot, signed_kib=shown,
                     checkpoints={point: rows[0]['checkpoints'][0 if point == 'M0' else 1][point] for point in ('M0', 'M1', 'M2', 'M3')})
