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
    parser.add_argument('--template', choices=('CPU Profiler', 'Allocations', 'System Trace', 'Processor Trace'))
    parser.add_argument('--instrument', choices=('VM Tracker',))
    parser.add_argument('--cycles', type=int, default=20)
    parser.add_argument('--pairs', type=int, default=6)
    parser.add_argument('--control-pairs', type=int, default=6)
    parser.add_argument('--budget', type=float, default=180)
    parser.add_argument('--data', type=Path)
    args = parser.parse_args(argv)
    if args.mode == 'compare' and (not args.base or not args.head):
        parser.error('compare requires --base and --head')
    if args.mode == 'profile' and not args.template:
        parser.error('profile requires --template')
    if not 1 <= args.cycles <= 100000 or not 0 < args.budget < float('inf'):
        parser.error('cycles must be 1..100000 and budget finite and positive')
    try:
        list(schedule(args.pairs, args.control_pairs))
    except ValueError as error:
        parser.error(str(error))
    return args


CONTROL = "A'"


def stage_arm(prefix, built_arm, driver, digest):
    """Copy one built product and the signed driver into a private prefix."""
    import shutil
    from . import identity
    (prefix / 'bin').mkdir(parents=True)
    (prefix / 'lib').mkdir()
    dylib = prefix / 'lib' / built_arm['dylib'].name
    shutil.copy2(built_arm['dylib'], dylib)
    shutil.copytree(str(built_arm['dylib']) + '.dSYM', str(dylib) + '.dSYM')
    executable = prefix / 'bin/bisect'
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
    raw, usage = bench.measured_process(command, env={**env, 'PERF_CYCLES': str(cycles)}, timeout=timeout)
    row = json.loads(raw)
    row['peak_rss_bytes'] = usage.ru_maxrss  # Darwin wait4 reports bytes, not Linux KiB.
    return reports.observation(json.dumps(row), cycles=cycles, operations=cycles * workload_operations())


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
                                      operations=args.cycles * workload_operations(), fingerprint=fingerprint)
            fingerprint = row['fingerprint']
            row.update(kind=kind, block=block, arm=label)
            rows.append(row)
            stream.write(json.dumps(row) + '\n')
            stream.flush()
    result = reports.comparison(rows)
    (output / 'summary.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))


def native_profile(args, arm, data, env, output):
    import json
    from . import bench, reports
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


def native(argv, env):
    """Native source-revision measurements using the existing recipe services."""
    import json
    from pathlib import Path
    import platform
    import tempfile
    from tools import cachedir
    from .. import contract_data
    from . import bench, neutrality, prepare
    args = native_arguments(argv)
    if sys.platform != 'darwin' or platform.machine() != 'arm64':
        bench.fail('native revision measurement currently requires macOS arm64', 2)
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
        built = {sha: prepare.native_build(sha, Path(build_directory) / sha, env)
                 for sha in dict.fromkeys(revisions.values())}
        arms = {label: built[sha] for label, sha in revisions.items()}
        if args.mode == 'neutrality':
            plain = prepare.native_build(revisions['A'], Path(build_directory) / 'plain', env, symbols=False)
            print(neutrality.compare_macho(arms['A']['dylib'], plain['dylib']))
            return 0
        driver = output / 'bisect'
        digest = prepare.native_driver(arms['A'], driver, env)
        # Private prefixes for A and B even when they share one built product.
        staged = {label: stage_arm(output / label, built_arm, driver, digest)
                  for label, built_arm in arms.items()}
        if args.mode == 'compare':
            # The control's second label: A's bytes again, staged as B is.
            staged[CONTROL] = stage_arm(output / 'A-control', arms['A'], driver, digest)
        arms = staged
        data = args.data or contract_data.fetch(contract_data.default_cache(env))
        metadata = dict(revisions=revisions, builds=[{key: value for key, value in arm.items()
                        if key in ('revision', 'default_features', 'cold_build_seconds', 'build_options')}
                        for arm in built.values()], cycles=args.cycles, pairs=args.pairs,
                        control_pairs=args.control_pairs, budget_seconds=args.budget,
                        staged={label: str(arm['dylib'].parents[1]) for label, arm in arms.items()},
                        operation_boundary='all typing cycles; initialization and teardown excluded',
                        footprint_boundary='process lifetime through teardown',
                        data_identity=contract_data.data_identity(data),
                        versions={name: bench.capture(command).stdout.decode().strip() for name, command in
                                  [('macOS', ['sw_vers', '-productVersion']),
                                   ('Xcode', ['xcodebuild', '-version']), ('Rust', ['rustc', '-vV'])]})
        (output / 'session.json').write_text(json.dumps(metadata, indent=2))
        print(json.dumps(metadata, indent=2), flush=True)
        print('Local output: ' + str(output), flush=True)
        if args.mode == 'compare':
            native_compare(args, arms, data, env, output)
        else:
            native_profile(args, arms['A'], data, env, output)
        return 0
