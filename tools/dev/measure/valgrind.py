"""Valgrind-based recipes behind `measure` (#1290): the callgrind differential and the system-import profiles.

callgrind-differential answers one question: does oxpinyin execute more
instructions than libpinyin on the keystroke path, or the same instructions more
slowly? One harness binary runs under callgrind against each library in turn;
because bisect.c drives both through the same C ABI the totals are comparable,
and because the collection anchors live in the harness the toggled region is the
same. Nothing here interprets the numbers; the decision rule lives in the
findings document.

system-import-profile prepares what system_import_profile.py needs (the arms'
function-order guard, a fresh work directory and the compiled driver) and hands
it the collections, which already run as units of one admitted pool.
"""
import contextlib
import io
import os
from pathlib import Path
import re
import sys
from collections.abc import Mapping, Sequence

from .. import pool, slot
from ..runners import Finished, emit, write
from . import bench, identity, reports
from .bench import Arm, Bench, fail, setting

CALLGRIND = 'run-callgrind-differential.sh'
SYSTEM_IMPORT = 'run-system-import-profile.sh'
LP_ROOT = Path('/opt/libpinyin-tkrzw')
DATA = LP_ROOT / 'lib/libpinyin/data'
ANCHOR = 'bisect_perf_steady_unit'
# Where the image installs the four libraries the differential compares.
ROOTS = dict(lp=LP_ROOT, ox=Path('/opt/oxpinyin-tkrzw/stage'), nodebug=Path('/opt/oxpinyin-nodebug/stage'),
             alloc=Path('/opt/oxpinyin-alloccount/stage'))
# The tool options of every callgrind run; the output file is added per run.
CALLGRIND_OPTIONS = ('valgrind', '--tool=callgrind', '--collect-atstart=no', f'--toggle-collect={ANCHOR}',
                     '--cache-sim=yes', '--branch-sim=yes', '--separate-threads=no')


def lines_of(path: Path, pattern: str, *, ignore_case: bool = False) -> list[str]:
    """The lines of PATH matching the extended regex PATTERN, as `grep -E` selects them.

    A file that cannot be read prints grep's complaint on stderr and selects nothing.
    """
    try:
        text = path.read_text(errors='replace')
    except OSError as error:
        emit(f'grep: {path}: {error.strerror}', error=True)
        return []
    expression = re.compile(pattern, re.IGNORECASE if ignore_case else 0)
    return [line for line in text.split('\n') if line and expression.search(line)]


def libraries() -> dict:
    """The five artifacts the differential needs, checked to exist before anything runs."""
    found = {}
    for key, root in ROOTS.items():
        # `find` on a root that is not there says so and ends the run; on one without the file it prints nothing.
        if not root.is_dir():
            fail(f"find: '{root}': No such file or directory")
        found[key] = bench.find_named(root, 'libpinyin.so')
    for artifact in found.values():
        if artifact is None or not artifact.exists():
            fail(f"fatal: missing artifact: '{'' if artifact is None else artifact}'")
    if not DATA.is_dir():
        fail(f'fatal: data dir {DATA} missing')
    return found


def environment_table(artifacts: Sequence[Path], run=None) -> str:
    """Stage 0: what the numbers depend on, written before any is taken."""
    lines = ['=== environment ===', f'captured_utc: {bench.utc_now()}', f'container_arch: {identity.uname("m")}',
             f'kernel: {identity.uname("srvm")}', f'nproc: {identity.processors()}', f'cpu: {identity.cpu_model()}',
             f'memtotal: {identity.memory_total()}',
             f'gcc: {identity.first_output_line(["gcc", "--version"], run=run)}',
             f'rustc: {identity.first_output_line(["rustc", "--version"], run=run)}',
             f'cargo: {identity.first_output_line(["cargo", "--version"], run=run)}',
             f'valgrind: {identity.first_output_line(["valgrind", "--version"], run=run)}',
             f'perf_event_paranoid: {identity.perf_event_paranoid()}',
             f'libpinyin_pkgversion: {identity.pc_version(LP_ROOT / "lib/pkgconfig/libpinyin.pc")}',
             f'libtkrzw: {identity.package_version("tkrzw", run=run)}']
    # `stat -c%s` on the path as found: for a symlink, the size of the link and not of its target.
    lines += [f'artifact: {identity.sha256(path)}  {path} {os.lstat(path).st_size} bytes' for path in artifacts]
    lines.append(f'data_dir: {DATA}')
    return '\n'.join(lines) + '\n'


def anchors_table(harness: Path, run=None) -> tuple[int, str]:
    """Stage 1: the two collection anchors are distinct symbols, or the toggle would collect the cold cycle.

    Return the table and the status the group ended with.
    """
    lines = ['=== collection anchors ===']
    done = (run or bench.capture)(['nm', harness])
    write(done.stderr, error=True)
    symbols = [line.split() for line in done.stdout.decode(errors='replace').split('\n')]
    shown = [line for line in done.stdout.decode(errors='replace').split('\n')
             if re.search('bisect_perf_(cold|steady)_unit', line)]
    if not shown:
        return 1, '\n'.join(lines) + '\n'
    lines += shown
    address = {name: row[0] for row in symbols if len(row) >= 3
               for name in ('bisect_perf_cold_unit', ANCHOR) if row[2] == name}
    if address.get('bisect_perf_cold_unit') == address.get(ANCHOR):
        lines.append('FAIL: anchors folded to one address; the toggle would collect the cold cycle')
        return 1, '\n'.join(lines) + '\n'
    lines += [f'PASS: distinct addresses, so --toggle-collect={ANCHOR}',
              '      excludes process init and the cold cycle']
    return 0, '\n'.join(lines) + '\n'


def malloc_interception(worker: Bench, arm: Arm, out: Path, cycles: str = '2') -> str:
    """Stage 3: is valgrind's malloc replacement in effect? Run one probe and read what it credited.

    vg_replace_malloc.c is linked into every valgrind tool, so client malloc and
    free are redirected and the Ir attributed to allocation is valgrind's
    replacement allocator, not glibc's. Establish that before any Ir number is read.
    The probe's own failure does not stop the capture; its diagnostics land in its files.
    """
    profile = out / 'callgrind.redir-probe.out'
    prefix = ['valgrind', '--tool=callgrind', '--collect-atstart=no', f'--toggle-collect={ANCHOR}',
              '--trace-redir=yes', f'--callgrind-out-file={profile}']
    diagnostics = io.StringIO()
    done = None
    # A refusal inside the probe went to its stderr file and the run went on, and so did the shell's
    # "command not found" for a host without taskset.
    with contextlib.redirect_stderr(diagnostics):
        with contextlib.suppress(Finished):
            try:
                done = worker.launch(Arm('probe', arm.so, arm.data, arm.libdir), 'speed', cycles, repeats=1,
                                     prefix=prefix)
            except FileNotFoundError as error:
                emit(f'{error.filename}: command not found', error=True)
    (out / 'redir-probe.stdout').write_bytes(b'' if done is None else done.stdout)
    stderr = out / 'redir-probe.stderr'
    stderr.write_bytes(diagnostics.getvalue().encode() if done is None else done.stderr)
    lines = ['=== valgrind malloc interception ===',
             '--- redirections naming malloc/free (from --trace-redir) ---']
    lines += lines_of(stderr, 'REDIR|vgpreload|malloc|free')[:40]
    lines.append('--- objects credited in the profile ---')
    credited = lines_of(profile, '^(ob|fl|fn)=')
    lines += sorted({line for line in credited if re.search('vgpreload|malloc', line, re.IGNORECASE)})[:20]
    lines += ['', 'Reading: a malloc entry credited to a vgpreload object is proof the',
              'replacement is active, so Ir attributed to allocation understates the',
              'real glibc cost. Call counts are unaffected.']
    return '\n'.join(lines) + '\n'


def annotate(out: Path) -> None:
    """Annotate each callgrind profile twice, self and inclusive; a failing annotate is not a failed run.

    The annotations are independent post-processing of profiles already on disk, not
    measurements, so they run as units of an admitted pool (pool.run_admitted) and not
    one after another. Admission takes the heavy slot (or inherits the caller's) and
    reserves memory for every worker, so a host short of memory annotates one profile
    at a time and a direct run, such as the image's command, never starts more than it
    holds. Each unit is charged the runner allowance (host.RUNNER_PEAK_MIB): the old
    script ran the annotations one at a time and Valgrind documents no figure, and
    callgrind_annotate holds a whole profile in memory.
    """
    def unit(profile: Path, suffix: str, extra: list):
        def annotate_one(context):
            command = ['callgrind_annotate', '--threshold=99.5', *extra, str(profile)]
            try:
                body = bench.capture(command, merge_stderr=True).stdout
            except OSError as error:
                body = f'callgrind_annotate: {error.strerror}\n'.encode()
            Path(f'{profile}.{suffix}.txt').write_bytes(body)
        return f'{profile.name}.{suffix}', annotate_one
    profiles = [profile for profile in sorted(out.glob('callgrind*.out'), key=lambda path: os.fsencode(path.name))
                if 'redir-probe' not in str(profile)]
    units = [unit(profile, suffix, extra) for profile in profiles
             for suffix, extra in (('annotated', []), ('inclusive', ['--inclusive=yes']))]
    if units:
        pool.run_admitted(units, f'{CALLGRIND} callgrind_annotate', cancelled=slot.CANCELLED.is_set,
                          cancel_error=slot.Cancelled, out=sys.stderr)


def callgrind_differential(argv: Sequence[str], env: Mapping[str, str]) -> int:
    """Instruction-level differential for the steady keystroke cycle: stages 0 through 7."""
    label = CALLGRIND
    env = dict(env)
    out = Path(setting(env, 'CG_OUT', '/out'))
    cpu = setting(env, 'PERF_CPU', '0')
    # Stage 4 and 5 capture size. Callgrind is deterministic, so this is chosen for a workable runtime
    # under a ~50x interpreter, not for noise.
    cg_cycles, cg_repeats = setting(env, 'CG_CYCLES', '4'), setting(env, 'CG_REPEATS', '1')
    # Stage 7 uses the cross-host record's own protocol, deliberately NOT the callgrind pass's cycle
    # count: the timing ratio has to be comparable to every previous steady-cycle record.
    t_cycles, t_repeats = setting(env, 'T_CYCLES', '8'), setting(env, 'T_REPEATS', '1')
    cg_rounds = bench.count(label, env, 'CG_ROUNDS', '2')
    t_runs = bench.count(label, env, 'T_RUNS', '20')
    hw_runs = bench.count(label, env, 'HW_RUNS', '10')
    out.mkdir(parents=True, exist_ok=True)

    found = libraries()
    lp, ox, nodebug, alloc = (found[key] for key in ('lp', 'ox', 'nodebug', 'alloc'))
    harness = bench.BISECT / 'bisect'
    worker = Bench(label, env, harness, cpu, cell='tkrzw', required=True)
    arms = {'lp': Arm('libpinyin-tkrzw', lp, DATA, lp.parent), 'ox': Arm('oxpinyin-tkrzw', ox, DATA, ox.parent),
            'nodebug': Arm('oxpinyin-nodebug', nodebug, DATA, nodebug.parent),
            'alloc': Arm('oxpinyin-alloccount', alloc, DATA, alloc.parent)}
    for key, arm in arms.items():
        worker.standalone('pin' if key == 'lp' else 'subject', arm)
    pair = [(arms['lp'], 'speed'), (arms['ox'], 'speed')]

    # 0. environment
    bench.tee(out / 'environment.txt', environment_table([lp, ox, nodebug, alloc]))

    # 1. collection anchors
    code, text = anchors_table(harness)
    bench.tee(out / 'anchors.txt', text)
    if code:
        raise Finished(code)

    # 2. debug-info neutrality: the profiled artifact carries debug info and the timed one does not, so
    # the two must be the same code. Three outcomes, and only the last is fatal: identical .text; the
    # same instruction multiset at other addresses (Ir is unaffected, the simulated cache and branch
    # figures are layout-sensitive); a different multiset, which is different code.
    from . import neutrality
    code, report = neutrality.compare(ox, nodebug)
    bench.tee(out / 'debuginfo-neutrality.txt', '=== debug-info neutrality ===\n' + report)
    if code:
        raise Finished(code)

    # 3. valgrind's malloc interception
    bench.tee(out / 'malloc-interception.txt', malloc_interception(worker, arms['ox'], out))

    # 4. the differential: both cells, two rounds each, allocator intercepted as in every valgrind run.
    # Callgrind is deterministic; the second round exists to demonstrate that (a divergence over 0.1%
    # between rounds is reported, not averaged). Serial: every run pins to the one PERF_CPU the caller
    # chose, so concurrent runs would only share it.
    for number in range(1, cg_rounds + 1):
        for arm, mode in pair:
            emit(f'--- callgrind: {arm.label} round {number} ---')
            profile = out / f'callgrind.{arm.label}.r{number}.out'
            stem = out / f'cg.{arm.label}.r{number}'
            Path(f'{stem}.stdout').write_bytes(b'')
            Path(f'{stem}.stderr').write_bytes(b'')
            done = worker.launch(arm, mode, cg_cycles, repeats=cg_repeats, diagnostics=Path(f'{stem}.stderr'),
                                 prefix=[*CALLGRIND_OPTIONS, f'--callgrind-out-file={profile}'])
            Path(f'{stem}.stdout').write_bytes(done.stdout)
            Path(f'{stem}.stderr').write_bytes(done.stderr)
            if done.status:
                raise Finished(done.status)
    annotate(out)

    # 5. hardware instruction counters: the interception distortion. Callgrind's Ir counts allocation
    # against valgrind's simplified allocator, an understatement whose size differs per engine and so
    # biases the Ir ratio. bisect.c brackets the same steady cycles with a PERF_COUNT_HW_INSTRUCTIONS
    # counter (exclude_kernel) in native processes, real glibc malloc included; the ratio of the two
    # engines' medians is the hardware counterpart of the Ir ratio, and the difference is the measured
    # distortion. Needs a container whose SELinux label and seccomp profile permit perf_event_open.
    hardware = out / 'hw-instructions.jsonl'
    hardware.write_bytes(b'')
    bench.round_robin(hw_runs, pair, lambda arm, mode: worker.sample(
        arm, mode, hardware, out / 'hw.err', t_cycles, repeats=t_repeats, extra={'PERF_HW': 'instructions'}))
    bench.tee(out / 'hw-instructions.md', reports.hardware_instructions(hardware))

    # 6. allocations per cycle. Native, not under valgrind: the counter reports what the client asked
    # for, which is exactly what the intercepted profile cannot.
    done = worker.launch(arms['alloc'], 'speed', t_cycles, repeats=t_repeats)
    write(done.stderr, error=True)
    lines = ['=== allocations per steady cycle (oxpinyin, alloc-count build) ===']
    if done.stdout:
        lines.append(done.stdout.decode(errors='replace').rstrip('\n'))
    if done.status == 0:
        lines += ['', 'steady_alloc_count / steady_alloc_cycles = allocating calls per cycle.',
                  'libpinyin has no counterpart symbol; its comparable figure is the',
                  'malloc/operator-new CALL COUNT in its stage 4 profile, which is',
                  'produced symmetrically for both cells by one instrument.']
    bench.tee(out / 'alloc-count.txt', '\n'.join(lines) + '\n')
    if done.status:
        raise Finished(done.status)

    # 7. wall-clock timing, same host, same session. The record's protocol, so this ratio is comparable
    # to every previous steady-cycle capture: 8 cycles, 20 runs per cell, cells interleaved round-robin,
    # pinned. Serial: a timed sample is only the arm's if nothing runs beside it.
    speed = out / 'speed.jsonl'
    speed.write_bytes(b'')
    emit(f'--- timing: {t_runs} alternating runs x {t_cycles} cycles x {t_repeats} passes ---')
    bench.round_robin(t_runs, pair, lambda arm, mode: worker.sample(
        arm, mode, speed, out / 'speed.err', t_cycles, repeats=t_repeats))
    code, text = bench.report('tools/bisection/perf-ci.py',
                              [str(speed), '--metric', 'steady', '--metric', 'cold', '--format', 'md'])
    bench.tee(out / 'timing.md', text)
    if code:
        raise Finished(code)
    emit()
    emit(f'=== captures written to {out} ===')
    for name in sorted(os.listdir(out)):
        emit(name)
    return 0


def system_import_profile(argv: Sequence[str], env: Mapping[str, str]) -> int:
    """Instruction counts, peak heap and mapped pages for import/save or cold open, pin and subject.

    Usage: PIN-SO OX-SO SYSTEM-DATA-DIR NEW-WORK-DIR [ITERATIONS|--cold-open]
    [--function-order ARM/FACADE=POLICY] [--unordered-arm ARM] [--jobs N]. The
    collections (pin and ox, each under Callgrind, Massif heap and Massif pages) are
    independent, so system_import_profile.py runs them in an admitted pool and reports
    in a fixed order; --jobs 1 runs them one after another.
    """
    label = SYSTEM_IMPORT
    arguments = list(argv)
    for number, name in enumerate(('pin-so', 'ox-so', 'system-data-dir', 'new-output-dir'), 1):
        if len(arguments) < number or not arguments[number - 1]:
            fail(f'{label}: {number}: {name}')
    pin, ox, data = (os.path.realpath(value) for value in arguments[:3])
    out, rest = arguments[3], arguments[4:]
    iterations = '1000'
    if rest and rest[0] not in ('--function-order', '--unordered-arm', '--jobs'):
        iterations, rest = rest[0], rest[1:]
    jobs, leftover = [], []
    while rest:
        if rest[0] == '--jobs':
            if len(rest) < 2 or not rest[1]:
                fail('--jobs needs a count', 2)
            jobs = ['--jobs', rest[1]]
            rest = rest[2:]
        else:
            leftover.append(rest.pop(0))
    options = bench.function_order_arguments(leftover)
    bench.check_function_order([('ox/pinyin', ox), ('pin/pinyin', pin)], options, references=['pin'])
    try:
        os.mkdir(out)
    except OSError as error:
        fail(f"mkdir: cannot create directory '{out}': {error.strerror}")
    out = os.path.realpath(out)
    bench.run(['cc', *bench.STRICT_FLAGS, bench.BISECT / 'system-import-profile.c', '-ldl', '-o',
               Path(out) / 'driver'])
    relative = 'tools/bisection/system_import_profile.py'
    module = bench.script(relative)
    try:
        with bench.as_script(bench.ROOT / relative):
            return module.main([*jobs, '--', pin, ox, data, out, iterations]) or 0
    except SystemExit as stop:
        # The collections' own cancellation message, then the status a signalled run has: the shell
        # script died of the signal that stopped its children.
        if not (slot.CANCELLED.is_set() and isinstance(stop.code, str)):
            raise
        fail(stop.code, slot.cancellation_status())


RECIPES = {'callgrind-differential': callgrind_differential, 'system-import-profile': system_import_profile}
