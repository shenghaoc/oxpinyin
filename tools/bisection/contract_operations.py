"""The two operations of the contract differential: `capture` and `verify`.

    contract-diff.py verify  CELL PINYIN_SO [--zhuyin-so PATH] [--data DIR]
                             [--references DIR] [--cases ...] [--issue N]... [--family F]...
                             [--cases-prefix P,...] [--expect-parent] [--plan-only]
                             [--case-timeout SECONDS] [--jobs N] [--observations FILE]
                             [--reverse-cross-open PIN_PREFIX]
    contract-diff.py capture CELL PREFIX --data DIR [--out DIR] [--references DIR]
                             [--cases ...] [--issue N]... [--family F]... [--cases-prefix P,...]
                             [--runs N] [--case-timeout SECONDS]

`verify` runs the selected cases on oxpinyin only and compares each with the committed pin
observation (contract_reference). It needs no pin prefix, pin library or container, and it
never captures: a missing, incomplete or incompatible reference fails the case with a message
that names the reason. `--expect-parent` runs the same cases against a parent build: a case
marked `control` must still match and every other case must differ.

`verify --reverse-cross-open PIN_PREFIX` is the one local-only check: for the four handoff cases it
lets the pin read the profile oxpinyin wrote (the direction no frozen reference can check). It
needs a pin build, so it refuses to run in GitHub Actions; see docs/runbooks/contract-references.md.

`capture` runs the selected cases on the pin, inside the Debian stable pin image
(tools/oracle/capture-contract.sh), and writes the references. An investigative live
comparison is a capture into a temporary `--out` followed by `verify --references` on it.

The system data is the Debian `libpinyin-data` package (tools/oracle/libpinyin-data.json,
`just fetch-contract-data`). Its bigram.db is a tkrzw file, so the cases that read it run on
the tkrzw cell only; KC and BDB run the cases that build their own data. Which cases read the
package data is recorded in their tkrzw references.

This module holds the driver as an argument, like the other contract_* helpers.
"""
import argparse
import json
from pathlib import Path
import sys
import tempfile
import time

import contract_reference as reference
from contract_selection import SelectionError


def require_pin_build(prefix, cell):
    manifest = (prefix / 'oracle-pin.txt').read_text()
    assert 'libpinyin-2.11.92-' + reference.PIN_COMMIT in manifest and '+dbm-' + cell + '\n' in manifest, \
        'the pin build does not match the pinned commit and cell'


def _selectors(parser):
    parser.add_argument('--cases')
    parser.add_argument('--cases-prefix', metavar='P[,P...]')
    parser.add_argument('--issue', action='append', default=[], metavar='TAG')
    parser.add_argument('--family', action='append', default=[], metavar='F')
    parser.add_argument('--case-timeout', type=float, default=300)


def _check_selectors(parser, args, driver):
    if args.cases is not None:
        try:
            driver.case_names(args.cases)
        except SelectionError as error:
            parser.error(str(error))
    try:
        driver.contract_selection.prefix_matches(driver.CASES, args.cases_prefix)
    except SelectionError as error:
        parser.error(str(error))
    if not 0 < args.case_timeout < float('inf'):
        parser.error('--case-timeout must be a positive finite number of seconds')


def _in_cell(driver, names, cell):
    return [name for name in names if cell in driver.CASES[name]['cells']]


def _without_package_data(driver, names, cell, references):
    """KC and BDB run the cases that read no package data (the tkrzw references say which)."""
    if cell == 'tkrzw':
        return names, []
    modules = {reference.module_of(driver.CASES[name]) for name in names}
    free = reference.data_free_cases(references, sorted(modules))
    return [name for name in names if name in free], [name for name in names if name not in free]


def default_suite(driver):
    """The unselected suite: every registered case but axis C (the opt-in battery) and its helper."""
    names, _ = driver.select_cases(None, (), (), None)
    return [name for name in names if name not in reference.HELPER_CASES]


def case_list(parser, value):
    """`--cases @FILE` reads the IDs, one per line."""
    if value is not None and value.startswith('@'):
        try:
            return ','.join(line.strip() for line in Path(value[1:]).read_text().splitlines() if line.strip())
        except OSError as error:
            parser.error(str(error))
    return value


def verify(driver, argv):
    """Run the selected cases on the subject alone and compare with the references."""
    parser = argparse.ArgumentParser(prog='contract-diff.py verify', description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('cell', choices=['bdb', 'kc', 'tkrzw'])
    parser.add_argument('pinyin_so', type=Path)
    parser.add_argument('--zhuyin-so', type=Path)
    parser.add_argument('--data', type=Path, help='the package system data directory (tkrzw cell)')
    parser.add_argument('--references', type=Path, default=reference.REFERENCES)
    _selectors(parser)
    parser.add_argument('--jobs', type=driver.positive_jobs)
    parser.add_argument('--expect-parent', action='store_true')
    parser.add_argument('--plan-only', action='store_true')
    parser.add_argument('--observations', type=Path, help='retain both complete observations as JSONL')
    parser.add_argument('--run-platform-excluded', action='store_true')
    parser.add_argument('--reverse-cross-open', type=Path, metavar='PIN_PREFIX',
                        help='local only: also let the pin read what oxpinyin wrote (the four handoff cases)')
    args = parser.parse_args(argv)
    args.cases = case_list(parser, args.cases)
    _check_selectors(parser, args, driver)
    if args.reverse_cross_open is not None:
        if driver.os.environ.get('GITHUB_ACTIONS'):
            parser.error('--reverse-cross-open runs the pin and is never run in GitHub Actions')
        if args.expect_parent or args.plan_only or args.observations:
            parser.error('--reverse-cross-open takes no --expect-parent, --plan-only or --observations')
    driver.os.environ['CONTRACT_DIFF_CELL'] = args.cell
    if not args.pinyin_so.is_file():
        parser.error(f'missing input: {args.pinyin_so}')
    if args.cell == 'tkrzw' and args.data is None:
        parser.error('the tkrzw cell reads the package system data: pass --data DIR (just fetch-contract-data)')
    try:
        digest = reference.data_identity(args.data) if args.data is not None else None
    except reference.ReferenceError as error:
        parser.error(str(error))
    for so in (args.pinyin_so, args.zhuyin_so):
        if so is not None:
            driver.check_tkrzw(args.cell, 'contract-diff.py', 'subject', so)
    args.prefix = None
    args.references = reference.Store(args.references, args.cell, digest, driver.NO_ABORT)

    def keep(names):
        names = [name for name in _in_cell(driver, names, args.cell) if name not in reference.HELPER_CASES]
        names, package = _without_package_data(driver, names, args.cell, args.references.root)
        if package:
            print('%d case%s read the package data and run on the tkrzw cell only' % (
                len(package), '' if len(package) == 1 else 's'), file=sys.stderr)
        return names

    if args.reverse_cross_open is not None:
        return reverse_cross_open(driver, parser, args)
    try:
        return driver.run_cases(args, parser, args.data or Path('/nonexistent-contract-data'),
                                default_names=keep(default_suite(driver)), keep=keep)
    except reference.ReferenceError as error:
        parser.error(str(error))


def reverse_cross_open(driver, parser, args):
    """Run the four handoff cases live, pin and oxpinyin, in both directions (local only).

    This reuses `Runner.run_case`: each side saves a profile, then the other library reads it. The
    pin is the build at PIN_PREFIX (as for `capture`); the verdict is the ordinary one, from the two
    live observations. Run it after any change to a persistence writer, to serialisation or to a
    database format."""
    names = list(reference.REVERSE_CROSS_OPEN_CASES)
    if args.cases is not None or args.issue or args.family or args.cases_prefix:
        selected, _ = driver.select_cases(args.cases, args.issue, args.family, args.cases_prefix)
        names = [name for name in names if name in selected]
    names = _in_cell(driver, names, args.cell)
    if not names:
        parser.error('no handoff case is selected on this cell: %s' % ', '.join(reference.REVERSE_CROSS_OPEN_CASES))
    require_pin_build(args.reverse_cross_open, args.cell)
    lib = args.reverse_cross_open / 'lib'
    pins = {'pinyin': lib / 'libpinyin.so', 'zhuyin': lib / 'libzhuyin.so'}
    for library in pins.values():
        if not library.is_file():
            parser.error(f'missing input: {library}')
        driver.check_tkrzw(args.cell, 'contract-diff.py', 'pin', library)
    failures = 0
    with tempfile.TemporaryDirectory(prefix='reverse-cross-open-') as scratch:
        for name in names:
            driver.check_cancelled()
            spec = driver.CASES[name]
            subject_so = args.zhuyin_so if spec['mode'] == 'zhuyin' else args.pinyin_so
            if subject_so is None:
                parser.error('%s needs %s' % (name, '--zhuyin-so' if spec['mode'] == 'zhuyin' else 'PINYIN_SO'))
            with tempfile.TemporaryDirectory(prefix='case-', dir=scratch) as isolated:
                pin, subject = driver.run_case(spec, pins[spec['mode']], subject_so, args.data or Path('/nonexistent-contract-data'), name, isolated,
                                               args.case_timeout)
            verdict, expected, _ = driver.verdict_for(spec, pin, subject, False)
            failures += not expected
            print(json.dumps(dict(case=name, cell=args.cell, verdict=verdict, expected_ok=expected,
                                  exit=[pin['exit'], subject['exit']], direction='both'), sort_keys=True), flush=True)
    return 1 if failures else 0


def capture(driver, argv):
    """Run the selected cases on the pin and write the references."""
    parser = argparse.ArgumentParser(prog='contract-diff.py capture', description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('cell', choices=['bdb', 'kc', 'tkrzw'])
    parser.add_argument('prefix', type=Path, help='the pin build for the cell (PREFIX/lib/libpinyin.so)')
    parser.add_argument('--data', type=Path, required=True)
    parser.add_argument('--out', type=Path, default=reference.REFERENCES)
    parser.add_argument('--references', type=Path, default=None,
                        help='where the tkrzw references name the package-data cases (default: --out)')
    parser.add_argument('--runs', type=int, default=2, help='captures per case that must agree (default 2)')
    _selectors(parser)
    args = parser.parse_args(argv)
    args.cases = case_list(parser, args.cases)
    _check_selectors(parser, args, driver)
    if args.runs < 1:
        parser.error('--runs must be at least 1')
    try:
        reference.require_capture_environment()
        digest = reference.data_identity(args.data)
    except reference.ReferenceError as error:
        parser.error(str(error))
    driver.os.environ['CONTRACT_DIFF_CELL'] = args.cell
    lib = args.prefix / 'lib'
    require_pin_build(args.prefix, args.cell)
    libraries = {'pinyin': lib / 'libpinyin.so', 'zhuyin': lib / 'libzhuyin.so'}
    for library in libraries.values():
        assert library.is_file(), 'missing input: %s' % library
        driver.check_tkrzw(args.cell, 'contract-diff.py', 'pin', library)
    try:
        if args.cases is None and not (args.issue or args.family or args.cases_prefix):
            names = default_suite(driver)
        else:
            names, _ = driver.select_cases(args.cases, args.issue, args.family, args.cases_prefix)
        names = [name for name in _in_cell(driver, names, args.cell) if name not in reference.HELPER_CASES]
        names, package = _without_package_data(driver, names, args.cell, args.references or args.out)
    except (SelectionError, reference.ReferenceError) as error:
        parser.error(str(error))
    environment = reference.environment()
    failures = 0
    written = {}
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix='contract-capture-') as scratch:
        for name in names:
            driver.check_cancelled()
            spec = driver.CASES[name]
            if driver.platform_skips(spec):
                print(json.dumps(dict(case=name, cell=args.cell, verdict='PLATFORM-EXCLUDED',
                                      reason=spec['platform_skip_reason']), sort_keys=True), flush=True)
                failures += 1
                continue
            pin_so = libraries[spec['mode']]
            observations = []
            for _ in range(args.runs):
                with tempfile.TemporaryDirectory(prefix='case-', dir=scratch) as isolated:
                    captured = driver.run_capture(spec, pin_so, args.data, name, isolated, args.case_timeout,
                                                  companion=libraries['pinyin'])
                    if name.startswith('axis-c-'):
                        # The battery compares diagnostics with generated paths normalised.
                        captured[0].update(driver.axis_c_normalize_diagnostics(
                            driver.axis_c_envelope(captured[0]), isolated))
                    observations.append(captured)
            first = observations[0]
            stable = all(reference.comparable(spec, other, driver.observed, driver.fatal_stderr)
                         == reference.comparable(spec, first, driver.observed, driver.fatal_stderr)
                         for other in observations[1:])
            if stable and name in ('lazy-user-bigram-pinyin', 'lazy-user-bigram-zhuyin'):
                snapshots = []
                for _, profile, _ in observations:
                    with tempfile.TemporaryDirectory(prefix='snapshot-', dir=scratch) as decoded:
                        reference.decode_profile(profile, decoded)
                        snapshots.append(tuple(driver.tkrzw_saved_bigram_bytes(
                            Path(decoded, filename).read_bytes())
                            for filename in ('empty.db', 'trained.db')))
                stable = all(saved == snapshots[0] for saved in snapshots[1:])
            verdict = 'CAPTURED'
            if not stable:
                verdict = 'UNSTABLE'
            elif first[0]['exit'] in ('TIMEOUT',) or (
                    first[0]['exit'] != 'NOT-RUN' and first[0]['result'] is None
                    and not driver.registered_termination(spec) and not driver.policy_pin_abort(spec)):
                verdict = 'INVALID'
            if verdict == 'CAPTURED':
                observation, profile, used = first
                pinned = spec.get('pin_not_run') is not None
                row = dict(schema=reference.SCHEMA, case=name, cell=args.cell, mode=spec['mode'],
                           provenance='pin_not_run' if pinned else 'pin',
                           identity=dict(
                               inputs=reference.inputs_digest(spec, driver.NO_ABORT),
                               pin=dict(commit=reference.PIN_COMMIT,
                                        library_sha256=None if pinned else reference.file_sha256(pin_so)),
                               backend=args.cell,
                               system_data=dict(kind=reference.PACKAGE, digest=digest) if used
                               else dict(kind=reference.SYNTHETIC),
                               environment=environment),
                           pin=reference.stable_pin(observation))
                if profile is not None:
                    row['profile'] = profile
                spelling = reference.scanner_spelling(spec)
                if spelling is not None:
                    row['scanner_verdict'] = driver.host_scanf_verdict(spelling)
                written.setdefault(reference.module_of(spec), {})[name] = row
            else:
                failures += 1
            print(json.dumps(dict(case=name, cell=args.cell, verdict=verdict, exit=first[0]['exit'],
                                  package_data=first[2]), sort_keys=True), flush=True)
    for module, rows in written.items():
        reference.write_rows(args.out, args.cell, module, rows)
    print(json.dumps(dict(captured=sum(map(len, written.values())), failed=failures,
                          seconds=round(time.monotonic() - started, 1), out=str(args.out)), sort_keys=True), flush=True)
    return 1 if failures else 0
