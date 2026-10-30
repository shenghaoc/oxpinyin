#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Execute the live parity registry. Standard library only; never cache the oracle."""
import argparse
import contextlib
try:
    import fcntl
except ImportError:
    fcntl = None
import json
import os
from pathlib import Path
import re
import signal
import shutil
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import host as _host
from tools.dev import registry as _registry
from tools.dev import slot as _slot
from tools.dev.cell import Settings as CellSettings
from tools.bisection.contract_selection import prefix_matches

RUSTC_PEAK_MIB = _host.RUSTC_PEAK_MIB
TEST_THREAD_PEAK_MIB = _host.TEST_THREAD_PEAK_MIB
RUNNER_PEAK_MIB = _host.RUNNER_PEAK_MIB
HEADROOM_MIB = _host.HEADROOM_MIB
memory_available_mib = _host.memory_available_mib
linux_cgroup_dirs = _host.linux_cgroup_dirs
fit = _host.fit
validate_settings = _host.validate_settings
memory_defaults = _host.memory_defaults
plan_defaults = _host.plan_defaults
prepare_heavy_env = _host.prepare_heavy_env
resource_wait_limit = _host.resource_wait_limit
ResourceBlocked = _host.ResourceBlocked
host_lock_dir = _host.host_lock_dir
heavy_slot_count = _host.heavy_slot_count
pid_alive = _host.pid_alive
read_holder = _host.read_holder
describe_holder = _host.describe_holder
write_holder = _host.write_holder
process_parent = _host.process_parent
is_ancestor = _host.is_ancestor
is_heavy = _registry.is_heavy
HEAVY_SLOT = _slot.HEAVY_SLOT
HEAVY_SLOT_HELD = _slot.HEAVY_SLOT_HELD


def resource_snapshot(*args, **kwargs):
    """Read resources through tools.dev.host, the shared admission entry point.

    Late-bound so one replacement of host.resource_snapshot covers every
    admission path in this module, including the wrapper and the aggregate.
    The arguments are the host reader's synthetic test roots.
    """
    return _host.resource_snapshot(*args, **kwargs)


lock_file = _slot.lock_file
admission_lock = _slot.admission_lock
CANCELLED = _slot.CANCELLED
Cancelled = _slot.Cancelled
check_cancelled = _slot.check_cancelled
cancellation_handlers = _slot.cancellation_handlers
terminate_group = _slot.terminate_group
wait_child = _slot.wait_child
HeavySlot = _slot.HeavySlot
other_reservations = _slot.other_reservations
reserve_heavy_env = _slot.reserve_heavy_env
run_admitted = _slot.run_admitted


class HeavyAdmission(_slot.HeavyAdmission):
    """Adapter: resolve the resource reader through this module's globals."""

    def __init__(self, *args, **kwargs):
        """Default admission callbacks to late-bound readers in this module."""
        kwargs.setdefault('resource_snapshot', lambda: resource_snapshot())
        kwargs.setdefault('prepare_heavy_env',
                          lambda *a, **k: prepare_heavy_env(*a, **k))
        super().__init__(*args, **kwargs)


CELLS = ('bdb', 'kc', 'tkrzw')
# The results table. build_seconds is the cell's one build, shared by every row
# of that cell; lock_wait_seconds is the runner's wait for its exclusive
# resources (#899); slot_wait_seconds is the runner's wait for the heavy slot,
# taken before its locks; build_slot_wait_seconds is the cell build's wait for
# the heavy slot, shared by every row of that cell.
# memory_wait_seconds adds the cell build and runner memory waits.
COLUMNS = ('runner', 'cell', 'status', 'seconds', 'build_seconds', 'lock_wait_seconds',
           'slot_wait_seconds', 'build_slot_wait_seconds', 'memory_wait_seconds', 'pin_ref', 'reason')


@contextlib.contextmanager
def scratch_directory(log_dir):
    """Yield a temporary scratch path and remove it on exit.

    When log_dir is set, copy .log files there with their relative paths before
    cleanup, including on exceptional exits.
    """
    with tempfile.TemporaryDirectory(prefix='oxpinyin-gates-') as name:
        try:
            yield name
        finally:
            if log_dir:
                for log in Path(name).rglob('*.log'):
                    dest = log_dir / log.relative_to(name)
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(log, dest)


def positive(value):
    """Convert a CLI integer, rejecting zero or negative values with ArgumentTypeError."""
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError('must be positive')
    return number


def assignments(values, allowed, parser):
    """Parse allowed KEY=VALUE overrides, with the last duplicate key winning.

    Call parser.error for missing values, separators, or disallowed keys.
    """
    result = {}
    for value in values:
        key, sep, item = value.partition('=')
        if not sep or key not in allowed or not item:
            parser.error(f'invalid assignment: {value}')
        result[key] = item
    return result


def execute(command, env, cwd, timeout, output, slot=None):
    """Bound the whole process group, including grandchildren retaining stdout."""
    start = time.monotonic()
    with output.open('wb') as log:
        try:
            process = start_child(command, env, slot=slot, cwd=cwd, stdout=log,
                                  stderr=subprocess.STDOUT, start_new_session=True)
        except OSError as exc:
            return 1, time.monotonic() - start, str(exc), False
        expired = False
        try:
            wait_child(process, timeout)
        except subprocess.TimeoutExpired:
            expired = True
    text = output.read_text(errors='replace')
    return process.returncode, time.monotonic() - start, text, expired


def reason_for(rc, output, timeout, expired=False):
    # Only the aggregate deadline is a timeout here; a runner that wraps its own
    # processes in timeout(1) and exits 124 is reported by its own output.
    """Summarize an exit with its deadline or last diagnostic, capped at 400 characters.

    Only expired marks an aggregate timeout; a successful exit says completed.
    """
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
    """Normalize an issue selector exactly as the shared registry does."""
    return _registry.normal_issue(str(tag))


def contract_tag_index():
    """contract-diff.py's tag index: {'tags': {tag: [case IDs]}, 'modes': {case ID: facade}}."""
    command = [sys.executable, str(HERE / 'contract-diff.py'), '--list-tags']
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    if result.returncode:
        raise ValueError(result.stderr.strip() or 'contract-diff.py --list-tags failed')
    return json.loads(result.stdout)


def requested_case_list(value):
    """The --cases IDs in the order given, each once, as contract-diff.py's select_cases keeps them.

    A set here made the order depend on the process (hash randomization), so the same
    selection ran, and was planned, in a different order each time. None means the
    option was omitted; a value with no IDs, or an empty component, raises ValueError."""
    if value is None:
        return None
    names = [name.strip() for name in value.split(',')]
    if not all(names):
        raise ValueError('--cases must name at least one case ID and must not contain an empty value')
    return list(dict.fromkeys(names))


def contract_cases_for(requested, index):
    """Map selected case IDs to the normalized tags that selected them.

    Visit requested tags in sorted order and each tag's cases in index order;
    retain the first insertion position of a case selected by multiple tags.
    """
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


def planned_inputs(row, base_env, substitutions):
    """The input paths a runner will check, as the plan shows them: resolved
    the way run() resolves them, with the row's own env entries applied and
    `{env[NAME]}` read from that environment. A path naming a variable the
    host does not set is shown unresolved; the runner skips on it later."""
    env = dict(base_env)
    shown_substitutions = dict(substitutions, work='<work>')
    try:
        env.update({key: value.format_map(shown_substitutions) for key, value in row.get('env', {}).items()})
    except KeyError:
        pass
    shown_substitutions['env'] = env
    inputs = []
    for item in row.get('input_paths', []):
        try:
            inputs.append(item.format_map(shown_substitutions))
        except KeyError:
            inputs.append(item)
    return inputs or ['none']


def plan_lines(cell, jobs, contract_cases, reasons, index):
    """The selection plan for one cell: each runner, why it runs, the inputs
    it needs and whether it needs the oracle; for a tag-selected contract
    run, the case IDs, why each was selected and their facades."""
    lines = [f'plan ({cell}): {len(jobs)} runner{"" if len(jobs) == 1 else "s"}']
    for row, _, env, substitutions in jobs:
        inputs = planned_inputs(row, env, substitutions)
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
ARTIFACTS = ('pinyin-capi', 'zhuyin-capi', 'dictool', 'user-rt-test', 'gen-pinyin-table', 'gen-zhuyin-table')


def artifact_union(rows):
    """The artifacts the selected runners need, in ARTIFACTS order: the one
    build a cell runs. Every row declares its set; a row without one is a
    registry error, not a request for everything."""
    needed = set()
    for row in rows:
        declared = row.get('artifacts')
        if declared is None:
            raise ValueError(f"runners.json: {row['name']} declares no artifacts list")
        unknown = set(declared) - set(ARTIFACTS)
        if unknown:
            raise ValueError(f"runners.json: {row['name']} names unknown artifacts: {', '.join(sorted(unknown))}")
        needed.update(declared)
    return [artifact for artifact in ARTIFACTS if artifact in needed]
ARTIFACT_ENV = {'pinyin-capi': 'OXPINYIN_CAPI_SO', 'zhuyin-capi': 'OXPINYIN_ZHUYIN_SO',
                'dictool': 'OXPINYIN_DICTOOL', 'user-rt-test': 'OXPINYIN_USER_RT_TEST',
                'gen-pinyin-table': 'OXPINYIN_GEN_PINYIN_TABLE', 'gen-zhuyin-table': 'OXPINYIN_GEN_ZHUYIN_TABLE'}


def needed_artifacts(rows, env):
    """The artifacts the selected rows execute, name -> path, from each row's
    declared `artifacts` list: the same declarations that chose what the cell
    built, so the identity check covers exactly what the runners load or run
    (dictool for import-diff included, not only the {capi}/{zhuyin} paths)."""
    needed = {}
    for row in rows:
        for name in row.get('artifacts', []):
            needed[name] = env[ARTIFACT_ENV[name]]
    return needed


def identity_of(target_dir):
    """The shared cell builder's target-directory identity, or None."""
    path = Path(target_dir) / 'oxpinyin-cell-identity.json'
    return json.loads(path.read_text()) if path.is_file() else None


def check_identity(identity, root, cell, needed):
    """A hard error unless the target directory's identity is this worktree's,
    this cell's, the dev profile's, and every artifact the run will execute
    (`needed`: name -> path) is the file the record staged, unchanged since.
    A missing record, another owner, or an artifact that changed or vanished
    after the record is a mixed identity, never a run."""
    if identity is None:
        raise ValueError('no artifact identity recorded for this target directory: build it with '
                         'just cell-artifacts (drop --no-build) before running')
    claimed = dict(worktree=identity.get('worktree'), cell=identity.get('cell'), profile=identity.get('profile'))
    wanted = dict(worktree=str(Path(root).resolve()), cell=cell, profile='dev')
    if claimed != wanted:
        raise ValueError('target directory identity mismatch: built for %s, this run is %s' % (
            json.dumps(claimed, sort_keys=True), json.dumps(wanted, sort_keys=True)))
    recorded = identity.get('artifacts', {})
    for name, path in needed.items():
        staged = recorded.get(name)
        if staged is None:
            raise ValueError(f'artifact {name} ({path}) was not staged by the recorded build')
        if not Path(path).exists():
            raise ValueError(f'artifact {name} ({path}) is missing although the record stages it')
        stat = Path(path).stat()
        if (staged.get('path'), staged.get('size'), staged.get('mtime_ns')) != (str(path), stat.st_size, stat.st_mtime_ns):
            raise ValueError(f'artifact {name} ({path}) changed after the recorded build '
                             f'(size/mtime {stat.st_size}/{stat.st_mtime_ns} vs {staged.get("size")}/{staged.get("mtime_ns")}): '
                             'a mixed target directory; rebuild it from this worktree')
    if os.environ.get('OXPINYIN_COMPAT_CONTAINER') == '1':
        from tools.dev.compat.identity import verify_artifacts
        verify_artifacts(identity, root, needed)

def start_child(command, env, *, slot=None, **kwargs):
    """Transfer an acquired slot to the workload before it executes."""
    descriptors = ()
    if slot is not None:
        descriptors = slot.pass_fds()
        if not slot.inherited:
            command = [sys.executable, str(HERE / 'run-all.py'), '--heavy-child-fd',
                       str(descriptors[0]), '--', *command]
    check_cancelled()
    kwargs.setdefault('start_new_session', True)
    process = subprocess.Popen(command, env=env, pass_fds=descriptors, **kwargs)
    if slot is not None and not slot.inherited:
        slot.handoff()
    return process


def heavy_child(fd, command):
    """Give the inherited descriptor and holder record to the actual child."""
    os.set_inheritable(fd, True)
    marker = json.loads(os.environ[HEAVY_SLOT_HELD])
    directory = Path(marker['lock_dir'])
    with admission_lock(directory):
        with os.fdopen(os.dup(fd), 'r+') as file:
            file.seek(0)
            holder = json.loads(file.read())
            holder.update(pid=os.getpid(), command=' '.join(command))
            write_holder(file, holder)
    marker.update(pid=os.getpid(), fd=fd)
    env = dict(os.environ, **{HEAVY_SLOT_HELD: json.dumps(marker)})
    check_cancelled()
    with subprocess.Popen(command, env=env, pass_fds=(fd,)) as process:
        while True:
            try:
                rc = process.wait(timeout=0.1)
                break
            except subprocess.TimeoutExpired:
                if CANCELLED.is_set():
                    # The group already received the signal. Keep the supervisor
                    # alive so it can reap a command that ignores cancellation.
                    try:
                        rc = process.wait(timeout=2)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        rc = process.wait()
                    break
    if CANCELLED.is_set():
        return 1
    if rc < 0:
        if -rc not in (signal.SIGKILL, signal.SIGSTOP):
            signal.signal(-rc, signal.SIG_DFL)
        os.kill(os.getpid(), -rc)
    return rc


def run_in_heavy_slot(command):
    """`run-all.py --heavy-slot -- COMMAND...`: run COMMAND holding a heavy
    slot (for campaigns, Valgrind runs and builds started outside run-all).
    The wait is printed; the command's exit status is returned."""
    with HeavyAdmission(' '.join(command), os.environ) as admission:
        slot = admission.slot
        started = time.monotonic()
        with start_child(command, admission.env, slot=slot) as process:
            rc = wait_child(process)
        print(f'heavy slot {slot.slot}: slot_wait_seconds {admission.slot_wait:.1f}, memory_wait_seconds {admission.memory_wait:.1f}, ran {time.monotonic() - started:.1f}s, exit {rc}',
              file=sys.stderr, flush=True)
        return rc


def cell_config(cell, target_root, env):
    """Read a cell's explicit artifact configuration through the shared service.

    Set the cell and target root in a copied environment. Return decoded JSON
    or raise ValueError for an invalid cell or pin configuration.
    """
    configured = env.copy()
    configured['PINYIN_ORACLE_DBM'] = cell
    configured['OXPINYIN_TARGET_ROOT'] = str(target_root)
    return CellSettings.read(configured, ROOT).describe()


def oracle_config(prefix, variant, env):
    """Read an oracle variant's configuration through the shared cell service.

    Raise ValueError for an unknown variant or invalid cell configuration.
    """
    return CellSettings.read(env, ROOT).oracle(str(prefix), variant).describe()


def run_main():
    """Select registry gates, build their artifacts, and execute admitted runner jobs.

    List mode only prints metadata. Execution validates cell identities, uses
    heavy admission and checkout resource locks, and prints a result table
    with optional JSON and retained logs. Return nonzero for failed gates or
    unallowed skips; heavy-child and heavy-slot modes return their child status.
    """
    if fcntl is None:
        raise ValueError("run-all.py needs POSIX fcntl; Windows support requires #895 Stage 2 (`just heavy`)")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runners', help='comma-separated registry names (default: every gate)')
    parser.add_argument('--cases', help='comma-separated contract-diff case IDs (requires contract-diff)')
    parser.add_argument('--cases-prefix', metavar='P[,P...]',
                        help='union of contract case ID prefixes from the full registry, including untagged '
                             'cases; every prefix must match (requires contract-diff)')
    parser.add_argument('--cells', default='bdb', help='comma-separated bdb,kc,tkrzw')
    parser.add_argument('--issue', action='append', default=[],
                        help='issue number, W tag, or row:N; selects the runners and the exact '
                             'contract-diff cases registered under it')
    parser.add_argument('--list', action='store_true', help='list gate and not-a-gate metadata; do not build')
    parser.add_argument('--jobs', type=positive, default=None,
                        help='concurrent runner jobs (default: from available memory, at most 2)')
    parser.add_argument('--contract-jobs', type=positive,
                        help='maximum concurrent cases per contract-diff cell, reduced by memory admission '
                             '(default: each concurrent cell\'s share of the CPUs, at most what fits in memory; '
                             '1 runs the cases serially)')
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
    parser.add_argument('--log-dir', type=Path, help='retain local build/runner logs here instead of deleting scratch')
    if sys.argv[1:2] == ['--heavy-child-fd']:
        return heavy_child(int(sys.argv[2]), sys.argv[4:])
    parser.epilog = ('run-all.py --heavy-slot -- COMMAND...  runs COMMAND holding one host-wide heavy slot '
                     '(for campaigns, Valgrind runs and builds started outside run-all) and exits with its status.')
    if sys.argv[1:2] == ['--heavy-slot']:
        command = sys.argv[2:]
        if command[:1] == ['--']:
            command = command[1:]
        if not command:
            parser.error('--heavy-slot needs a command after --')
        return run_in_heavy_slot(command)
    args = parser.parse_args()
    if args.runners is not None and any(not name.strip() for name in args.runners.split(',')):
        parser.error('--runners must not contain an empty value')
    requested_issues = {normal_tag(item) for item in args.issue}
    if any(not issue for issue in requested_issues):
        parser.error('--issue must not contain an empty value')
    catalog = _registry.load()
    names = {row['name'] for row in catalog}
    selected = set(args.runners.split(',')) if args.runners is not None else names
    unknown = selected - names
    if unknown:
        parser.error('unknown runners: ' + ', '.join(sorted(unknown)))
    cells = list(dict.fromkeys(args.cells.split(',')))
    if not cells or set(cells) - set(CELLS):
        parser.error('--cells must select bdb,kc,tkrzw')
    # An issue tag also resolves to the exact contract cases registered under
    # it; those run through the contract-diff runner with --cases. Nothing
    # matching anywhere is an error that names what exists, never the suite.
    contract_cases, index = {}, None
    if requested_issues and 'contract-diff' in selected:
        index = contract_tag_index()
        contract_cases = contract_cases_for(requested_issues, index)
    try:
        requested_cases = requested_case_list(args.cases)
    except ValueError as error:
        parser.error(str(error))
    if requested_cases is not None:
        if 'contract-diff' not in selected:
            parser.error('--cases requires --runners contract-diff')
        index = index or contract_tag_index()
        unknown_cases = [name for name in requested_cases if name not in index['modes']]
        if unknown_cases:
            parser.error('unknown contract case(s): ' + ', '.join(sorted(unknown_cases)))
        for name in requested_cases:
            contract_cases.setdefault(name, ['--cases'])
    if args.cases_prefix is not None:
        if 'contract-diff' not in selected:
            parser.error('--cases-prefix requires --runners contract-diff')
        index = index or contract_tag_index()
        try:
            matches = prefix_matches(index['modes'], args.cases_prefix)
        except ValueError as error:
            parser.error(str(error))
        for name, selectors in matches.items():
            contract_cases.setdefault(name, []).extend(selectors)
    runner_tags = {normal_tag(tag) for row in catalog for tag in row.get('issues', [])}
    known_issues = runner_tags | {normal_tag(tag) for tag in (index or {}).get('tags', {})}
    unknown_issues = requested_issues - known_issues
    if unknown_issues:
        parser.error('unknown issue(s): ' + ', '.join(sorted(unknown_issues)))
    # The registry selects runner rows by name and runner issue tag. Its cell
    # filter drops rows, while run-all keeps every selected row and reports an
    # unsupported cell as SKIPPED; cell selection stays below. Contract issue
    # tags are not runners.json rows and are resolved through contract-diff.
    runner_requests = requested_issues & runner_tags
    if requested_issues and not runner_requests:
        rows = []
    else:
        try:
            rows = _registry.select(catalog,
                                    names=None if args.runners is None else selected,
                                    issues=sorted(runner_requests) if requested_issues else None)
        except _registry.NoMatch:
            rows = []
    reasons = {row['name']: ('--issue ' + ', '.join(sorted(requested_issues.intersection(
        normal_tag(tag) for tag in row.get('issues', [])))) if requested_issues else
        '--runners' if args.runners is not None else 'default gate set') for row in rows}
    if contract_cases and all(row['name'] != 'contract-diff' for row in rows):
        rows.append(_registry.by_name(catalog)['contract-diff'])
        reasons['contract-diff'] = '--issue %s: %d contract case%s' % (
            ', '.join(sorted({tag for tags in contract_cases.values() for tag in tags})),
            len(contract_cases), '' if len(contract_cases) == 1 else 's')
    elif contract_cases:
        reasons['contract-diff'] += '; %d contract case%s' % (
            len(contract_cases), '' if len(contract_cases) == 1 else 's')
    if not rows:
        contract_tags = sorted(tag for tag in (index or contract_tag_index())['tags']
                               if not tag.startswith('family/'))
        parser.error('no runner or contract case carries issue tag(s) %s; runner tags: %s; '
                     'contract tags: %s' % (', '.join(sorted(requested_issues)),
                                            ', '.join(sorted(runner_tags)), ', '.join(contract_tags)))
    if contract_cases:
        rows = [dict(row, command=row['command'] + ['--', '--cases', ','.join(contract_cases)])
                if row['name'] == 'contract-diff' else row for row in rows]
    if args.list:
        for row in rows:
            print(json.dumps(row, ensure_ascii=False))
        return 0
    measurements = [row['name'] for row in rows if row['kind'] != 'gate']
    if args.runners is not None and measurements:
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
    resources = resource_snapshot()
    print('resources (provisional): ' + '; '.join(resources['notes']), file=sys.stderr, flush=True)
    defaults = memory_defaults(base_env, args.jobs, cpus=resources['cpus'],
                               available_mib=resources['available_mib'],
                               memory_limit_mib=resources.get('memory_limit_mib'),
                               total_mib=resources.get('total_mib'))
    plan_defaults(defaults, phase='provisional defaults')
    # A worker can run light work even when no heavy process fits.
    args.jobs = max(1, defaults['--jobs'][0])
    if len(cells) > 1 and base_env.get('CARGO_TARGET_DIR'):
        parser.error('CARGO_TARGET_DIR is a single-cell override; use --target-root for multiple cells')
    targets = args.target_root.resolve()
    results, jobs = [], []
    build_seconds = {}
    # Locks live beside the checkout rather than in per-cell or per-invocation
    # directories. This protects fixed native outputs across concurrent invocations.
    lock_root = ROOT / 'target/runner-locks'
    lock_root.mkdir(parents=True, exist_ok=True)
    if args.log_dir:
        if args.log_dir.exists() and (not args.log_dir.is_dir() or any(args.log_dir.iterdir())):
            parser.error('--log-dir must be a new or empty directory')
        args.log_dir.mkdir(parents=True, exist_ok=True)
    with scratch_directory(args.log_dir) as scratch_name:
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
            for utility in ('gen_pinyin_table', 'gen_zhuyin_table'):
                env['OXPINYIN_' + utility.upper()] = str(Path(cfg['CARGO_TARGET_DIR']) / 'debug' / utility)
            env.update(extra_env[cell])
            env['OXPINYIN_USER_RT_TEST'] = str(Path(cfg['CARGO_TARGET_DIR']) / 'debug/user-dir-round-trip-test')
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
                              slot_wait_seconds=0.0, build_slot_wait_seconds=0.0, memory_wait_seconds=0.0,
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
            # Build the union of what the selected runners declare, once per
            # cell; a selection that needs no subject (pc-version) builds nothing.
            artifacts = artifact_union(row for row, _, _ in build_candidates)
            if not artifacts:
                print(f'No subject artifacts needed: {cell} (no build)', file=sys.stderr, flush=True)
            elif not args.no_build:
                print(f'Building dev opt-level 1 artifacts: {cell}: {", ".join(artifacts)}',
                      file=sys.stderr, flush=True)
                # The build is heavy work: it holds a host-wide slot while cargo runs.
                admission = HeavyAdmission(f'cell build {cell} in {ROOT}', env)
                try:
                    with admission:
                        rc, seconds, output, expired = execute(
                            [sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev',
                             'cell-artifacts', '--artifacts', ','.join(artifacts)],
                            admission.env, ROOT, args.build_timeout, scratch / f'build-{cell}.log', slot=admission.slot)
                except ResourceBlocked as exc:
                    rc, seconds, output, expired = 1, 0.0, str(exc), False
                build_seconds[cell] = round(seconds, 3)
                for _, result, _ in build_candidates:
                    admission.record(result)
                    result['build_slot_wait_seconds'] = result['slot_wait_seconds']
                    result['slot_wait_seconds'] = 0.0
                if rc or expired:
                    for _, result, _ in build_candidates:
                        # An admission refusal is already a whole reason; only a build failure takes the prefix.
                        result.update(status='FAIL', seconds=round(seconds, 3), build_seconds=build_seconds[cell],
                                      reason=output if output.startswith(('resource-blocked:', 'slot-wait-expired:')) else
                                      'cell build: ' + reason_for(rc, output, args.build_timeout, expired))
                        results.append(result)
                    continue
            # Execute only what this worktree staged into this cell's target.
            needed = needed_artifacts((row for row, _, _ in build_candidates), env)
            # A selection that executes nothing from the cell (pc-version) built
            # nothing above and needs no record; everything else must match it.
            if needed:
                identity = identity_of(cfg['CARGO_TARGET_DIR'])
                check_identity(identity, ROOT, cell, needed)
                head = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, capture_output=True, text=True).stdout.strip()
                source = identity.get('source', {})
                if source.get('head') != head:
                    print(f"Note: {cell} artifacts were built from {source.get('head', '?')[:12]} at "
                          f"{identity.get('built_utc', '?')}; this worktree is at {head[:12]}", file=sys.stderr, flush=True)
            jobs.extend(cell_jobs)

        # Contract-diff runs that can be live together share the CPUs; with no
        # OXPINYIN_CONTRACT_JOBS, admission gives each its share as the case
        # maximum (#1240). --contract-jobs 1 keeps a run serial.
        contract_rows = min(args.jobs, sum(row['name'] == 'contract-diff' for row, *_ in jobs))
        prepared = []
        for row, result, base, substitutions in jobs:
            timeout = timeouts.get(row['name'], args.timeout or row['timeout'])
            work = scratch / (row['name'] + '-' + result['cell'])
            work.mkdir()
            substitutions = dict(substitutions, work=str(work))
            env = base.copy()
            env.update({key: value.format_map(substitutions) for key, value in row.get('env', {}).items()})
            if row['name'] == 'contract-diff' and args.contract_jobs is not None:
                env['OXPINYIN_CONTRACT_JOBS'] = str(args.contract_jobs)
            env['TMPDIR'] = str(work)
            substitutions['env'] = env
            absent = [name for name in row.get('required_env', []) if not env.get(name)]
            if absent:
                result.update(status='SKIPPED', reason='missing required environment: ' + ', '.join(absent))
                results.append(result)
                continue
            command = [part.format_map(substitutions) for part in row['command']]
            missing = [item.format_map(substitutions) for item in row.get('input_paths', [])
                       if not Path(item.format_map(substitutions)).exists()]
            if missing:
                result.update(status='FAIL' if row.get('oracle_variant', 'unpatched') != 'unpatched'
                              else 'SKIPPED', reason='missing required input: ' + ', '.join(missing))
                results.append(result)
                continue
            admission = HeavyAdmission(f"{row['name']} / {result['cell']} in {ROOT}", env, runner=True,
                                       contract_rows=contract_rows if row['name'] == 'contract-diff' else 0
                                       ) if is_heavy(row) else None
            prepared.append(dict(row=row, result=result, env=env, command=command,
                                 timeout=timeout, work=work, admission=admission))

        def run(job):
            """Run one admitted job under its sorted checkout resource locks.

            Update and return its result with timing, status, and failure reason.
            Treat required SKIP output on an otherwise successful exit as skipped.
            """
            row, result = job['row'], job['result']
            admission = job['admission']
            env = admission.env if admission else job['env']
            timeout = job['timeout']
            with contextlib.ExitStack() as stack:
                # Admission holds the slot before these sorted checkout locks.
                # Light holders never wait for a slot. Heavy holders have one.
                # These waits cannot form a cycle.
                waiting = time.monotonic()
                for resource in sorted(row['exclusive_resources']):
                    lock = stack.enter_context((lock_root / resource).open('a'))
                    while True:
                        check_cancelled()
                        try:
                            lock_file(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                            break
                        except BlockingIOError:
                            time.sleep(0.05)
                lock_wait = time.monotonic() - waiting
                rc, seconds, output, expired = execute(job['command'], env, ROOT, timeout, job['work'] / 'runner.log',
                                                        slot=admission.slot if admission else None)
            # Legacy CI behavior is preserved in the direct live-typing runner;
            # aggregate skips cannot silently turn into passes.
            required_skips = [line for line in output.splitlines() if 'SKIP:' in line and
                              not any(token in line for token in row.get('optional_skips', []))]
            if rc == 0 and required_skips and not expired:
                rc, output = 77, '\n'.join(required_skips)
            result.update(status='FAIL' if expired else 'PASS' if rc == 0 else 'SKIPPED' if rc == 77 else 'FAIL',
                          seconds=round(seconds, 3), build_seconds=build_seconds.get(result['cell'], 0.0),
                          lock_wait_seconds=round(lock_wait, 3),
                          phases=timing_of(output),
                          reason=reason_for(rc, output, timeout, expired))
            print(f"Completed {result['runner']} / {result['cell']}: {result['status']}",
                  file=sys.stderr, flush=True)
            return result

        results.extend(run_admitted(prepared, args.jobs, run))
    order = {row['name']: index for index, row in enumerate(rows)}
    results.sort(key=lambda row: (order[row['runner']], cells.index(row['cell'])))
    print(' | '.join(COLUMNS))
    print(' | '.join('---' for _ in COLUMNS))
    for row in results:
        print(' | '.join(str(row[key]).replace('|', '/').replace('\n', ' ') for key in COLUMNS))
    if args.json:
        args.json.write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n')
    def allowed(row):
        """Accept a skip covered by a wildcard, runner, or runner:cell allowance."""
        return any(item in args.allow_skipped for item in
                   ('*', row['runner'], row['runner'] + ':' + row['cell']))
    return int(any(row['status'] == 'FAIL' or row['status'] == 'SKIPPED' and not allowed(row)
                   for row in results))


def main():
    """Install cancellation handlers for the aggregate run and return its status."""
    with cancellation_handlers():
        return run_main()


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr)
        sys.exit(1)
