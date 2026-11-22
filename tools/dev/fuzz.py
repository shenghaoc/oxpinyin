"""Install AFL++, minimize corpora, and fuzz targets with the `just fuzz` recipes.

AFL++ fuzzing is supported on Linux only, as in libchewing
(docs/runbooks/setup.md#fuzzing). The version of the `afl` crate that
fuzz/Cargo.lock resolves is the cargo-afl version, stated once: the crate and
cargo-afl must match. The AFL++ release follows from it: cargo-afl bundles it.
That release is Apache-2.0; cargo-afl builds it from source into the user's
data directory, and no shipping crate depends on it. The tools rule, stated
once in `tools_rule`, decides which installation `run` accepts, and `just
doctor` asks the same function.

`setup` installs cargo-afl and builds its bundled AFL++ with the LLVM plugins,
against the LLVM of the fuzz nightly. `minimize` prepares a target's corpus:
the restored inputs and the target's curated inputs, replayed one by one, then
afl-cmin. `run` builds one target with AddressSanitizer, the plugins and AFL++
persistent mode, runs afl-fuzz in a new run directory and fails on any finding
(docs/testing/upstream-test-coverage.md#fuzz-harness). `archive` packs the
inputs of every run directory for upload. Each target builds the host's cell
(`backend_flags`): Kyoto Cabinet on Linux, the one host that fuzzes
(tools/dev/cell.py).
"""

if __name__ == '__main__':
    from .entry import require_utf8_mode

    require_utf8_mode()

import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import tarfile
import time
import tomllib
from typing import NamedTuple

ROOT = Path(__file__).resolve().parents[2]
FUZZ = ROOT / 'fuzz'
LOCKFILE = FUZZ / 'Cargo.lock'
SETUP = 'just fuzz-setup'

# The inputs of each target that the corpus must always hold, beyond
# fuzz/seeds/<target>: (corpus name, source relative to the checkout, bytes
# to skip at the start). The MemoryChunk files are the Kyoto Cabinet cell's,
# byte-identical across the cells (fixtures/w3/README.md). phrase-library
# writes its own chunk header in front of the input, so its input is a
# payload: art.bin, which holds four items, without its 8-byte header.
# table-conf: libc scans the header, so no coverage feedback leads a mutation
# to a valid one; a real file starts the target past it.
CURATED_FIXTURES = {
    'dict-loader': tuple((name, f'fixtures/w3/kct/{name}', 0)
                         for name in ('gb_char.bin', 'gbk_char.bin', 'opengram.bin', 'merged.bin', 'art.bin')),
    'phrase-library': (('art.payload', 'fixtures/w3/kct/art.bin', 8),),
    'table-conf': (('table.conf', 'fixtures/w3/kct/table.conf', 0),),
}
# dict-loader reads whole files. A mutation longer than afl-fuzz's input limit
# is cut, and the chunk length check then refuses the file before any
# decoding, so its default limit (-G) admits its largest curated input.
LIMIT_FROM_CURATED = {'dict-loader'}
# The three header prefixes of table.conf are matched inside libc's scanner,
# outside the instrumented comparisons, so this target gets a dictionary.
DICTIONARIES = {'table-conf': FUZZ / 'dictionaries' / 'table-conf.dict'}
DENYLIST = FUZZ / 'instrument-denylist.txt'

# afl-fuzz's option string in AFL++ 4.40c (src/afl-fuzz.c, the getopt call in
# main). A letter followed by ':' takes a value. '+' stops at the first
# argument that is not an option: afl-fuzz takes it as the target command.
AFL_FUZZ_OPTIONS = '+a:Ab:B:c:CdDe:E:f:F:g:G:hi:I:K:l:L:m:M:nNo:Op:P:QRs:S:t:T:uUV:w:WXx:YzZ'
STDIN = 'every target reads its input from standard input'
INSTANCE = 'the recipe runs one afl-fuzz instance, default/, and reads its results there'
REFUSED_OPTIONS = {
    'f': f'-f makes afl-fuzz write each input to a file and connect the target to /dev/null; {STDIN}',
    'M': f'-M names a main instance; {INSTANCE}',
    'S': f'-S names a secondary instance; {INSTANCE}',
}

# AddressSanitizer with LeakSanitizer. abort_on_error=1 and symbolize=0 are
# afl-fuzz's conditions for a custom ASAN_OPTIONS. LSAN_OPTIONS is set
# explicitly: without it afl-fuzz exports fast_unwind_on_malloc=0 and
# malloc_context_size=30, and every allocation takes the slow unwinder.
ASAN_OPTIONS = 'abort_on_error=1:symbolize=0:detect_leaks=1:detect_odr_violation=0'
LSAN_OPTIONS = 'detect_leaks=1:fast_unwind_on_malloc=1:malloc_context_size=30:print_suppressions=0:symbolize=0'
PLUGINS = 'afl-llvm-dict2file, split-switches, SanitizerCoveragePCGUARD, afl-llvm-ijon'
# afl-fuzz takes no -t: it derives each run's timeout from its calibration of
# the corpus on this host and build. The forkserver's start-up, which builds
# each target's shared state, has its own limit, and afl-cmin and the curated
# replay, which run before any calibration, have a fixed one.
FORKSRV_INIT_TIMEOUT_MS = '60000'
REPLAY_TIMEOUT_MS = 15000
# MAP_SIZE of AFL++ 4.40c (include/config.h), the runtime's default map size.
CMIN_MAP_SIZE = '65536'
GUARD_GRACE_SECONDS = 30
OUTPUT_CLOSE_SECONDS = 2
# afl-fuzz names a seed issue in "Test case '<entry>' results in a crash" or
# "... a timeout" (afl-fuzz-init.c, perform_dry_run). <entry> is the name of
# the input's entry in the run's queue. Only those lines count: afl-fuzz also
# prints the AFL_EXIT_ON_SEED_ISSUES setting in every log.
SEED_ISSUE = re.compile(r"Test case '([^']+)' results in a (?:crash|timeout)")
# AFL++ writes this file into crashes/ and hangs/; it is not a finding.
FINDINGS_README = 'README.txt'


class FuzzError(RuntimeError):
    """A refused installation, option or run; the message states the problem and the step."""


def cargo_afl_version():
    """The cargo-afl version: the `afl` crate version that fuzz/Cargo.lock resolves."""
    versions = {package['version'] for package in tomllib.loads(LOCKFILE.read_text()).get('package', ())
                if package.get('name') == 'afl'}
    if len(versions) != 1:
        raise FuzzError(f'{LOCKFILE} must resolve exactly one version of the afl crate, not {sorted(versions)}.')
    return versions.pop()


def data_home(environ):
    """cargo-afl's data root: XDG_DATA_HOME, else ~/.local/share."""
    return Path(environ.get('XDG_DATA_HOME') or Path(environ.get('HOME') or Path.home()) / '.local/share')


def aflplusplus_source(environ):
    """The one AFL++ source tree that every cargo-afl version shares."""
    return data_home(environ) / 'afl.rs/AFLplusplus'


def require_linux():
    if not sys.platform.startswith('linux'):
        raise FuzzError('AFL++ fuzzing is supported on Linux only; on another host, fuzz in a native Linux '
                        'container (docs/runbooks/setup.md#fuzzing).')


def refuse_checkout(environ, cargo_afl_version):
    """Refuse an AFL++ git checkout in cargo-afl's source tree; never delete it.

    `cargo afl config --update` leaves one, and `config --build` copies the bundled
    release into the tree only when the tree is not a git checkout. The checkout
    may hold the user's own work, so the user removes it."""
    source = aflplusplus_source(environ)
    if (source / '.git').exists():
        raise FuzzError(f'{source} is an AFL++ git checkout (cargo afl config --update leaves one); cargo-afl '
                        f'would build it in place of the release that cargo-afl {cargo_afl_version} bundles. '
                        f'Remove {source}, then run {SETUP}.')


def tools_rule(environ):
    """The rule for the fuzzing tools, stated once; return the accepted version line.

    cargo-afl must be the `afl` version of fuzz/Cargo.lock, with the AFL++ LLVM
    plugins, and its AFL++ must be the release that it bundles: the AFL++ version
    follows from the cargo-afl pin and is not compared. `run` applies this rule
    first, and `just doctor` reports its answer."""
    version = cargo_afl_version()
    try:
        result = subprocess.run(['cargo', 'afl', '--version'], capture_output=True, text=True,
                                env=environ, cwd=FUZZ, check=False)
        found = result.stdout.strip() if result.returncode == 0 else ''
    except OSError:
        found = ''
    if not re.fullmatch(rf'cargo-afl {re.escape(version)} \(AFL\+\+ version .+ with plugins\)', found):
        raise FuzzError(f"found '{found or 'no cargo-afl'}', expected 'cargo-afl {version} "
                        f"(AFL++ version <bundled> with plugins)'; run {SETUP}.")
    refuse_checkout(environ, version)
    return found


def refuse_options(args):
    """Refuse afl-fuzz arguments that break the recipe's assumptions, before any build.

    The recipe feeds each input on standard input, runs one instance and supplies
    the target command itself. The arguments are read as afl-fuzz reads them, so
    a value such as `-V 60` or `-x dict` is not taken for an option."""
    valued = set(re.findall(r'([A-Za-z0-9]):', AFL_FUZZ_OPTIONS))
    given = set()
    for argument in args:
        if '@@' in argument:
            raise FuzzError(f"refused '{argument}': @@ passes the target a file name; {STDIN}.")
    index = 0
    while index < len(args):
        argument = args[index]
        if argument == '--':
            raise FuzzError("refused '--': the recipe supplies the target command after --.")
        if not argument.startswith('-') or argument == '-':
            raise FuzzError(f"refused '{argument}': afl-fuzz would take it as the target command; "
                            'the recipe supplies the target command.')
        for position, letter in enumerate(argument[1:], start=1):
            given.add(letter)
            if letter in REFUSED_OPTIONS:
                raise FuzzError(f"refused '{argument}': {REFUSED_OPTIONS[letter]}.")
            if letter in valued:
                if position == len(argument) - 1:
                    index += 1  # the value is the next argument
                break
        index += 1
    return given


def default_options(target, given):
    """afl-fuzz options that TARGET gets unless ARGS set them: its dictionary and its input limit."""
    added = []
    dictionary = DICTIONARIES.get(target)
    if dictionary is not None and 'x' not in given:
        added += ['-x', str(dictionary)]
    if target in LIMIT_FROM_CURATED and 'G' not in given:
        added += ['-G', str(max(len(data) for _, _, data in curated_inputs(target)))]
    return added


def write_file(data, destination):
    """Atomically replace DESTINATION without modifying files hard-linked into an earlier queue."""
    destination = Path(destination)
    handle, temporary = tempfile.mkstemp(dir=destination.parent, prefix='.fuzz-')
    try:
        with os.fdopen(handle, 'wb') as written:
            written.write(data)
        os.replace(temporary, destination)
    except BaseException:
        Path(temporary).unlink(missing_ok=True)
        raise


def content_name(path):
    return content_hash(Path(path).read_bytes())


def content_hash(data):
    return hashlib.sha1(data).hexdigest()


def curated_inputs(target):
    """TARGET's curated inputs as (corpus name, source, bytes): fuzz/seeds/<target>, then its fixtures.

    A fixture that is missing fails, so a lost input cannot shrink the corpus unnoticed."""
    inputs = []
    seeds = FUZZ / 'seeds' / target
    if seeds.is_dir():
        inputs += [(seed.name, seed, seed.read_bytes()) for seed in sorted(seeds.iterdir()) if seed.is_file()]
    for name, relative, skip in CURATED_FIXTURES.get(target, ()):
        source = FUZZ.parent / relative
        if not source.is_file():
            raise FuzzError(f'{target}: the curated input {relative} is missing.')
        inputs.append((name, source, source.read_bytes()[skip:]))
    return inputs


def seed_corpus(target, corpus):
    """Write TARGET's curated inputs into the corpus by name; one byte stands for an empty corpus.

    A corpus file that has a curated input's name but other content is kept
    under its content name, so no distinct input is lost. It is renamed, not
    copied: a queue entry that afl-fuzz linked to it keeps its bytes."""
    corpus.mkdir(parents=True, exist_ok=True)
    for name, _, data in curated_inputs(target):
        destination = corpus / name
        if destination.is_file() and destination.read_bytes() != data:
            kept = corpus / content_name(destination)
            if not kept.exists():
                os.replace(destination, kept)
        write_file(data, destination)
    if not any(corpus.iterdir()):
        (corpus / 'one-byte').write_bytes(b'\0')


def promote_queue(queue, corpus):
    """Add the run's queue entries to the corpus under content names.

    An entry whose content the corpus already holds, under any name, is not
    added: a seed keeps its own name and gets no second copy. That matters with
    AFL_SHA1_FILENAMES=1, where afl-fuzz names each seed's queue entry by its
    hash and stops on two seeds with the same content. Every queue file counts,
    whatever its name. No corpus file is changed or removed."""
    present = {content_name(path) for path in corpus.iterdir() if path.is_file()}
    added = 0
    if queue.is_dir():
        for entry in sorted(queue.iterdir()):
            if entry.is_file():
                data = entry.read_bytes()
                name = content_hash(data)
                if name not in present:
                    write_file(data, corpus / name)
                    present.add(name)
                    added += 1
    return added


def keep_seed_issues(log_text, instance, run_directory):
    """Copy each queue entry that afl-fuzz names as a seed issue into <run>/seed-issues/.

    Return the number kept."""
    kept = 0
    for name in sorted(set(SEED_ISSUE.findall(log_text))):
        entry = instance / 'queue' / Path(name).name
        if entry.is_file():
            issues = run_directory / 'seed-issues'
            issues.mkdir(exist_ok=True)
            shutil.copyfile(entry, issues / entry.name)
            kept += 1
    return kept


def count_findings(directory):
    """Every file in crashes/ or hangs/ except AFL++'s README.txt, whatever its name."""
    if not directory.is_dir():
        return 0
    return sum(1 for path in directory.iterdir() if path.is_file() and path.name != FINDINGS_README)


def read_stats(path):
    stats = {}
    for line in Path(path).read_text(errors='replace').splitlines():
        key, separator, value = line.partition(':')
        if separator:
            stats[key.strip()] = value.strip()
    return stats


def rustc_facts(environ):
    """host, release, commit hash and LLVM major of the fuzz nightly (`rustc -vV` in fuzz/)."""
    text = subprocess.run(['rustc', '-vV'], capture_output=True, text=True, env=environ, cwd=FUZZ,
                          check=True).stdout
    facts = dict(line.split(': ', 1) for line in text.splitlines() if ': ' in line)
    return {'host': facts['host'], 'release': facts['release'], 'commit': facts['commit-hash'][:7],
            'llvm_major': facts['LLVM version'].split('.')[0]}


def target_directory(environ):
    """cargo's target directory for fuzz/ (CARGO_TARGET_DIR, build.target-dir or target/).

    The one source for the build and the launch paths: the builds name it with
    --target-dir, and the executables are at cargo's layout <host>/release/<bin>."""
    text = subprocess.run(['cargo', 'metadata', '--format-version', '1', '--no-deps', '--locked'],
                          capture_output=True, text=True, env=environ, cwd=FUZZ, check=True).stdout
    return Path(json.loads(text)['target_directory'])


class TargetBuild(NamedTuple):
    env: dict
    command: list
    binary: Path
    target_dir: Path
    host: str
    afl_bin: Path
    version: str
    afl_version: str


def build_target(target, environ, found):
    """Build one target with the instrumentation shared by cmin and fuzzing."""
    version = cargo_afl_version()
    afl_version = found.split('AFL++ version ', 1)[1].split(' ', 1)[0]
    rustc = rustc_facts(environ)
    host = rustc['host']
    afl_bin = (data_home(environ) / f'afl.rs/rustc-{rustc["release"]}-{rustc["commit"]}'
               / f'afl.rs-{version}/afl/bin')
    target_dir = target_directory(environ)
    # Cargo prefers CARGO_ENCODED_RUSTFLAGS to RUSTFLAGS: an inherited value
    # would drop AddressSanitizer and the flags that cargo-afl adds.
    environ = {name: value for name, value in environ.items() if name != 'CARGO_ENCODED_RUSTFLAGS'}
    env = dict(environ,
               ASAN_OPTIONS=ASAN_OPTIONS,
               LSAN_OPTIONS=LSAN_OPTIONS,
               AFL_SKIP_CPUFREQ='1',
               # A crashing or timed-out seed must stop the soak.
               AFL_EXIT_ON_SEED_ISSUES='1',
               AFLRS_REQUIRE_PLUGINS='1',
               AFL_FORKSRV_INIT_TMOUT=environ.get('AFL_FORKSRV_INIT_TMOUT') or FORKSRV_INIT_TIMEOUT_MS,
               # The persistent loop stays uninstrumented (fuzz/src/lib.rs).
               AFL_LLVM_DENYLIST=str(DENYLIST),
               # --target keeps build scripts and proc macros out of ASan.
               RUSTFLAGS=' '.join(filter(None, ('-Zsanitizer=address', environ.get('RUSTFLAGS')))))
    # The afl feature selects the persistent-mode entry point (fuzz/src/lib.rs).
    command = ['cargo', 'afl', 'build', '--release', '--locked', '--target', host, *backend_flags(),
               '--features', 'afl']
    _call([*command, '--target-dir', str(target_dir), '--bin', target], dict(env, AFLRS_NO_CMPLOG='1'),
          f'{target}: cargo afl build')
    binary = target_dir / host / 'release' / target
    return TargetBuild(env, command, binary, target_dir, host, afl_bin, version, afl_version)


def new_run_directory(target):
    """Keep each minimization and fuzz run under its own directory."""
    run_name = f'{time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())}-{os.getpid()}'
    directory = FUZZ / 'out' / target / run_name
    directory.mkdir(parents=True)
    print(f'fuzz: {target}: run directory {directory.relative_to(FUZZ)}')
    return directory


def guard_duration(environ):
    """Return the optional wall-time guard, in seconds."""
    value = environ.get('FUZZ_GUARD_SECONDS')
    if not value:
        return None
    try:
        seconds = float(value)
    except ValueError as error:
        raise FuzzError(f'FUZZ_GUARD_SECONDS must be a non-negative number of seconds: {value!r}.') from error
    if not math.isfinite(seconds) or seconds < 0:
        raise FuzzError(f'FUZZ_GUARD_SECONDS must be a non-negative number of seconds: {value!r}.')
    return seconds or None


def _call(command, environ, what):
    sys.stdout.flush()
    status = subprocess.run(command, env=environ, cwd=FUZZ, check=False).returncode
    if status != 0:
        raise FuzzError(f'{what} failed with exit status {status}.')


def setup(environ):
    """Install cargo-afl at the pinned version and build its bundled AFL++ with the LLVM plugins.

    The plugins must build against the LLVM of the fuzz nightly, so the LLVM major
    comes from `rustc -vV`, and llvm-config must report it: `llvm-config-<major>`,
    or the LLVM_CONFIG you set. AFL++ goes to cargo-afl's data directory;
    nothing is installed system-wide."""
    require_linux()
    version = cargo_afl_version()
    refuse_checkout(environ, version)
    major = rustc_facts(environ)['llvm_major']
    llvm_config = environ.get('LLVM_CONFIG') or f'llvm-config-{major}'
    try:
        found = subprocess.run([llvm_config, '--version'], capture_output=True, text=True, env=environ,
                               check=False).stdout.strip()
    except OSError:
        found = ''
    if found.split('.')[0] != major:
        raise FuzzError(f"the fuzz nightly uses LLVM {major}, but {llvm_config} reports '{found or 'nothing'}'; "
                        f'install LLVM {major} (llvm-{major}-dev) or set LLVM_CONFIG.')
    _call(['cargo', 'install', 'cargo-afl', '--version', version, '--locked'], environ, 'cargo install cargo-afl')
    _call(['cargo', 'afl', 'config', '--build', '--force', '--plugins'], dict(environ, LLVM_CONFIG=llvm_config),
          'cargo afl config --build')
    _call(['cargo', 'afl', '--version'], environ, 'cargo afl --version')
    return 0


def backend_flags():
    """The Cargo feature flags of the host's cell (tools/dev/cell.py)."""
    from .cell import CELL_TO_BACKEND, feature_flags, host_cell
    return feature_flags(CELL_TO_BACKEND[host_cell()])


def _signal_group(process, sig):
    try:
        os.killpg(process.pid, sig)
    except ProcessLookupError:
        pass


def _wait_for_output(process, reader, deadline):
    """Wait for the pipe and child without reaping the group leader early."""
    def remaining():
        return None if deadline is None else max(0, deadline - time.monotonic())

    # An unreaped leader keeps its PID, and hence the group ID, from being reused.
    reader.join(timeout=remaining())
    if reader.is_alive():
        return None
    try:
        return process.wait(timeout=remaining())
    except subprocess.TimeoutExpired:
        return None


def _tee(command, environ, log, guard=None, grace=GUARD_GRACE_SECONDS):
    """Run COMMAND with its output on standard output and in LOG.

    Return its exit status, whether the guard fired, and the elapsed
    time when afl-fuzz reported that its startup run was complete."""
    sys.stdout.flush()
    written = open(log, 'wb')
    try:
        process = subprocess.Popen(command, env=environ, cwd=FUZZ, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, start_new_session=True)
    except BaseException:
        written.close()
        raise
    started = time.monotonic()
    stopped = threading.Event()
    abandoned = threading.Event()
    startup = []
    errors = []

    def copy_output():
        try:
            with written, process.stdout:
                for line in iter(process.stdout.readline, b''):
                    if abandoned.is_set():
                        break
                    if not startup and not stopped.is_set() and b'All set and ready to roll!' in line:
                        startup.append(time.monotonic() - started)
                    sys.stdout.buffer.write(line)
                    sys.stdout.buffer.flush()
                    written.write(line)
        except BaseException as error:
            if not abandoned.is_set():
                errors.append(error)
                _signal_group(process, signal.SIGTERM)

    reader = threading.Thread(target=copy_output, daemon=True)
    reader.start()
    try:
        deadline = None if guard is None else started + guard
        status = _wait_for_output(process, reader, deadline)
        if status is None:
            stopped.set()
            _signal_group(process, signal.SIGINT)
            status = _wait_for_output(process, reader, time.monotonic() + grace)
            if status is None:
                _signal_group(process, signal.SIGKILL)
                status = _wait_for_output(process, reader, time.monotonic() + OUTPUT_CLOSE_SECONDS)
                if status is None:
                    abandoned.set()
                    process.poll()
                    if reader.is_alive():
                        raise FuzzError('guard stopped the command, but its output pipe did not close after SIGKILL.')
                    raise FuzzError('guard stopped the command, but its process did not exit after SIGKILL.')
    except KeyboardInterrupt:
        _signal_group(process, signal.SIGINT)
        if _wait_for_output(process, reader, time.monotonic() + grace) is None:
            _signal_group(process, signal.SIGKILL)
            _wait_for_output(process, reader, time.monotonic() + OUTPUT_CLOSE_SECONDS)
        abandoned.set()
        raise
    if errors:
        raise errors[0]
    return status, stopped.is_set(), startup[0] if startup else None


def replay_curated(target, curated, run_directory, built):
    """Run each curated input alone through the target's standard input; return the failures.

    A failing input keeps its bytes and its output in <run>/seed-issues/, under
    its corpus name, with the source in the message."""
    failed = []
    issues = run_directory / 'seed-issues'
    for name, source, data in curated:
        try:
            # setarch -R, as for afl-fuzz (see `run`).
            result = subprocess.run(['setarch', platform.machine(), '-R', str(built.binary)], input=data, capture_output=True, env=built.env, cwd=FUZZ,
                         timeout=REPLAY_TIMEOUT_MS / 1000, check=False)
            problem = f'exit status {result.returncode}' if result.returncode else None
            output = result.stdout + result.stderr
        except subprocess.TimeoutExpired as error:
            problem = f'no exit within {REPLAY_TIMEOUT_MS} ms'
            output = (error.stdout or b'') + (error.stderr or b'')
        if problem:
            issues.mkdir(exist_ok=True)
            (issues / name).write_bytes(data)
            (issues / f'{name}.log').write_bytes(output)
            failed.append(f'{name} (from {source}): {problem}')
    return failed


def stage_inputs(target, corpus, staging, curated):
    """Stage the restored corpus and the curated inputs, one file per content.

    A curated input keeps its own name. A restored file is staged under its
    content name, unless that content is already staged, so a restored file
    whose name is a curated input's but whose content differs is kept."""
    staging.mkdir()
    staged = set()
    for name, _, data in curated:
        if content_hash(data) not in staged:
            staged.add(content_hash(data))
            (staging / name).write_bytes(data)
    if corpus.is_dir():
        for path in sorted(corpus.iterdir()):
            if path.is_file():
                name = content_name(path)
                if name not in staged:
                    staged.add(name)
                    shutil.copyfile(path, staging / name)
    if not staged:
        (staging / 'one-byte').write_bytes(b'\0')
    return sum(path.is_file() for path in staging.iterdir())


def minimize_corpus(target, corpus, run_directory, built, curated=None):
    """Prepare a target's corpus; replace it only when every step succeeds.

    The restored inputs and the curated inputs are staged and deduplicated by
    content; each curated input is replayed alone; afl-cmin minimizes the
    staging; a curated input that afl-cmin dropped is put back. A failure at
    any step leaves the corpus as it was."""
    corpus.parent.mkdir(parents=True, exist_ok=True)
    if corpus.exists() and not corpus.is_dir():
        raise FuzzError(f'{target}: corpus is not a directory: {corpus}.')
    if curated is None:
        curated = curated_inputs(target)
    issues = run_directory / 'seed-issues'
    log = run_directory / 'afl-cmin.log'
    failed = replay_curated(target, curated, run_directory, built)
    if failed:
        raise FuzzError(f'{target}: curated inputs fail on their own; they are in {issues}: ' + '; '.join(failed)
                        + '. The corpus was not replaced.')
    with tempfile.TemporaryDirectory(prefix=f'.{target}-cmin-', dir=corpus.parent) as temporary:
        staging = Path(temporary)
        source = staging / 'inputs'
        before = stage_inputs(target, corpus, source, curated)
        minimized = staging / 'minimized'
        command = ['setarch', platform.machine(), '-R', str(built.afl_bin / 'afl-cmin'),
                   '-i', str(source), '-o', str(minimized), '--crash-dir', str(issues),
                   '-t', str(REPLAY_TIMEOUT_MS), '-m', 'none', '--', str(built.binary)]
        cmin_env = dict(built.env, AFL_PATH=str(built.afl_bin))
        # afl-cmin otherwise passes afl-showmap the map size that the binary
        # reports, and afl-showmap makes its shared memory that size. A
        # persistent-mode child is larger: the AFL++ 4.40c runtime maps the
        # shared memory before the coverage guards are counted, so
        # __afl_set_map_size stays MAP_SIZE (afl-compiler-rt.o.c:534), and the
        # first pass of __afl_persistent_loop clears that many bytes (:1389),
        # past the end of the smaller map. MAP_SIZE is the smallest safe size.
        cmin_env.setdefault('AFL_MAP_SIZE', CMIN_MAP_SIZE)
        # These make afl-showmap hide failures or discard normal inputs.
        cmin_env.pop('AFL_CMIN_ALLOW_ANY', None)
        cmin_env.pop('AFL_CMIN_CRASHES_ONLY', None)
        started = time.monotonic()
        status, _, _ = _tee(command, cmin_env, log)
        elapsed = time.monotonic() - started
        if issues.is_dir() and any(issues.iterdir()):
            raise FuzzError(f'{target}: afl-cmin found seeds that crash or time out; they are in {issues}.')
        if status != 0:
            raise FuzzError(f'{target}: afl-cmin exited with status {status}; the corpus was not replaced.')
        after = sum(path.is_file() for path in minimized.iterdir()) if minimized.is_dir() else 0
        if not after:
            raise FuzzError(f'{target}: afl-cmin produced no inputs; the corpus was not replaced.')
        kept = {content_name(path) for path in minimized.iterdir() if path.is_file()}
        restored = 0
        for name, _, data in curated:
            if content_hash(data) not in kept:
                (minimized / name).write_bytes(data)
                kept.add(content_hash(data))
                restored += 1
        original = staging / 'original'
        if corpus.exists():
            os.replace(corpus, original)
        try:
            os.replace(minimized, corpus)
        except BaseException:
            if original.exists():
                os.replace(original, corpus)
            raise
    print(f'fuzz: {target}: afl-cmin {before} -> {after} inputs in {elapsed:.1f} s, '
          f'{restored} curated inputs put back')


def minimize(target, environ):
    """Minimize the cached corpus with the same target and sanitizer as the soak."""
    require_linux()
    found = tools_rule(environ)
    built = build_target(target, environ, found)
    directory = new_run_directory(target)
    minimize_corpus(target, FUZZ / 'corpus' / target, directory, built)
    return 0


def minimize_all(environ):
    """Minimize every fuzz target, including those after a failed target."""
    require_linux()
    metadata = subprocess.run(['cargo', 'metadata', '--no-deps', '--format-version', '1'],
                              capture_output=True, text=True, env=environ, cwd=FUZZ, check=True).stdout
    targets = [target['name'] for package in json.loads(metadata)['packages']
               for target in package['targets'] if target['kind'] == ['bin']]
    if not targets:
        raise FuzzError('fuzz/Cargo.toml lists no targets.')
    failed = False
    for target in targets:
        if environ.get('GITHUB_ACTIONS'):
            print(f'::group::minimize {target}', flush=True)
        try:
            minimize(target, environ)
        except (FuzzError, OSError, subprocess.CalledProcessError) as error:
            print(f'fuzz: {target}: minimization failed: {error}', file=sys.stderr)
            failed = True
        finally:
            if environ.get('GITHUB_ACTIONS'):
                print('::endgroup::', flush=True)
    return 1 if failed else 0


def check_fuzz_status(target, status, guard_stopped, guard_value, stats_path):
    """Report the guard and a missing statistics file only when they occurred."""
    if guard_stopped:
        raise FuzzError(f'{target}: afl-fuzz did not end within {guard_value} s.')
    if not stats_path.is_file():
        raise FuzzError(f'{target}: afl-fuzz exited with status {status} and wrote no statistics.')
    if status != 0:
        raise FuzzError(f'{target}: afl-fuzz exited with status {status}.')


def run(target, args, environ):
    """Fuzz TARGET with AFL++; return the exit status of the recipe.

    The target is built with AddressSanitizer, the AFL++ LLVM plugins
    (cargo-afl fails if the plugins are absent) and the persistent-mode entry
    point. afl-fuzz sets each input's timeout from its calibration, and a
    target's dictionary and input limit apply unless ARGS set -x or -G
    (`default_options`). FUZZ_CMPLOG=1 adds a CmpLog
    binary, run through afl-fuzz -c. afl-fuzz reads corpus/<target> and writes a
    new run directory, out/<target>/<UTC time>-<pid>, so a run never deletes an
    earlier run's findings. ARGS go to afl-fuzz. FUZZ_GUARD_SECONDS stops
    afl-fuzz after that wall time. The run fails when afl-fuzz fails, stops on a
    seed issue, ran no input, or saved a crash or a hang."""
    require_linux()
    found = tools_rule(environ)
    given = refuse_options(args)
    guard = guard_duration(environ)
    built = build_target(target, environ, found)
    cmplog = []
    if environ.get('FUZZ_CMPLOG', '0') == '1':
        # An inherited AFLRS_NO_CMPLOG would build this binary without CmpLog.
        cmplog_env = {name: value for name, value in built.env.items() if name != 'AFLRS_NO_CMPLOG'}
        _call([*built.command, '--target-dir', str(built.target_dir / 'cmplog'), '--bin', target], cmplog_env,
              f'{target}: cargo afl build (CmpLog)')
        cmplog_binary = built.target_dir / 'cmplog' / built.host / 'release' / target
        cmplog = ['-c', str(cmplog_binary)]
        mode = f'CmpLog on (-c {cmplog_binary}, plus cmplog-switches, cmplog-instructions, cmplog-routines)'
    else:
        mode = 'CmpLog off'
    print(f'fuzz: {target}: AFL++ {built.afl_version} (cargo-afl {built.version}), LLVM plugins {PLUGINS}; {mode}; '
          f'ASan, LSAN_OPTIONS={LSAN_OPTIONS}')
    print(f'fuzz: {target}: {built.binary}')
    corpus = FUZZ / 'corpus' / target
    run_directory = new_run_directory(target)
    seed_corpus(target, corpus)
    # setarch -R disables ASLR: runner kernels' mmap entropy exceeds ASan's shadow
    # mapping (tools/dev/test_runner.py does the same).
    aslr = ['setarch', platform.machine(), '-R']
    log = run_directory / 'afl-fuzz.log'
    defaults = default_options(target, given)
    status, guard_stopped, startup = _tee([*aslr, str(built.afl_bin / 'afl-fuzz'), '-i', str(corpus), '-o',
                                           str(run_directory), '-m', 'none', *cmplog, *defaults, *args,
                                           '--', str(built.binary)], built.env, log, guard=guard)
    if startup is not None:
        print(f'fuzz: {target}: startup {startup:.1f} s')
    instance = run_directory / 'default'
    log_text = log.read_text(errors='replace')
    if status != 0 and SEED_ISSUE.search(log_text):
        # No queue entry joins the corpus after a seed issue, and the offending
        # input stays in the corpus under its own name: it fails every run until
        # the code is fixed.
        if keep_seed_issues(log_text, instance, run_directory):
            raise FuzzError(f'{target}: afl-fuzz stopped on a seed that crashes or times out; '
                            f'it is in {run_directory / "seed-issues"}.')
        raise FuzzError(f'{target}: afl-fuzz stopped on a seed issue, but the recipe could not find '
                        f'the offending input in {log}.')
    promote_queue(instance / 'queue', corpus)
    stats_path = instance / 'fuzzer_stats'
    check_fuzz_status(target, status, guard_stopped, environ.get('FUZZ_GUARD_SECONDS'), stats_path)
    stats = read_stats(stats_path)
    stat = lambda name: stats.get(name, '?')
    print(f'fuzz: {target}: afl_version {stat("afl_version")}, execs_done {stat("execs_done")}, '
          f'calibration_time {stat("calibration_time")} s, fuzz_time {stat("fuzz_time")} s, '
          f'cmplog_time {stat("cmplog_time")} s, edges_found {stat("edges_found")}/{stat("total_edges")}, '
          f'stability {stat("stability")}, execs_per_sec {stat("execs_per_sec")}, '
          f'exec_timeout {stat("exec_timeout")} ms')
    failed = False
    if stats.get('execs_done', '0') == '0':
        print(f'fuzz: {target}: afl-fuzz ran no input.', file=sys.stderr)
        failed = True
    for kind in ('crashes', 'hangs'):
        count = count_findings(instance / kind)
        if count:
            print(f'fuzz: {target}: {count} {kind} in {instance / kind}.', file=sys.stderr)
            failed = True
    return 1 if failed else 0


# Per run directory, the inputs that the archive keeps. AFL++ names them with
# ':' (id:000000,...), which an uploaded artifact's file name cannot hold.
ARCHIVED = ('default/queue', 'default/crashes', 'default/hangs', 'seed-issues')


def archive(destination):
    """Pack the queue, findings and seed issues of every run directory into a gzip tar file.

    Runs that ended early and runs of targets that passed are packed too.
    Return the number of files packed."""
    out = FUZZ / 'out'
    destination = Path(destination).resolve()
    destination.parent.mkdir(parents=True, exist_ok=True)
    packed = 0
    with tarfile.open(destination, 'w:gz') as written:
        for run_directory in sorted(out.glob('*/*')):
            for relative in ARCHIVED:
                directory = run_directory / relative
                if not directory.is_dir():
                    continue
                for path in sorted(directory.iterdir()):
                    if path.is_file() and path.resolve() != destination:
                        written.add(path, arcname=str(path.relative_to(out)))
                        packed += 1
    print(f'fuzz: archived {packed} inputs in {destination}')
    return packed


USAGE = ('usage: python -m tools.dev.fuzz setup | minimize [target] | run <target> [afl-fuzz options] '
         '| archive <file>')


def main(argv):
    environ = dict(os.environ)
    try:
        if argv[:1] == ['setup'] and len(argv) == 1:
            return setup(environ)
        if argv == ['minimize']:
            return minimize_all(environ)
        if argv[:1] == ['minimize'] and len(argv) == 2:
            return minimize(argv[1], environ)
        if argv[:1] == ['run'] and len(argv) >= 2:
            return run(argv[1], argv[2:], environ)
        if argv[:1] == ['archive'] and len(argv) == 2:
            archive(argv[1])
            return 0
    except FuzzError as error:
        print(f'fuzz: {error}', file=sys.stderr)
        return 1
    except OSError as error:
        # A corpus file that cannot be read or copied stops the command before anything is replaced.
        print(f'fuzz: {error}', file=sys.stderr)
        return 1
    except subprocess.CalledProcessError as error:
        print(f'fuzz: {error.cmd[0]} failed with exit status {error.returncode}.', file=sys.stderr)
        return 1
    print(USAGE, file=sys.stderr)
    return 2


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
