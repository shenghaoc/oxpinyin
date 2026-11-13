"""Execution checks shared by the Valgrind recipes."""
import os
import subprocess
import sys

from .. import runners


def require_performance_environment(env):
    """Only pin comparisons require the local oracle performance stage."""
    from pathlib import Path
    from ..runners import Finished
    marker = Path('/usr/local/share/oxpinyin/performance-os-release')
    release = Path('/etc/os-release')
    if (env.get('GITHUB_ACTIONS') == 'true' or not marker.is_file() or not release.is_file()
            or marker.read_bytes() != release.read_bytes()):
        runners.emit('pin comparisons require the local oracle performance stage; '
                     'enter it with just performance', error=True)
        raise Finished(2)


def run(operation, argv=None, env=None, *, pin_comparison=False):
    """Run the operation under the shared cancellation and failure handlers."""
    environment = dict(os.environ if env is None else env)

    def execute():
        if pin_comparison:
            require_performance_environment(environment)
        return operation(list(sys.argv[1:] if argv is None else argv), environment)

    status = runners.guarded(execute)
    sys.stdout.flush()
    return status


def schedule(pairs, controls):
    """Balanced paired blocks, with control blocks interleaved in the same session."""
    if pairs < 2 or controls < 2 or pairs % 2 or controls % 2:
        raise ValueError('pairs and control pairs must be positive even counts of at least two')
    for block in range(max(pairs, controls)):
        for kind, count in (('comparison', pairs), ('control', controls)):
            if block < count:
                for arm in ('AB' if block % 2 == 0 else 'BA'):
                    yield kind, block, arm


def native_arguments(argv):
    import argparse
    from pathlib import Path
    parser = argparse.ArgumentParser(prog='just measure')
    parser.add_argument('mode', choices=('compare', 'profile', 'neutrality'))
    parser.add_argument('--base')
    parser.add_argument('--head')
    parser.add_argument('--revision', default='origin/main')
    parser.add_argument('--template', choices=(*XCTRACE_TEMPLATES, *VALGRIND_TEMPLATES))
    parser.add_argument('--instrument', choices=('VM Tracker',))
    parser.add_argument('--cycles', type=int, default=20)
    parser.add_argument('--pairs', type=int, default=6)
    parser.add_argument('--control-pairs', type=int, default=6)
    parser.add_argument('--budget', type=float, default=180)
    parser.add_argument('--data', type=Path)
    args = parser.parse_args(argv)
    if args.mode == 'compare' and (not args.base or not args.head):
        parser.error('compare requires --base and --head')
    if args.mode == 'profile' and not args.template and sys.platform != 'win32':
        parser.error('profile requires --template')
    if not 1 <= args.cycles <= 100000 or not 0 < args.budget < float('inf'):
        parser.error('cycles must be 1..100000 and budget finite and positive')
    try:
        list(schedule(args.pairs, args.control_pairs))
    except ValueError as error:
        parser.error(str(error))
    return args


CONTROL = "A'"
# Staged prefix names have one length. A longer path for the same bytes changes
# heap sizes and instruction counts, which an exact same-bytes control would see.
PREFIXES = {'A': 'A', 'B': 'B', CONTROL: 'C'}
XCTRACE_TEMPLATES = ('CPU Profiler', 'Allocations', 'System Trace', 'Processor Trace')
VALGRIND_TEMPLATES = ('Callgrind', 'Massif')


def stage_arm(prefix, built_arm, driver, digest, *, windows=False):
    """Copy one built product and the driver into a private prefix.

    Windows resolves the implicitly linked DLL beside the driver and has no dSYM.
    Linux has no dSYM either: its driver opens the staged library by path.
    """
    import shutil
    from . import identity
    (prefix / 'bin').mkdir(parents=True)
    (prefix / 'lib').mkdir()
    dylib = prefix / ('bin' if windows else 'lib') / built_arm['dylib'].name
    shutil.copy2(built_arm['dylib'], dylib)
    if not windows and not built_arm.get('linux'):
        shutil.copytree(str(built_arm['dylib']) + '.dSYM', str(dylib) + '.dSYM')
    executable = prefix / 'bin' / ('bisect.exe' if windows else 'bisect')
    shutil.copy2(driver, executable)
    if identity.sha256(executable) != digest:
        raise ValueError('staged driver bytes differ')
    if identity.sha256(dylib) != identity.sha256(built_arm['dylib']):
        raise ValueError('staged product bytes differ')
    return {**built_arm, 'dylib': dylib, 'driver': executable}


def native_sample(arm, data, cycles, env, timeout):
    import json
    from . import bench, reports
    command = [arm['driver'], '--measure', arm['dylib'], arm['uuid'], data]
    if arm.get('windows'):
        from . import identity
        identity.verify_modules([item['path'] for item in arm['modules']], arm['modules'])
    raw, usage = bench.measured_process(command, env={**env, 'PERF_CYCLES': str(cycles)}, timeout=timeout, usage=not arm.get('windows'))
    row = json.loads(raw)
    if not arm.get('windows'):
        row['peak_rss_bytes'] = usage.ru_maxrss  # Darwin wait4 reports bytes, not Linux KiB.
    if arm.get('windows'):
        from . import identity
        identity.verify_modules(row['modules'], arm['modules'])
        if not identity.same_file(row['library'], arm['dylib']):
            raise ValueError('wrong loaded library')
    return reports.observation(json.dumps(row), cycles=cycles, operations=cycles * workload_operations(),
                               windows=arm.get('windows', False))


def workload_operations():
    """Read the existing frozen corpus, rather than maintaining another work-count constant."""
    import re
    from . import bench
    source = bench.HARNESS_SOURCE.read_text()
    corpus = source.split('static const char *PERF_CYCLE_INPUTS[] = {', 1)[1].split('};', 1)[0]
    return sum(len(value) for value in re.findall(r'"([a-z\']+)"', corpus))


def native_compare(args, arms, data, env, output):
    import json
    import time
    from . import reports
    rows, fingerprint = [], None
    windows = getattr(args, 'windows', False)
    deadline = time.monotonic() + args.budget
    # Append each valid run before launching the next. No outlier filtering or retries.
    with (output / 'runs.jsonl').open('w') as stream:
        for kind, block, label in schedule(args.pairs, args.control_pairs):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ValueError('measurement budget exhausted; campaign incomplete')
            # Both control labels are arm A, each from its own staged prefix.
            arm = arms[CONTROL if kind == 'control' and label == 'B' else label]
            try:
                row = native_sample(arm, data, args.cycles, env, remaining)
            except subprocess.TimeoutExpired:
                raise ValueError('measurement budget exhausted; campaign incomplete') from None
            row = reports.observation(json.dumps(row), cycles=args.cycles,
                                      operations=args.cycles * workload_operations(), fingerprint=fingerprint, windows=windows)
            fingerprint = row['fingerprint']
            row.update(kind=kind, block=block, arm=label)
            rows.append(row)
            stream.write(json.dumps(row) + '\n')
            stream.flush()
    result = reports.comparison(rows, windows=windows)
    (output / 'summary.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))


def native_profile(args, arm, data, env, output):
    from . import bench
    trace = output / 'capture.trace'
    stdout = output / 'driver.json'
    command = ['xcrun', 'xctrace', 'record', '--template', args.template,
               '--output', trace, '--time-limit', str(args.budget) + 's',
               '--target-stdout', stdout, '--env', 'PERF_CYCLES=' + str(args.cycles)]
    if args.instrument:
        command += ['--instrument', args.instrument]
    command += ['--launch', '--', arm['driver'], '--measure', arm['dylib'], arm['uuid'], data]
    # xctrace is the child here: never label its rusage as the driver's RSS.
    try:
        bench.run(command, env=env, timeout=args.budget)
    except subprocess.TimeoutExpired:
        raise ValueError('profile budget exhausted before xctrace finished') from None
    return finish_profile(args, arm, output)


def finish_profile(args, arm, output):
    """Validate an existing recording without launching another measured process."""
    import json
    from . import bench, reports
    trace = output / 'capture.trace'
    row = reports.driver_result((output / 'driver.json').read_text())
    # Validate operation data independently; profile has no wait4 observation of its grandchild.
    reports.observation(json.dumps(row), cycles=args.cycles,
                        operations=args.cycles * workload_operations(), child_rss=False)
    bench.run(['xcrun', 'xctrace', 'symbolicate', '--input', trace,
               '--dsym', str(arm['dylib']) + '.dSYM'])
    path = '/trace-toc/run[@number="1"]/data/table'
    if args.template == 'Allocations':
        toc = bench.capture(['xcrun', 'xctrace', 'export', '--input', trace, '--toc'])
        if toc.status:
            raise ValueError('required trace TOC failed')
        path = reports.allocation_path(toc.stdout, pid=row['pid'], driver=arm['driver'])
    export = bench.capture(['xcrun', 'xctrace', 'export', '--input', trace, '--xpath', path])
    if export.status:
        raise ValueError('required trace export failed')
    if args.template == 'Allocations':
        validation = reports.allocation_rows(export.stdout, uuid=arm['uuid'], dylib=arm['dylib'])
        statistics_path = reports.allocation_path(toc.stdout, pid=row['pid'], driver=arm['driver'],
                                                  detail_name='Statistics')
        statistics = bench.capture(['xcrun', 'xctrace', 'export', '--input', trace,
                                    '--xpath', statistics_path])
        if statistics.status:
            raise ValueError('required allocation statistics export failed')
        validation['statistics'] = reports.allocation_statistics(statistics.stdout)
        (output / 'statistics.xml').write_bytes(statistics.stdout)
    else:
        validation = reports.capture_rows(export.stdout, pid=row['pid'], uuid=arm['uuid'], allocations=False)
    (output / 'export.xml').write_bytes(export.stdout)
    print(json.dumps({'workload': row, 'capture': validation}, indent=2))


def linux_worker(arm, env):
    """The existing bisect --perf launcher for one staged arm: libtkrzw check, loader path, CPU pin."""
    from . import bench
    worker = bench.Bench('just measure', env, arm['driver'], str(min(os.sched_getaffinity(0))),
                         cell=arm['backend'], required=True)
    return worker, bench.Arm(arm['label'], arm['dylib'], arm['data'], arm['dylib'].parent)


def linux_source(arm, env, timeout):
    """Choose the instruction source once, from one unmeasured launch of the driver's PERF_HW mode."""
    import json
    worker, cell = linux_worker(arm, env)
    done = worker.launch(cell, 'speed', 2, extra={'PERF_HW': 'instructions'}, timeout=timeout)
    row = json.loads(done.stdout)
    if 'steady_hw_instructions' in row:
        return 'perf_event_open'
    if 'hw_errno' not in row:
        raise ValueError('the driver reported neither a hardware count nor its refusal')
    return 'callgrind'


def linux_sample(arm, cycles, env, deadline, *, source, fingerprint=None, profile=None):
    """One fresh bisect --perf process, bounded by the campaign DEADLINE (time.monotonic()).

    PROFILE selects the Callgrind worker for the same loop.
    """
    from . import reports, sentence, valgrind
    worker, cell = linux_worker(arm, env)
    extra = {'PERF_WORKLOAD_CHECKSUM': '1'}
    if profile is None:
        if source == 'perf_event_open':
            extra['PERF_HW'] = 'instructions'
        # rss-diag times the same loop as speed and adds the smaps_rollup readings around it.
        done = worker.launch(cell, 'rss-diag', cycles, extra=extra, deadline=deadline)
        return reports.linux_observation(done.stdout, cycles=cycles, library=arm['dylib'],
                                         source=source, fingerprint=fingerprint), None
    # The sentence campaign's Ir-only collection, on the keystroke cycle's steady anchor.
    prefix = ['valgrind', '--tool=callgrind', '--collect-atstart=no', '--toggle-collect=' + valgrind.ANCHOR,
              '--cache-sim=no', '--branch-sim=no', '--separate-threads=no', '--callgrind-out-file=' + str(profile)]
    done = worker.launch(cell, 'speed', cycles, extra=extra, prefix=prefix, deadline=deadline)
    row = reports.linux_observation(done.stdout, cycles=cycles, library=arm['dylib'], fingerprint=fingerprint)
    parsed = sentence.checked_profile(profile.read_bytes(), arm['driver'], row['pid'], arm['dylib'])
    row[reports.LINUX_SOURCES['callgrind']] = parsed['total']
    return row, parsed


def linux_compare(args, arms, env, output):
    """The shared schedule over the existing Linux machinery; return the exit status.

    The native pass gives wall time, CPU time, residency and, where the host
    grants it, the hardware instruction count. Where it does not, a second pass
    over the same schedule collects Callgrind Ir for the same loop. One budget
    covers both passes. Every completed run is kept; nothing is retried.
    """
    import json
    import time
    from ..runners import emit
    from . import reports
    source = linux_source(arms['A'], env, args.budget)
    counter = reports.LINUX_SOURCES[source]
    emit('instruction source: ' + source + ' (' + counter + ')')
    rows, functions, creators, fingerprint = {}, {}, [], None
    deadline = time.monotonic() + args.budget
    with (output / 'runs.jsonl').open('w') as stream:
        for phase in ('native', 'callgrind') if source == 'callgrind' else ('native',):
            for kind, block, label in schedule(args.pairs, args.control_pairs):
                if deadline - time.monotonic() <= 0:
                    raise ValueError('measurement budget exhausted; campaign incomplete')
                arm = arms[CONTROL if kind == 'control' and label == 'B' else label]
                profile = output / f'callgrind.{kind}.{block}.{label}.out' if phase == 'callgrind' else None
                try:
                    # The launcher takes the worker's bound after its libtkrzw check, from the same deadline.
                    row, parsed = linux_sample(arm, args.cycles, env, deadline, source=source,
                                               fingerprint=fingerprint, profile=profile)
                except subprocess.TimeoutExpired:
                    raise ValueError('measurement budget exhausted; campaign incomplete') from None
                fingerprint = row['fingerprint']
                row.update(kind=kind, block=block, arm=label, phase=phase, instruction_source=source)
                stream.write(json.dumps(row) + '\n')
                stream.flush()
                if parsed is None:
                    rows[kind, block, label] = row
                else:
                    rows[kind, block, label][counter] = row[counter]
                    functions[kind, block, label] = parsed['functions']
                    creators.append(parsed['creator'])
    result = reports.linux_comparison(list(rows.values()), source=source, functions=functions,
                                      creators=creators or [None])
    (output / 'summary.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))
    # As the Callgrind campaign: an invalid exact control is a failed run, after its record is kept.
    return 3 if 'INVALID' in result['verdicts'].values() else 0


def linux_profile(args, arm, env, output):
    """Attribution with the existing Callgrind and Massif recipes; no scoped-loop estimate."""
    import json
    from . import bench, instructions, reports, valgrind
    worker, cell = linux_worker(arm, env)
    if args.template == 'Callgrind':
        profile = output / 'callgrind.out'
        prefix = [*valgrind.CALLGRIND_OPTIONS, '--callgrind-out-file=' + str(profile)]
    else:
        profile = output / 'massif.out'
        collector = bench.script('tools/bisection/system_import_profile.py')
        prefix = ['valgrind', *(option for option in collector.valgrind_options('heap')
                                if not option.startswith('--massif-out-file=')), '--massif-out-file=' + str(profile)]
    try:
        done = worker.launch(cell, 'speed', args.cycles, extra={'PERF_WORKLOAD_CHECKSUM': '1'},
                             prefix=prefix, timeout=args.budget)
    except subprocess.TimeoutExpired:
        raise ValueError('profile budget exhausted before Valgrind finished') from None
    row = reports.linux_observation(done.stdout, cycles=args.cycles, library=arm['dylib'])
    if args.template == 'Callgrind':
        costs = instructions.callgrind_costs(profile.read_text(errors='replace'), arm['dylib'])
        subject = costs['objects'].get('SUBJECT', {}).get('Ir', 0)
        if not subject:
            raise ValueError('Callgrind profile credits no instructions to the measured library')
        valgrind.annotate(output)
        capture = dict(scope='steady cycles (' + valgrind.ANCHOR + ')', totals=costs['totals'],
                       subject_Ir=subject, functions=costs['functions'][:20])
    else:
        try:
            capture = dict(scope='process lifetime', heap=collector.report('A', 'heap', None, profile.read_bytes()))
        except collector.Failure as failure:
            raise ValueError(failure.message) from None
    print(json.dumps({'workload': row, 'capture': capture}, indent=2))
    return 0


def native(argv, env):
    """Native source-revision measurements using the existing recipe services."""
    import json
    from pathlib import Path
    import platform
    import tempfile
    from tools import cachedir
    from .. import contract_data
    from . import bench, identity, neutrality, prepare, reports
    args = native_arguments(argv)
    windows = sys.platform == 'win32'
    linux = sys.platform == 'linux'
    args.windows = windows
    if windows and args.mode != 'compare':
        bench.fail('Windows supports compare only; profile attribution and neutrality are done on macOS', 2)
    if linux:
        if args.mode == 'neutrality':
            bench.fail('Linux checks debug-info neutrality with just measure-callgrind-control', 2)
        if args.mode == 'profile' and (args.template not in VALGRIND_TEMPLATES or args.instrument):
            bench.fail('Linux profiles with --template Callgrind or Massif and no --instrument', 2)
        if args.cycles < 2:
            bench.fail('Linux measures the steady cycles after the first; --cycles must be at least 2', 2)
        from .. import vcpkg
        # The overlay tkrzw for every arm; tools/tkrzw/runtime.py certifies each launch against it.
        run_env = vcpkg.environment(env)
    elif args.template in VALGRIND_TEMPLATES:
        bench.fail('Callgrind and Massif profiles run on Linux', 2)
    if not windows and not linux and (sys.platform != 'darwin' or platform.machine() != 'arm64'):
        bench.fail('native revision measurement requires macOS arm64, Windows x64 or Linux', 2)
    if windows:
        if platform.machine().lower() not in ('amd64', 'x86_64'):
            bench.fail('Windows comparison requires the x64 MSVC host and x64-windows triplet', 2)
        if not env.get('VCPKG_ROOT'):
            bench.fail('Windows comparison requires VCPKG_ROOT', 2)
        from tools.dev.check import msvc_environment
        env = {key.upper(): value for key, value in msvc_environment(env).items()}
        installed = Path(env['VCPKG_ROOT']).resolve() / 'installed/x64-windows'
        env['PATH'] = str(installed / 'bin') + os.pathsep + env['PATH']
        env.pop('MEASURE_PREFLIGHT', None)
        env.pop('MEASURE_MODULES', None)
    toolchain = bench.capture(['rustup', 'show', 'active-toolchain'], env=env)
    if toolchain.status or not toolchain.stdout.strip():
        raise ValueError('cannot identify the selected Rust toolchain')
    env = {**env, 'RUSTUP_TOOLCHAIN': toolchain.stdout.decode().split()[0]}
    def resolve(revision):
        done = bench.capture(['git', 'rev-parse', '--verify', revision + '^{commit}'])
        if done.status:
            raise ValueError('unknown source revision: ' + revision)
        return done.stdout.decode().strip()
    revisions = {label: resolve(rev) for label, rev in
                 ({'A': args.base, 'B': args.head} if args.mode == 'compare' else {'A': args.revision}).items()}
    runs = cachedir.root(env) / 'measure/runs'
    runs.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix=args.mode + '-', dir=runs))
    with tempfile.TemporaryDirectory(prefix='builds-', dir=output) as build_directory:
        # Linux compares plain shipping builds; only a profile adds debug information.
        options = ({'windows': True} if windows else
                   {'linux': True, 'symbols': args.mode == 'profile'} if linux else {})
        built = {sha: prepare.native_build(sha, Path(build_directory) / sha, env, **options)
                 for sha in dict.fromkeys(revisions.values())}
        arms = {label: built[sha] for label, sha in revisions.items()}
        if args.mode == 'neutrality':
            plain = prepare.native_build(revisions['A'], Path(build_directory) / 'plain', env, symbols=False)
            print(neutrality.compare_macho(arms['A']['dylib'], plain['dylib']))
            return 0
        artifacts = Path(build_directory) / 'arms' if windows or linux else output
        artifacts.mkdir(exist_ok=True)
        driver = artifacts / ('bisect.exe' if windows else 'bisect')
        digest = prepare.native_driver(arms['A'], driver, env)
        # Private prefixes for A and B even when they share one built product.
        staged = {label: stage_arm(artifacts / PREFIXES[label], built_arm, driver, digest, windows=windows)
                  for label, built_arm in arms.items()}
        if args.mode == 'compare':
            # The control's second label: A's bytes again, staged as B is.
            staged[CONTROL] = stage_arm(artifacts / PREFIXES[CONTROL], arms['A'], driver, digest, windows=windows)
        arms = staged
        data = args.data or contract_data.fetch(contract_data.default_cache(env))
        if linux:
            # The staged prefix name is also the worker's PERF_BACKEND label.
            for label, arm in arms.items():
                arm.update(label=PREFIXES[label], data=data)
        if windows:
            modules = identity.windows_preflight(arms, data, env, installed)
            manifest = artifacts / 'modules.txt'
            manifest.write_text(''.join(item['path'] + '\n' for item in modules), encoding='utf-8')
            env['MEASURE_MODULES'] = str(manifest)
        metadata = dict(revisions=revisions, builds=[{key: value for key, value in arm.items()
                        if key in ('revision', 'default_features', 'cold_build_seconds', 'build_options')}
                        for arm in built.values()], cycles=args.cycles, pairs=args.pairs,
                        control_pairs=args.control_pairs, budget_seconds=args.budget,
                        staged={label: str(arm['dylib'].parents[1]) for label, arm in arms.items()},
                        operation_boundary=(reports.LINUX_LOOP if linux else
                                            'all typing cycles; initialization and teardown excluded'),
                        footprint_boundary=('after the measured loop, before teardown' if linux else
                                            'process lifetime through teardown'),
                        data_identity=contract_data.data_identity(data),
                        versions={name: identity.first_output_line(command) for name, command in
                                  (('Linux', ['uname', '-srm']), ('Rust', ['rustc', '-V']),
                                   ('Valgrind', ['valgrind', '--version']))} if linux else
                                 {name: bench.capture(command).stdout.decode().strip() for name, command in
                                  ([('Windows', ['cmd', '/c', 'ver']), ('Rust', ['rustc', '-vV'])] if windows else
                                   [('macOS', ['sw_vers', '-productVersion']),
                                   ('Xcode', ['xcodebuild', '-version']), ('Rust', ['rustc', '-vV'])])})
        if windows:
            metadata.update(runtime_modules=modules,
                            expected_cow_commit=identity.expected_cow_commit(data),
                            artifacts={label: dict(name=arm['dylib'].name, sha256=identity.sha256(arm['dylib']),
                                                  driver_sha256=digest) for label, arm in arms.items()},
                            cpu_boundary='all-thread process CPU during the typing loop',
                            cpu_headline='loop_cpu_cycles (QueryProcessCycleTime delta); paired B over A, never seconds',
                            cpu_secondary='cpu_seconds (GetProcessTimes); 1/64-second resolution',
                            memory_headline='peak_private_commit_bytes; paired B minus A in bytes',
                            memory_diagnostic='peak_working_set_bytes; process lifetime through teardown')
        (output / 'session.json').write_text(json.dumps(metadata, indent=2))
        print(json.dumps(metadata, indent=2), flush=True)
        print('Local output: ' + str(output), flush=True)
        if linux:
            return (linux_compare(args, arms, run_env, output) if args.mode == 'compare'
                    else linux_profile(args, arms['A'], run_env, output))
        if args.mode == 'compare':
            native_compare(args, arms, data, env, output)
        else:
            native_profile(args, arms['A'], data, env, output)
        return 0
