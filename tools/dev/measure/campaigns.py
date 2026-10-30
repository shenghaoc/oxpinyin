"""The round-robin measurement campaigns behind `measure` (#1290).

perf-baseline, perf-matrix, perf-same-data, shipped-check, tree-recipe-control and
rss-diag each drove the `bisect --perf` harness over several arms and summarised
the capture. They differ in their arms, their output names and their banners;
the machinery is bench.py's. Every refusal still names the legacy script that
used to run, because those names are what reports and runbooks quote.

The sampling loops are serial and interleave the arms one process at a time:
a timed or resident-set sample is only the arm's if nothing runs beside it, and
the interleave puts drift in host state on every arm alike. The builds are not
samples; shipped-check and tree-recipe-control run theirs as units of one
admitted pool, and perf-same-data builds in sequence because its two builds
share one target directory.
"""
import glob
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
from collections.abc import Mapping, Sequence

from ..runners import Finished, emit, write
from .. import slot
from . import bench, identity, reports
from .bench import Arm, Bench, fail, setting

BASELINE, MATRIX, SAME_DATA = 'run-perf-baseline.sh', 'run-perf-matrix.sh', 'run-perf-same-data.sh'
SHIPPED, TREE, RSS = 'run-shipped-check.sh', 'run-tree-recipe-control.sh', 'run-rss-diag.sh'
ORACLE_PIN = 'libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99'
# The record's own documented Tkrzw artifact (docs/findings/perf-steady-cycle-cross-host-2026-09-07.md,
# amd64 capi-artifacts row).
RECORD_X_SHA = 'bf8d3b5707b5bc1bafc09670185b96770b73f596dd2eb57b8a9cba46bad4aec8'
# These two runners compile the image's own harness, not the running tree's: the timed unit has to be
# the one the record was taken with (the image's run_perf_unit is byte-identical to the record's).
IMAGE_HARNESS_SOURCE = Path('/repo/tools/bisection/bisect.c')
# The control runners build into fixed names here, and their identity files and findings quote them.
SCRATCH = Path('/tmp')
LIBPINYIN_TKRZW, LIBPINYIN_KC = Path('/opt/libpinyin-tkrzw'), Path('/opt/libpinyin-kc')
DATA_SUBDIRECTORY = 'lib/libpinyin/data'
REQUIRED_DATA = ('pinyin_index.bin', 'phrase_index.bin', 'bigram.db', 'table.conf',
                 'addon_pinyin_index.bin', 'addon_phrase_index.bin', 'punct.bin', 'gb_char.bin')


def truncate(*paths) -> None:
    """Empty each file, creating it: the `: > FILE` that starts a capture."""
    for path in paths:
        Path(path).write_bytes(b'')


def write_report(script: str, argv: Sequence[str], destination, *, echo: bool = True) -> None:
    """Run a report script into DESTINATION and print it: `uv run … SCRIPT ARGV > DEST; cat DEST`.

    The file is truncated before the script runs, so a failing script leaves it
    empty, and the report is printed only once the script has succeeded.
    """
    destination = Path(destination)
    destination.write_bytes(b'')
    code, text = bench.report(script, argv)
    destination.write_text(text)
    if code:
        raise Finished(code)
    if echo:
        sys.stdout.write(text)
        sys.stdout.flush()


def pkg_config(env: Mapping[str, str], *arguments: str) -> str:
    """The output of `pkg-config ARGUMENTS libpinyin`, a failing pkg-config ending the run with its status."""
    done = bench.capture(['pkg-config', *arguments, 'libpinyin'], env=env)
    if done.status:
        write(done.stderr, error=True)
        raise Finished(done.status)
    return done.stdout.decode(errors='replace').rstrip('\n')


def elf_hooks_and_needed(library) -> tuple[int, list[str]]:
    """The fixture-hook count and DT_NEEDED names of the file LIBRARY resolves to."""
    real = identity.realpath(library)
    return identity.fixture_hooks(real), identity.needed(real)


def measured(env: Mapping[str, str], label: str, harness: Path, cpu: str, *, cell: str = 'auto',
             required: bool = False) -> Bench:
    """The bench a runner launches workers through; REQUIRED says taskset must exist."""
    return Bench(label, env, harness, cpu, cell=cell, required=required)


# ── perf-baseline ──────────────────────────────────────────────────────────────

def perf_baseline(argv: Sequence[str], env: Mapping[str, str]) -> int:
    """Oracle versus installed oxpinyin: speed, RAM and installed size, alternating the two."""
    label = BASELINE
    env = dict(env)
    with bench.working_directory(bench.BISECT):
        bench.no_arguments(argv)
        home = Path(env['HOME']) if env.get('HOME') else Path.home()
        oracle_prefix = Path(setting(env, 'PINYIN_ORACLE_PREFIX', str(home / '.local/opt/pinyin-oracle')))
        work = Path(setting(env, 'OXPINYIN_PERF_WORK', str(bench.ROOT / 'target/perf-baseline')))
        stage = work / 'stage'
        cpu = setting(env, 'PERF_CPU', '0')
        runs = bench.count(label, env, 'PERF_RUNS', '20')
        cycles = setting(env, 'PERF_CYCLES', '8')
        ram_runs = bench.count(label, env, 'PERF_RAM_RUNS', '10')
        oracle_so = oracle_prefix / 'lib/libpinyin.so'
        oracle_data = oracle_prefix / DATA_SUBDIRECTORY
        work.mkdir(parents=True, exist_ok=True)
        env['PINYIN_ORACLE_PREFIX'] = str(oracle_prefix)

        pin_file = oracle_prefix / 'oracle-pin.txt'
        if not pin_file.is_file() or not oracle_so.is_file():
            emit(f'fatal: pin-built oracle not found at {oracle_prefix}', error=True)
            fail('  build it with tools/oracle/build-oracle.sh')
        pattern = re.compile('^pin_ref=.*' + ORACLE_PIN)
        if not any(pattern.search(line) for line in pin_file.read_text(errors='replace').split('\n')):
            fail(f'fatal: oracle prefix at {oracle_prefix} is off-pin')

        emit('--- building bisect harness ---')
        harness = bench.BISECT / 'bisect'
        bench.run(bench.harness_build(harness))
        emit('build: ok')

        staged = bench.find_named(stage / 'usr', 'libpinyin.pc', any_type=True)
        if staged and env.get('PERF_REUSE_STAGE', '0') == '1':
            emit(f'--- reusing stage: {stage} ---')
        else:
            emit(f'--- cargo cinstall oxpinyin-capi -> {stage} ---')
            shutil.rmtree(stage, ignore_errors=True)
            bench.heavy(['cargo', 'cinstall', '--locked', '--release', '-p', 'oxpinyin-capi',
                         f'--destdir={stage}', '--prefix=/usr', '--manifest-path', bench.ROOT / 'Cargo.toml'],
                        env=env)
            staged = bench.find_named(stage / 'usr', 'libpinyin.pc', any_type=True)
        if staged is None:
            fail(f'fatal: libpinyin.pc not found under {stage / "usr"}')
        env['PKG_CONFIG_PATH'] = bench.loader_path(staged.parent, env.get('PKG_CONFIG_PATH'))
        env['PKG_CONFIG_SYSROOT_DIR'] = str(stage)
        cflags = pkg_config(env, '--cflags')
        libs = pkg_config(env, '--libs')
        libdir = pkg_config(env, '--variable=libdir')
        capi_so = Path(f'{libdir}/libpinyin.so')
        capi_data = stage / 'usr/share/oxpinyin'
        emit(f'pkg-config oxpinyin cflags: {cflags}')
        emit(f'pkg-config oxpinyin libs:   {libs}')
        emit(f'installed capi .so:         {capi_so}')
        if not capi_so.is_file():
            fail(f'fatal: {capi_so} not found')

        oracle = Arm('oracle', oracle_so, oracle_data, oracle_prefix / 'lib')
        oxpinyin = Arm('oxpinyin', capi_so, capi_data, Path(libdir))
        worker = measured(env, label, harness, cpu)
        worker.standalone('pin', Arm(oracle.label, oracle_so, oracle_data, oracle_so.parent))
        worker.standalone('subject', Arm(oxpinyin.label, capi_so, capi_data, capi_so.parent))

        populate_data(capi_data, Path(setting(env, 'PINYIN_EXPORT_DIR', '/tmp/oxpinyin-export')))
        for name in REQUIRED_DATA:
            if not (capi_data / name).is_file():
                fail(f'fatal: missing {capi_data / name}')
        emit(f'installed capi data:        {capi_data}')

        def sample(arm: Arm, mode: str) -> None:
            worker.sample(arm, mode, work / f'{mode}.jsonl', work / f'{arm.label}-{mode}.err', cycles)

        speed = work / 'speed.jsonl'
        truncate(speed)
        emit(f'--- speed: {runs} alternating runs x {cycles} cycles, CPU {cpu} ---')
        bench.round_robin(runs, [(oracle, 'speed'), (oxpinyin, 'speed')], sample)
        write_report('tools/bisection/perf-baseline.py', ['summarize-speed', str(speed)], work / 'speed-report.md')
        emit(f'captures: {speed}')

        ram_init, ram_cycle = work / 'ram-init.jsonl', work / 'ram-cycle.jsonl'
        truncate(ram_init, ram_cycle)
        emit(f'--- RAM: {ram_runs} runs per mode per backend, CPU {cpu} ---')
        bench.round_robin(ram_runs, [(oracle, 'ram-init'), (oxpinyin, 'ram-init'),
                                     (oracle, 'ram-cycle'), (oxpinyin, 'ram-cycle')], sample)
        write_report('tools/bisection/perf-baseline.py', ['summarize-ram', str(ram_init), str(ram_cycle)],
                     work / 'ram-report.md')
        emit(f'captures: {ram_init} {ram_cycle}')

        write_report('tools/bisection/perf-baseline.py', ['size', str(oracle_prefix), str(stage / 'usr')],
                     work / 'size-report.md')
        emit(f'reports: {work}/{{speed,ram,size}}-report.md')
    return 0


def populate_data(destination: Path, export: Path) -> None:
    """Fill the staged data directory from the export when it lacks a table.

    cargo-c installs no data; the packager adds it. Every exported table, database
    and table.conf is copied (symlinks followed) and each is checked to have arrived.
    """
    if all((destination / name).is_file() for name in REQUIRED_DATA[:4]):
        return
    emit('--- populating oxpinyin data from export + model cache ---')
    missing = [name for name in REQUIRED_DATA[:4] if not (export / name).is_file()]
    if missing:
        emit(f'fatal: exported file(s){"".join(" " + name for name in missing)} not found at {export};',
             error=True)
        fail('  generate with: cargo run --release -p oxpinyin-datagen -- compile --out-dir DIR')
    destination.mkdir(parents=True, exist_ok=True)

    def expand(pattern: str) -> list[str]:
        return sorted(glob.glob(os.path.join(glob.escape(str(export)), pattern)))
    for source in [*expand('*.bin'), *expand('*.db'), str(export / 'table.conf'), *expand('*.table')]:
        if os.path.isfile(source):
            bench.copy_file(source, destination)
    for source in [*expand('*.bin'), *expand('*.db'), str(export / 'table.conf')]:
        if os.path.isfile(source) and not (destination / os.path.basename(source)).is_file():
            fail(f'fatal: {os.path.basename(source)} failed to copy from {export} to {destination}')


# ── perf-matrix ────────────────────────────────────────────────────────────────

def matrix_cells() -> list[tuple[str, Path, Path]]:
    """The four cells of the backend-by-implementation matrix: (label, library, data directory)."""
    a_so = bench.find_library(LIBPINYIN_TKRZW)
    b_so = bench.find_library(LIBPINYIN_KC)
    c_so = bench.find_library('/opt/oxpinyin-tkrzw/stage')
    d_so = bench.find_library('/opt/oxpinyin-kc/stage')
    return [('libpinyin-tkrzw', a_so, LIBPINYIN_TKRZW / DATA_SUBDIRECTORY),
            ('libpinyin-kc', b_so, LIBPINYIN_KC / DATA_SUBDIRECTORY),
            ('oxpinyin-tkrzw', c_so, Path('/opt/oxpinyin-tkrzw/data')),
            ('oxpinyin-kc', d_so, Path('/opt/oxpinyin-kc/data'))]


def perf_matrix(argv: Sequence[str], env: Mapping[str, str]) -> int:
    """Four cells (libpinyin and oxpinyin over Tkrzw and Kyoto Cabinet), round-robin, with size."""
    label = MATRIX
    env = dict(env)
    with bench.working_directory(bench.BISECT):
        bench.no_arguments(argv)
        out = Path(setting(env, 'MATRIX_OUT', '/out'))
        cpu = setting(env, 'PERF_CPU', '0')
        runs = bench.count(label, env, 'PERF_RUNS', '20')
        cycles = setting(env, 'PERF_CYCLES', '8')
        repeats = setting(env, 'PERF_REPEATS', '1')
        ram_runs = bench.count(label, env, 'PERF_RAM_RUNS', '10')
        out.mkdir(parents=True, exist_ok=True)

        cells = matrix_cells()
        arms = [Arm(name, so, data, so.parent) for name, so, data in cells]
        a, b, c, d = arms
        worker = measured(env, label, bench.BISECT / 'bisect', cpu)
        worker.standalone('pin', a, 'tkrzw')
        worker.standalone('subject', c, 'tkrzw')

        emit('=== Matrix Configuration ===')
        for arm in arms:
            emit(f'  {arm.label}:')
            emit(f'    so:   {arm.so}')
            emit(f'    data: {arm.data}')
            emit(f'    lib:  {arm.libdir}')
            if not arm.so.is_file():
                fail(f'fatal: {arm.so} not found')
            if not arm.data.is_dir():
                fail(f'fatal: {arm.data} not found')

        def sample(arm: Arm, mode: str) -> None:
            worker.sample(arm, mode, out / f'{mode}.jsonl', out / f'{arm.label}-{mode}.err', cycles, repeats=repeats)

        speed, ram_init, ram_cycle = out / 'speed.jsonl', out / 'ram-init.jsonl', out / 'ram-cycle.jsonl'
        truncate(speed)
        emit(f'--- speed: {runs} alternating runs × {cycles} cycles × {repeats} passes, CPU {cpu} ---')
        bench.round_robin(runs, [(arm, 'speed') for arm in arms], sample)

        truncate(ram_init, ram_cycle)
        emit(f'--- RAM: {ram_runs} runs per mode per cell, CPU {cpu} ---')
        bench.round_robin(ram_runs, [(arm, mode) for arm in arms for mode in ('ram-init', 'ram-cycle')], sample)

        emit('--- installed size ---')
        # libpinyin sizes its install prefix; oxpinyin its staged tree. The Dockerfile strips both alike.
        prefixes = [LIBPINYIN_TKRZW, LIBPINYIN_KC, Path('/opt/oxpinyin-tkrzw/stage/usr'),
                    Path('/opt/oxpinyin-kc/stage/usr')]
        write_report('tools/bisection/perf-matrix.py',
                     ['summarize', '--speed', str(speed), '--ram-init', str(ram_init), '--ram-cycle', str(ram_cycle),
                      '--labels', *(arm.label for arm in arms),
                      '--size-prefixes', *map(str, prefixes),
                      '--data-dirs', *(str(arm.data) for arm in arms)],
                     out / 'matrix-report.md')
        emit()
        emit(f'captures: {out}/{{speed,ram-init,ram-cycle}}.jsonl')
        emit(f'report:   {out}/matrix-report.md')
    return 0


# ── perf-same-data ─────────────────────────────────────────────────────────────

def verify_backend(backend: str, library: Path) -> None:
    """Refuse an artifact that does not link the backend it is labelled with, and only that one.

    The backend is never inherited from workspace defaults: the default flipped
    from Kyoto Cabinet to Tkrzw at 05688575, and the KC cell's implicit-default
    build silently produced a second tkrzw artifact whose processes failed
    pinyin_init on KC-format data. This runs on the resolved paths because the
    OXPINYIN_*_SO variables skip the build, which is exactly the mode in which a
    mislabelled artifact arrives. oxpinyin-store has a compile_error! for two
    backends at once, so no legitimate artifact links both; one that does was
    not produced by this tree's build and its storage format is unknown.
    """
    if not library.is_file():
        fail(f'fatal: {library} not found')
    patterns = {'kyotocabinet': ('^libkyotocabinet\\.so', '^libtkrzw\\.so'),
                'tkrzw': ('^libtkrzw\\.so', '^libkyotocabinet\\.so')}
    if backend not in patterns:
        fail(f"fatal: verify_backend: unknown backend '{backend}'")
    wanted, other = patterns[backend]
    names = identity.needed(library)
    listing = ' '.join(names)
    if not any(re.search(wanted, name) for name in names):
        fail(f'fatal: {library} does not link a {backend} library (NEEDED: {listing})')
    if any(re.search(other, name) for name in names):
        fail(f"fatal: {library} links a second backend's library (NEEDED: {listing})")


def build_capi(backend: str, destination: Path, target: Path, env: Mapping[str, str]) -> None:
    """Build the capi library for exactly one named backend and copy it, stripped, to DESTINATION."""
    if not backend:
        fail('fatal: build_capi requires an explicit backend')
    if backend not in ('kyotocabinet', 'tkrzw'):
        fail(f"fatal: build_capi: unknown backend '{backend}'")
    bench.heavy(['cargo', 'build', '--locked', '--release', '-p', 'oxpinyin-capi', '--no-default-features',
                 '--features', backend, '--manifest-path', bench.ROOT / 'Cargo.toml'], env=env)
    bench.copy_file(target / 'release/libpinyin_capi.so', destination)
    bench.run(['strip', '--strip-all', destination])


def perf_same_data(argv: Sequence[str], env: Mapping[str, str]) -> int:
    """The four-cell benchmark on one data directory per backend: the drop-in configuration."""
    label = SAME_DATA
    env = dict(env)
    with bench.working_directory(bench.BISECT):
        bench.no_arguments(argv)
        out = Path(setting(env, 'MATRIX_OUT', '/out'))
        cpu = setting(env, 'PERF_CPU', '0')
        runs = bench.count(label, env, 'PERF_RUNS', '20')
        cycles = setting(env, 'PERF_CYCLES', '8')
        repeats = setting(env, 'PERF_REPEATS', '1')
        ram_runs = bench.count(label, env, 'PERF_RAM_RUNS', '10')
        target = Path(setting(env, 'CARGO_TARGET_DIR', str(bench.ROOT / 'target')))
        out.mkdir(parents=True, exist_ok=True)

        # Serial: both builds write $TARGET/release/libpinyin_capi.so, and each is copied out before
        # the next replaces it, so they cannot overlap.
        kc_so = Path(env['OXPINYIN_KC_SO']) if env.get('OXPINYIN_KC_SO') else None
        tkrzw_so = Path(env['OXPINYIN_TKRZW_SO']) if env.get('OXPINYIN_TKRZW_SO') else None
        if kc_so is None:
            kc_so = out / 'libpinyin_capi-kc.so'
            emit('--- building oxpinyin capi (kyotocabinet) ---')
            build_capi('kyotocabinet', kc_so, target, env)
        if tkrzw_so is None:
            tkrzw_so = out / 'libpinyin_capi-tkrzw.so'
            emit('--- building oxpinyin capi (tkrzw) ---')
            build_capi('tkrzw', tkrzw_so, target, env)

        emit('--- verifying capi backend linkage ---')
        verify_backend('kyotocabinet', kc_so)
        verify_backend('tkrzw', tkrzw_so)

        emit('--- building bisect harness ---')
        harness = bench.BISECT / 'bisect'
        bench.run(bench.harness_build(harness, flags=('-std=gnu11', '-Wall', '-Wextra', '-O2')))

        a_so, b_so = bench.find_library(LIBPINYIN_TKRZW), bench.find_library(LIBPINYIN_KC)
        a_data, b_data = LIBPINYIN_TKRZW / DATA_SUBDIRECTORY, LIBPINYIN_KC / DATA_SUBDIRECTORY
        arms = [Arm('libpinyin-tkrzw', a_so, a_data, a_so.parent), Arm('libpinyin-kc', b_so, b_data, b_so.parent),
                Arm('oxpinyin-tkrzw', tkrzw_so, a_data, tkrzw_so.parent),
                Arm('oxpinyin-kc', kc_so, b_data, kc_so.parent)]
        worker = measured(env, label, harness, cpu)
        worker.standalone('pin', arms[0], 'tkrzw')
        worker.standalone('subject', arms[2], 'tkrzw')

        emit('=== Same-data matrix configuration ===')
        for arm in arms:
            emit(f'  {arm.label}: so={arm.so} data={arm.data}')
            if not arm.so.is_file():
                fail(f'fatal: {arm.so} not found')

        def sample(arm: Arm, mode: str) -> None:
            worker.sample(arm, mode, out / f'{mode}.jsonl', out / f'{arm.label}-{mode}.err', cycles, repeats=repeats)

        speed, ram_init, ram_cycle = out / 'speed.jsonl', out / 'ram-init.jsonl', out / 'ram-cycle.jsonl'
        truncate(speed)
        emit(f'--- speed: {runs} alternating runs × {cycles} cycles × {repeats} passes, CPU {cpu} ---')
        bench.round_robin(runs, [(arm, 'speed') for arm in arms], sample)

        truncate(ram_init, ram_cycle)
        emit(f'--- RAM: {ram_runs} runs per mode per cell, CPU {cpu} ---')
        bench.round_robin(ram_runs, [(arm, mode) for arm in arms for mode in ('ram-init', 'ram-cycle')], sample)

        emit('--- summary (medians) ---')
        sys.stdout.write(reports.same_data_summary(speed, ram_init, ram_cycle))
        sys.stdout.flush()
    return 0


# ── shipped-check and tree-recipe-control ──────────────────────────────────────

def settings_of(label: str, env: Mapping[str, str], sizes: str) -> dict:
    """The CTRL_* settings the two control runners share, with their paths and counts."""
    runs = bench.count(label, env, 'CTRL_RUNS', '20')
    return dict(out=Path(setting(env, 'CTRL_OUT', '/out')), runs=runs, cycles=setting(env, 'CTRL_CYCLES', '8'),
                sizes=setting(env, 'CTRL_SIZES', sizes).split(), cpu=setting(env, 'CTRL_CPU', '0'),
                tools=Path(setting(env, 'CTLTOOLS', '/ctltools')))


def cinstall(source: Path, destination: Path, features: str):
    """A build unit: `cargo cinstall` SOURCE with FEATURES into scratch, then copy libpinyin.so.15.0.0 out."""
    def build(context, jobs):
        with tempfile_directory() as target, tempfile_directory() as stage:
            command = ['cargo', 'cinstall', '--locked', '--release', '-p', 'oxpinyin-capi', '--no-default-features',
                       '--features', features, '--manifest-path', source / 'Cargo.toml',
                       '--target-dir', target, '--destdir', stage, '--prefix', '/usr']
            output = bench.cargo(context, command, jobs)
            built = bench.find_named(stage, 'libpinyin.so.15.0.0', any_type=True)
            if built is None:
                write(output, error=True)
                fail("cp: cannot stat '': No such file or directory")
            bench.copy_file(built, destination)
        return output
    return build


def cargo_build(source: Path, destination: Path):
    """A build unit: the record recipe, `cargo build` then `strip`, copied to DESTINATION."""
    def build(context, jobs):
        with tempfile_directory() as target:
            command = ['cargo', 'build', '--locked', '--release', '-p', 'oxpinyin-capi', '--no-default-features',
                       '--features', 'tkrzw', '--manifest-path', source / 'Cargo.toml', '--target-dir', target]
            output = bench.cargo(context, command, jobs)
            bench.copy_file(Path(target) / 'release/libpinyin_capi.so', destination)
            bench.run(['strip', '--strip-all', destination])
        return output
    return build


def tempfile_directory():
    """A scratch directory removed on exit: `mktemp -d` without leaving a build tree behind."""
    return tempfile.TemporaryDirectory(prefix='oxpinyin-build-')


def control_sources(env: Mapping[str, str]) -> Path:
    """The current tree the control runners build from."""
    return Path(setting(env, 'SRC_CURR', '/src-curr'))


def library_in(root: Path) -> Path:
    """The pin library below ROOT. `find` on a root that is not there says so and ends the run."""
    if not Path(root).is_dir():
        fail(f"find: '{root}': No such file or directory")
    return bench.find_library(root)


def shipped_check(argv: Sequence[str], env: Mapping[str, str]) -> int:
    """Is cinstall --features shipped, the product, the same as cinstall alone, within one session?"""
    label = SHIPPED
    env = dict(env)
    bench.no_arguments(argv)
    src = control_sources(env)
    cfg = settings_of(label, env, '8 16')
    out, runs, cycles, sizes, cpu, tools = (cfg[key] for key in ('out', 'runs', 'cycles', 'sizes', 'cpu', 'tools'))
    out.mkdir(parents=True, exist_ok=True)

    l_label, z_label, s_label = 'libpinyin-tkrzw', 'oxpinyin-cur-cinstall', 'oxpinyin-shipped'
    data = LIBPINYIN_TKRZW / DATA_SUBDIRECTORY
    l_so = library_in(LIBPINYIN_TKRZW)
    z_so, s_so = SCRATCH / 'Z-plain.so', SCRATCH / 'S-shipped.so'
    bench.build_all([('Z', cinstall(src, z_so, 'tkrzw')), ('S', cinstall(src, s_so, 'tkrzw,shipped'))],
                    'shipped-check builds', env=env)
    worker = measured(env, label, SCRATCH / 'bisect', cpu, cell='tkrzw', required=True)
    arms = [Arm(l_label, l_so, data, l_so.parent), Arm(z_label, z_so, data, z_so.parent),
            Arm(s_label, s_so, data, s_so.parent)]
    worker.standalone('pin', arms[0])
    worker.standalone('subject', arms[1])
    worker.standalone('subject', arms[2])

    lines = ['=== identity (shipped check) ===', f'captured_utc: {bench.utc_now()}']
    for tag, so in (('L', l_so), ('Z', z_so), ('S', s_so)):
        real = identity.realpath(so)
        lines.append(f'{tag}: {identity.sha256(real)} {os.path.getsize(real)} B')
    for tag, so in (('Z', z_so), ('S', s_so)):
        lines.append(f'{tag} fixture hooks: {identity.fixture_hooks(identity.realpath(so))}')
    bench.tee(out / 'shipped-identity.txt', '\n'.join(lines) + '\n')

    harness = SCRATCH / 'bisect'
    bench.run(bench.harness_build(harness, source=IMAGE_HARNESS_SOURCE))
    speed = out / 'speed-shipped.jsonl'
    truncate(speed)
    # Serial: timed samples, one process at a time (see bench.round_robin).
    for n in sizes:
        emit(f'--- n={n}: {runs} runs, round-robin L Z S ---')
        bench.round_robin(runs, [(arm, 'speed') for arm in arms],
                          lambda arm, mode, n=n: worker.sample(arm, mode, speed, out / 'shipped.err', cycles, repeats=n))
    # In-process bootstrap arithmetic over the finished capture, not a campaign of subprocesses: threads
    # would not overlap it under the GIL.
    for n in sizes:
        write_ratios(out / f'shipped-ratios-n{n}.md', f'## n={n}\n', tools, speed,
                     [(z_label, l_label), (s_label, l_label)], n)
    rows = speed.read_bytes().count(b'\n')
    emit(f'done: {rows} rows')
    return 0


def write_ratios(destination: Path, header: str, tools: Path, speed: Path,
                 pairs: Sequence[tuple[str, str]], repeats: str) -> None:
    """Write HEADER and perf-ratio.py's markdown for each (numerator, denominator) pair to DESTINATION.

    The shell wrote them with `{ echo HEADER; perf-ratio.py …; } > DESTINATION`: a script
    that fails ends the run with the status it failed with, and the file holds the header and
    whatever had been printed so far.
    """
    text = header
    try:
        for numerator, denominator in pairs:
            code, printed = bench.report(str(tools / 'bisection/perf-ratio.py'),
                                         [str(speed), '--numerator', numerator, '--denominator', denominator,
                                          '--repeats', repeats, '--format', 'md'])
            text += printed
            if code:
                raise Finished(code)
    finally:
        destination.write_text(text)


def tree_recipe_control(argv: Sequence[str], env: Mapping[str, str]) -> int:
    """Separate the engine-tree effect from the build-recipe effect on the steady-cycle ratio."""
    label = TREE
    env = dict(env)
    bench.no_arguments(argv)
    old, src = Path(setting(env, 'SRC_OLD', '/src-old')), control_sources(env)
    cfg = settings_of(label, env, '1 8 16')
    out, runs, cycles, sizes, cpu, tools = (cfg[key] for key in ('out', 'runs', 'cycles', 'sizes', 'cpu', 'tools'))
    out.mkdir(parents=True, exist_ok=True)

    labels = dict(L='libpinyin-tkrzw', X='oxpinyin-old-cargo', Y='oxpinyin-cur-cargo',
                  Z='oxpinyin-cur-cinstall', S='oxpinyin-cur-shipped')
    lp_lib = LIBPINYIN_TKRZW / 'lib'
    data = lp_lib / 'libpinyin/data'
    l_so = library_in(LIBPINYIN_TKRZW)

    lines = ['=== environment ===', f'captured_utc: {bench.utc_now()}',
             f'controller_commit: {setting(env, "CTRL_COMMIT", "unset")}',
             f'container_arch: {identity.uname("m")}', f'kernel: {identity.uname("srvm")}',
             f'nproc: {identity.processors()}', f'cpu: {identity.cpu_model()}', f'memtotal: {identity.memory_total()}',
             f'gcc: {identity.first_output_line(["gcc", "--version"])}',
             f'rustc: {identity.first_output_line(["rustc", "--version"])}',
             f'cargo: {identity.first_output_line(["cargo", "--version"])}',
             f'libtkrzw: {identity.package_version("tkrzw")}',
             f'libpinyin: {identity.pc_version(LIBPINYIN_TKRZW / "lib/pkgconfig/libpinyin.pc")}',
             f'data_dir: {data}', f'loadavg: {identity.loadavg()}']
    bench.tee(out / 'environment.txt', '\n'.join(lines) + '\n')

    x_so, y_so, z_so, s_so = (SCRATCH / f'{name}.so' for name in ('X-old-cargo', 'Y-cur-cargo', 'Z-cur-cinstall',
                                                                     'S-cur-shipped'))
    bench.build_all([('X', cargo_build(old, x_so)), ('Y', cargo_build(src, y_so)),
                     ('Z', cinstall(src, z_so, 'tkrzw')), ('S', cinstall(src, s_so, 'tkrzw,shipped'))],
                    'tree-recipe-control builds', env=env)
    worker = measured(env, label, SCRATCH / 'bisect', cpu, cell='tkrzw', required=True)
    arms = [Arm(labels[tag], so, data, so.parent) for tag, so in (('L', l_so), ('X', x_so), ('Y', y_so),
                                                                  ('Z', z_so), ('S', s_so))]
    worker.standalone('pin', arms[0])
    for arm in arms[1:]:
        worker.standalone('subject', arm)

    lines = ['=== build identity ===']
    for tag, so in (('L', l_so), ('X', x_so), ('Y', y_so), ('Z', z_so), ('S', s_so)):
        real = identity.realpath(so)
        hooks, needed = elf_hooks_and_needed(so)
        lines.append(f'{tag}: {identity.sha256(real)} {os.path.getsize(real)} B  fixture-hooks: {hooks}  '
                     f'NEEDED: {"".join(name + " " for name in needed)}')
    lines += [f'record X: {RECORD_X_SHA} 1688840 B (docs/findings/perf-steady-cycle-cross-host-2026-09-07.md)',
              'fixture-hook expectation: Z 2, S 0 (the shipped feature compiles them out)']
    bench.tee(out / 'build-identity.txt', '\n'.join(lines) + '\n')

    built = identity.sha256(x_so)
    if built != RECORD_X_SHA:
        emit("FATAL: cell X is not the record's artifact.", error=True)
        emit(f'  built:  {built}', error=True)
        emit(f'  record: {RECORD_X_SHA}', error=True)
        emit('The control cannot proceed: X must reproduce the record\'s build before', error=True)
        emit('any ratio is read. Stop and report.', error=True)
        raise Finished(1)
    emit(f"GATE: X reproduces the record's artifact byte-for-byte ({built})")

    harness = SCRATCH / 'bisect'
    bench.run(bench.harness_build(harness, source=IMAGE_HARNESS_SOURCE))
    emit(f'harness: {identity.sha256(IMAGE_HARNESS_SOURCE)} (record image /repo)')

    speed, load = out / 'speed.jsonl', out / 'load.txt'
    truncate(speed, load)
    # Serial: timed samples, one process at a time (see bench.round_robin).
    for n in sizes:
        emit(f'--- n={n}: {runs} runs x {cycles} cycles x {n} repeats, round-robin L X Y Z S ---')
        bench.append(load, f'before n={n}: {identity.loadavg()}\n'.encode())
        bench.round_robin(runs, [(arm, 'speed') for arm in arms],
                          lambda arm, mode, n=n: worker.sample(arm, mode, speed, out / 'run.err', cycles, repeats=n))
        bench.append(load, f'after  n={n}: {identity.loadavg()}\n'.encode())
    emit(f'speed rows: {speed.read_bytes().count(b"\n")}')

    # In-process bootstrap arithmetic over the finished capture (see shipped_check).
    pairs = (('X', 'L'), ('Y', 'L'), ('Z', 'L'), ('S', 'L'), ('S', 'Z'))
    for n in sizes:
        write_ratios(out / f'ratios-n{n}.md', f'## size n={n} — ratios vs L (perf-ratio.py, seed 20260907)\n',
                     tools, speed, [(labels[numerator], labels[denominator]) for numerator, denominator in pairs], n)
    for n in sizes:
        code, text = bench.report(str(tools / 'bisection/perf-decomp.py'),
                                  [str(speed), '--repeats', n, '--l', labels['L'], '--x', labels['X'],
                                   '--y', labels['Y'], '--z', labels['Z'], '--format', 'md'])
        (out / f'decomp-n{n}.md').write_text(text)
        if code:
            raise Finished(code)

    emit()
    emit(f'=== captures written to {out} ===')
    for name in sorted(os.listdir(out)):
        emit(name)
    return 0


# ── rss-diag ───────────────────────────────────────────────────────────────────

# The shell printed its own header comment as the usage. That text named the script and, through
# an off-by-two `sed -n 2,22p`, stopped at the word "Usage:" before the command; here it names the
# recipe and ends with the command.
RSS_USAGE = """\
# rss-diag — the RSS diagnosis matrix for one backend.
#
# Two cells on ONE data directory, the drop-in configuration:
#   L  the pin-built libpinyin .so
#   S  the oxpinyin C ABI as the shipping recipe produces it
#      (tools/packaging/release-stage.sh, i.e. `--features <backend>,shipped`)
# Both open libpinyin's own install data/, so nothing oxpinyin-generated is
# involved and the two cells differ only in the implementation under test.
#
# Rounds are round-robin (L, S, L, S, ...) rather than blocked, so a drift
# in host state over the capture window lands on both cells equally instead
# of on whichever one ran second.
#
# Two kinds of round are taken, and they must not be mixed:
#   * measurement rounds run with RSS_DIAG_DIR unset. Writing the /proc
#     text dumps allocates, which would move the very RSS the malloc_trim
#     delta is measuring.
#   * one mapping round per cell runs with RSS_DIAG_DIR set, to capture
#     /proc/self/maps and malloc_info for the attribution question.
#
# Usage:
#   just measure rss-diag --lp <libpinyin.so> --ox <oxpinyin.so> --data <dir> \\
#                        [--out DIR] [--rounds N] [--cycles N]
"""


def rss_usage():
    """Print the runner's header comment as its usage, on stderr, and stop with status 2."""
    sys.stderr.write(RSS_USAGE)
    sys.stderr.flush()
    raise Finished(2)


def rss_options(argv: Sequence[str]) -> dict:
    """Parse rss-diag's options as the shell loop did; anything unknown or missing prints the usage."""
    values = dict(lp='', ox='', data='', out='', rounds=10, cycles='8')
    rest = list(argv)

    def operand(flag):
        if len(rest) < 2:
            fail(f'{RSS}: {flag}: option requires an argument')
        return rest[1]
    while rest:
        flag = rest[0]
        if flag in ('--lp', '--ox', '--data', '--out'):
            values[flag[2:]] = operand(flag)
        elif flag == '--rounds':
            value = rest[1] if len(rest) > 1 else ''
            if not re.fullmatch('[0-9]+', value):
                emit(f'--rounds must be a decimal integer: {value}', error=True)
                rss_usage()
            values['rounds'] = int(value)
        elif flag == '--cycles':
            values['cycles'] = operand(flag)
        elif flag in ('-h', '--help'):
            rss_usage()
        else:
            emit(f'unknown argument: {flag}', error=True)
            rss_usage()
        del rest[:2]
    if not (values['lp'] and values['ox'] and values['data']):
        rss_usage()
    return values


def rss_identity(lp: Path, ox: Path, data: str, rounds: int, cycles: str, cpu: str, harness: Path) -> str:
    """The identity of everything the RSS numbers depend on, taken before any of them."""
    lines = [f'captured_utc={bench.utc_now()}', f'kernel={identity.uname("srm")}',
             f'glibc={identity.first_output_line(["ldd", "--version"])}', f'nproc={identity.processors()}',
             f'rounds={rounds} cycles={cycles} cpu={cpu}', f'data_dir={data}']
    # The harness is part of the subject: a capture is not reproducible from the .so hashes alone.
    def git(*arguments):
        return subprocess.run(['git', '-C', str(bench.ROOT), *arguments], capture_output=True, text=True)
    revision = git('rev-parse', 'HEAD')
    clean = git('diff', '--quiet', 'HEAD').returncode == 0
    lines.append('repo_revision=' + (revision.stdout.rstrip('\n') if revision.returncode == 0 else 'unknown')
                 + ('' if clean else ' (working tree dirty)'))
    lines += [f'bisect={harness} sha256={identity.sha256(harness)}',
              f'bisect_c_sha256={identity.sha256(bench.HARNESS_SOURCE)}']
    for so in (lp, ox):
        # `stat -c %s` on the path as given: a symlink reports its own size.
        lines.append(f'so={so} sha256={identity.sha256(so)} size={os.lstat(so).st_size}')
        lines.append(f'  needed={"".join(name + " " for name in identity.needed(so))}')
    return '\n'.join(lines) + '\n'


def rss_diag(argv: Sequence[str], env: Mapping[str, str]) -> int:
    """The RSS diagnosis matrix: two cells on one data directory, then one mapping round per cell."""
    label = RSS
    env = dict(env)
    with bench.working_directory(bench.BISECT):
        options = rss_options(argv)
        lp, ox = Path(options['lp']), Path(options['ox'])
        cpu = setting(env, 'PERF_CPU', '0')
        cycles, rounds = options['cycles'], options['rounds']
        out = Path(options['out'] or '/tmp/rss-diag')
        arms = dict(L=Arm('L', lp, Path(options['data'])), S=Arm('S', ox, Path(options['data'])))
        harness = bench.BISECT / 'bisect'
        worker = measured(env, label, harness, cpu, required=True)
        worker.standalone('pin', arms['L'])
        worker.standalone('subject', arms['S'])
        (out / 'maps').mkdir(parents=True, exist_ok=True)

        # Rebuild when the binary is missing, not executable, or older than its source. A stale binary
        # silently measures a different harness than the tree describes, which is the provenance gap
        # the identity table below exists to close.
        if (not os.access(harness, os.X_OK)
                or bench.HARNESS_SOURCE.stat().st_mtime_ns > harness.stat().st_mtime_ns):
            bench.run(bench.harness_build(harness))

        (out / 'identity.txt').write_text(rss_identity(lp, ox, options['data'], rounds, cycles, cpu, harness))

        def run_one(arm: Arm, number: int) -> str:
            before = identity.loadavg(fields=3)
            done = worker.launch(arm, 'rss-diag', cycles)
            write(done.stderr, error=True)
            if done.status:
                raise Finished(done.status)
            after = identity.loadavg(fields=3)
            result = done.stdout.decode(errors='replace').rstrip('\n')
            return (f'{{"cell":"{arm.label}","round":{number},"loadavg_before":"{before}",'
                    f'"loadavg_after":"{after}","result":{result}}}\n')

        rounds_file = out / 'rounds.jsonl'
        truncate(rounds_file)
        for number in range(1, rounds + 1):
            # Serial: each round is a resident-set sample; L then S, interleaved so drift lands on both.
            for tag in ('L', 'S'):
                bench.append(rounds_file, run_one(arms[tag], number).encode())

        # The mapping round, separate and last: RSS_DIAG_DIR makes bisect write /proc/self/maps,
        # smaps_rollup and malloc_info at each snapshot point.
        for tag in ('L', 'S'):
            extra = dict(RSS_DIAG_DIR=str(out / 'maps'), RSS_DIAG_TAG=tag)
            destination = out / 'maps' / f'{tag}-round.json'
            destination.write_bytes(b'')
            done = worker.launch(arms[tag], 'rss-diag', cycles, extra=extra)
            destination.write_bytes(done.stdout)
            write(done.stderr, error=True)
            if done.status:
                raise Finished(done.status)
        emit(f'wrote {out}')
    return 0


def steady_sample(done, cycles):
    """Read one successful bisect row, requiring a complete steady window."""
    if done.status:
        write(done.stderr, error=True)
        raise Finished(done.status)
    rows = [json.loads(line) for line in done.stdout.decode().splitlines() if line.startswith('{')]
    if len(rows) != 1:
        raise ValueError('steady comparison requires one worker row')
    row = rows[0]
    if (len(row.get('cycles_ns', [])) != cycles
            or any(type(n) is not int or n <= 0 for n in row['cycles_ns'])
            or not re.fullmatch(r'[0-9a-f]{16}', row.get('workload_checksum', ''))):
        raise ValueError('steady comparison has an incomplete window or checksum')
    return row


def steady_summary(times, ir, allocations):
    """Report paired controls and ranges without treating varying Ir as exact."""
    medians = {name: statistics.median(values) for name, values in times.items()}
    base = medians['base']
    return dict(
        wall_ns_per_cycle={name: dict(median=value, minimum=min(times[name]), maximum=max(times[name]),
                                     ratio_to_base=value / base) for name, value in medians.items()},
        ir_per_cycle={name: [min(values), max(values)] for name, values in ir.items()},
        base_minus_pr_ir_bounds=[min(ir['base']) - max(ir['pr__']), max(ir['base']) - min(ir['pr__'])],
        same_bytes_wall_delta_ns={name: medians[control] - medians[name]
                                  for name, control in (('base', 'bctl'), ('pr__', 'pctl'))},
        allocations_per_cycle={name: {metric: [min(row[metric] for row in rows),
                                               max(row[metric] for row in rows)]
                                     for metric in ('count', 'bytes')} for name, rows in allocations.items()})


def steady_comparison_inner(argv: Sequence[str], env: Mapping[str, str]) -> int:
    """Compare supplied base/PR/pin artifacts with the existing steady worker.

    Run through just heavy. All native, allocation and Ir launches are serial;
    the warmup cycle is excluded. Allocation fixtures are never shipping arms.
    """
    from ...bisection import startup_profile
    from ...bisection.teardown import callgrind_costs
    from . import native
    if sys.platform != 'linux':
        fail('steady-comparison requires Linux, ext4 and Valgrind', 2)
    parser = argparse.ArgumentParser(description=steady_comparison_inner.__doc__)
    parser.add_argument('--cell', required=True, choices=('bdb', 'kc', 'tkrzw'))
    for name in ('base', 'pr', 'pin', 'base-alloc', 'pr-alloc', 'system-dir', 'out'):
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--user-dir', type=Path, help='starting profile, copied privately for every launch')
    parser.add_argument('--rounds', type=int, default=20)
    parser.add_argument('--ir-rounds', type=int, default=2)
    parser.add_argument('--cycles', type=int, default=8)
    parser.add_argument('--cpu', type=int, default=min(os.sched_getaffinity(0)))
    args = parser.parse_args(argv)
    if args.rounds < 20 or args.ir_rounds < 2 or args.cycles < 2:
        parser.error('need at least 20 native rounds, 2 Ir rounds and 2 cycles')
    if args.cpu not in os.sched_getaffinity(0):
        parser.error('--cpu is outside this process affinity')
    if args.user_dir and args.out.resolve().is_relative_to(args.user_dir.resolve()):
        parser.error('--out must be outside --user-dir (including the profile directory itself)')
    sources = dict(base=args.base, pr__=args.pr, pin_=args.pin, bctl=args.base, pctl=args.pr)
    fixtures = dict(ball=args.base_alloc, pall=args.pr_alloc)
    for name, path in {**sources, **fixtures}.items():
        if identity.library_backend(path) != args.cell:
            raise ValueError(f'{name}: library backend differs from --cell')
    args.out.mkdir(parents=True)
    native.check_filesystems([args.out, args.system_dir, Path(env.get('TMPDIR') or tempfile.gettempdir()),
                              *sources.values(), *fixtures.values(), *([args.user_dir] if args.user_dir else [])])
    profile_snapshot, profile_identity = None, None
    if args.user_dir:
        profile_snapshot = args.out / 'profile-input'
        shutil.copytree(args.user_dir, profile_snapshot, symlinks=True)
        profile_identity = startup_profile.identity(profile_snapshot)
    arms = {}
    for name, source in {**sources, **fixtures}.items():
        directory = args.out / name
        directory.mkdir()
        library = directory / 'libpinyin.so'
        shutil.copyfile(source, library)
        if identity.sha256(source) != identity.sha256(library):
            raise ValueError(name + ': copy is not byte-identical')
        arms[name] = Arm(name, library, args.system_dir, directory)
    harness = args.out / 'bisect'
    bench.run(bench.harness_build(harness), env=env)
    worker = measured(env, 'steady-comparison', harness, str(args.cpu), cell=args.cell, required=True)
    for name, arm in arms.items():
        worker.standalone('pin' if name == 'pin_' else 'subject', arm)
    metadata = dict(captured_utc=bench.utc_now(), cell=args.cell, cpu=args.cpu, cycles=args.cycles,
                    native_rounds=args.rounds, ir_rounds=args.ir_rounds, system_dir=str(args.system_dir),
                    kernel=identity.uname('srvm'), cpu_model=identity.cpu_model(),
                    user_dir=str(args.user_dir) if args.user_dir else None,
                    user_profile_identity=profile_identity,
                    artifacts={name: dict(path=str(path), sha256=identity.sha256(path))
                               for name, path in {**sources, **fixtures}.items()})
    (args.out / 'environment.json').write_text(json.dumps(metadata, indent=2) + '\n')
    checksums, times, counts = {}, {name: [] for name in sources}, {name: [] for name in sources}
    allocations = {name: [] for name in fixtures}

    def sample(name, prefix=()):
        with tempfile.TemporaryDirectory(prefix='profile-', dir=args.out) as scratch:
            extra = {'PERF_WORKLOAD_CHECKSUM': '1', 'PERF_USER_DIR': ''}
            if profile_snapshot is not None:
                user = Path(scratch) / 'user'
                shutil.copytree(profile_snapshot, user)
                extra['PERF_USER_DIR'] = str(user)
            row = steady_sample(worker.launch(arms[name], 'speed', args.cycles, repeats=1, prefix=prefix,
                                             extra=extra), args.cycles)
        checksum = row['workload_checksum']
        if name in checksums and checksums[name] != checksum:
            raise ValueError(name + ': fresh-process checksum changed')
        checksums[name] = checksum
        return row

    # Rotate equal-length private library paths so drift reaches each arm.
    names = list(sources)
    for number in range(args.rounds):
        emit(f'steady native round {number + 1}/{args.rounds}')
        for name in names[number % len(names):] + names[:number % len(names)]:
            row = sample(name)
            times[name].append(statistics.median(row['cycles_ns'][1:]))
    for number in range(3):
        emit(f'steady allocation round {number + 1}/3')
        for name in list(fixtures)[number % 2:] + list(fixtures)[:number % 2]:
            row = sample(name)
            if row.get('steady_alloc_cycles') != args.cycles - 1:
                raise ValueError(name + ': allocation counters unavailable or incomplete')
            values = {metric: row['steady_alloc_' + metric] / row['steady_alloc_cycles']
                      for metric in ('count', 'bytes')}
            if any(value < 0 for value in values.values()):
                raise ValueError(name + ': negative allocation counters')
            allocations[name].append(values)
    for number in range(args.ir_rounds):
        for name in names[number % len(names):] + names[:number % len(names)]:
            emit(f'steady Ir round {number + 1}/{args.ir_rounds}: {name}')
            profile = args.out / f'{name}.{number}.callgrind'
            sample(name, ['valgrind', '--tool=callgrind', '--collect-atstart=no', '--cache-sim=no',
                          '--branch-sim=no', '--toggle-collect=bisect_perf_steady_unit',
                          '--callgrind-out-file=' + str(profile)])
            parsed = callgrind_costs(profile.read_text(), arms[name].so)
            if set(parsed['totals']) != {'Ir'} or parsed['totals']['Ir'] <= 0:
                raise ValueError(name + ': invalid Callgrind instruction total')
            counts[name].append(parsed['totals']['Ir'] / (args.cycles - 1))
    if len({checksums[name] for name in ('base', 'pr__', 'bctl', 'pctl', 'ball', 'pall')}) != 1:
        raise ValueError('base/PR/control/allocation-fixture checksums differ')
    result = dict(steady_summary(times, counts, allocations), checksums=checksums,
                  pin_checksum_equal=checksums['pin_'] == checksums['base'],
                  user_profile_identity=profile_identity)
    (args.out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    emit(json.dumps(result, indent=2))
    return 0


def steady_comparison(argv: Sequence[str], env: Mapping[str, str]) -> int:
    """Hold heavy admission for the complete serial steady comparison."""
    with slot.cancellation_handlers():
        with slot.HeavyAdmission('steady comparison', dict(env), runner=True) as admission:
            return steady_comparison_inner(argv, admission.env)


RECIPES = {'perf-baseline': perf_baseline, 'perf-matrix': perf_matrix, 'perf-same-data': perf_same_data,
           'shipped-check': shipped_check, 'tree-recipe-control': tree_recipe_control, 'rss-diag': rss_diag,
           'steady-comparison': steady_comparison}
