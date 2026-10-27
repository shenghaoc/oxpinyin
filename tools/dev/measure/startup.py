"""Native startup controller (§7.8). Native runs verify inputs and never provision."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import runpy
import shutil
import subprocess
import sys
import tempfile

from .. import mman, slot
from . import cold, cold_linux, instructions, native, pages, stats, worker

ROOT = Path(__file__).resolve().parents[3]
PROFILES = ('empty', 'modest', 'phrases', 'journals')
ARMS = ('base', 'pr', 'control')
TIMING_METRICS = ('lib_load', 'init', 'config', 'alloc', 'first_candidate_phase', 'ttfc',
                  'setup_plus_first_candidate', 'drop_instance', 'fini', 'process_wall')
DIAGNOSTIC_METRICS = ('minor_faults', 'major_faults', 'faults', 'pageins',
                      'disk_bytes_read', 'disk_bytes_written', 'instructions', 'cycles',
                      'instructions_available', 'cycles_available')
SAMPLE_SCHEMA = 'oxpinyin-initbench-sample-v1'
OBSERVATION_SCHEMA = 'oxpinyin-initbench-observation-v1'
# Stands in for the value of a sample whose cold preparation was rejected.
REJECTED = object()


class NotResult(ValueError):
    """Indicate incomparable observations or invalid controls, mapped to CLI status 3."""
    pass


def module(path, name):
    """Load and execute a Python file under a supplied module name and return it."""
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


PROFILE = module(ROOT / 'tools/bisection/startup_profile.py', 'startup_profile')


def emit(kind, **values):
    """Print and flush one sorted JSON record with the given startup event kind."""
    print(json.dumps(dict(startup=kind, **values), sort_keys=True), flush=True)


def report_page_residency(facade, profile, arm, samples):
    """Report Darwin checkpoint changes after all rounds, parsing verified descriptors."""
    layouts = {}
    for sample in samples:
        if sample is None:
            continue
        round_index = sample['round']
        observations = sample.get('residency', {})
        for before_point, after_point in pages.checkpoint_pairs(list(observations)):
            before, after = observations[before_point], observations[after_point]
            files = {}
            for label in sorted(before.keys() | after.keys()):
                if label not in before or label not in after:
                    files[label] = dict(reason='file appeared or disappeared between checkpoints')
                    continue
                first, last = before[label], after[label]
                if first['identity'] != last['identity']:
                    files[label] = dict(reason='file replaced between checkpoints')
                    continue
                offsets = pages.newly_resident(first['vector'], last['vector'], mman.PAGE_SIZE)
                if not offsets:
                    continue
                entry = dict(darwin_resident_pages=len(offsets))
                layout = None
                if last['kind'] == 'object':
                    expected = (*last['identity'], last['size'], last['mtime_ns'])
                    key = (last['path'], expected)
                    if key not in layouts:
                        try:
                            if (first['size'], first['mtime_ns']) != (last['size'], last['mtime_ns']):
                                raise ValueError('object modified between checkpoints')
                            with open(last['path'], 'rb') as handle:
                                info = os.fstat(handle.fileno())
                                actual = (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns)
                                if actual != expected:
                                    raise ValueError('object replaced or modified before parsing')
                                data = handle.read()
                                end = os.fstat(handle.fileno())
                                if (end.st_dev, end.st_ino, end.st_size, end.st_mtime_ns) != actual:
                                    raise ValueError('object modified while parsing')
                                layout = pages.object_layout(last['path'], data=data)
                                slice_name = pages._host_slice(data)[2] if data[:4] in pages.FAT_MAGICS else None
                                layouts[key] = (layout, None, slice_name)
                        except (OSError, ValueError) as error:
                            layouts[key] = (None, str(error), None)
                    layout, reason, slice_name = layouts[key]
                    if reason:
                        entry['layout_error'] = reason
                    if slice_name:
                        entry['macho_slice'] = slice_name
                entry['rows'] = (pages.object_rows(layout, offsets, mman.PAGE_SIZE, label) if layout is not None
                                 else pages.data_rows(offsets, mman.PAGE_SIZE, last['size'], label))
                files[label] = entry
            emit('page-residency', facade=facade, profile=profile, arm=arm, round=round_index,
                 checkpoints=[before_point, after_point], page_size=mman.PAGE_SIZE,
                 metric='darwin_resident_pages', limits=list(pages.LIMITS), files=files)


def parser():
    """Build the native startup CLI, including retained-instruction recheck options."""
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--base', type=Path, help='base installed prefix (oxpinyin only)')
    p.add_argument('--pr', type=Path, help='PR installed prefix (oxpinyin only)')
    p.add_argument('--pin', nargs='?', const='requested', help=argparse.SUPPRESS)
    p.add_argument('--system-dir', type=Path)
    p.add_argument('--profiles-dir', type=Path)
    p.add_argument('--work-dir', type=Path, help='owned parent directory; default is TMPDIR')
    p.add_argument('--facades', choices=('pinyin', 'zhuyin', 'both'), default='both')
    p.add_argument('--profiles', default=','.join(PROFILES))
    p.add_argument('--stage', choices=('verify', 'instrument', 'run'), default='run')
    p.add_argument('--rounds', type=positive, help='timed rounds, default 60 (30 with lazy binding)')
    p.add_argument('--warmups', type=nonnegative, default=3)
    p.add_argument('--memory-rounds', type=positive, default=30)
    p.add_argument('--memory-warmups', type=nonnegative, default=2)
    p.add_argument('--cache', choices=('warm', 'cold'), default='warm')
    p.add_argument('--bind', choices=('now', 'lazy'), default='now')
    p.add_argument('--cpu', type=nonnegative)
    # Every native shipping arm must pass #1062. No silent unordered fallback.
    p.add_argument('--function-order', action='append', default=[], metavar='ARM/FACADE=POLICY')
    p.add_argument('--recheck-instructions', type=Path, help='re-evaluate retained JSONL without running workers')
    p.add_argument('--instructions-from', type=Path, help='reuse qualified retained profiler scopes; new verification/timing/memory are a separate segment')
    return p


def nonnegative(text):
    """Parse a nonnegative integer for argparse; reject invalid or negative values."""
    try:
        value = int(text)
    except ValueError as error:
        raise argparse.ArgumentTypeError('expected a nonnegative integer') from error
    if value < 0:
        raise argparse.ArgumentTypeError('expected a nonnegative integer')
    return value


def positive(text):
    """Parse a strictly positive integer for argparse; zero and invalid values fail."""
    value = nonnegative(text)
    if not value:
        raise argparse.ArgumentTypeError('expected a positive integer')
    return value


def validate(args):
    """Validate native startup options and return selected profile user directories.

    Reject pin requests, unsupported platforms (cold cache included),
    missing inputs, unknown/repeated profiles, and invalid manifests or
    witnesses. Read input files without provisioning or modifying them;
    validation errors propagate.
    """
    if args.pin is not None:
        raise ValueError('native pin rows require the Layer 4 container path: just container run (T11C)')
    if sys.platform not in ('linux', 'darwin'):
        raise ValueError('native startup supports Linux/macOS; Windows is deferred (#950)')
    if sys.platform == 'darwin' and args.function_order:
        raise ValueError('ELF function-order policies are unsupported on macOS')
    for name in ('base', 'pr', 'system_dir', 'profiles_dir'):
        path = getattr(args, name)
        if path is None or not path.is_dir():
            raise ValueError('missing input --' + name.replace('_', '-'))
    selected = args.profiles.split(',')
    if not selected or len(set(selected)) != len(selected) or any(name not in PROFILES for name in selected):
        raise ValueError('unknown or repeated profile')
    profiles = {}
    for name in selected:
        root = args.profiles_dir / name
        user, witness, manifest = root / 'user', root / 'witness.txt', root / 'profile.manifest'
        PROFILE.check(user, manifest)
        if name != 'empty' and (not witness.is_file() or not witness.read_text().strip()):
            raise ValueError(f'populated profile {name} requires a nonempty witness.txt (provision through the pin)')
        if witness.is_file():
            raw = witness.read_bytes()
            if b'\0' in raw or (raw and not raw.endswith(b'\n')):
                raise ValueError('invalid witness file: ' + str(witness))
            raw.decode('utf-8')
        profiles[name] = user
    return profiles


def library(prefix, facade):
    """Resolve an installed facade library under prefix or raise ValueError.

    On macOS, run nm and require an unstripped oxpinyin symbol; Linux expects
    the versioned .so.15.0.0 file. Read and subprocess errors propagate.
    """
    if sys.platform == 'darwin':
        candidates = [prefix / 'lib' / ('lib' + facade + '.dylib'),
                      prefix / 'lib' / ('lib' + facade + '.15.dylib')]
    else:
        candidates = [prefix / 'lib' / ('lib' + facade + '.so.15.0.0')]
    path = next((path for path in candidates if path.is_file()), None)
    if path is None:
        raise ValueError('missing installed ' + facade + ' library under ' + str(prefix))
    if sys.platform == 'darwin':
        symbols = subprocess.check_output(['nm', '-a', str(path)], timeout=native.COMMAND_TIMEOUT_SECONDS)
        if b'oxpinyin_' not in symbols:
            raise ValueError('native startup requires an unstripped oxpinyin library: ' + str(path))
    return path.resolve()


def artifact(path):
    """Describe a library by path, SHA-256, byte size, and dynamic loader metadata.

    Run otool on macOS or readelf elsewhere; subprocess and file errors propagate.
    """
    if sys.platform == 'darwin':
        metadata = {key: subprocess.check_output(['otool', option, str(path)], text=True,
                                                 timeout=native.COMMAND_TIMEOUT_SECONDS).splitlines()[1:]
                    for key, option in (('LC_LOAD_DYLIB', '-L'), ('LC_ID_DYLIB', '-D'))}
    else:
        dynamic = subprocess.check_output(['readelf', '-dW', str(path)], text=True,
                                          timeout=native.COMMAND_TIMEOUT_SECONDS)
        metadata = dict(NEEDED=re.findall(r'\(NEEDED\).*\[([^]]+)\]', dynamic))
    return dict(path=str(path), sha256=worker.sha256(path.read_bytes()), bytes=path.stat().st_size, **metadata)


def order(round_index):
    """Rotate the base, PR, and control execution order by the round index."""
    offset = round_index % len(ARMS)
    return ARMS[offset:] + ARMS[:offset]


def observation(row):
    """Extract comparable documents, exact stderr bytes, and user-file observations.

    Exclude run and timing records; encode stderr as hex.
    """
    return dict(documents=[d for d in row['documents'] if d.get('startup') not in ('run', 'times')],
                stderr_hex=row['stderr'].hex(), userfiles=row['userfiles'])


def first_difference(left, right, path='$'):
    """Find the first recursive difference between JSON-like values.

    Return a path and differing values, keys, or lengths; return None when
    equal. Traverse dictionaries in left-hand insertion order and lists in
    index order; differing Python types count as differences.
    """
    if type(left) is not type(right):
        return dict(path=path, base=left, subject=right)
    if isinstance(left, dict):
        if left.keys() != right.keys():
            return dict(path=path, base_keys=sorted(left), subject_keys=sorted(right))
        for key in left:
            if left[key] != right[key]:
                return first_difference(left[key], right[key], path + '.' + str(key))
    elif isinstance(left, list):
        for i, (a, b) in enumerate(zip(left, right)):
            if a != b:
                return first_difference(a, b, path + '[' + str(i) + ']')
        if len(left) != len(right):
            return dict(path=path + '.length', base=len(left), subject=len(right))
    elif left != right:
        return dict(path=path, base=left, subject=right)
    return None


def require_digest(row, reference):
    """Raise NotResult when a timed units digest differs from its verification reference."""
    if row['digest'] != reference['digest']:
        raise NotResult('NOT COMPARABLE: timed U1–U6 digest differs from native verification')


def describe(values):
    """Combine finite-sample summaries with a deterministic median bootstrap interval."""
    return dict(stats.summary(values), ci95=stats.paired_bootstrap(values)['ci95'])


def cold_account(samples, warmup_rejected):
    """Count one cold family's accepted and rejected samples per arm.

    Each arm's list holds one row per measured round, None where the sample's
    cold preparation was rejected. warmup_rejected counts the discarded
    warm-up samples that were rejected; they never count toward the limit.
    The limit is cold.REJECTION_LIMIT on both hosts: a constant read here,
    never derived from these counts. The metric is the host's own name.
    """
    rejected = {arm: rows.count(None) for arm, rows in samples.items()}
    metric = cold_linux.METRIC if sys.platform == 'linux' else cold.METRIC
    return dict(metric=metric, limit=cold.REJECTION_LIMIT, rejected=rejected,
                accepted={arm: len(rows) - rejected[arm] for arm, rows in samples.items()},
                warmup_rejected=dict(warmup_rejected))


def pool_cold(values, counts):
    """Return a cold metric's pooled values, or the reason it is INCONCLUSIVE.

    More rejected samples than the limit in any arm is INCONCLUSIVE. The pool
    is the measured rounds in which every arm was accepted, so paired ratios
    and controls compare like with like. Rejected rounds hold REJECTED. Return
    (pooled, None), or (None, reason) when an arm is over the limit or no round
    has every arm accepted.
    """
    over = ', '.join(f'{arm} {n}' for arm, n in counts['rejected'].items() if n > counts['limit'])
    if over:
        return None, f"rejected cold samples exceed the limit of {counts['limit']} per arm: {over}"
    kept = [i for i, round_values in enumerate(zip(*values.values()))
            if all(value is not REJECTED for value in round_values)]
    if not kept:
        return None, 'no measured round has every arm accepted'
    return {arm: [rows[i] for i in kept] for arm, rows in values.items()}, None


def report_values(kind, values, **context):
    """Emit a distribution report with the appropriate PR/control stability rule.

    Timing uses paired ratios; residency uses KiB differences. Fault, page-in
    and disk-I/O counts are diagnostic. macOS reports resident and physical
    footprint metrics; the Linux names stay unavailable there. Missing values
    produce unavailable reports; PR/base ratios require stable controls and
    positive base samples. A cold campaign passes cold_account as context
    ['cold'] and marks each rejected sample's value REJECTED: a metric over the
    limit is INCONCLUSIVE with no summaries or ratio (pool_cold). A field with
    at least one sample that was not rejected and None in every such sample is
    unavailable even over the limit (with every sample rejected, pool_cold
    decides), and a None in a pooled round is unavailable after pooling,
    because a field the host does not record cannot be INCONCLUSIVE.
    """
    recorded = [value for rows in values.values() for value in rows if value is not REJECTED]
    if recorded and all(value is None for value in recorded):
        emit(kind, **context, verdict='unavailable', summaries=None, pr_base_ratio=None)
        return
    if context.get('cold') is not None:
        values, reason = pool_cold(values, context['cold'])
        if reason:
            emit(kind, **context, verdict='INCONCLUSIVE', summaries=None, pr_base_ratio=None, reason=reason)
            return
    if any(any(value is None for value in rows) for rows in values.values()):
        emit(kind, **context, verdict='unavailable', summaries=None, pr_base_ratio=None)
        return
    if kind == 'timing':
        control = stats.stable_control(values['pr'], values['control'])
    elif context['metric'] in DIAGNOSTIC_METRICS:
        # Counts are observations, not deterministic instruction/heap accounting
        # or residency levels. Report their distributions without a ratio gate.
        emit(kind, **context, verdict='diagnostic', summaries={a: describe(v) for a, v in values.items()},
             pr_base_ratio=None)
        return
    elif sys.platform == 'darwin' and context['metric'] == 'virtual_size':
        # The macOS reader reports Darwin-named resident and footprint fields;
        # virtual size is unavailable there. Retained or synthetic Linux-shaped
        # samples still get the old refusal rather than a misleading verdict.
        emit(kind, **context, verdict='unavailable', summaries=None, pr_base_ratio=None,
             reason='not informative on macOS: virtual size includes the shared region')
        return
    else:
        control = stats.stable_residency(values['pr'], values['control'])
    emit(kind, **context, summaries={a: describe(v) for a, v in values.items()}, control=control,
         verdict='STABLE' if control['stable'] else 'INCONCLUSIVE',
         pr_base_ratio=stats.paired_ratio(values['pr'], values['base'])
         if control['stable'] and all(v > 0 for v in values['base']) else None)


def verify(campaign, libraries, facade, name, profiles):
    """Compare base, PR, and control verification, persistence, and units observations.

    Run workers on private profiles, check populated witnesses against an
    empty profile, and emit equivalence results. Raise NotResult on changed
    digests or unequal observations. After comparability passes, emit twelve
    hostile probes per arm and return the verification rows.
    """
    user = profiles[name]
    witness = user.parent / 'witness.txt'
    witness = witness if witness.is_file() else None
    rows, observed = {}, {}
    for arm in ARMS:
        rows[arm] = campaign.run(libraries[arm][facade], facade, user, mode='verify', witness=witness)
        unit_sample = campaign.run(libraries[arm][facade], facade, user)
        require_digest(unit_sample, rows[arm])
        rows[arm]['unit_stderr'] = unit_sample['stderr']
        rows[arm]['unit_userfiles'] = unit_sample['userfiles']
        persisted = campaign.run(libraries[arm][facade], facade, user, mode='persist')
        observed[arm] = dict(verify=observation(rows[arm]), persist=observation(persisted),
                             unit_stderr_hex=unit_sample['stderr'].hex(), unit_userfiles=unit_sample['userfiles'])
        if name != 'empty':
            # Witness discrimination uses a genuinely empty private profile input.
            with tempfile.TemporaryDirectory(prefix='empty-', dir=campaign.work) as empty:
                baseline = campaign.run(libraries[arm][facade], facade, Path(empty), mode='verify', witness=witness)
            witnesses = set(witness.read_text().splitlines())
            def answers(row):
                """Select transcript records whose inputs belong to the captured witness set."""
                return [r for r in row['documents'] if r.get('startup') == 'transcript' and r.get('input') in witnesses]
            if answers(baseline) == answers(rows[arm]):
                raise NotResult('NOT COMPARABLE: populated profile changes no witness answer: ' + name)
    comparable = all(value == observed['pr'] for value in observed.values())
    emit('equivalence', schema=OBSERVATION_SCHEMA, facade=facade, profile=name,
         verdict='COMPARABLE' if comparable else 'NOT COMPARABLE',
         digests={a: worker.sha256(json.dumps(v, sort_keys=True).encode()) for a, v in observed.items()},
         unit_digests={a: v['digest'] for a, v in rows.items()},
         differences={a: first_difference(observed['base'], v) for a, v in observed.items() if v != observed['base']})
    if not comparable:
        raise NotResult('NOT COMPARABLE: native verify/reopen/raw user-file observations differ')
    # Hostile probes are deliberately outside comparability and timed transcripts.
    for arm in ARMS:
        for index in range(12):
            emit('hostile', arm=arm, facade=facade, profile=name, input_index=index,
                 **campaign.run(libraries[arm][facade], facade, user, mode='probe', probe=index))
    return rows


def instrument(campaign, libraries, facade, name, user, references, *, completed=None):
    """Run or reuse qualified instruction scopes and emit their control results.

    On Linux, take two init samples and five first-candidate/steady samples
    per arm, requiring verified units digests and exact or range controls.
    Report unavailable on macOS. Emit invalid evidence and raise NotResult
    when a required control or attribution check fails.
    """
    if sys.platform == 'darwin':
        emit('instructions', facade=facade, profile=name, verdict='unavailable', reason='native macOS Callgrind unavailable')
        return
    for scope in ('init', 'first_candidate', 'steady'):
        if completed and completed.get((facade, name, scope), {}).get('verdict') in ('VALID', 'RANGE-QUALIFIED'):
            emit('rechecked-instructions', facade=facade, profile=name, scope=scope,
                 **completed[facade, name, scope])
            continue
        counts, profiles = {}, {a: [] for a in ARMS}
        raw_profiles = {a: [] for a in ARMS}
        pids = {a: [] for a in ARMS}
        for repeat in range(2 if scope == 'init' else 5):
            for arm in order(repeat):
                row = campaign.run(libraries[arm][facade], facade, user, scope=scope)
                require_digest(row, references[arm])
                totals = re.findall(rb'^summary:\s*(\d+)\s*$', row['stderr'], re.M)
                if len(totals) != 1:
                    emit('instructions', facade=facade, profile=name, scope=scope, counts=counts,
                         raw_stderr_hex=row['stderr'].hex(), loader_data=[d for d in row.get('documents', [])
                         if d.get('startup') == 'loaded_object'], pid=row.get('pid'), verdict='INVALID', stop=True)
                    raise NotResult('INVALID: Callgrind scoped Ir total unavailable')
                counts.setdefault(arm, []).append(int(totals[0]))
                raw_profiles[arm].append(row['stderr'].hex())
                pids[arm].append(row.get('pid'))
                if scope != 'init':
                    # Retain the sample before attribution: a parser refusal must
                    # not discard the very loader evidence needed to diagnose it.
                    profiles[arm].append(dict(pid=row.get('pid'), parsed=False,
                        raw_profile_hex=row['stderr'].hex(), raw_stdout_hex=row.get('stdout', b'').hex(),
                        loader_data=[d for d in row.get('documents', []) if d.get('startup') == 'loaded_object']))
                    try:
                        parsed = instructions.profile(row['stderr'], libraries[arm][facade], campaign.executable, row.get('pid'), row.get('documents', []))
                        profiles[arm][-1].update(parsed, parsed=True)
                    except (ValueError, UnicodeError) as error:
                        emit('instructions', facade=facade, profile=name, scope=scope, counts=counts,
                             profiles=profiles, raw_profiles_hex=raw_profiles, verdict='INVALID', reason=str(error), stop=True)
                        raise NotResult('INVALID: Callgrind attribution unavailable') from error
                # Stop at the first same-bytes mismatch, not after a campaign.
                group = [v for a, values in counts.items() if worker.sha256(libraries[a][facade].read_bytes()) ==
                         worker.sha256(libraries[arm][facade].read_bytes()) for v in values]
                if scope == 'init' and len(set(group)) > 1:
                    emit('instructions', facade=facade, profile=name, scope=scope, counts=counts,
                         raw_profiles_hex=raw_profiles, verdict='INVALID', rule='exact', stop=True)
                    raise NotResult(f'STOP: same-bytes {scope} Ir differ: {counts}')
        if scope == 'init':
            try:
                exact_profiles = {arm: [instructions.total_profile(bytes.fromhex(raw), campaign.executable, pid)
                                        for raw, pid in zip(raw_profiles[arm], pids[arm])] for arm in ARMS}
                if len({p['creator'] for samples in exact_profiles.values() for p in samples}) != 1:
                    raise ValueError('Callgrind profiler identity changed between samples')
            except ValueError as error:
                emit('instructions', facade=facade, profile=name, scope=scope, counts=counts,
                     raw_profiles_hex=raw_profiles, verdict='INVALID', reason=str(error), stop=True)
                raise NotResult('INVALID: exact total profiler identity') from error
            emit('instructions', facade=facade, profile=name, scope=scope, counts=counts,
                 raw_profiles_hex=raw_profiles,
                 control=stats.exact_control(counts['pr'][0], counts['control'][0]), verdict='VALID')
        else:
            try:
                result = instructions.qualify(counts, profiles)
            except ValueError as error:
                emit('instructions', facade=facade, profile=name, scope=scope, counts=counts,
                     profiles=profiles, raw_profiles_hex=raw_profiles, verdict='INVALID', reason=str(error), stop=True)
                raise NotResult('INVALID: Callgrind range inputs') from error
            emit('instructions', facade=facade, profile=name, scope=scope, counts=counts,
                 profiles=profiles, **result)
            if result['verdict'] == 'INVALID':
                raise NotResult('INVALID: ' + scope + ' range or exclusive attribution')


def recheck_instructions(path):
    """Re-evaluate retained profiler records without launching workers.

    Return original start metadata, scope results, and source identity.
    Normalize malformed-record key/type/index/attribute failures to ValueError;
    other validation, comparability, and read errors propagate.
    """
    try:
        return _recheck_instructions(path)
    except (KeyError, TypeError, IndexError, AttributeError) as error:
        raise ValueError('incomplete or malformed retained instruction record: ' + str(error)) from error


def _recheck_instructions(path):
    """Parse one retained run and rebuild exact or range qualification results.

    Require one start record, unique scopes, consistent counts, and complete
    profiler samples. Invalid exact controls raise NotResult; invalid range
    controls remain result verdicts. Return source SHA-256 with the start and
    scope records; no workers or profilers are executed.
    """
    raw = path.read_bytes()
    documents = [json.loads(line) for line in raw.splitlines() if line.strip()]
    starts = [d for d in documents if d.get('startup') == 'start']
    if len(starts) != 1:
        raise ValueError('retained instructions require exactly one original start record')
    start = starts[0]
    qualified = {}
    for row in documents:
        if row.get('startup') != 'instructions' or row.get('scope') not in ('init', 'first_candidate', 'steady'):
            continue
        key = (row['facade'], row['profile'], row['scope'])
        if key in qualified:
            raise ValueError('duplicate retained profiler scope; do not mix runs')
        counts = row['counts']
        if row['scope'] == 'init':
            creators = set()
            for arm in ARMS:
                originals = row['raw_profiles_hex'][arm]
                if len(originals) < 2 or len(originals) != len(counts[arm]):
                    raise ValueError('incomplete retained exact samples')
                for encoded, count in zip(originals, counts[arm]):
                    text = bytes.fromhex(encoded)
                    pid = int(re.search(rb'^pid:[ \t]+(\d+)[ \t]*$', text, re.M)[1])
                    sample = instructions.total_profile(text, Path(start['worker']['path']), pid)
                    if sample['total'] != count:
                        raise ValueError('retained exact total differs from original')
                    creators.add(sample['creator'])
                if len(set(counts[arm])) != 1:
                    raise NotResult('INVALID: retained init varies')
            # The live exact path refuses a profiler identity change between samples.
            if len(creators) != 1:
                raise NotResult('INVALID: retained exact profiler identity')
            if counts['pr'][0] != counts['control'][0]:
                raise NotResult('INVALID: retained exact control')
            result = dict(verdict='VALID', rule='exact', creator=creators.pop())
        else:
            parsed = {}
            for arm in ARMS:
                originals = row['profiles'][arm]
                if len(originals) != len(counts[arm]):
                    raise ValueError('incomplete retained range samples')
                if len({p['pid'] for p in originals}) != len(originals):
                    raise ValueError('duplicate retained range process')
                parsed[arm] = [instructions.profile(p['raw'].encode(),
                    Path(start['libraries'][arm][row['facade']]['path']), Path(start['worker']['path']),
                    p['pid'], p['loader_data']) for p in originals]
                if [p['total'] for p in parsed[arm]] != counts[arm]:
                    raise ValueError('retained range totals differ from originals')
            result = instructions.qualify(counts, parsed)
        qualified[key] = dict(result, counts=counts)
    if not qualified:
        raise ValueError('no retained instruction scopes')
    return dict(start=start, scopes=qualified, source_sha256=worker.sha256(raw), source=str(path))


def campaign_run(args, profiles, work, admission):
    """Execute the selected native campaign inside an admitted work directory.

    Validate filesystems, copy libraries, build the worker, and verify every
    facade/profile before instrumentation or timed/memory sampling. Emit JSON
    records and return zero on success. Temporarily narrow CPU affinity when
    requested; restore it and check source input identities on exit. Work
    files are left for the caller's temporary-directory cleanup.
    """
    facades = ('pinyin', 'zhuyin') if args.facades == 'both' else (args.facades,)
    original = {arm: {facade: library(getattr(args, arm), facade) for facade in facades} for arm in ('base', 'pr')}
    native.check_filesystems([work, args.system_dir, args.profiles_dir, *[p for arm in original.values() for p in arm.values()]])
    before = {str(p): PROFILE.identity(p) for p in [args.system_dir, args.profiles_dir]}
    libraries = {}
    # Equal-length paths keep path-allocation costs out of the same-bytes control.
    for arm, dirname in zip(ARMS, ('baseobj', 'subject', 'control')):
        directory = work / dirname
        directory.mkdir()
        libraries[arm] = {}
        for facade in facades:
            source = original['pr' if arm == 'control' else arm][facade]
            destination = directory / source.name
            shutil.copyfile(source, destination)
            libraries[arm][facade] = destination
    if sys.platform == 'linux':
        checks = runpy.run_path(str(ROOT / 'tools/packaging/function_order.py'))
        checks['check_arms'](args, libraries)
    else:
        emit('function-order', verdict='unavailable', reason='ELF policy is Linux-only')
    executable, record = worker.build(work, admission.env, admission.slot.pass_fds())
    retained = recheck_instructions(args.instructions_from) if args.instructions_from else None
    if retained:
        old = retained['start']
        if (old['input_digests'] != before or old['bind'] != args.bind or old['cache'] != args.cache or
                old['worker']['source_sha256'] != record['source_sha256']):
            raise ValueError('retained input/worker provenance mismatch')
        for arm in ARMS:
            for facade, path in libraries[arm].items():
                if old['libraries'][arm][facade]['sha256'] != worker.sha256(path.read_bytes()):
                    raise ValueError('retained shipping library provenance mismatch')
        emit('retained-instructions', source=retained['source'], source_sha256=retained['source_sha256'],
             separate_measurement_segment=True)
    emit('start', host=platform.platform(), cpu=args.cpu, cache=args.cache, bind=args.bind,
         worker=record, input_digests=before,
         libraries={a: {f: artifact(p) for f, p in paths.items()} for a, paths in libraries.items()},
         timing_rounds=args.rounds, warmups=args.warmups, memory_rounds=args.memory_rounds,
         memory_warmups=args.memory_warmups, native_information_only=True)
    for name, user in profiles.items():
        path = user.parent / 'witness.txt'
        emit('profile', profile=name, manifest_sha256=worker.sha256((user.parent / 'profile.manifest').read_bytes()),
             witness_sha256=worker.sha256(path.read_bytes()) if path.is_file() else None,
             witnesses=path.read_text().splitlines() if path.is_file() else [])
    campaign = worker.Worker(executable, record, args.system_dir.resolve(), work,
                             admission.env, admission.slot.pass_fds(), args.cpu, args.bind)
    cold_sample = args.cache == 'cold'
    old_affinity = os.sched_getaffinity(0) if sys.platform == 'linux' else None
    try:
        if args.cpu is not None:
            os.sched_setaffinity(0, {args.cpu})
        for facade in facades:
            for name, user in profiles.items():
                references = verify(campaign, libraries, facade, name, profiles)
                if args.stage == 'verify':
                    continue
                instrument(campaign, libraries, facade, name, user, references,
                           completed=retained['scopes'] if retained else None)
                if args.stage == 'instrument':
                    continue
                for memory, rounds, warmups in ((False, args.rounds, args.warmups),
                                                (True, args.memory_rounds, args.memory_warmups)):
                    samples = {arm: [] for arm in ARMS}
                    warmup_rejected = dict.fromkeys(ARMS, 0)
                    for index in range(rounds + warmups):
                        for arm in order(index):
                            try:
                                row = campaign.run(libraries[arm][facade], facade, user, memory=memory,
                                                   cold=cold_sample)
                            except cold.Rejected as error:
                                # A rejected sample keeps its round: reported, never pooled, never rerun.
                                emit('sample', facade=facade, profile=name, arm=arm, round=index,
                                     warmup=index < warmups, memory=memory, cold=error.record)
                                if index < warmups:
                                    warmup_rejected[arm] += 1
                                else:
                                    samples[arm].append(None)
                                continue
                            sample = dict(facade=facade, profile=name, arm=arm, round=index,
                                          warmup=index < warmups, memory=memory,
                                          process_wall_ns=row['process_wall'], documents=row['documents'],
                                          checkpoints=row['checkpoints'])
                            if row.get('cold') is not None:
                                sample['cold'] = row['cold']
                            emit('sample', **sample)
                            require_digest(row, references[arm])
                            if (row['stderr'] != references[arm]['unit_stderr'] or
                                    row['userfiles'] != references[arm]['unit_userfiles']):
                                raise NotResult('NOT COMPARABLE: timed stderr/raw user files changed')
                            if index >= warmups:
                                row['round'] = index
                                samples[arm].append(row)
                    if cold_sample and memory and sys.platform == 'darwin':
                        for arm in ARMS:
                            report_page_residency(facade, name, arm, samples[arm])
                    cold_context = dict(cold=cold_account(samples, warmup_rejected)) if cold_sample else {}
                    if memory:
                        def memory_value(row, point, metric):
                            """Return one checkpoint metric, deriving Linux rss_delta only when both RSS values exist."""
                            if row is None:
                                return REJECTED
                            if metric == 'rss_delta':
                                current = row['checkpoints'][point].get('rss')
                                start = row['checkpoints']['start'].get('rss')
                                return None if current is None or start is None else current - start
                            return row['checkpoints'][point].get(metric)
                        fields = list(native.MEMORY_FIELDS)
                        if sys.platform == 'darwin':
                            fields.extend(native.DARWIN_COUNTER_FIELDS)
                            fields.extend(native.DARWIN_AVAILABILITY_FIELDS)
                        for point in worker.POINTS:
                            for metric in fields:
                                values = {a: [memory_value(r, point, metric) for r in rows]
                                          for a, rows in samples.items()}
                                unit = ('count' if metric in ('minor_faults', 'major_faults', 'faults',
                                                              'pageins', 'instructions', 'cycles')
                                        else 'flag' if metric.endswith('_available')
                                        else 'bytes' if metric.startswith('disk_') else 'KiB')
                                report_values('memory', values, facade=facade, profile=name, point=point,
                                              metric=metric, unit=unit, **cold_context)
                    else:
                        for metric in TIMING_METRICS:
                            def timing(row):
                                """Extract the captured metric's wall time in ms from a worker row.

                                Use process_wall directly for whole-process time; otherwise require one
                                times record and convert its wall_ns value.
                                """
                                if row is None:
                                    return REJECTED
                                if metric == 'process_wall':
                                    return row['process_wall'] / 1e6
                                clocks = [d for d in row['documents'] if d.get('startup') == 'times']
                                if len(clocks) != 1:
                                    raise ValueError('missing native timing record')
                                return clocks[0][metric]['wall_ns'] / 1e6
                            report_values('timing', {a: [timing(r) for r in rows] for a, rows in samples.items()},
                                          facade=facade, profile=name, metric=metric, unit='ms', schema=SAMPLE_SCHEMA,
                                          **cold_context)
        return 0
    finally:
        if old_affinity is not None:
            os.sched_setaffinity(0, old_affinity)
        for path, digest in before.items():
            if PROFILE.identity(Path(path)) != digest:
                raise ValueError('source profile/system input changed during campaign: ' + path)


def run(args):
    """Validate inputs and run a startup campaign under heavy admission.

    Fill default rounds and CPU on args, require suitable filesystems, and
    create a temporary work directory removed on exit. Return the campaign
    status; input, admission, and worker errors propagate.
    """
    profiles = validate(args)
    if args.rounds is None:
        args.rounds = 30 if args.bind == 'lazy' else 60
    args.cpu = native.pin_cpu(args.cpu)
    parent = Path(tempfile.gettempdir()) if args.work_dir is None else args.work_dir
    if not parent.is_dir():
        raise ValueError('work directory is missing')
    volumes = native.check_filesystems([parent], recursive=False)
    volumes |= native.check_filesystems([args.system_dir, args.profiles_dir, args.base, args.pr])
    if sys.platform == 'darwin' and len(volumes) != 1:
        raise ValueError('all startup inputs and work must be on one APFS volume')
    with slot.cancellation_handlers():
        with slot.HeavyAdmission('native startup ' + args.stage, dict(os.environ), runner=True) as admission:
            with tempfile.TemporaryDirectory(prefix='startup-', dir=parent) as temporary:
                return campaign_run(args, profiles, Path(temporary), admission)


def main(argv=None):
    """Run a native campaign or retained-instruction recheck and return CLI status.

    Refuse optimised Python with status 2 before parsing or running work.
    Emit rechecked scopes without workers when requested. Return 130 for
    cancellation, 3 for invalid/incomparable results, or 1 for reported
    filesystem, value, and subprocess errors; argument errors exit separately.
    """
    if sys.flags.optimize != 0:
        print('startup: optimised Python is unsupported; unset PYTHONOPTIMIZE and remove -O/-OO',
              file=sys.stderr)
        return 2
    args = parser().parse_args(argv)
    try:
        if args.recheck_instructions:
            retained = recheck_instructions(args.recheck_instructions)
            for (facade, name, scope), result in retained['scopes'].items():
                emit('rechecked-instructions', facade=facade, profile=name, scope=scope,
                     source_sha256=retained['source_sha256'], **result)
            return 0 if retained['scopes'] and all(r['verdict'] != 'INVALID' for r in retained['scopes'].values()) else 3
        return run(args)
    except (slot.Cancelled, KeyboardInterrupt) as error:
        print('startup: cancelled; child groups terminated and reaped', file=sys.stderr)
        return 130
    except NotResult as error:
        print('startup: ' + str(error), file=sys.stderr)
        return 3
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print('startup: ' + str(error), file=sys.stderr)
        return 1
