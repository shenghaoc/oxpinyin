"""Axis C of the contract differential (contract-diff.py): the replacement battery.

Load the committed replacement definition, run the battery and the instance
snapshot check through the driver's worker, and compare the repeated pin and
subject observations row by row, with the register-covered class (c) layout
rows licensed at the nested-observation level. The axis C cases themselves (the
probe builders, their observation helpers and the registration loops) are the
contract_axis_c_cases family. The registry, the worker callback and the
definition and covered-row tables the registration builds are keyword-only
parameters, passed by the driver's same-signature adapters.
"""
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import tempfile
import time

from contract_verdict import WARNING_DOMAIN


def axis_c_definition(*, verify_sources=False):
    """The committed replacement definition, never inferred from results.

    Loading happens at import for every run (ordinary case selection builds
    the Axis C groups too). Source-fixture hashing is deferred to
    `verify_sources=True`, asked for once in the Axis C branch, so an
    ordinary run is not failed by a changed fixture and an Axis C worker,
    which imports this module after the parent validated, does not repeat it.
    """
    root = Path(__file__).resolve().parents[2]
    definition = json.loads((root / 'tools/bisection/axis-c-replacement.json').read_text())
    assert definition['format'] == 'axis-c-replacement-v1'
    if verify_sources:
        for name, digest in definition['sources'].items():
            assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
    return definition


def axis_c_canonical(value):
    """Serialize a JSON-compatible value to compact ASCII bytes with sorted keys."""
    return json.dumps(value, sort_keys=True, ensure_ascii=True, separators=(',', ':')).encode()


def axis_c_normalize_diagnostics(value, scratch, field=''):
    """Only generated paths in diagnostics, never candidate/input payloads."""
    if isinstance(value, dict):
        return {key: axis_c_normalize_diagnostics(item, scratch, key) for key, item in value.items()}
    if isinstance(value, list):
        return [axis_c_normalize_diagnostics(item, scratch, field) for item in value]
    if isinstance(value, str) and field in ('stderr', 'reopen_stderr', 'log_messages'):
        return re.sub(r'(user|sys)-(\udcff)?[A-Za-z0-9_]+', r'\1-\2X', value.replace(scratch, 'SCRATCH'))
    return value


def check_axis_c_snapshots(args, *, run_worker):
    """Compare fresh allocation with forked instance snapshots for six word/library pairs.

    Temporarily toggle AXIS_C_ALLOCATE_EACH, restore its caller value even on
    failure, and assert equal exit/result/stderr envelopes. Print progress and
    return zero; reject suspend-offset movement of at least 0.01 seconds.
    """
    names = ['axis-c-options-single-00000008', 'axis-c-options-pair-00000060',
             'axis-c-options-boundary-ffffffff']
    offset = time.clock_gettime(time.CLOCK_BOOTTIME) - time.clock_gettime(time.CLOCK_MONOTONIC)
    # The private switch belongs to the allocation half only; clear any caller
    # value while validating the fork-snapshot half and restore it on the way
    # out so an invoking shell is left as found.
    caller_allocate_each = os.environ.pop('AXIS_C_ALLOCATE_EACH', None)
    try:
        with tempfile.TemporaryDirectory(prefix='contract-diff-') as scratch:
            for side in (args.prefix / 'lib/libpinyin.so', args.pinyin_so):
                for name in names:
                    os.environ['AXIS_C_ALLOCATE_EACH'] = '1'
                    original = run_worker('pinyin', side.resolve(), args.prefix / 'lib/libpinyin/data', name, scratch)
                    os.environ.pop('AXIS_C_ALLOCATE_EACH', None)
                    snapshot = run_worker('pinyin', side.resolve(), args.prefix / 'lib/libpinyin/data', name, scratch)
                    # Ignore only incidental directory inventory, as in the battery.
                    assert all(original[key] == snapshot[key] for key in ('exit', 'result', 'stderr')), name
                    print(f'instance snapshot: {side.name} {name} identical', flush=True)
    finally:
        if caller_allocate_each is None:
            os.environ.pop('AXIS_C_ALLOCATE_EACH', None)
        else:
            os.environ['AXIS_C_ALLOCATE_EACH'] = caller_allocate_each
    delta = time.clock_gettime(time.CLOCK_BOOTTIME) - time.clock_gettime(time.CLOCK_MONOTONIC) - offset
    assert abs(delta) < 0.01, 'discard suspend-overlapping snapshot validation'
    print(f'instance snapshot: 6 word/library comparisons, 96 input observations; identical; suspend delta {delta:.6f}', flush=True)
    return 0


def axis_c_reproduced_abort(pin_row, subject_row):
    """A register-covered scheme-setter row where both sides abort alike.

    Class (c) is retired (2026-10-31 UTC): the facade's setter reproduces the
    pin's `abort()` (rows 5c/5d), so both children die of SIGABRT before
    returning, with the worker envelope alone and the same stderr (an
    `abort()` writes nothing).
    """
    return (isinstance(pin_row, dict) and pin_row == subject_row
            and pin_row.get('exit') == -signal.SIGABRT and pin_row.get('result') is None
            and set(pin_row) == set(AXIS_C_ABORT_ROW_FIELDS))


def axis_c_covered_abort(pin_row, subject_row, mode):
    """Register-covered class (c) site at the nested-observation level.

    Mirrors the classic ``abort=`` expectation: the pin child dies of SIGABRT
    before returning, and the subject returns normally with the scheme setter
    refused (``set_scheme`` false, no observation) and exactly one warning in
    the facade's GLib domain at level 16. Rows 5c/5d of the register, on the
    zhuyin facade only: the pinyin facade reproduces the abort
    (`axis_c_reproduced_abort`).
    """
    if mode != 'zhuyin' or not isinstance(pin_row, dict) or not isinstance(subject_row, dict):
        return False
    result = subject_row.get('result')
    return (pin_row.get('exit') == -signal.SIGABRT and pin_row.get('result') is None
            and set(pin_row) == set(AXIS_C_ABORT_ROW_FIELDS)
            and subject_row.get('exit') == 0 and isinstance(result, dict)
            and set(result) == set(AXIS_C_REFUSAL_RESULT_FIELDS)
            and result.get('set_scheme') is False and result.get('observation') is None
            and result.get('logs') == [[WARNING_DOMAIN[mode], 16]])


# The exact registered shape of a scheme-setter observation. The abort row
# carries only the worker envelope; the refusal row's result carries exactly
# these four keys, so an extra unregistered field is never licensed.
AXIS_C_ABORT_ROW_FIELDS = ('exit', 'result', 'stderr')
AXIS_C_REFUSAL_RESULT_FIELDS = ('set_scheme', 'observation', 'logs', 'log_messages')


def axis_c_live(row):
    """A worker row that exited cleanly and returned a result.

    A row that died, was signalled, or printed no result observed nothing, so
    two identical failures must never read as an exact match. This mirrors the
    normal runner's rejection (`run_worker` verdicts); a pin abort is an
    explicit class (c) expectation and is matched by `axis_c_covered_abort`,
    never by equality.
    """
    return isinstance(row, dict) and row.get('exit') == 0 and row.get('result') is not None


def axis_c_licensed_remnant(row):
    """A covered row with only its licensed abort/refusal fields removed.

    Anything the row carries beyond those three fields stays and is compared,
    so an unregistered extra field still fails the case.
    """
    return {key: value for key, value in row.items() if key not in AXIS_C_ABORT_ROW_FIELDS}


def axis_c_covered_residual(pin, subject, covered_keys):
    """Both envelopes with only the licensed fields of the covered rows removed.

    Root exit/result/stderr, root `log_messages` and the parent-side call
    results, plus every non-covered observation in full, are retained and
    compared; a covered verdict therefore never hides an unrelated difference.
    """
    pin_residual = copy.deepcopy(pin)
    subject_residual = copy.deepcopy(subject)
    pin_rows = (pin_residual.get('result') or {}).get('observations')
    subject_rows = (subject_residual.get('result') or {}).get('observations')
    for key in covered_keys:
        pin_rows[key] = axis_c_licensed_remnant(pin_rows[key])
        subject_rows[key] = axis_c_licensed_remnant(subject_rows[key])
    return pin_residual, subject_residual


def axis_c_first_difference(left, right, path=''):
    """Describe the first unequal descendant reached in dictionaries or paired list items.

    Use sorted dictionary keys and index order for lists, extending path with
    .key or [index]. Missing dictionary entries are read as None; a length-only
    list difference or scalar comparison is reported at the current path. Values
    with repr length at least 300 are represented by its first 300 characters.
    """
    if type(left) is type(right) and isinstance(left, dict):
        for key in sorted(set(left) | set(right)):
            if left.get(key) != right.get(key):
                return axis_c_first_difference(left.get(key), right.get(key), path + '.' + key)
    if type(left) is type(right) and isinstance(left, list):
        for index, (a, b) in enumerate(zip(left, right)):
            if a != b:
                return axis_c_first_difference(a, b, path + f'[{index}]')
    return dict(field=path, pin=left if len(repr(left)) < 300 else repr(left)[:300],
                subject=right if len(repr(right)) < 300 else repr(right)[:300])


def run_axis_c(args, names, *, CASES, run_worker, AXIS_C_LAYOUT_COVERED, AXIS_C_DEFINITION):
    """Repeat and compare existing worker observations, with no saved output."""
    from collections import Counter
    from concurrent.futures import ThreadPoolExecutor
    offset = time.clock_gettime(time.CLOCK_BOOTTIME) - time.clock_gettime(time.CLOCK_MONOTONIC)
    totals, fingerprints = Counter(), {side: hashlib.sha256() for side in ('pin', 'subject')}
    differences, examples = Counter(), {}
    case_verdicts = {}
    def record_difference(name, key, left, right):
        """Count a difference signature and retain its first case/row example.

        For a nonzero pin exit, group by exits and diagnostics; otherwise group by
        the first differing field. Mutate the enclosing counters and example map.
        """
        first = axis_c_first_difference(left, right)
        pin_exit = left.get('exit') if isinstance(left, dict) else None
        subject_exit = right.get('exit') if isinstance(right, dict) else None
        if pin_exit:
            diagnostic = left.get('stderr', '')
            result = (right.get('result') or {}) if isinstance(right, dict) else {}
            signature = json.dumps(dict(pin_exit=pin_exit, subject_exit=subject_exit,
                                        pin_diagnostic=diagnostic,
                                        subject_logs=result.get('log_messages', result.get('logs'))),
                                   sort_keys=True, ensure_ascii=True)
        else:
            signature = json.dumps(first, sort_keys=True, ensure_ascii=True)
        differences[signature] += 1
        examples.setdefault(signature, dict(case=name, row=key, first=first))
    def observe(name, subject, companion):
        """Run one case in a private scratch directory and normalize its diagnostics.

        Pass the companion pinyin library through the worker environment and retain
        only exit, result and stderr before cleaning up the directory.
        """
        spec = CASES[name]
        # Each simultaneous observation owns its scratch/profile inventory.
        # The existing cleanup helper must never see another worker's files.
        with tempfile.TemporaryDirectory(prefix='worker-', dir=scratch) as isolated:
            value = run_worker(spec['mode'], subject, args.prefix / 'lib/libpinyin/data', name, isolated,
                               {'AXIS_C_COMPANION_PINYIN': str(companion)})
            return axis_c_normalize_diagnostics(
                {key: value[key] for key in ('exit', 'result', 'stderr')}, isolated)
    with tempfile.TemporaryDirectory(prefix='contract-diff-') as scratch, ThreadPoolExecutor(max_workers=2) as pool:
        for ordinal, name in enumerate(names, 1):
            mode = CASES[name]['mode']
            if args.axis_c_mutation:
                pin_so = args.pinyin_so if mode == 'pinyin' else args.zhuyin_so
                subject_so = args.axis_c_mutant_pinyin_so if mode == 'pinyin' else args.axis_c_mutant_zhuyin_so
                pin_companion, subject_companion = args.pinyin_so, args.axis_c_mutant_pinyin_so
            else:
                pin_so = args.prefix / ('lib/libpinyin.so' if mode == 'pinyin' else 'lib/libzhuyin.so')
                subject_so = args.pinyin_so if mode == 'pinyin' else args.zhuyin_so
                pin_companion, subject_companion = args.prefix / 'lib/libpinyin.so', args.pinyin_so
            assert pin_so and subject_so and pin_so.is_file() and subject_so.is_file(), name
            futures = [pool.submit(observe, name, so.resolve(), companion.resolve())
                       for so, companion in ((pin_so, pin_companion), (subject_so, subject_companion),
                                             (pin_so, pin_companion), (subject_so, subject_companion))]
            pin, subject, pin_repeat, subject_repeat = [future.result() for future in futures]
            stable = pin == pin_repeat and subject == subject_repeat
            assert stable, 'NOT-ESTABLISHED: unstable case ' + name
            for side, observation in (('pin', pin), ('subject', subject)):
                fingerprints[side].update(axis_c_canonical([name, observation]))
            totals['cases'] += 1
            totals['diagnostics_match' if pin['stderr'] == subject['stderr'] else 'diagnostics_differ'] += 1
            covered_row = AXIS_C_LAYOUT_COVERED.get(name)
            covered_rows, differ_rows = [], 0
            residual_first = None
            pin_rows = (pin['result'] or {}).get('observations')
            subject_rows = (subject['result'] or {}).get('observations')
            if pin_rows is not None and subject_rows is not None:
                for key in pin_rows:
                    left, right = pin_rows[key], subject_rows.get(key)
                    totals['observations'] += 1
                    if axis_c_live(left) and axis_c_live(right) and left == right:
                        totals['payload_match'] += 1
                    elif covered_row and axis_c_reproduced_abort(left, right):
                        totals['payload_reproduced_abort'] += 1
                    elif covered_row and axis_c_covered_abort(left, right, mode):
                        totals['payload_covered'] += 1
                        covered_rows.append(key)
                    else:
                        totals['payload_differ'] += 1
                        differ_rows += 1
                        record_difference(name, key, left, right)
                # First differing row only; all rows were compared in full.
                if differ_rows == 0 and axis_c_live(pin) and axis_c_live(subject) and pin == subject:
                    totals['case_match'] += 1
                    verdict = 'MATCH'
                elif (covered_row and differ_rows == 0 and covered_rows
                      and axis_c_live(pin) and axis_c_live(subject)):
                    # Don't just trust the covered rows: remove only their
                    # licensed fields and require the rest of the envelope to
                    # agree, so a new parent-side warning, root log message or
                    # outer stderr still makes the case DIFFER.
                    residual_pin, residual_subject = axis_c_covered_residual(pin, subject, covered_rows)
                    if residual_pin == residual_subject:
                        totals['covered'] += 1
                        covered_by_row = totals.setdefault('covered_by_row', {})
                        covered_by_row[covered_row] = covered_by_row.get(covered_row, 0) + 1
                        verdict = 'COVERED'
                    else:
                        totals['case_differ'] += 1
                        record_difference(name, 'covered-residual', residual_pin, residual_subject)
                        residual_first = axis_c_first_difference(residual_pin, residual_subject)
                        verdict = 'DIFFER'
                else:
                    totals['case_differ'] += 1
                    if differ_rows == 0:
                        record_difference(name, 'case', pin, subject)
                    verdict = 'DIFFER'
            else:
                totals['observations'] += 1
                if axis_c_live(pin) and axis_c_live(subject) and pin == subject:
                    totals['payload_match'] += 1
                    totals['case_match'] += 1
                    verdict = 'MATCH'
                else:
                    totals['payload_differ'] += 1
                    totals['case_differ'] += 1
                    record_difference(name, 'case', pin, subject)
                    verdict = 'DIFFER'
            case_verdicts[name] = verdict
            if verdict == 'DIFFER':
                print(json.dumps(dict(case=name, verdict='DIFFER',
                                      first=residual_first or axis_c_first_difference(pin, subject)),
                                 ensure_ascii=True), flush=True)
            elif verdict == 'COVERED':
                print(json.dumps(dict(case=name, verdict='COVERED', row=covered_row,
                                      covered_observations=len(covered_rows)), ensure_ascii=True), flush=True)
            if ordinal % 25 == 0:
                print(f'axis C progress: {ordinal}/{len(names)} cases', flush=True)
    elapsed_suspend = (time.clock_gettime(time.CLOCK_BOOTTIME) -
                       time.clock_gettime(time.CLOCK_MONOTONIC) - offset)
    assert abs(elapsed_suspend) < 0.01, 'NOT-ESTABLISHED: discard suspend-overlapping run'
    for signature in sorted(differences):
        print(json.dumps(dict(difference_signature=json.loads(signature), count=differences[signature],
                              example=examples[signature]), ensure_ascii=True), flush=True)
    print(json.dumps(dict(battery=args.axis_c_replacement, mutation=args.axis_c_mutation,
                          counts=dict(totals), repeats='identical', suspend_delta=elapsed_suspend,
                          hashes={side: h.hexdigest() for side, h in fingerprints.items()}), sort_keys=True), flush=True)
    if args.axis_c_mutation:
        controls = AXIS_C_DEFINITION['mutations'][args.axis_c_mutation]['controls']
        for control in controls:
            print(json.dumps(dict(mutation=args.axis_c_mutation, control=control,
                                  verdict=case_verdicts.get(control)), ensure_ascii=True), flush=True)
        undetected = [control for control in controls if case_verdicts.get(control) != 'DIFFER']
        assert not undetected, ('NOT-ESTABLISHED: %s control(s) not detected: %s'
                                % (args.axis_c_mutation, ', '.join(undetected)))
        return 0
    return 1 if totals['case_differ'] else 0
