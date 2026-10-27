"""The W8 keystroke-cycle profile, `just profile-w8` (#1290).

Stage the installed capi with cargo-c's `profiling` profile (release, line tables,
thin LTO), build the `bisect --perf` harness with debug info, and record one
keystroke cycle against it with the best profiler the host has. Linux prefers
`samply record` and falls back to `flamegraph`, then `perf record`, then Valgrind
Callgrind. macOS requires samply: taskset/PERF_CPU, perf and its
perf_event_paranoid hint are Linux-only, and the Linux fallbacks are not tried.
Artifacts land under target/profile/.

When samply supports `--unstable-presymbolicate`, the record also writes
`<profile>.syms.json` and extract-hot-stacks.py reports function names. A samply
without that flag still records; the report shows unresolved frames instead.

After a samply record the run prints the profile's sample total and warns when it
is below the level extract-hot-stacks.py defines (MIN_SAMPLES): with fewer samples
the order of the hot stacks is mostly sampling noise. The other profilers write no
samply profile, print no total and keep their shorter default run.

Requirements: exported tables (PINYIN_EXPORT_DIR or /tmp/oxpinyin-export), the
pinned model20 cache (tools/model/fetch-model.sh), cargo-c, and on macOS samply
and OXPINYIN_TKRZW_PREFIX or OXPINYIN_TKRZW_LIBRARY, so the libtkrzw check can
verify the staged dylib.

Environment:
  PINYIN_EXPORT_DIR     tkrzw export (default /tmp/oxpinyin-export); produce it
                        with `--features tkrzw` (the oracle this cycle compares
                        against is tkrzw)
  PINYIN_MODEL_DIR      extracted model20 (default target/model20/extracted)
  PERF_CYCLES           bisect cycles for every profiler. Unset, samply runs 2048,
                        which gave about three times MIN_SAMPLES samples on the
                        measuring host (a faster host takes fewer), and the other
                        profilers run 32, as before
  PERF_CPU              taskset CPU (Linux only; refused on macOS)
  PROFILE_REUSE_STAGE=1 reuse a previous cargo-c staging tree
  OXPINYIN_PROFILE_OS   test override for the host name; native runs use uname
  OXPINYIN_TKRZW_PREFIX Homebrew prefix for libtkrzw on macOS

The recipe holds the heavy slot (`just profile-w8` runs it under `just heavy`), so
nothing here asks for admission again. Standard library only.
"""
import glob
import hashlib
import os
from pathlib import Path
import platform
import select
import shutil
import subprocess
import sys
import traceback
from collections.abc import Callable, Mapping, Sequence

from .. import slot
from ..runners import Finished, emit, status as shell_status, write
from . import bench

# The runner name the libtkrzw refusal has always carried.
LABEL = 'run-w8-cycle.sh'
HERE = bench.ROOT / 'tools/profile'
# The W8 cycle profiles against the tkrzw cell (its exports and records are
# tkrzw). build-oracle.sh defaults to Berkeley DB since 2026-09-27, so build this
# prefix with `build-oracle.sh --dbm tkrzw`.
PIN_REF = ('libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99'
           '+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw')
MODEL20_SHA256 = '59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155'
EXPECTED_MODEL_FILES = (
    'art.table', 'culture.table', 'economy.table', 'gb_char.table', 'gbk_char.table',
    'geology.table', 'history.table', 'interpolation2.text', 'life.table', 'merged.table',
    'nature.table', 'opengram.table', 'people.table', 'punct.table', 'science.table',
    'society.table', 'sport.table', 'technology.table')
# A sampling profile ranks stacks by sample count, and samply takes about one
# sample per CPU millisecond, so its default run is long enough to reach the level
# extract-hot-stacks.py defines (MIN_SAMPLES). The other profilers keep the shorter
# default: Callgrind counts instructions exactly, and perf record keeps a stack
# dump for every sample.
SAMPLY_CYCLES = '2048'
FALLBACK_CYCLES = '32'
CHUNK = 1 << 16


def say(text: str) -> None:
    """One line on stdout."""
    emit(text)


def warn(text: str) -> None:
    """One line on stderr."""
    emit(text, error=True)


def die(*lines: str, code: int = 1):
    """Stop with LINES on stderr and CODE as the exit status."""
    for line in lines:
        warn(line)
    raise Finished(code)


def head(data: bytes, lines: int) -> bytes:
    """The first LINES lines of DATA, as `head -n LINES` prints them."""
    end = -1
    for _ in range(lines):
        end = data.find(b'\n', end + 1)
        if end < 0:
            return data
    return data[:end + 1]


def remove_tree(path: str) -> None:
    """`rm -rf PATH`: a link or file goes, a directory goes with its contents, a missing path is fine."""
    target = Path(path)
    if target.is_symlink() or target.is_file():
        target.unlink()
    elif target.is_dir():
        shutil.rmtree(target)


def file_sha256(path: Path) -> str:
    """The SHA-256 of a file, in hex."""
    digest = hashlib.sha256()
    with open(path, 'rb') as stream:
        for block in iter(lambda: stream.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


class Commands:
    """How a profile run starts programs. A test substitutes its own.

    Every program runs in its own process group, polled for cancellation and
    reaped, with its descendants, before an error propagates. A program that does
    not exist raises FileNotFoundError, which the shared failure ladder reports.
    """

    def probe(self, command: Sequence, env: Mapping[str, str]) -> tuple[int, str]:
        """Run COMMAND quietly: (status, its stdout and stderr as text). A program that will not start is 127."""
        sys.stdout.flush()
        sys.stderr.flush()
        try:
            done = subprocess.run([str(part) for part in command], env=dict(env), stdin=subprocess.DEVNULL,
                                  stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
        except OSError:
            return 127, ''
        return shell_status(done.returncode), os.fsdecode(done.stdout)

    def run(self, command: Sequence, env: Mapping[str, str]) -> int:
        """Run COMMAND on the console and return its status."""
        return bench.run(command, env=env, check_status=False)

    def output(self, command: Sequence, env: Mapping[str, str]) -> tuple[int, bytes]:
        """Run COMMAND with its stdout returned and its stderr on the console: (status, stdout)."""
        chunks = []
        return self.stream(command, env, chunks.append), b''.join(chunks)

    def tee(self, command: Sequence, env: Mapping[str, str], destination: str) -> int:
        """Run COMMAND with its stdout on the console and in DESTINATION, as `| tee` does; its status.

        The file is opened first, so it exists (empty) even when the program never starts.
        """
        with open(destination, 'wb') as saved:
            def both(chunk: bytes) -> None:
                write(chunk)
                saved.write(chunk)
                saved.flush()
            return self.stream(command, env, both)

    def pipe(self, first: Sequence, second: Sequence, env: Mapping[str, str], destination: str) -> int:
        """Run FIRST | SECOND > DESTINATION and return the rightmost nonzero status (the shell's pipefail), else 0."""
        first, second = [str(part) for part in first], [str(part) for part in second]
        sys.stdout.flush()
        sys.stderr.flush()
        with open(destination, 'wb') as sink:
            with subprocess.Popen(first, env=dict(env), stdout=subprocess.PIPE, start_new_session=True) as producer:
                try:
                    consumer = subprocess.Popen(second, env=dict(env), stdin=producer.stdout, stdout=sink,
                                                start_new_session=True)
                except BaseException:
                    slot.terminate_group(producer)
                    raise
                # The consumer owns the read end now; closing ours lets the producer see it go.
                producer.stdout.close()
                with consumer:
                    try:
                        right = shell_status(slot.wait_child(consumer))
                    except BaseException:
                        slot.terminate_group(producer)
                        raise
                    left = shell_status(slot.wait_child(producer))
        return right or left

    @staticmethod
    def stream(command: Sequence, env: Mapping[str, str], consume: Callable[[bytes], None]) -> int:
        """Run COMMAND, hand its stdout to CONSUME as it arrives, leave its stderr on the console; its status."""
        command = [str(part) for part in command]
        sys.stdout.flush()
        sys.stderr.flush()
        with subprocess.Popen(command, env=dict(env), stdout=subprocess.PIPE, start_new_session=True) as process:
            try:
                while True:
                    slot.check_cancelled()
                    if select.select([process.stdout], [], [], 0.1)[0]:
                        chunk = os.read(process.stdout.fileno(), CHUNK)
                        if not chunk:
                            break
                        consume(chunk)
            except BaseException:
                slot.terminate_group(process)
                raise
            return shell_status(slot.wait_child(process))

    def guard(self, obj: str, env: Mapping[str, str]) -> None:
        """Refuse OBJ unless its libtkrzw is the verified runtime (tools/tkrzw/runtime.py)."""
        bench.runtime_check(LABEL, 'auto', 'subject', obj, env=env)


class W8:
    """One W8 profile run: the settings the script read from its environment, then its steps in its order.

    SYSTEM is the host name (default: OXPINYIN_PROFILE_OS, else uname -s); WHICH finds
    a program on PATH; COMMANDS starts programs. Each defaults to the real thing, and a
    test replaces them to drive the Darwin and Linux paths from either OS. ENV is copied:
    what the script exported (RUSTFLAGS, PKG_CONFIG_PATH, LD_LIBRARY_PATH, PERF_*) is
    added to the copy and reaches every program started after it.
    """

    def __init__(self, argv: Sequence[str], env: Mapping[str, str], system: str | None = None,
                 which: Callable[[str], str | None] | None = None, commands: Commands | None = None):
        self.argv = list(argv)
        self.env = dict(env)
        self.commands = commands or Commands()
        self.system = system or self.env.get('OXPINYIN_PROFILE_OS') or platform.system()
        path = self.env.get('PATH')
        self.which = which or (lambda name: shutil.which(name, path=path))
        self.darwin = self.system == 'Darwin'

    # ── settings ──────────────────────────────────────────────────────────

    def settings(self) -> None:
        """Read the environment into the strings the script's variables held (paths keep their spelling)."""
        env = self.env
        self.profile_dir = env.get('OXPINYIN_PROFILE_DIR') or f'{bench.ROOT}/target/profile'
        self.stage = f'{self.profile_dir}/stage'
        self.cycles = env.get('PERF_CYCLES') or FALLBACK_CYCLES
        self.samply_cycles = env.get('PERF_CYCLES') or SAMPLY_CYCLES
        if self.darwin or self.samply_cycles == self.cycles:
            self.cycles_note = self.samply_cycles
        else:
            self.cycles_note = f'{self.samply_cycles} (samply), {self.cycles} (other profilers)'
        self.export_dir = env.get('PINYIN_EXPORT_DIR') or '/tmp/oxpinyin-export'
        if env.get('PINYIN_MODEL_DIR'):
            self.model_dir = env['PINYIN_MODEL_DIR']
        elif env.get('PINYIN_MODEL_CACHE'):
            self.model_dir = env['PINYIN_MODEL_CACHE'] + '/extracted'
        else:
            self.model_dir = f'{bench.ROOT}/target/model20/extracted'

    def samply_has_presymbolicate(self) -> bool:
        """The flag is unstable and absent from older samply releases; probe the installed help text."""
        if not self.which('samply'):
            return False
        _, text = self.commands.probe(['samply', 'record', '--help'], self.env)
        return '--unstable-presymbolicate' in text

    # ── the run ───────────────────────────────────────────────────────────

    def execute(self) -> None:
        """Run the profile, ending with a Finished status when it stops early."""
        self.settings()
        env = self.env
        if self.darwin and env.get('PERF_CPU'):
            die('fatal: PERF_CPU is unsupported on macOS: Mach affinity is a cache hint, '
                'not CPU pinning; unset PERF_CPU.', code=2)
        if self.darwin and not self.which('samply'):
            die('fatal: samply is required on macOS; install it (for example: cargo install samply) '
                'or put it on PATH.')
        presymbolicate = self.samply_has_presymbolicate()
        if self.which('samply') and not presymbolicate:
            warn('warning: samply has no --unstable-presymbolicate; the hot-stack report '
                 'will show unresolved frames.')
        if self.argv[:1] == ['--check']:
            self.check(presymbolicate)
            return
        if self.darwin and self.commands.probe(['cargo', 'cinstall', '--version'], env)[0]:
            die('fatal: cargo-c is required on macOS; install cargo-cinstall or put it on PATH.')
        if self.darwin and not env.get('OXPINYIN_TKRZW_PREFIX') and not env.get('OXPINYIN_TKRZW_LIBRARY'):
            die('fatal: set OXPINYIN_TKRZW_PREFIX or OXPINYIN_TKRZW_LIBRARY so runtime.sh '
                'can verify libtkrzw.')
        Path(self.profile_dir).mkdir(parents=True, exist_ok=True)
        self.inputs()
        say('oxpinyin Stage-2 W8-cycle profile')
        say(f'  pin: {PIN_REF}')
        say(f'  model20 SHA-256: {MODEL20_SHA256}')
        say(f'  model dir: {self.model_dir}')
        say(f'  artifacts: {self.profile_dir}')
        say(f'  cycles: {self.cycles_note}')
        self.install()
        self.stage_data()
        self.harness()
        self.record(presymbolicate)

    def check(self, presymbolicate: bool) -> None:
        """`--check`: report the profiler and cycle qualification and stop before any input is read."""
        say(f'samply cycles: {self.samply_cycles}')
        if self.which('samply'):
            say('samply presymbolicate: ' + ('available' if presymbolicate else 'unavailable'))
        if self.darwin:
            say('profile platform: Darwin; profiler=samply')
            warn('note: taskset and PERF_CPU are unsupported on macOS: Mach affinity is a cache hint, '
                 'not CPU pinning')
            warn('note: perf record and the perf_event_paranoid hint are Linux-only; skipped on macOS')
        else:
            say(f'profile platform: {self.system}; profiler fallback chain: samply, flamegraph, perf, valgrind')
            say(f'fallback cycles: {self.cycles}')

    # ── inputs ────────────────────────────────────────────────────────────

    def inputs(self) -> None:
        """Require a tkrzw export, its tables and the pinned model before anything is built."""
        # All three backend flavours name their tables identically, so only the
        # manifest's backend token distinguishes them; the cycle's oracle is tkrzw.
        manifest = f'{self.export_dir}/datagen-manifest.txt'
        try:
            tokens = Path(manifest).read_bytes().split(b'\n')
        except OSError as error:
            # grep named the file it could not read before the verdict below.
            warn(f'grep: {manifest}: {error.strerror}')
            tokens = []
        if b'backend=tkt' not in tokens:
            die(f'fatal: {self.export_dir} is not a tkrzw export; rebuild it with --features tkrzw', code=2)
        for name in ('pinyin_index.bin', 'phrase_index.bin', 'bigram.db'):
            if not Path(f'{self.export_dir}/{name}').is_file():
                die(f'fatal: exported table missing at {self.export_dir}/{name}')
        self.verify_pinned_model()

    @staticmethod
    def marker_matches(marker: Path) -> bool:
        """`grep -qx "sha256=DIGEST" MARKER`: some whole line is the pinned digest.

        A marker that cannot be read is named as grep names it, and is no match.
        """
        try:
            return f'sha256={MODEL20_SHA256}'.encode() in marker.read_bytes().split(b'\n')
        except OSError as error:
            warn(f'grep: {marker}: {error.strerror}')
            return False

    def verify_pinned_model(self) -> None:
        """The extracted tree is the pin only when fetch-model.sh's `verified` marker matches.

        The sibling archive's digest is the other proof. Either one makes the resolved directory the
        model directory for the rest of the run.
        """
        directory = self.model_dir
        fetch = '  run: tools/model/fetch-model.sh'
        if not Path(directory).is_dir():
            die(f'fatal: model dir is not a directory: {directory}', fetch)
        for name in EXPECTED_MODEL_FILES:
            if not Path(f'{directory}/{name}').is_file():
                die(f'fatal: model dir {directory} is missing {name}', fetch)
        resolved = os.path.realpath(directory)
        cache_root = os.path.dirname(resolved)
        marker = Path(cache_root, 'verified')
        if marker.is_file() and self.marker_matches(marker):
            self.model_dir = resolved
            return
        archive = Path(cache_root, 'downloads/model20.text.tar.gz')
        if archive.is_file():
            digest = file_sha256(archive)
            if digest == MODEL20_SHA256:
                self.model_dir = resolved
                return
            die(f'fatal: archive {archive} is not the pinned model20',
                f'  expected: {MODEL20_SHA256}', f'  actual:   {digest}')
        die(f'fatal: {directory} is not a verified model20 cache (no sha256={MODEL20_SHA256} marker)', fetch)

    # ── cargo-c install with the profiling profile ────────────────────────

    def libdir(self, package: str) -> str:
        """`pkg-config --variable=libdir PACKAGE`; a failure ends the run with its status."""
        status, out = self.commands.output(['pkg-config', '--variable=libdir', package], self.env)
        if status:
            raise Finished(status)
        return os.fsdecode(out).rstrip('\n')

    def cinstall(self, command: list[str]) -> None:
        """Stage the capi: `cargo cinstall` with the profiling profile into a fresh STAGE."""
        say(f'--- cargo cinstall --profile profiling -> {self.stage} ---')
        remove_tree(self.stage)
        status = self.commands.run(command, self.env)
        if status:
            raise Finished(status)

    def install(self) -> None:
        """Stage the installed tree (or reuse the last one) and find the capi library in it."""
        stage, env = self.stage, self.env
        reuse = (env.get('PROFILE_REUSE_STAGE') or '0') == '1'
        install = ['cargo', 'cinstall', '--locked', '--profile', 'profiling']
        tail = ['-p', 'oxpinyin-capi', f'--destdir={stage}', '--prefix=/usr',
                '--manifest-path', str(bench.ROOT / 'Cargo.toml')]
        previous = env.get('PKG_CONFIG_PATH')
        inherited = f':{previous}' if previous else ''
        if self.darwin:
            ready = (Path(f'{stage}/usr/lib/pkgconfig/libpinyin.pc').is_file()
                     and bool(glob.glob(f'{glob.escape(stage)}/usr/lib/libpinyin*.dylib')) and reuse)
            if ready:
                say(f'--- reusing stage: {stage} ---')
            else:
                # Homebrew's native libraries are outside the shared cache; opt the
                # profiling dylib out so the linker can bind them. The export stays
                # for the rest of the run, as it did in the script.
                flags = env.get('RUSTFLAGS')
                env['RUSTFLAGS'] = (f'{flags} ' if flags else '') + '-C link-arg=-Wl,-not_for_dyld_shared_cache'
                self.cinstall([*install, '--no-default-features', '--features', 'tkrzw', *tail])
            env['PKG_CONFIG_PATH'] = f'{stage}/usr/lib/pkgconfig:{stage}/usr/lib64/pkgconfig{inherited}'
            env['PKG_CONFIG_SYSROOT_DIR'] = stage
            self.capi_libdir = self.libdir('libpinyin') or self.libdir('oxpinyin')
            self.capi_so = ''
            for candidate in ('libpinyin.dylib', 'libpinyin.15.dylib', 'libpinyin.15.0.0.dylib',
                              'libpinyin_capi.dylib'):
                if Path(f'{self.capi_libdir}/{candidate}').is_file():
                    self.capi_so = f'{self.capi_libdir}/{candidate}'
                    break
        else:
            ready = (Path(f'{stage}/usr/lib64/pkgconfig/oxpinyin.pc').is_file()
                     and Path(f'{stage}/usr/lib64/libpinyin_capi.so').is_file() and reuse)
            if ready:
                say(f'--- reusing stage: {stage} ---')
            else:
                self.cinstall([*install, *tail])
            env['PKG_CONFIG_PATH'] = f'{stage}/usr/lib64/pkgconfig{inherited}'
            env['PKG_CONFIG_SYSROOT_DIR'] = stage
            self.capi_libdir = self.libdir('oxpinyin')
            self.capi_so = f'{self.capi_libdir}/libpinyin_capi.so'
        self.capi_data = f'{stage}/usr/share/oxpinyin'
        if not Path(self.capi_so).is_file():
            die(f'fatal: {self.capi_so} not found')

    def stage_data(self) -> None:
        """cargo-c does not install data files; this is the documented packager step."""
        export, data = self.export_dir, Path(self.capi_data)
        data.mkdir(parents=True, exist_ok=True)
        if self.darwin:
            # The native engine resolves the remaining character, merged and table
            # files from the same directory; copy the export's data set, not only the
            # three files the validation above requires.
            sources = sorted(glob.glob(f'{glob.escape(export)}/*.bin')) + [f'{export}/bigram.db']
            if Path(f'{export}/table.conf').is_file():
                sources.append(f'{export}/table.conf')
        else:
            sources = [f'{export}/pinyin_index.bin', f'{export}/phrase_index.bin', f'{export}/bigram.db']
        for source in sources:
            bench.copy_file(source, data / Path(source).name)
        bench.copy_file(f'{self.model_dir}/interpolation2.text', data / 'interpolation2.text')
        say(('installed capi dylib: ' if self.darwin else 'installed capi .so:  ') + self.capi_so)
        say(f'installed capi data: {self.capi_data}')

    # ── harness ───────────────────────────────────────────────────────────

    def harness(self) -> None:
        """Build `bisect --perf` with debug info, then export what the profiled worker reads."""
        say('--- building bisect --perf harness ---')
        flags = (*bench.STRICT_FLAGS, '-g')
        if self.darwin:
            build = bench.harness_build(f'{self.profile_dir}/bisect', compiler='cc', flags=flags, libraries=())
        else:
            build = bench.harness_build(f'{self.profile_dir}/bisect', compiler='gcc', flags=flags)
        status = self.commands.run(build, self.env)
        if status:
            raise Finished(status)
        if not self.darwin:
            self.env['LD_LIBRARY_PATH'] = bench.loader_path(self.capi_libdir, self.env.get('LD_LIBRARY_PATH'))
        self.env.update(PERF_BACKEND='oxpinyin', PERF_MODE='speed', PERF_CYCLES=self.cycles)

    def pinned(self, command: Sequence) -> list[str]:
        """Pin COMMAND to PERF_CPU with taskset: Linux only, only when a CPU was asked for and taskset exists."""
        cpu = self.env.get('PERF_CPU')
        if not self.darwin and cpu and self.which('taskset'):
            return ['taskset', '-c', cpu, *map(str, command)]
        return [str(part) for part in command]

    # ── record ────────────────────────────────────────────────────────────

    def report_samples(self, profile_json: str) -> None:
        """Print the samply profile's sample total; extract-hot-stacks.py warns when it is below its level.

        A profile it cannot read costs a warning, not the run: samply has already written it and the
        timing is saved.
        """
        try:
            status = bench.script('tools/profile/extract-hot-stacks.py').main(['--total', profile_json])
        except Exception:
            traceback.print_exc()
            status = 1
        if status:
            warn(f'warning: could not read the sample total from {profile_json}')

    def record(self, presymbolicate: bool) -> None:
        """Check the subject, write the header, and record with the first profiler that works."""
        profile_dir, env = self.profile_dir, self.env
        # Check the dlopened subject before the profiler starts, then profile the worker.
        self.commands.guard(self.capi_so, env)
        cycle_command = [f'{profile_dir}/bisect', '--perf', self.capi_so, self.capi_data]
        timing_json = f'{profile_dir}/w8-cycle-timing.json'
        profile_json = f'{profile_dir}/w8-cycle.profile.json.gz'
        profile_syms = profile_json[:profile_json.rindex('.')] + '.syms.json'
        flamegraph_svg = f'{profile_dir}/w8-cycle.svg'
        perf_data = f'{profile_dir}/perf.data'
        header = f'{profile_dir}/header.txt'
        Path(header).write_text(
            f'pin: {PIN_REF}\nmodel20_sha256: {MODEL20_SHA256}\nso: {self.capi_so}\ndata: {self.capi_data}\n'
            f'cycles: {self.cycles_note}\nprofile: profiling (release + line-tables-only + thin LTO)\n')
        recorded = False
        # Never pair a fresh profile with a sidecar from an earlier run.
        for stale in (profile_json, profile_syms):
            Path(stale).unlink(missing_ok=True)
        # The profilers form a fallback chain: each runs only if its predecessors failed or are missing, so
        # there is nothing to run at once. Sampling and timing need the host to themselves in any case, and
        # every attempt writes the same timing file.
        if self.which('samply'):
            say('--- samply record (preferred) ---')
            arguments = ['record', '--save-only', *(['--unstable-presymbolicate'] if presymbolicate else [])]
            status = self.commands.tee(
                self.pinned(['samply', *arguments, '-o', profile_json, '--', *cycle_command]),
                {**env, 'PERF_CYCLES': self.samply_cycles}, timing_json)
            if status == 0:
                say(f'samply profile: {profile_json}')
                if Path(profile_syms).is_file():
                    say(f'samply symbols: {profile_syms}')
                else:
                    warn('warning: samply wrote no symbol sidecar; the hot-stack report will show unresolved frames.')
                self.report_samples(profile_json)
                recorded = True
            elif self.darwin:
                die('error: samply failed; macOS has no perf or Callgrind fallback.')
            else:
                warn('warning: samply failed (often /proc/sys/kernel/perf_event_paranoid > 1); falling back')
        if self.darwin:
            warn('note: perf record and the perf_event_paranoid hint are Linux-only; skipped on macOS')
            warn('note: flamegraph and Valgrind/Callgrind fallbacks are not used on macOS; samply is required')
        if not self.darwin and not recorded and self.which('flamegraph'):
            say('--- flamegraph wrapper (cargo-flamegraph) ---')
            status = self.commands.tee(self.pinned(['flamegraph', '-o', flamegraph_svg, '--', *cycle_command]),
                                       env, timing_json)
            if status == 0:
                say(f'flamegraph: {flamegraph_svg}')
                recorded = True
            else:
                warn('warning: flamegraph failed; falling back')
        if not self.darwin and not recorded and self.which('perf'):
            say('--- perf record ---')
            status = self.commands.tee(
                self.pinned(['perf', 'record', '--call-graph', 'dwarf', '-F', '999', '-o', perf_data, '--',
                             *cycle_command]), env, timing_json)
            if status == 0:
                # `perf report | head -n 80 > perf-report.txt || true`: a failure costs the report only.
                _, report = self.commands.output(['perf', 'report', '--stdio', '-i', perf_data, '--no-children',
                                                  '--sort', 'overhead'], env)
                Path(f'{profile_dir}/perf-report.txt').write_bytes(head(report, 80))
                if self.which('inferno-flamegraph'):
                    status = self.commands.pipe(['perf', 'script', '-i', perf_data], ['inferno-flamegraph'],
                                                env, flamegraph_svg)
                    if status:
                        raise Finished(status)
                    say(f'flamegraph: {flamegraph_svg}')
                say(f'perf data: {perf_data}')
                recorded = True
            else:
                warn('warning: perf record failed; falling back')
        if not self.darwin and not recorded and self.which('valgrind'):
            say('--- valgrind callgrind (no perf_events needed) ---')
            callgrind_out = f'{profile_dir}/callgrind.out'
            status = self.commands.tee(
                self.pinned(['valgrind', '--tool=callgrind', '--error-exitcode=0', '--trace-children=no',
                             f'--callgrind-out-file={callgrind_out}', *cycle_command]), env, timing_json)
            if status:
                raise Finished(status)
            if self.which('callgrind_annotate') and self.nonempty(callgrind_out):
                annotation = f'{profile_dir}/callgrind-annotate.txt'
                # `callgrind_annotate … > annotation || true`: a failure costs the annotation only.
                _, annotated = self.commands.output(['callgrind_annotate', '--auto=no', '--inclusive=no',
                                                     '--threshold=1', callgrind_out], env)
                Path(annotation).write_bytes(annotated)
                say(f'callgrind annotate: {annotation}')
            say(f'callgrind: {callgrind_out}')
            recorded = True
        if not recorded:
            warn('warning: samply, flamegraph, perf, and valgrind all missing; writing timing only')
            status = self.commands.tee(self.pinned(cycle_command), env, timing_json)
            if status:
                raise Finished(status)
        say(f'timing: {timing_json}')
        say(f'header: {header}')
        say('done.')

    @staticmethod
    def nonempty(path: str) -> bool:
        """`[ -s PATH ]`: the file exists and has a byte in it."""
        try:
            return os.path.getsize(path) > 0
        except OSError:
            return False


def profile_w8(argv: Sequence[str], env: Mapping[str, str], *, system: str | None = None,
               which: Callable[[str], str | None] | None = None, commands: Commands | None = None) -> int:
    """Run the W8 profile (or `--check` it) and return its status.

    The script `cd`ed into its own directory first, so a relative path in the
    environment resolves against tools/profile.
    """
    with bench.working_directory(HERE):
        W8(argv, dict(env), system, which, commands).execute()
    return 0


RECIPES = {'profile-w8': profile_w8}
