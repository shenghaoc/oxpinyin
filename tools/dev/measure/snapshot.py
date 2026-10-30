"""Nightly performance snapshot: measure the tree once and emit one JSON sample (#1290).

The recipe behind `python -m tools.dev measure snapshot` and the nightly lane. It
is one half of the series in docs/perf/ci-perf-size-gate-proposal-2026-09-09.md;
tools/perf-gate/series.py is the other and compares this sample with the last.

This is not a gate. It measures and records: no threshold lives here, nothing
fails over a number, and wall clock is absent (the proposal's evidence against
timing in CI stands). The environment travels with the sample, because the series
can localize the night a level moved but cannot say what moved it.

Standard library only. Every external command and host fact goes through `Tools`,
so the order of the run and each metric rule are tested without a toolchain. The
builds take no heavy-slot admission, as the shell script's never did: the lane is
one job on a fresh runner.
"""
import dataclasses
import hashlib
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
from . import bench

# The runner's legacy name: it still labels the one count refusal.
LABEL = 'snapshot.sh'
# Lines 2-19 of the retired tools/perf-gate/snapshot.sh, which `--help` printed: the header,
# its Usage line and the blank line after it. Only the name and the Usage line follow the move to
# `measure snapshot`; the rest is the script's own text.
HELP = '''# snapshot — measure the tree once and emit one perf snapshot as JSON.
#
# One half of the nightly series described in
# docs/perf/ci-perf-size-gate-proposal-2026-09-09.md; tools/perf-gate/series.py
# is the other, and compares this sample against the previous night's.
#
# This is NOT a gate. It measures and records. Nothing here decides anything,
# nothing here fails a build over a number, and no threshold lives in this
# file. Wall clock is deliberately absent: the evidence against timing in CI
# is in the document above and is unchanged by moving to a nightly cadence.
#
# The environment is recorded WITH the sample, because the series can localize
# the night a level moved but cannot say what moved it. Without the
# environment beside each number, a toolchain bump and a large refactor are
# indistinguishable.
#
# Usage: just measure snapshot --out snapshot.json [--backend tkrzw]

'''
# Backend name to the extension of its committed w3 fixture directory.
BACKENDS = {'tkrzw': 'tkt', 'kyotocabinet': 'kct'}
# What the run needs on PATH, checked in this order before anything is built.
REQUIRED_TOOLS = ('cargo', 'gcc', 'readelf', 'strip', 'nm')
INSTALL = [sys.executable, '-X', 'lazy_imports=normal', str(bench.ROOT / 'tools/dev/__main__.py'), 'packaging', 'install']
SHIPPED_NAME = 'libpinyin.so.15.0.0'
# The sections whose sizes make the resolving size measure. Stripped file size is
# page-quantized on these hosts (perf-so-size-2026-09.md measured seven distinct
# probe cdylibs all reporting an identical 266,736 B), so a real few-KiB change can
# land with a zero file-size delta; the section sum does not.
WANTED_SECTIONS = frozenset({'.text', '.rodata', '.data.rel.ro', '.rela.dyn',
                             '.eh_frame', '.eh_frame_hdr', '.gcc_except_table'})
# One `readelf -S -W` row: [Nr] Name Type Address Off Size ...; all three numerics are hex.
# An awk reading was wrong twice over: it split "[ 1] .text" into "[", "1]" and ".text",
# so a name test matched only two-digit indices, and its sixth field was the offset.
SECTION_ROW = re.compile(r'\s*\[\s*\d+\]\s+(\S+)\s+\S+\s+[0-9a-fA-F]+\s+[0-9a-fA-F]+\s+([0-9a-fA-F]+)')
# The oxpinyin_alloc_* readers belong to the alloc-count fixture, never a shipped artifact.
ALLOC_SYMBOL = b'oxpinyin_alloc_'


@dataclasses.dataclass(frozen=True)
class Options:
    """The parsed command line: where to write and which backend."""
    out: str
    backend: str


def parse_options(argv: Sequence[str]) -> Options:
    """Parse the command line in order, as the script's `case` loop did.

    `--out` and `--backend` take the next word (empty when there is none, which
    the later checks then refuse); `-h` prints the header and ends the run with 0.
    """
    out, backend, rest = '', 'tkrzw', list(argv)
    while rest:
        flag = rest.pop(0)
        if flag == '--out':
            out = rest.pop(0) if rest else ''
        elif flag == '--backend':
            backend = rest.pop(0) if rest else ''
        elif flag in ('-h', '--help'):
            sys.stdout.write(HELP)
            sys.stdout.flush()
            raise Finished(0)
        else:
            bench.fail(f'fatal: unknown argument {flag}', 2)
    return Options(out, backend)


def section_sum(readelf_text: str) -> int:
    """The summed size of the wanted sections in `readelf -S -W` output."""
    total = 0
    for line in readelf_text.split('\n'):
        row = SECTION_ROW.match(line)
        if row and row.group(1) in WANTED_SECTIONS:
            total += int(row.group(2), 16)
    return total


def alloc_metrics(row: Mapping) -> dict:
    """Allocations and bytes per steady cycle from one harness row.

    bisect.c emits steady_alloc_{cycles,count,bytes} in speed mode only when the
    oxpinyin_alloc_* readers resolved, so on an artifact built without
    --features alloc-count the keys are absent. Absence is None, and series.py
    refuses to treat it as a measurement.
    """
    cycles = row.get('steady_alloc_cycles', 0)
    if not cycles or 'steady_alloc_count' not in row:
        return {'alloc_count_per_cycle': None, 'alloc_bytes_per_cycle': None}
    return {'alloc_count_per_cycle': row['steady_alloc_count'] // cycles,
            'alloc_bytes_per_cycle': row['steady_alloc_bytes'] // cycles}


def rss_metrics(rows: Sequence[Mapping]) -> dict:
    """Median resident set after the first and the last cycle, in KiB, over the harness rows.

    rss_kib is nested under after_first and after_last. after_first is written
    after cycle 0, a completed cycle and not an init snapshot the speed mode never
    takes, so the fields are named for that. Recorded as a trend, never a trigger.
    """
    def median(tag, key):
        # Absence is not zero. A default of 0 would record a missing measurement as
        # a real one and invent an RSS cliff in the series.
        values = [row[tag][key] for row in rows
                  if isinstance(row.get(tag), dict) and isinstance(row[tag].get(key), (int, float))]
        return int(statistics.median(values)) if values else None
    return {'rss_first_cycle_kib': median('after_first', 'rss_kib'),
            'rss_last_cycle_kib': median('after_last', 'rss_kib')}


def first_row(output: bytes) -> dict:
    """The JSON object on the first line of a harness's output (ValueError when there is none)."""
    return json.loads(output.decode().partition('\n')[0])


def fixture_digest(data: Path) -> str:
    """SHA-256 over every fixture file's name and bytes, in path order."""
    digest = hashlib.sha256()
    for path in sorted(entry for entry in Path(data).rglob('*') if entry.is_file()):
        digest.update(path.name.encode())
        digest.update(path.read_bytes())
    return digest.hexdigest()


def check_provenance(doc: Mapping) -> None:
    """Refuse a sample whose commit or harness commit could not be resolved: it must not enter the series."""
    for field, value in (('commit', doc['commit']),
                         ('harness_commit', doc['environment']['harness_commit'])):
        if value == 'unknown':
            bench.fail(f'fatal: {field} could not be resolved — a sample with no '
                       f'provenance must not enter the series (is this a git '
                       f'checkout, and is safe.directory set?)', 1)


def document(tools: 'Tools', env: Mapping[str, str], *, backend: str, pinned: bool, shipped: Path,
             data: Path, alloc_recipe: str, metrics: Mapping) -> dict:
    """The snapshot: provenance, the environment that produced it, the artifacts and the metrics.

    The environment is recorded with the sample so a move can be read against whether
    the environment moved too; the series cannot attribute cause without it. Nothing
    here is a measurement, so a tool that is missing reads as 'unknown'.
    """
    digest = fixture_digest(data)
    run_id = env.get('GITHUB_RUN_ID')
    return {
        'captured_utc': tools.now(),
        'commit': tools.query('git', 'rev-parse', 'HEAD'),
        'event': env.get('GITHUB_EVENT_NAME', 'local'),
        'run_url': (f'{env["GITHUB_SERVER_URL"]}/{env["GITHUB_REPOSITORY"]}/actions/runs/{env["GITHUB_RUN_ID"]}'
                    if run_id else None),
        'environment': {
            'kernel': tools.query('uname', '-srvm'),
            'runner_image': f'{env.get("ImageOS", "local")}/{env.get("ImageVersion", "local")}',
            'rustc': tools.query('rustc', '-V'),
            'cc': tools.query('cc', '--version'),
            'glibc': tools.query('ldd', '--version'),
            'valgrind': tools.query('valgrind', '--version'),
            'binutils': tools.query('readelf', '--version'),
            'backend': backend,
            'fixture_sha256': digest,
            'cpu_pinned': pinned,
            'harness_commit': tools.query('git', 'log', '-1', '--format=%H', '--',
                                          'tools/bisection/bisect.c', 'tools/perf-gate'),
        },
        'artifacts': {
            'shipped_libpinyin': {
                'recipe': ('just packaging install libpinyin --prefix=/usr --destdir=<stage> -- '
                           f'--no-default-features --features {backend},shipped'),
                'sha256': hashlib.sha256(Path(shipped).read_bytes()).hexdigest(),
                'bytes': os.path.getsize(shipped),
            },
            'alloccount_libpinyin': {'recipe': alloc_recipe},
        },
        'metrics': dict(metrics),
    }


class Tools:
    """The processes and host facts a snapshot depends on; tests replace this class's methods."""

    def which(self, name: str):
        """The path of NAME on PATH, or None."""
        return shutil.which(name)

    def run(self, command: Sequence, **options) -> int:
        """Run COMMAND on the console; a failure ends the run with its status (bench.run)."""
        return bench.run(command, **options)

    def capture(self, command: Sequence, **options):
        """Run COMMAND with its output captured; Completed(status, stdout, stderr) (bench.capture)."""
        return bench.capture(command, **options)

    def query(self, *command: str) -> str:
        """The first line of a command's output, or 'unknown' when it printed nothing or cannot run."""
        try:
            done = subprocess.run(command, capture_output=True, text=True)
        except OSError:
            return 'unknown'
        return done.stdout.strip().split('\n')[0] or 'unknown'


    def object_ir(self, path, want: str):
        """Total self Ir attributed to the object whose name contains WANT, or None."""
        return bench.script('tools/perf-gate/callgrind-ir.py').object_ir(str(path), want)

    def now(self) -> str:
        """The UTC time as `date -u +%Y-%m-%dT%H:%M:%SZ` prints it."""
        return bench.utc_now()


def build_shipped(tools: Tools, env: Mapping[str, str], options: Options, work: Path) -> Path:
    """Install and strip the shipping artifact, then return its path."""
    emit('==> building the shipped artifact (cinstall, the product path)')
    stage = work / 'stage'
    tools.run([*INSTALL, 'libpinyin', '--prefix=/usr', f'--destdir={stage}', '--',
               '--no-default-features', '--features', f'{options.backend},shipped',
               '--target-dir', str(work / 'target-shipped')], env=env, stdout=sys.stderr)
    shipped = bench.find_named(stage, SHIPPED_NAME, any_type=True)
    if shipped is None:
        bench.fail(f'fatal: cinstall produced no {SHIPPED_NAME}', 2)
    tools.run(['strip', '--strip-all', shipped], env=env)
    # The runbook's rule, enforced rather than trusted: the alloc-count readers are
    # never in a shipped artifact. A failed nm proves nothing, so it refuses nothing.
    exports = tools.capture(['nm', '-D', shipped], env=env)
    write(exports.stderr, error=True)
    if exports.status == 0 and ALLOC_SYMBOL in exports.stdout:
        bench.fail('fatal: the shipped artifact exports oxpinyin_alloc_* symbols', 2)
    return shipped


def build_fixture(tools: Tools, env: Mapping[str, str], backend: str, work: Path) -> tuple[Path, str]:
    """Build the alloc-count cdylib, a fixture nothing installs; return it and the recipe that built it."""
    emit('==> building the alloc-count artifact (a fixture; nothing installs it)')
    recipe = ('cargo build --locked --release -p oxpinyin-capi --no-default-features '
              f'--features {backend},alloc-count')
    tools.run(recipe.split(), env=dict(env, CARGO_TARGET_DIR=str(work / 'target-alloc')), stdout=sys.stderr)
    cdylib = work / 'target-alloc/release/libpinyin_capi.so'
    if not cdylib.is_file():
        bench.fail(f'fatal: no alloc-count cdylib at {cdylib}', 2)
    return cdylib, recipe


def measure(tools: Tools, env: Mapping[str, str], *, shipped: Path, fixture: Path, harness: Path, data: Path,
            work: Path, pin: Sequence[str], cycles: str, callgrind_cycles: str, rss_processes: int) -> dict:
    """Size, allocations, resident set and instruction count of the built artifacts."""
    def gate(library, count, profiler=()):
        """One pinned harness process on LIBRARY; stderr is discarded, as `2>/dev/null` did."""
        environment = dict(env, PERF_BACKEND='gate', PERF_MODE='speed', PERF_CYCLES=count)
        return tools.capture([*pin, *profiler, str(harness), '--perf', str(library), str(data)],
                             env=environment)

    # Serial: the alloc, RSS and Callgrind passes all run one artifact pinned to the one
    # PERF_CPU, and the RSS processes are resident-set samples that need a host otherwise
    # quiet. None of them is an independent campaign.
    readelf = tools.capture(['readelf', '-S', '-W', shipped], env=env)
    write(readelf.stderr, error=True)
    metrics = {'section_sum': section_sum(readelf.stdout.decode())}
    if readelf.status:
        raise Finished(readelf.status)
    metrics['stripped_size'] = os.path.getsize(shipped)

    done = gate(fixture, cycles)
    metrics.update(alloc_metrics(first_row(done.stdout)))
    if done.status:
        raise Finished(done.status)

    # Serial for the same reason, one process per sample. A failed process does not stop
    # the loop, only the last one's status counts, as in the script's pipeline.
    outputs, last = [], 0
    for _ in range(rss_processes):
        done = gate(shipped, cycles)
        outputs.append(done.stdout)
        last = done.status
    rows = [json.loads(line) for line in b''.join(outputs).decode().split('\n') if line.strip()]
    metrics.update(rss_metrics(rows))
    if last:
        raise Finished(last)

    # Optional by design: valgrind is not on every image, and a missing instrument
    # reports None rather than zero. Attributed to the oxpinyin object only, so glibc
    # and libtkrzw cannot move the number or mask a move in ours.
    metrics['ir_oxpinyin_object'] = None
    if tools.which('valgrind'):
        callgrind = work / 'callgrind.out'
        done = gate(shipped, callgrind_cycles, [
            'valgrind', '--tool=callgrind', '--collect-atstart=no',
            '--toggle-collect=bisect_perf_steady_unit', f'--callgrind-out-file={callgrind}'])
        if done.status == 0:
            try:
                metrics['ir_oxpinyin_object'] = tools.object_ir(callgrind, 'libpinyin')
            except OSError as error:
                bench.fail(f'callgrind-ir: {error}', 1)
    return metrics


def snapshot(argv: Sequence[str], env: Mapping[str, str], *, tools: Tools | None = None,
             root: Path = bench.ROOT) -> int:
    """Build the shipped artifact and the alloc-count fixture, measure them, and write the sample."""
    tools = tools or Tools()
    options = parse_options(argv)
    if not options.out:
        bench.fail('fatal: --out is required', 2)
    if options.backend not in BACKENDS:
        bench.fail(f"fatal: unsupported backend '{options.backend}'", 2)
    cpu = bench.setting(env, 'PERF_CPU', '0')
    with bench.working_directory(root):
        data = root / 'fixtures/w3' / BACKENDS[options.backend]
        if not data.is_dir():
            bench.fail(f'fatal: fixture dir {data} missing', 2)
        for tool in REQUIRED_TOOLS:
            if not tools.which(tool):
                bench.fail(f'fatal: {tool} not found', 2)
        # taskset pins the measured processes when it is available. It is not on every
        # image, and its absence must not silently change what is measured without
        # saying so, so it is recorded in the sample.
        pin = bench.pinned([], cpu, required=False, which=tools.which)
        # A count is a plain decimal. The script's arithmetic loop evaluated any other text
        # as an expression, and only after both builds; refuse it before the first build.
        rss_processes = bench.count(LABEL, env, 'PERF_GATE_RSS_PROCESSES', '5')
        with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as name:
            work = Path(name)
            shipped = build_shipped(tools, env, options, work)
            fixture, alloc_recipe = build_fixture(tools, env, options.backend, work)
            harness = work / 'bisect'
            tools.run(bench.harness_build(harness, source=Path('tools/bisection/bisect.c'),
                                          flags=('-std=gnu11', '-O2')), env=env)
            emit('==> measuring')
            metrics = measure(tools, env, shipped=shipped, fixture=fixture, harness=harness, data=data,
                              work=work, pin=pin, cycles=bench.setting(env, 'PERF_GATE_CYCLES', '8'),
                              callgrind_cycles=bench.setting(env, 'PERF_GATE_CG_CYCLES', '4'),
                              rss_processes=rss_processes)
            doc = document(tools, env, backend=options.backend, pinned=bool(pin), shipped=shipped, data=data,
                           alloc_recipe=alloc_recipe, metrics=metrics)
            check_provenance(doc)
            with open(options.out, 'w') as stream:
                json.dump(doc, stream, indent=2, sort_keys=True)
                stream.write('\n')
            emit(f'wrote {options.out}')
    return 0


RECIPES = {'snapshot': snapshot}
