"""Install AFL++, minimize corpora, and fuzz targets with the `just fuzz` recipes.

AFL++ fuzzing is supported on Linux only, as in libchewing
(docs/runbooks/setup.md#fuzzing). fuzz/tool-versions.env pins cargo-afl, and
the AFL++ release follows from it: cargo-afl bundles it. The tools rule,
stated once in `tools_rule`, decides which installation `run` accepts, and
`just doctor` asks the same function.

`setup` installs cargo-afl and builds its bundled AFL++ with the LLVM plugins,
against the LLVM of the fuzz nightly. `run` builds one target with
AddressSanitizer and the plugins, runs afl-fuzz in a new run directory and
fails on any finding (docs/testing/upstream-test-coverage.md#fuzz-harness).
Each target builds the host's cell (`backend_flags`): Kyoto Cabinet on Linux,
the one host that fuzzes (tools/dev/cell.py).
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
import time
from typing import NamedTuple

ROOT = Path(__file__).resolve().parents[2]
FUZZ = ROOT / 'fuzz'
TOOL_VERSIONS = FUZZ / 'tool-versions.env'
SETUP = 'just fuzz-setup'

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
TIMEOUT_MS = '15000'
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


def read_tool_versions(path=TOOL_VERSIONS):
    """The KEY=VALUE pins of fuzz/tool-versions.env."""
    pins = {}
    for line in Path(path).read_text().splitlines():
        line = line.strip()
        if line and not line.startswith('#'):
            key, _, value = line.partition('=')
            pins[key.strip()] = value.strip()
    return pins


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


def tools_rule(environ, run=subprocess.run):
    """The rule for the fuzzing tools, stated once; return the accepted version line.

    cargo-afl must be the version of fuzz/tool-versions.env, with the AFL++ LLVM
    plugins, and its AFL++ must be the release that it bundles: the AFL++ version
    follows from the cargo-afl pin and is not compared. `run` applies this rule
    first, and `just doctor` reports its answer."""
    version = read_tool_versions()['CARGO_AFL_VERSION']
    try:
        result = run(['cargo', 'afl', '--version'], capture_output=True, text=True, env=environ, cwd=FUZZ,
                     check=False)
        found = result.stdout.strip() if result.returncode == 0 else ''
    except OSError:
        found = ''
    if not re.fullmatch(rf'cargo-afl {re.escape(version)} \(AFL\+\+ version .+ with plugins\)', found):
        raise FuzzError(f"found '{found or 'no cargo-afl'}', expected 'cargo-afl {version} "
                        f"(AFL++ version <bundled> with plugins)'; run {SETUP}.")
    refuse_checkout(environ, version)
    return found


def _option_letters(spec=AFL_FUZZ_OPTIONS):
    """The option letters of a getopt string, and those that take a value."""
    letters, valued = set(), set()
    for index, letter in enumerate(spec):
        if letter in '+:':
            continue
        letters.add(letter)
        if spec[index + 1:index + 2] == ':':
            valued.add(letter)
    return letters, valued


def refuse_options(args):
    """Refuse afl-fuzz arguments that break the recipe's assumptions, before any build.

    The recipe feeds each input on standard input, runs one instance and supplies
    the target command itself. The arguments are read as afl-fuzz reads them, so
    a value such as `-V 60` or `-x dict` is not taken for an option."""
    _, valued = _option_letters()
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
            if letter in REFUSED_OPTIONS:
                raise FuzzError(f"refused '{argument}': {REFUSED_OPTIONS[letter]}.")
            if letter in valued:
                if position == len(argument) - 1:
                    index += 1  # the value is the next argument
                break
        index += 1


def replace_file(source, destination):
    """Write SOURCE's bytes to DESTINATION through a temporary file and a rename.

    afl-fuzz hard-links its seed queue entries to the corpus files, and every
    run directory is kept, so writing into an existing corpus file would change
    an older run's queue entry. A rename gives the corpus path a new inode."""
    destination = Path(destination)
    handle, temporary = tempfile.mkstemp(dir=destination.parent, prefix='.fuzz-')
    try:
        with os.fdopen(handle, 'wb') as written, open(source, 'rb') as read:
            shutil.copyfileobj(read, written)
        os.replace(temporary, destination)
    except BaseException:
        Path(temporary).unlink(missing_ok=True)
        raise


def content_name(path):
    return hashlib.sha1(Path(path).read_bytes()).hexdigest()


def seed_corpus(target, corpus, fuzz=FUZZ):
    """Copy fuzz/seeds/<target> into the corpus; one byte stands for an empty corpus."""
    corpus.mkdir(parents=True, exist_ok=True)
    seeds = fuzz / 'seeds' / target
    if seeds.is_dir():
        for seed in sorted(seeds.iterdir()):
            if seed.is_file():
                replace_file(seed, corpus / seed.name)
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
                name = content_name(entry)
                if name not in present:
                    replace_file(entry, corpus / name)
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
    version = read_tool_versions()['CARGO_AFL_VERSION']
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
               # --target keeps build scripts and proc macros out of ASan.
               RUSTFLAGS=' '.join(filter(None, ('-Zsanitizer=address', environ.get('RUSTFLAGS')))))
    command = ['cargo', 'afl', 'build', '--release', '--locked', '--target', host, *backend_flags()]
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
    version = read_tool_versions()['CARGO_AFL_VERSION']
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


def minimize_corpus(target, corpus, run_directory, built):
    """Minimize a corpus without replacing it when cmin reports a seed issue."""
    corpus.parent.mkdir(parents=True, exist_ok=True)
    if corpus.exists() and not corpus.is_dir():
        raise FuzzError(f'{target}: corpus is not a directory: {corpus}.')
    issues = run_directory / 'seed-issues'
    log = run_directory / 'afl-cmin.log'
    with tempfile.TemporaryDirectory(prefix=f'.{target}-cmin-', dir=corpus.parent) as temporary:
        staging = Path(temporary)
        source = corpus
        if not corpus.is_dir() or not any(path.is_file() for path in corpus.iterdir()):
            source = staging / 'seeds'
            seed_corpus(target, source)
        before = sum(path.is_file() for path in source.iterdir())
        minimized = staging / 'minimized'
        command = ['setarch', platform.machine(), '-R', str(built.afl_bin / 'afl-cmin'),
                   '-i', str(source), '-o', str(minimized), '--crash-dir', str(issues),
                   '-t', TIMEOUT_MS, '-m', 'none', '--', str(built.binary)]
        cmin_env = dict(built.env, AFL_PATH=str(built.afl_bin))
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
        original = staging / 'original'
        if corpus.exists():
            os.replace(corpus, original)
        try:
            os.replace(minimized, corpus)
        except BaseException:
            if original.exists():
                os.replace(original, corpus)
            raise
    seed_corpus(target, corpus)
    print(f'fuzz: {target}: afl-cmin {before} -> {after} inputs in {elapsed:.1f} s')


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

    The target is built with AddressSanitizer and the AFL++ LLVM plugins
    (cargo-afl fails if the plugins are absent). FUZZ_CMPLOG=1 adds a CmpLog
    binary, run through afl-fuzz -c. afl-fuzz reads corpus/<target> and writes a
    new run directory, out/<target>/<UTC time>-<pid>, so a run never deletes an
    earlier run's findings. ARGS go to afl-fuzz. FUZZ_GUARD_SECONDS stops
    afl-fuzz after that wall time. The run fails when afl-fuzz fails, stops on a
    seed issue, ran no input, or saved a crash or a hang."""
    require_linux()
    found = tools_rule(environ)
    refuse_options(args)
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
    status, guard_stopped, startup = _tee([*aslr, str(built.afl_bin / 'afl-fuzz'), '-i', str(corpus), '-o',
                                           str(run_directory), '-t', TIMEOUT_MS, '-m', 'none', *cmplog, *args,
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
          f'stability {stat("stability")}')
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


USAGE = 'usage: python -m tools.dev.fuzz setup | minimize [target] | run <target> [afl-fuzz options]'


def main(argv, environ=None):
    environ = dict(os.environ if environ is None else environ)
    try:
        if argv[:1] == ['setup'] and len(argv) == 1:
            return setup(environ)
        if argv == ['minimize']:
            return minimize_all(environ)
        if argv[:1] == ['minimize'] and len(argv) == 2:
            return minimize(argv[1], environ)
        if argv[:1] == ['run'] and len(argv) >= 2:
            return run(argv[1], argv[2:], environ)
    except FuzzError as error:
        print(f'fuzz: {error}', file=sys.stderr)
        return 1
    except subprocess.CalledProcessError as error:
        print(f'fuzz: {error.cmd[0]} failed with exit status {error.returncode}.', file=sys.stderr)
        return 1
    print(USAGE, file=sys.stderr)
    return 2


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
