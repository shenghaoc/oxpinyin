#!/usr/bin/env python3
"""Execute the live parity registry. Standard library only; never cache the oracle."""
import argparse
import concurrent.futures
import contextlib
import fcntl
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
CELLS = ('bdb', 'kc', 'tkrzw')


def positive(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError('must be positive')
    return number


def assignments(values, allowed, parser):
    result = {}
    for value in values:
        key, sep, item = value.partition('=')
        if not sep or key not in allowed or not item:
            parser.error(f'invalid assignment: {value}')
        result[key] = item
    return result


def execute(command, env, cwd, timeout, output):
    """Bound the whole process group, including grandchildren retaining stdout."""
    start = time.monotonic()
    with output.open('wb') as log:
        try:
            process = subprocess.Popen(command, env=env, cwd=cwd, stdout=log,
                                       stderr=subprocess.STDOUT, start_new_session=True)
        except OSError as exc:
            return 1, time.monotonic() - start, str(exc), False
        expired = False
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            expired = True
            with contextlib.suppress(ProcessLookupError):
                os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
            # A descendant may outlive its leader, even after the leader exits.
            with contextlib.suppress(ProcessLookupError):
                os.killpg(process.pid, signal.SIGKILL)
            process.wait()
    text = output.read_text(errors='replace')
    return process.returncode, time.monotonic() - start, text, expired


def reason_for(rc, output, timeout, expired=False):
    # Only the aggregate deadline is a timeout here; a runner that wraps its own
    # processes in timeout(1) and exits 124 is reported by its own output.
    if expired:
        return f'timeout after {timeout}s (exit {rc})'
    lines = [line.strip() for line in output.splitlines() if line.strip()]
    important = [line for line in lines if re.search(
        r'FAIL|DIVERGEN|SKIP|missing input|fatal:|Assertion .* failed|error:', line)]
    if rc == 0:
        return 'completed'
    # Preserve the concrete failure rather than a shell crash wrapper.
    return (important[-1] if important else (lines[-1] if lines else f'exit {rc}'))[:400]


def normal_tag(tag):
    """`#640`, `640`, `W11`, `row:54` and `Row 54` are the same selector (as contract-diff.py)."""
    tag = str(tag).strip().lstrip('#').lower()
    return re.sub(r'^row[ :]*', 'row:', tag)


def contract_tag_index():
    """contract-diff.py's tag index: {'tags': {tag: [case IDs]}, 'modes': {case ID: facade}}."""
    command = [sys.executable, str(HERE / 'contract-diff.py'), '--list-tags']
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    if result.returncode:
        raise ValueError(result.stderr.strip() or 'contract-diff.py --list-tags failed')
    return json.loads(result.stdout)


def contract_cases_for(requested, index):
    """The contract case IDs the requested tags select, in registration order,
    with the tag(s) that selected each: {case ID: [tag, ...]}."""
    chosen = {}
    for tag in sorted(requested):
        for name in index['tags'].get(tag, []):
            chosen.setdefault(name, []).append(tag)
    return chosen


def timing_of(output):
    """The phase split a runner reports: its last stdout line that is a JSON
    object with a `timing` key (contract-diff.py prints one), else None."""
    for line in reversed(output.splitlines()):
        if line.startswith('{"timing"') or line.startswith('{"timing":'):
            try:
                return json.loads(line)['timing']
            except (ValueError, KeyError):
                return None
    return None


def plan_lines(cell, jobs, contract_cases, reasons, index):
    """The selection plan for one cell: each runner, why it runs, the inputs
    it needs and whether it needs the oracle; for a tag-selected contract
    run, the case IDs, why each was selected and their facades."""
    lines = [f'plan ({cell}): {len(jobs)} runner{"" if len(jobs) == 1 else "s"}']
    for row, _, _, substitutions in jobs:
        inputs = [item.format_map(substitutions) for item in row.get('input_paths', [])] or ['none']
        lines.append(f"  {row['name']}: {reasons[row['name']]}; oracle: "
                     f"{'yes' if row.get('oracle', True) else 'no'}; inputs: {', '.join(inputs)}")
        if row['name'] == 'contract-diff' and contract_cases:
            modes = {}
            for name in contract_cases:
                mode = index['modes'].get(name, '?')
                modes[mode] = modes.get(mode, 0) + 1
            lines.append('    %d contract case%s; facades: %s' % (
                len(contract_cases), '' if len(contract_cases) == 1 else 's',
                ', '.join('%s %d' % item for item in sorted(modes.items()))))
            names = list(contract_cases)
            shown = names if len(names) <= 50 else names[:3]
            width = max(len(name) for name in shown)
            for name in shown:
                lines.append('    %-*s  %-7s issue %s' % (width, name, index['modes'].get(name, '?'),
                                                        ', '.join(contract_cases[name])))
            if len(names) > 50:
                lines.append('    ... %d more (contract-diff.py --list-tags lists them)' % (len(names) - 3))
    return lines


def cell_config(cell, target_root, env):
    configured = env.copy()
    configured['PINYIN_ORACLE_DBM'] = cell
    configured['OXPINYIN_TARGET_ROOT'] = str(target_root)
    command = ['bash', str(HERE / 'cell-artifacts.sh'), '--describe']
    result = subprocess.run(command, env=configured, cwd=ROOT, capture_output=True, text=True)
    if result.returncode:
        raise ValueError(result.stderr.strip() or 'cell configuration failed')
    return json.loads(result.stdout)


def oracle_config(prefix, variant, env):
    command = ['bash', str(HERE / 'cell-artifacts.sh'), '--describe-oracle',
               str(prefix), variant]
    result = subprocess.run(command, env=env, cwd=ROOT, capture_output=True, text=True)
    if result.returncode:
        raise ValueError(result.stderr.strip() or 'oracle configuration failed')
    return json.loads(result.stdout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runners', help='comma-separated registry names (default: every gate)')
    parser.add_argument('--cells', default='bdb', help='comma-separated bdb,kc,tkrzw')
    parser.add_argument('--issue', action='append', default=[],
                        help='issue number, W tag, or row:N; selects the runners and the exact '
                             'contract-diff cases registered under it')
    parser.add_argument('--list', action='store_true', help='list gate and not-a-gate metadata; do not build')
    parser.add_argument('--jobs', type=positive, default=2)
    parser.add_argument('--timeout', type=positive, help='override the registry default for every runner')
    parser.add_argument('--runner-timeout', action='append', default=[], metavar='RUNNER=SECONDS')
    parser.add_argument('--allow-skipped', action='append', default=[], metavar='RUNNER[:CELL]',
                        help='explicitly allow these skips; * allows all skips, never failures')
    parser.add_argument('--oracle-root', type=Path,
                        default=Path(os.environ.get('OXPINYIN_ORACLE_ROOT', Path.home() / '.local/opt/pinyin-oracle')))
    parser.add_argument('--oracle', action='append', default=[], metavar='CELL=PREFIX')
    parser.add_argument('--cell-env', action='append', default=[], metavar='CELL:NAME=VALUE',
                        help='additional per-cell runner inputs, e.g. staged packaging roots')
    parser.add_argument('--ibus-build', action='append', default=[], metavar='CELL=DIR',
                        help='pin-built IBus frontend work directory for import interop')
    parser.add_argument('--data', action='append', default=[], metavar='CELL=DIR',
                        help='subject full tables plus interpolation2.text, matching that cell')
    parser.add_argument('--target-root', type=Path,
                        default=Path(os.environ.get('OXPINYIN_TARGET_ROOT', ROOT / 'target/cells')))
    parser.add_argument('--no-build', action='store_true', help='use existing dev artifacts at the cell targets')
    parser.add_argument('--build-timeout', type=positive, default=900)
    parser.add_argument('--json', type=Path, help='write the same result table as JSON (no raw captures)')
    args = parser.parse_args()
    registry = json.loads((HERE / 'runners.json').read_text())
    names = {row['name'] for row in registry}
    selected = set(args.runners.split(',')) if args.runners else names
    unknown = selected - names
    if unknown:
        parser.error('unknown runners: ' + ', '.join(sorted(unknown)))
    cells = list(dict.fromkeys(args.cells.split(',')))
    if not cells or set(cells) - set(CELLS):
        parser.error('--cells must select bdb,kc,tkrzw')
    requested_issues = {normal_tag(item) for item in args.issue}
    rows = [row for row in registry if row['name'] in selected and
            (not requested_issues or requested_issues.intersection(
                normal_tag(tag) for tag in row['issues']))]
    reasons = {row['name']: ('--issue ' + ', '.join(sorted(requested_issues.intersection(
        normal_tag(tag) for tag in row['issues']))) if requested_issues else
        '--runners' if args.runners else 'default gate set') for row in rows}
    # An issue tag also resolves to the exact contract cases registered under
    # it; those run through the contract-diff runner with --cases. Nothing
    # matching anywhere is an error that names what exists, never the suite.
    contract_cases, index = {}, None
    if requested_issues and 'contract-diff' in selected:
        index = contract_tag_index()
        contract_cases = contract_cases_for(requested_issues, index)
        if contract_cases and all(row['name'] != 'contract-diff' for row in rows):
            rows.append(next(row for row in registry if row['name'] == 'contract-diff'))
            reasons['contract-diff'] = '--issue %s: %d contract case%s' % (
                ', '.join(sorted({tag for tags in contract_cases.values() for tag in tags})),
                len(contract_cases), '' if len(contract_cases) == 1 else 's')
        elif contract_cases:
            reasons['contract-diff'] += '; %d contract case%s' % (
                len(contract_cases), '' if len(contract_cases) == 1 else 's')
    if not rows:
        runner_tags = sorted({normal_tag(tag) for row in registry for tag in row['issues']})
        contract_tags = sorted(tag for tag in (index or contract_tag_index())['tags']
                               if not tag.startswith('family/'))
        parser.error('no runner or contract case carries issue tag(s) %s; runner tags: %s; '
                     'contract tags: %s' % (', '.join(sorted(requested_issues)),
                                            ', '.join(runner_tags), ', '.join(contract_tags)))
    if contract_cases:
        rows = [dict(row, command=row['command'] + ['--', '--cases', ','.join(contract_cases)])
                if row['name'] == 'contract-diff' else row for row in rows]
    if args.list:
        for row in rows:
            print(json.dumps(row, ensure_ascii=False))
        return 0
    measurements = [row['name'] for row in rows if row['kind'] != 'gate']
    if args.runners and measurements:
        parser.error('not-a-gate (measurement recipes are never executed): ' + ', '.join(measurements))
    rows = [row for row in rows if row['kind'] == 'gate']
    if not rows:
        parser.error('selection contains no gates')
    oracle_overrides = assignments(args.oracle, CELLS, parser)
    data_overrides = assignments(args.data, CELLS, parser)
    ibus_builds = assignments(args.ibus_build, CELLS, parser)
    timeouts = assignments(args.runner_timeout, names, parser)
    try:
        timeouts = {name: positive(value) for name, value in timeouts.items()}
    except (ValueError, argparse.ArgumentTypeError):
        parser.error('runner timeouts must be positive integers')
    for allowance in args.allow_skipped:
        runner, _, cell = allowance.partition(':')
        if allowance != '*' and (runner not in names or cell and cell not in CELLS):
            parser.error(f'invalid skip allowance: {allowance}')
    extra_env = {cell: {} for cell in CELLS}
    protected = {'CARGO_TARGET_DIR', 'PINYIN_ORACLE_DBM', 'PINYIN_ORACLE_PREFIX',
                 'OXPINYIN_CAPI_SO', 'OXPINYIN_ZHUYIN_SO', 'OXPINYIN_DICTOOL',
                 'OXPINYIN_SYSTEM_DIR', 'CARGO_PROFILE_DEV_OPT_LEVEL'}
    for value in args.cell_env:
        cell, sep, assignment = value.partition(':')
        key, equals, item = assignment.partition('=')
        if (cell not in CELLS or not sep or not equals or
                not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', key) or key in protected):
            parser.error(f'invalid or reserved cell environment setting: {value}')
        extra_env[cell][key] = item
    base_env = os.environ.copy()
    if len(cells) > 1 and base_env.get('CARGO_TARGET_DIR'):
        parser.error('CARGO_TARGET_DIR is a single-cell override; use --target-root for multiple cells')
    targets = args.target_root.resolve()
    results, jobs = [], []
    build_seconds = {}
    # Locks live beside the checkout rather than in per-cell or per-invocation
    # directories. This protects fixed native outputs across concurrent invocations.
    lock_root = ROOT / 'target/runner-locks'
    lock_root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='oxpinyin-gates-') as scratch_name:
        scratch = Path(scratch_name)
        for cell in cells:
            cfg = cell_config(cell, targets, base_env)
            prefix = Path(oracle_overrides.get(cell) or
                          (base_env.get('PINYIN_ORACLE_PREFIX') if len(cells) == 1 else '') or
                          args.oracle_root / cell).resolve()
            data = Path(data_overrides.get(cell) or
                        (base_env.get('OXPINYIN_SYSTEM_DIR') if len(cells) == 1 else '') or
                        prefix / 'lib/libpinyin/data').resolve()
            env = base_env.copy()
            env.update({key: str(value) for key, value in cfg.items()})
            env.update(PINYIN_ORACLE_PREFIX=str(prefix), OXPINYIN_SYSTEM_DIR=str(data),
                       OXPINYIN_CAPI_SO=str(Path(cfg['CARGO_TARGET_DIR']) / 'debug/libpinyin_capi.so'),
                       OXPINYIN_ZHUYIN_SO=str(Path(cfg['CARGO_TARGET_DIR']) / 'debug/libzhuyin_capi.so'),
                       OXPINYIN_DICTOOL=str(Path(cfg['CARGO_TARGET_DIR']) / 'debug/oxpinyin-dictool'))
            env.update(extra_env[cell])
            env['OXPINYIN_USER_RT_TEST'] = str(Path(cfg['CARGO_TARGET_DIR']) / 'debug/user-dir-round-trip-test')
            env['OXPINYIN_BUILD_USER_RT'] = '1' if any(row['name'] == 'user-dir-round-trip' for row in rows) else '0'
            env['PINYIN_IBUS_PREFIX'] = str(prefix)
            if cell in ibus_builds:
                env['PINYIN_IBUS_BUILD_DIR'] = str(Path(ibus_builds[cell]).resolve())
            elif len(cells) > 1:
                env.pop('PINYIN_IBUS_BUILD_DIR', None)
            substitutions = dict(root=str(ROOT), prefix=str(prefix), data=str(data), cell=cell,
                                 capi=env['OXPINYIN_CAPI_SO'], zhuyin=env['OXPINYIN_ZHUYIN_SO'])
            build_candidates = []
            oracle_configs = {}
            for row in rows:
                variant = row.get('oracle_variant', 'unpatched')
                if variant not in oracle_configs:
                    oracle_configs[variant] = oracle_config(prefix, variant, env)
                oracle = oracle_configs[variant]
                runner_prefix = Path(oracle['PINYIN_ORACLE_PREFIX'])
                result = dict(runner=row['name'], cell=cell, status='SKIPPED', seconds=0.0,
                              build_seconds=0.0, lock_wait_seconds=0.0, phases=None,
                              pin_ref=oracle['EXPECTED_ORACLE_PIN_REF'].removeprefix('pin_ref='), reason='')
                if cell not in row['cells']:
                    result['reason'] = 'unsupported cell'
                elif row.get('oracle', True):
                    manifest = runner_prefix / 'oracle-pin.txt'
                    patches = runner_prefix / 'oracle-patches.sha256'
                    if not manifest.is_file() or not (runner_prefix / 'lib/libpinyin.so').is_file():
                        result['reason'] = f'missing oracle: {runner_prefix}'
                        if variant != 'unpatched':
                            result['status'] = 'FAIL'
                    elif oracle['EXPECTED_ORACLE_PIN_REF'] not in manifest.read_text().splitlines():
                        result.update(status='FAIL', reason=f'oracle pin/cell mismatch: {manifest}')
                    elif oracle['ORACLE_PATCH_FILE'] and (not patches.is_file() or
                            oracle['ORACLE_PATCH_FILE'] not in patches.read_text()):
                        result.update(status='FAIL', reason=f'missing required oracle patch: {patches}')
                if result['reason']:
                    results.append(result)
                else:
                    build_candidates.append((row, result, runner_prefix))
            if not build_candidates:
                continue
            cell_jobs = []
            for row, result, runner_prefix in build_candidates:
                runner_env = dict(env, PINYIN_ORACLE_PREFIX=str(runner_prefix))
                runner_substitutions = dict(substitutions, prefix=str(runner_prefix))
                cell_jobs.append((row, result, runner_env, runner_substitutions))
            for line in plan_lines(cell, cell_jobs, contract_cases, reasons, index):
                print(line, file=sys.stderr, flush=True)
            if not args.no_build:
                print(f'Building dev opt-level 1 artifacts: {cell}', file=sys.stderr, flush=True)
                rc, seconds, output, expired = execute(['bash', str(HERE / 'cell-artifacts.sh')], env, ROOT,
                                                       args.build_timeout, scratch / f'build-{cell}.log')
                build_seconds[cell] = round(seconds, 3)
                if rc or expired:
                    for _, result, _ in build_candidates:
                        result.update(status='FAIL', seconds=round(seconds, 3), build_seconds=build_seconds[cell],
                                      reason='cell build: ' + reason_for(rc, output, args.build_timeout, expired))
                        results.append(result)
                    continue
            jobs.extend(cell_jobs)

        def run(job):
            row, result, base, substitutions = job
            timeout = timeouts.get(row['name'], args.timeout or row['timeout'])
            work = scratch / (row['name'] + '-' + result['cell'])
            work.mkdir()
            substitutions = dict(substitutions, work=str(work))
            env = base.copy()
            env.update({key: value.format_map(substitutions) for key, value in row.get('env', {}).items()})
            env['TMPDIR'] = str(work)
            substitutions['env'] = env
            absent = [name for name in row.get('required_env', []) if not env.get(name)]
            if absent:
                result.update(status='SKIPPED', reason='missing required environment: ' + ', '.join(absent))
                return result
            command = [part.format_map(substitutions) for part in row['command']]
            missing = [item.format_map(substitutions) for item in row.get('input_paths', [])
                       if not Path(item.format_map(substitutions)).exists()]
            if missing:
                result.update(status='FAIL' if row.get('oracle_variant', 'unpatched') != 'unpatched'
                              else 'SKIPPED', reason='missing required input: ' + ', '.join(missing))
                return result
            with contextlib.ExitStack() as stack:
                waiting = time.monotonic()
                for resource in sorted(row['exclusive_resources']):
                    lock = stack.enter_context((lock_root / resource).open('a'))
                    fcntl.flock(lock, fcntl.LOCK_EX)
                lock_wait = time.monotonic() - waiting
                rc, seconds, output, expired = execute(command, env, ROOT, timeout, work / 'runner.log')
            # Legacy CI behavior is preserved in the direct live-typing runner;
            # aggregate skips cannot silently turn into passes.
            required_skips = [line for line in output.splitlines() if 'SKIP:' in line and
                              not any(token in line for token in row.get('optional_skips', []))]
            if rc == 0 and required_skips and not expired:
                rc, output = 77, '\n'.join(required_skips)
            result.update(status='FAIL' if expired else 'PASS' if rc == 0 else 'SKIPPED' if rc == 77 else 'FAIL',
                          seconds=round(seconds, 3), build_seconds=build_seconds.get(result['cell'], 0.0),
                          lock_wait_seconds=round(lock_wait, 3), phases=timing_of(output),
                          reason=reason_for(rc, output, timeout, expired))
            print(f"Completed {result['runner']} / {result['cell']}: {result['status']}",
                  file=sys.stderr, flush=True)
            return result

        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
            results.extend(pool.map(run, jobs))
    order = {row['name']: index for index, row in enumerate(rows)}
    results.sort(key=lambda row: (order[row['runner']], cells.index(row['cell'])))
    # build_seconds is the cell's one build, shared by every row of that cell.
    print('runner | cell | status | seconds | build_seconds | lock_wait_seconds | pin_ref | reason')
    print('--- | --- | --- | ---: | ---: | ---: | --- | ---')
    for row in results:
        print(' | '.join(str(row[key]).replace('|', '/').replace('\n', ' ') for key in
                         ('runner', 'cell', 'status', 'seconds', 'build_seconds', 'lock_wait_seconds',
                          'pin_ref', 'reason')))
    if args.json:
        args.json.write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n')
    def allowed(row):
        return any(item in args.allow_skipped for item in
                   ('*', row['runner'], row['runner'] + ':' + row['cell']))
    return int(any(row['status'] == 'FAIL' or row['status'] == 'SKIPPED' and not allowed(row)
                   for row in results))


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr)
        sys.exit(1)
