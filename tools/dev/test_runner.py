"""Native nextest and doctest entry points. Standard library only.

The just recipes stay thin. Resolve nextest's concurrency before admission,
then pass its final budget through the same slot implementation as just heavy.
"""
from collections.abc import Mapping
from dataclasses import dataclass
import fnmatch
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import stat
import subprocess
import sys
import tempfile
import tomllib

from . import host, mounts, native, pool, slot

ROOT = Path(__file__).resolve().parents[2]

# Compiler-backed J-ledger checks need clang++, LLVM tools and a discoverable
# libclang, which the CI harness jobs do not provision (docs/testing/README.md).
HARNESS_EXCLUDED = {'tools/bisection/internal-ledger.test.py'}

# No harness test file inherits a variable whose name matches one of these
# patterns, compared case-insensitively. A failing mock assertion prints the
# call's arguments, and launcher and HeavyAdmission mocks receive the whole
# environment: one such failure printed API and GitHub tokens (#1262). AUTH
# matches only as _AUTH, _AUTH_ or AUTH_CONFIG, never as a bare substring,
# which would also remove GIT_AUTHOR_NAME and GIT_AUTHOR_EMAIL, the identity
# git needs to make a commit.
CREDENTIAL_PATTERNS = ('*_KEY', '*_KEY_ID', '*APIKEY*', '*_TOKEN', '*_SECRET', '*SECRET*',
                       '*PASSWORD*', '*_PASSWD', '*_PAT', '*CREDENTIAL*', '*_AUTH', '*_AUTH_*',
                       '*AUTH_CONFIG*', 'NETRC', '*_NETRC', 'GITHUB_TOKEN', 'GH_TOKEN',
                       'ANTHROPIC_AUTH_TOKEN', 'ANTHROPIC_API_KEY')


def harness_files():
    """Discover regular tools/**/*.test.py files in LC_ALL=C path order.

    Like CI's find -type f, do not follow directory or file symlinks.
    """
    files = []
    for directory, _, names in os.walk(ROOT / 'tools'):
        for name in names:
            path = Path(directory, name)
            if name.endswith('.test.py') and stat.S_ISREG(path.lstat().st_mode):
                files.append(path.relative_to(ROOT).as_posix())
    return sorted(files, key=os.fsencode)


def harness_environment(environ):
    """Copy environ without its credential-shaped variables.

    Return the copy and the number of variables left out.
    """
    kept = {name: value for name, value in environ.items()
            if not any(fnmatch.fnmatchcase(name.upper(), pattern) for pattern in CREDENTIAL_PATTERNS)}
    return kept, len(environ) - len(kept)


def memory_backed_tmpdir():
    """Return the refusal line when the harness temporary directory is on tmpfs or ramfs, else None.

    teardown.test.py's cold eviction tests need fadvise to evict, which tmpfs
    and ramfs ignore. An unreadable mount type is not a refusal.
    """
    directory = tempfile.gettempdir()
    try:
        kind = mounts.filesystem(directory)
    except (OSError, ValueError):
        return None
    if kind not in ('tmpfs', 'ramfs'):
        return None
    return (f'just test-tools: TMPDIR {directory} is on {kind}; the cold teardown tests need a '
            'disk-backed TMPDIR (for example ~/.cache/oxpinyin/tmp)')


# Explicit dependency mapping; shared infrastructure and unknown paths fall back
# to the full suite rather than guessing from a matching filename.
PACKAGING_HARNESSES = (
    'tools/dev/packaging.test.py',
    'tools/packaging/function-order.test.py',
    'tools/generate-function-order.test.py',
    'tools/bisection/teardown.test.py',
    'tools/dev/measure/snapshot.test.py',
    'tools/dev/cli.test.py',
    'tools/dev/bash_floor.test.py',
)
# U4-owned services and removed entry points only. Shared callers, registry,
# helpers and documentation retain the full-suite fallback.
PACKAGING_PATHS = (
    'tools/dev/packaging.py',
    'tools/dev/packaging_templates.py',
    'tools/packaging/function_order.py',
    'tools/packaging/function-order.sh',
    'tools/packaging/install.sh',
    'tools/packaging/relink-versioned.sh',
    'tools/packaging/pc-version.sh',
    'tools/packaging/check-pc-version.sh',
    'tools/packaging/check-pc-metadata.sh',
    'tools/packaging/release-stage.sh',
    'tools/packaging/release-deb.sh',
    'tools/packaging/release-rpm.sh',
    'tools/packaging/release-arch.sh',
)

HARNESS_DEPENDENCIES = {
    **dict.fromkeys(PACKAGING_PATHS, PACKAGING_HARNESSES),
    'tools/dev/test_runner.py': ('tools/dev/test_runner.test.py', 'tools/dev/cli.test.py',
                                 'tools/dev/sanitize.test.py'),
    'tools/bisection/contract-diff.py': ('tools/bisection/contract-diff.test.py',
                                        'tools/bisection/contract-modules.test.py',
                                        'tools/bisection/teardown.test.py'),
    'tools/bisection/contract_abi.py': ('tools/bisection/contract_abi.test.py',
                                        'tools/bisection/contract-modules.test.py',
                                        'tools/bisection/contract-diff.test.py',
                                        'tools/bisection/teardown.test.py'),
    'tools/bisection/contract_fixtures.py': ('tools/bisection/contract_fixtures.test.py',
                                             'tools/bisection/contract-modules.test.py',
                                             'tools/bisection/contract-diff.test.py',
                                             'tools/bisection/teardown.test.py'),
    'tools/bisection/contract_verdict.py': ('tools/bisection/contract_verdict.test.py',
                                            'tools/bisection/contract-modules.test.py',
                                            'tools/bisection/contract-diff.test.py',
                                            'tools/bisection/teardown.test.py'),
    'tools/bisection/contract_selection.py': ('tools/bisection/contract_selection.test.py',
                                              'tools/bisection/contract-modules.test.py',
                                              'tools/bisection/contract-diff.test.py',
                                              'tools/bisection/teardown.test.py'),
    'tools/bisection/teardown.py': ('tools/bisection/teardown.test.py',
                                  'tools/dev/measure/startup.test.py',
                                  'tools/generate-function-order.test.py'),
    'tools/bisection/system_import_profile.py': ('tools/packaging/function-order.test.py',),
    'tools/dev/native.py': ('tools/dev/check.test.py', 'tools/dev/backend.test.py',
                            'tools/dev/sanitize.test.py'),
    'tools/dev/check.py': ('tools/dev/check.test.py', 'tools/dev/backend.test.py',
                           'tools/dev/lint-crate-cfg-placement.test.py',
                           'tools/dev/cli.test.py'),
    'tools/dev/cli.py': ('tools/dev/cli.test.py', 'tools/dev/sanitize.test.py',
                         'tools/dev/lint-crate-cfg-placement.test.py',
                         'tools/dev/backend.test.py'),
    # The measure recipes (#1290). bench.py is every recipe module's service layer.
    'tools/dev/measure/bench.py': tuple(f'tools/dev/measure/{name}.test.py' for name in (
        'bench', 'campaigns', 'neutrality', 'probes', 'profile', 'recipes', 'snapshot', 'valgrind')),
    'tools/dev/measure/identity.py': ('tools/dev/measure/bench.test.py', 'tools/dev/measure/campaigns.test.py',
                                      'tools/dev/measure/valgrind.test.py'),
    'tools/dev/measure/reports.py': ('tools/dev/measure/bench.test.py', 'tools/dev/measure/campaigns.test.py',
                                     'tools/dev/measure/valgrind.test.py'),
    'tools/dev/measure/fake_world.py': ('tools/dev/measure/campaigns.test.py', 'tools/dev/measure/valgrind.test.py'),
    'tools/dev/measure/campaigns.py': ('tools/dev/measure/campaigns.test.py', 'tools/bisection/memory-defaults.test.py',
                                       'tools/packaging/function-order.test.py'),
    'tools/dev/measure/valgrind.py': ('tools/dev/measure/valgrind.test.py', 'tools/dev/measure/campaigns.test.py',
                                      'tools/packaging/function-order.test.py'),
    'tools/dev/measure/recipes.py': ('tools/dev/measure/recipes.test.py', 'tools/dev/cli.test.py'),
    'tools/dev/measure/neutrality.py': ('tools/dev/measure/neutrality.test.py', 'tools/dev/measure/probes.test.py',
                                        'tools/dev/measure/valgrind.test.py'),
    'tools/dev/measure/probes.py': ('tools/dev/measure/probes.test.py',),
    'tools/dev/measure/profile.py': ('tools/dev/measure/profile.test.py',),
    'tools/dev/measure/snapshot.py': ('tools/dev/measure/snapshot.test.py',),
    'tools/perf-gate/callgrind-ir.py': ('tools/perf-gate/callgrind-ir.test.py', 'tools/dev/measure/snapshot.test.py'),
    'tools/perf-gate/series.py': ('tools/perf-gate/series.test.py',),
}
# U3-owned compatibility services and entry points only. Shared callers and
# documentation stay unmapped: repository-wide audits read their tracked text.
COMPAT_HARNESS = (
    'tools/dev/compat/boundary.test.py',
    'tools/dev/compat/memory.test.py',
    'tools/dev/compat/recipes.test.py',
    'tools/tkrzw/runtime.test.py',
    'tools/dev/bash_floor.test.py',
    'tools/bisection/cell-identity.test.py',
    'tools/dev/measure/bench.test.py',
    'tools/dev/measure/profile.test.py',
)
HARNESS_DEPENDENCIES.update(dict.fromkeys((
    'tools/capture/run-capture.sh',
    'tools/container/runtime.sh',
    'tools/datagen/libpinyin-drop-in-differential.sh',
    'tools/dev/compat/identity.py',
    'tools/oracle/check-reproducible.sh',
    'tools/oracle/compat-inside.sh',
    'tools/oracle/compat.sh',
    'tools/oracle/docker-user-rt.sh',
    'tools/oracle/image-tag.sh',
    'tools/oracle/nightly-fixture-differentials.sh',
    'tools/oracle/pin.test.sh',
    'tools/oracle/run-differentials.sh',
    'tools/oracle/user-dir-round-trip.sh',
    'tools/oracle/verify-oracle-image.sh',
    'tools/tkrzw/runtime.py',
    'tools/tkrzw/runtime.sh',
    'tools/dev/compat/campaigns.py',
    'tools/dev/compat/commands.py',
    'tools/dev/compat/fixtures.py',
    'tools/dev/compat/guest.py',
    'tools/dev/compat/help.txt',
    'tools/dev/compat/help.py',
    'tools/dev/compat/images.py',
    'tools/dev/compat/launcher.py',
    'tools/dev/compat/process.py',
    'tools/oracle/round-trip-bootstrap.sh',
    'tools/dev/compat/boundary.py',
), COMPAT_HARNESS))



def changed_harness_files(files, paths):
    """Map changed paths conservatively; any unknown mapping selects all files."""
    selected = set()
    for path in paths:
        mapped = (path,) if path in files else HARNESS_DEPENDENCIES.get(path)
        if mapped is None or any(name not in files for name in mapped):
            return files
        selected.update(mapped)
    return [path for path in files if path in selected]


def changed_paths(base):
    """Tracked changes since BASE (including staged/worktree edits), plus untracked files.

    Disable rename folding so both old and new paths take part in selection.
    Git failures propagate: an invalid base must not silently select no tests.
    """
    paths = set()
    for args in (['diff', '--name-only', '-z', '--no-renames', base, '--'],
                 ['ls-files', '--others', '--exclude-standard', '-z']):
        result = subprocess.run(['git', *args], cwd=ROOT, capture_output=True, check=True)
        paths.update(os.fsdecode(path) for path in result.stdout.split(b'\0') if path)
    return sorted(paths, key=os.fsencode)


def select_harness_files(files, requested, changed):
    """Validate explicit files, or map a Git diff; return unique paths in discovery order."""
    if requested and changed is not None:
        raise ValueError('FILE and --changed cannot be combined')
    if changed is not None:
        return changed_harness_files(files, changed_paths(changed))
    selected = set()
    for value in requested:
        path = Path(os.path.abspath(ROOT / value))
        try:
            name = path.relative_to(ROOT).as_posix()
        except ValueError:
            raise ValueError(f'not a discovered harness file: {value}') from None
        if name not in files:
            raise ValueError(f'not a discovered harness file: {value}')
        selected.add(name)
    return [path for path in files if not requested or path in selected]


def run_tools(files=(), pattern=None, changed=None):
    """Run selected harness files concurrently, returning 1 if any fails.

    Return 2 before any harness runs when the temporary directory is
    memory-backed. Each harness runs without the credential-shaped
    variables; only their count is printed.
    """
    refusal = memory_backed_tmpdir()
    if refusal:
        print(refusal, file=sys.stderr, flush=True)
        return 2
    try:
        selected = select_harness_files(harness_files(), files, changed)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f'just test-tools: {error}', file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            detail = error.stderr.decode(errors='replace') if isinstance(error.stderr, bytes) else error.stderr
            print(detail, end='' if detail.endswith('\n') else '\n', file=sys.stderr)
        return 2
    env, removed = harness_environment(os.environ)
    print(f'tools harness: credential-shaped environment variables removed: {removed}', flush=True)

    def execute(path, context):
        if path in HARNESS_EXCLUDED:
            return None, ''
        # Harnesses are light work. Private roots keep real lock tests independent.
        try:
            with tempfile.TemporaryDirectory(prefix='oxpinyin-harness-') as directory:
                temporary = Path(directory) / 'tmp'
                locks = Path(directory) / 'locks'
                temporary.mkdir()
                locks.mkdir()
                child = dict(env, TMPDIR=str(temporary), TMP=str(temporary), TEMP=str(temporary),
                             OXPINYIN_LOCK_DIR=str(locks))
                command = [sys.executable, '-X', 'lazy_imports=normal', str(ROOT / path)]
                if pattern is not None:
                    command += ['-k', pattern]
                result = context.launch(command, env=child)
                output = (result.stdout + result.stderr).decode(errors='replace')
                if result.status == 5 and pattern is not None and not files:
                    # unittest's no-tests status is selection, unless the file was explicit.
                    output += f'\ntools harness unselected (no tests match -k {pattern!r}): {path}\n'
                    return None, output
                return result.status, output
        except OSError as error:
            return 1, f'{error}\n'

    def emit(path, result):
        if path in HARNESS_EXCLUDED:
            print(f'tools harness excluded (missing CI input): {path}', flush=True)
            return
        print(f'tools harness: {path}', flush=True)
        print(result[1], end='' if result[1].endswith('\n') or not result[1] else '\n', flush=True)

    units = [(path, lambda context, path=path: execute(path, context)) for path in selected]
    with slot.cancellation_handlers():
        try:
            results = pool.run_units(units, max(1, min(len(units), len(host.allowed_cpus()))),
                                     cancelled=slot.CANCELLED.is_set, emit=emit)
        except pool.UnitsCancelled:
            return 130
    status = int(any(result[0] for result in results))
    if pattern is not None and not any(result[0] is not None for result in results):
        print(f'just test-tools: no tests match -k {pattern!r} in any selected file', file=sys.stderr)
        return 1
    if status:
        print('::error::one or more tools/ Python harness tests failed', file=sys.stderr)
    return status


SANITIZER_TOOLCHAIN = 'nightly-2026-09-07'
SANITIZER_ALIASES = {'asan': 'address', 'tsan': 'thread', 'lsan': 'leak', 'msan': 'memory'}
SANITIZER_NAMES = ('address', 'thread', 'leak', 'memory')
SANITIZER_BACKENDS = ('bdb', 'kc', 'tkrzw')
SANITIZE_USAGE = ('usage: sanitize.sh [--check] <bdb|kc|tkrzw|all> '
                  '[address|thread|leak|memory]\n')


@dataclass(frozen=True)
class SanitizerBackend:
    """One store backend's sanitizer data: feature, flags and display names."""

    name: str
    feature: str
    locked: bool
    target_from_nightly: bool
    flags: Mapping[str, str]
    display: Mapping[str, str]
    override_options: bool = False


_COMMON_FLAGS = {
    'thread': '-Zsanitizer=thread -Cdebuginfo=1 -Cunsafe-allow-abi-mismatch=sanitizer',
    'leak': '-Zsanitizer=leak -Cdebuginfo=1 -Cunsafe-allow-abi-mismatch=sanitizer',
}
_COMMON_DISPLAY = {'address': 'AddressSanitizer', 'thread': 'ThreadSanitizer',
                   'leak': 'LeakSanitizer'}
BACKEND_DATA = {
    'bdb': SanitizerBackend(
        'bdb', 'bdb', locked=False, target_from_nightly=False,
        flags={'address': '-Zsanitizer=address -Cdebuginfo=1', **_COMMON_FLAGS},
        display=_COMMON_DISPLAY),
    'kc': SanitizerBackend(
        'kc', 'kyotocabinet', locked=False, target_from_nightly=False,
        flags={'address': '-Zsanitizer=address',
               'thread': '-Zsanitizer=thread -Cunsafe-allow-abi-mismatch=sanitizer',
               'leak': '-Zsanitizer=leak -Cunsafe-allow-abi-mismatch=sanitizer'},
        display=_COMMON_DISPLAY),
    'tkrzw': SanitizerBackend(
        'tkrzw', 'tkrzw', locked=True, target_from_nightly=True,
        flags={'address': '-D warnings -Zsanitizer=address -Cdebuginfo=1',
               'thread': '-D warnings ' + _COMMON_FLAGS['thread'],
               'leak': '-D warnings ' + _COMMON_FLAGS['leak']},
        display=_COMMON_DISPLAY, override_options=True),
}


def _nightly(environment):
    """Return the selected sanitizer nightly."""
    return environment.get('OXPINYIN_SANITIZER_TOOLCHAIN') or SANITIZER_TOOLCHAIN


def _toolchain_installed(tools, nightly, environment):
    """Return whether rustup lists the selected nightly."""
    try:
        result = tools.run(['rustup', 'toolchain', 'list'], environment=environment)
    except OSError:
        return False
    return any(line.startswith(nightly) for line in result.stdout.splitlines())


def _rustc_host(tools, environment, nightly=None):
    """Return the host triple rustc reports, with the nightly when given."""
    command = ['rustc'] + ([f'+{nightly}'] if nightly else []) + ['-vV']
    try:
        result = tools.run(command, environment=environment)
    except OSError:
        return ''
    for line in result.stdout.splitlines():
        if line.startswith('host: '):
            return line.removeprefix('host: ')
    return ''


def _toolchain_failure(backend, nightly, environment, out, err):
    """Print the missing-toolchain diagnostic on the stream the old gate used."""
    stream = out if backend.name == 'kc' else err
    print(f'{nightly} is not installed.', file=stream, flush=True)
    print(f'  install it with: rustup toolchain install {nightly} --profile minimal',
          file=stream, flush=True)
    if environment.get('OXPINYIN_SANITIZER_ALLOW_SKIP'):
        print('  SKIP: OXPINYIN_SANITIZER_ALLOW_SKIP is set; the gate did NOT run.',
              file=stream, flush=True)
        return 0
    print('  ERROR: the sanitizer gate cannot run. Install the toolchain, or set',
          file=stream, flush=True)
    print('  OXPINYIN_SANITIZER_ALLOW_SKIP=1 to allow skipping it.', file=stream, flush=True)
    return 1


def _sanitizer_options(backend, sanitizer, host_os, environment):
    """Return the child environment with the gate's sanitizer options applied."""
    child = dict(environment)
    if sanitizer == 'address':
        default = 'detect_leaks=0:abort_on_error=1'
        if backend.override_options or host_os == 'Darwin':
            child['ASAN_OPTIONS'] = default if backend.override_options else (
                child.get('ASAN_OPTIONS') or default)
    elif sanitizer == 'thread':
        if backend.override_options:
            child['TSAN_OPTIONS'] = 'abort_on_error=1'
        else:
            default = ('detect_deadlocks=0:abort_on_error=1'
                       if backend.name == 'kc' and host_os == 'Darwin' else 'abort_on_error=1')
            child['TSAN_OPTIONS'] = child.get('TSAN_OPTIONS') or default
    elif sanitizer == 'leak':
        child['LSAN_OPTIONS'] = ('abort_on_error=1' if backend.override_options else
                                 child.get('LSAN_OPTIONS') or 'abort_on_error=1')
    return child


def _launch(tools, argv, *, environment, err, **streams):
    """Run a required gate child; a launch failure has the shell's status, 127 or 126."""
    try:
        return tools.run(argv, environment=environment, **streams)
    except FileNotFoundError as error:
        print(f'{argv[0]}: command not found', file=err, flush=True)
        return subprocess.CompletedProcess([str(part) for part in argv],
                                           native.launch_status(error), '', '')
    except OSError as error:
        print(f'{argv[0]}: {error}', file=err, flush=True)
        return subprocess.CompletedProcess([str(part) for part in argv],
                                           native.launch_status(error), '', '')


def _cargo_test(backend, nightly, target):
    """Return the backend's cargo test argv, preserving the old argument order."""
    command = ['cargo', f'+{nightly}', 'test']
    if backend.locked:
        command += ['--locked', '--target', target, '-p', 'oxpinyin-store',
                    '--no-default-features', '--features', backend.feature]
    else:
        command += ['-p', 'oxpinyin-store', '--no-default-features',
                    '--features', backend.feature, '--target', target]
    return command


def _run_bdb(backend, sanitizer, *, environment, out, err, tools, host_os, nightly, target):
    display = backend.display[sanitizer]
    child = _sanitizer_options(backend, sanitizer, host_os, environment)
    test_environment = dict(child, RUSTFLAGS=backend.flags[sanitizer])
    command = _cargo_test(backend, nightly, target)
    if sanitizer == 'address':
        suffix = ('(LeakSanitizer off on Darwin)' if host_os == 'Darwin'
                  else '(+ LeakSanitizer)')
        print(f'--- 1/2: Rust under {display} {suffix} ---', file=out, flush=True)
    else:
        print(f'--- 1/2: Rust under {display} ---', file=out, flush=True)
    result = _launch(tools, command, environment=test_environment, err=err,
                     stdout=None, stderr=None)
    if result.returncode:
        return native.shell_status(result.returncode)
    if sanitizer != 'address':
        print('', file=out, flush=True)
        print("sanitizers: clean (libdb's own internals are NOT instrumented)", file=out, flush=True)
        return 0
    print('', file=out, flush=True)
    print('--- 2/2: the libdb call pattern under ASan + UBSan (C) ---', file=out, flush=True)
    work = tempfile.mkdtemp(prefix='tmp.')
    try:
        compile = ['gcc', '-Wall', '-Wextra', '-Werror', '-O1', '-g',
                   '-fsanitize=address,undefined', '-fno-sanitize-recover=all']
        include = child.get('OXPINYIN_BDB_INCLUDE_DIR')
        if include:
            compile.append(f'-I{include}')
        library = child.get('OXPINYIN_BDB_LIB_DIR')
        if library:
            compile.append(f'-L{library}')
        hash_walk = str(Path(work) / 'hash-walk')
        btree_order = str(Path(work) / 'btree-order')
        for output, source in ((hash_walk, 'tools/bdb/hash-walk.c'),
                               (btree_order, 'tools/bdb/btree-order.c')):
            result = _launch(tools, [*compile, '-o', output, source, '-ldb'],
                             environment=child, err=err, stdout=None, stderr=None)
            if result.returncode:
                return native.shell_status(result.returncode)
        result = _launch(
            tools, ['cargo', f'+{nightly}', 'build', '-p', 'oxpinyin-store',
                    '--no-default-features', '--features', 'bdb', '--target', target,
                    '--example', 'bdb_write_profile'],
            environment=child, err=err, stdout=None, stderr=None)
        if result.returncode:
            return native.shell_status(result.returncode)
        target_dir = Path(child.get('CARGO_TARGET_DIR') or 'target')
        example = target_dir / target / 'debug/examples/bdb_write_profile'
        result = _launch(tools, [example, str(Path(work) / 'user_bigram.db')],
                         environment=child, err=err, stdout=None, stderr=None)
        if result.returncode:
            return native.shell_status(result.returncode)
        print(f'walking the backend-written {work}/user_bigram.db', file=out, flush=True)
        result = _launch(tools, [hash_walk, str(Path(work) / 'user_bigram.db')],
                         environment=child, err=err, stdout=None, stderr=None)
        if result.returncode:
            return native.shell_status(result.returncode)
        data_dir = child.get('OXPINYIN_LIBPINYIN_DATA_DIR', '')
        if not data_dir:
            for candidate in ('/usr/lib/x86_64-linux-gnu/libpinyin/data',
                              '/usr/lib/aarch64-linux-gnu/libpinyin/data',
                              '/usr/lib64/libpinyin/data',
                              '/usr/lib/libpinyin/data',
                              '/usr/local/lib/libpinyin/data'):
                if (Path(candidate) / 'bigram.db').is_file():
                    data_dir = candidate
                    break
        if data_dir:
            print(f'walking {data_dir}/bigram.db', file=out, flush=True)
            result = _launch(tools, [hash_walk, f'{data_dir}/bigram.db'],
                             environment=child, err=err, stdout=None, stderr=None)
            if result.returncode:
                return native.shell_status(result.returncode)
        else:
            print('note: no installed libpinyin bigram.db; the real-file walk did not run.',
                  file=out, flush=True)
        print('', file=out, flush=True)
        print(f'exercising DB_BTREE create/put/cursor in {work}', file=out, flush=True)
        result = _launch(tools, [btree_order, str(Path(work) / 'btree-order.db')],
                         environment=child, err=err, stdout=subprocess.DEVNULL,
                         stderr=None)
        if result.returncode:
            return native.shell_status(result.returncode)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    print('', file=out, flush=True)
    print("sanitizers: clean (libdb's own internals are NOT instrumented)", file=out, flush=True)
    return 0


def _run_kc(backend, sanitizer, *, environment, out, err, tools, host_os, nightly, target):
    display = backend.display[sanitizer]
    child = _sanitizer_options(backend, sanitizer, host_os, environment)
    test_environment = dict(child, RUSTFLAGS=backend.flags[sanitizer])
    if sanitizer == 'address':
        suffix = ('(LeakSanitizer off on Darwin)' if host_os == 'Darwin'
                  else '(+ LeakSanitizer)')
        print(f'--- Rust under {display} {suffix} ---', file=out, flush=True)
    else:
        print(f'--- Rust under {display} ---', file=out, flush=True)
    result = _launch(tools, _cargo_test(backend, nightly, target),
                     environment=test_environment, err=err, stdout=None, stderr=None)
    if result.returncode:
        return native.shell_status(result.returncode)
    print('', file=out, flush=True)
    print("sanitizers: clean (libkyotocabinet's own internals are NOT instrumented)",
          file=out, flush=True)
    return 0


def _run_tkrzw(backend, sanitizer, *, environment, out, err, tools, host_os, nightly, target,
               check_object=None, caller_directory=None):
    display = backend.display[sanitizer]
    child = _sanitizer_options(backend, sanitizer, host_os, environment)
    child['RUSTFLAGS'] = backend.flags[sanitizer]
    if caller_directory is not None and child.get('LD_LIBRARY_PATH'):
        from .cell import library_path
        inherited = library_path(child['LD_LIBRARY_PATH'], caller_directory)
        if inherited:
            child['LD_LIBRARY_PATH'] = inherited
    if not (child.get('OXPINYIN_TKRZW_LIBRARY') or child.get('OXPINYIN_TKRZW_PREFIX')
            or child.get('OXPINYIN_TKRZW_PREFIXES')):
        try:
            host_library = tools.tkrzw_host(environment=child)
        except native.NativeRefused:
            print('ERROR: no verified libtkrzw; the sanitizer gate did NOT run.', file=err, flush=True)
            print('  Source the environment file tools/cloud/setup.sh writes, or provide a host',
                  file=err, flush=True)
            print('  libtkrzw that passes tools/tkrzw/distro-probe.sh (tkrzw_dbm_util on PATH).',
                  file=err, flush=True)
            print("  Ubuntu's packaged libtkrzw fails it: it cannot read files it writes.",
                  file=err, flush=True)
            return 1
        child['OXPINYIN_TKRZW_LIBRARY'] = str(host_library)
    command = _cargo_test(backend, nightly, target)
    if platform.system() == 'Linux' and sanitizer == 'address':
        command = ['setarch', platform.machine(), '-R', *command]
    print(f'--- building the tkrzw feature tests under {display} ---', file=out, flush=True)
    result = _launch(tools, [*command, '--no-run', '--message-format=json-render-diagnostics'],
                     environment=child, err=err, stdout=subprocess.PIPE, stderr=None)
    if result.returncode:
        return native.shell_status(result.returncode)
    executables = []
    for line in (result.stdout or '').splitlines():
        match = re.search(r'"executable":"([^"]*)"', line)
        if match:
            executables.append(match.group(1))
    package = ROOT / 'crates/oxpinyin-store'
    if check_object is None:
        def check_object(obj):
            return tools.tkrzw(obj, runner='run-sanitizers', side='subject',
                               cwd=package, environment=child)
    checked = 0
    library = None
    for executable in executables:
        try:
            resolved = check_object(Path(executable))
        except native.NativeRefused as error:
            print(f'REFUSED: {error}', file=err, flush=True)
            print('ERROR: the sanitizer gate did NOT run.', file=err, flush=True)
            return 1
        if resolved:
            library = resolved
            checked += 1
    if checked == 0:
        print('ERROR: no test binary loads libtkrzw; the sanitizer gate did NOT run.',
              file=err, flush=True)
        return 1
    print(f'libtkrzw: {checked} test binaries resolve it to {library}', file=out, flush=True)
    if sanitizer == 'address':
        print('', file=out, flush=True)
        print(f'--- Rust under {display} (LeakSanitizer off) ---', file=out, flush=True)
    else:
        print('', file=out, flush=True)
        print(f'--- Rust under {display} ---', file=out, flush=True)
    result = _launch(tools, command, environment=child, err=err,
                     stdout=None, stderr=None)
    if result.returncode:
        return native.shell_status(result.returncode)
    print('', file=out, flush=True)
    print("sanitizers: clean (libtkrzw's own internals are NOT instrumented)", file=out, flush=True)
    return 0


def _run_sanitizer_backend(backend, sanitizer, *, environment, out, err, tools,
                           host_os, caller_directory=None):
    """Run one backend's sanitizer gate and return its shell-style status."""
    if sanitizer not in backend.flags:
        print(f'unknown sanitizer: {sanitizer}', file=err, flush=True)
        return 2
    nightly = _nightly(environment)
    if not _toolchain_installed(tools, nightly, environment):
        return _toolchain_failure(backend, nightly, environment, out, err)
    if backend.target_from_nightly:
        target = environment.get('OXPINYIN_SANITIZER_TARGET') or _rustc_host(
            tools, environment, nightly)
    else:
        target = environment.get('OXPINYIN_SANITIZER_TARGET') or _rustc_host(tools, environment)
    if backend.name == 'bdb':
        return _run_bdb(backend, sanitizer, environment=environment, out=out, err=err,
                        tools=tools, host_os=host_os, nightly=nightly, target=target)
    if backend.name == 'kc':
        return _run_kc(backend, sanitizer, environment=environment, out=out, err=err,
                       tools=tools, host_os=host_os, nightly=nightly, target=target)
    return _run_tkrzw(backend, sanitizer, environment=environment, out=out, err=err,
                      tools=tools, host_os=host_os, nightly=nightly, target=target,
                      caller_directory=caller_directory)


def sanitize(argv=None, *, out=sys.stdout, err=sys.stderr, environment=None, tools=None,
             caller_directory=None):
    """Select and run one sanitizer gate, preserving the old entry grammar.

    Return the backend's shell-style status; for `all`, 0 when every gate
    succeeded and 1 otherwise; 2 on a usage error. `--check` validates and
    prints the selection without running a gate.
    """
    arguments = list(sys.argv[1:] if argv is None else argv)
    environment = dict(os.environ if environment is None else environment)
    check = False
    if arguments[:1] == ['--check']:
        check = True
        arguments = arguments[1:]
    if not 1 <= len(arguments) <= 2:
        err.write(SANITIZE_USAGE)
        return 2
    backend_name = arguments[0]
    sanitizer = arguments[1] if len(arguments) > 1 else 'address'
    sanitizer = SANITIZER_ALIASES.get(sanitizer, sanitizer)
    if backend_name not in (*SANITIZER_BACKENDS, 'all'):
        err.write(SANITIZE_USAGE)
        return 2
    if sanitizer not in SANITIZER_NAMES:
        err.write(SANITIZE_USAGE)
        return 2
    environment['OXPINYIN_SANITIZER_TOOLCHAIN'] = _nightly(environment)
    selection_os = environment.get('OXPINYIN_SANITIZER_HOST_OS') or platform.system()
    selection_arch = environment.get('OXPINYIN_SANITIZER_HOST_ARCH') or platform.machine()
    # The backend scripts read the real host; the override only selects and
    # refuses in the dispatcher, as tools/sanitize.sh did.
    host_os = platform.system()
    if sanitizer == 'memory':
        if selection_os == 'Darwin':
            print('sanitize: MemorySanitizer is unsupported on macOS: rustc has no Darwin '
                  'MSan target.', file=err, flush=True)
        else:
            print('sanitize: MemorySanitizer has no in-tree wrapper; use address, thread '
                  'or leak.', file=err, flush=True)
        return 1
    if sanitizer == 'leak':
        if backend_name in ('tkrzw', 'all'):
            print('sanitize: tkrzw LeakSanitizer is unsupported: the Rust harness keeps '
                  'process-lifetime allocations that LSan reports as leaks; use address '
                  'or thread.', file=err, flush=True)
            return 1
        if selection_os == 'Darwin' and selection_arch == 'arm64':
            print('sanitize: standalone LeakSanitizer is unsupported on macOS arm64; '
                  'use address (leak detection off on Darwin) or an x86_64 host.',
                  file=err, flush=True)
            return 1
    environment['OXPINYIN_SANITIZER'] = sanitizer
    if check:
        print(f'sanitize: check backend={backend_name} sanitizer={sanitizer} '
              f'host={selection_os}/{selection_arch}', file=out, flush=True)
        return 0
    if caller_directory is None:
        caller_directory = Path.cwd()
    tools = native.Tools(environment=environment) if tools is None else tools
    if backend_name != 'all':
        return _run_sanitizer_backend(
            BACKEND_DATA[backend_name], sanitizer, environment=environment,
            out=out, err=err, tools=tools, host_os=host_os,
            caller_directory=caller_directory)
    results = []
    failed = 0
    for name in SANITIZER_BACKENDS:
        print(f'=== sanitize {name} ===', file=out, flush=True)
        code = native.shell_status(_run_sanitizer_backend(
            BACKEND_DATA[name], sanitizer, environment=environment,
            out=out, err=err, tools=tools, host_os=host_os,
            caller_directory=caller_directory))
        results.append(f'{name}: exit {code}')
        failed = failed or code != 0
        print('', file=out, flush=True)
    for result in results:
        print(f'sanitize all: {result}', file=out, flush=True)
    return 0 if not failed else 1


def sanitize_toolchain():
    """Print the single sanitizer nightly for callers that select the toolchain."""
    print(SANITIZER_TOOLCHAIN)
    return 0


def _strip_separator(args):
    """Copy arguments, removing one leading -- separator if present."""
    args = list(args)
    if args[:1] == ['--']:
        args = args[1:]
    return args


def _nextest_args(args):
    """Translate libtest ignored-test switches before the first -- for nextest."""
    translated = []
    for arg in args:
        if arg == '--':
            translated.extend(args[len(translated):])
            break
        if arg == '--include-ignored':
            translated.append('--run-ignored=all')
        elif arg == '--ignored':
            translated.append('--run-ignored=only')
        else:
            translated.append(arg)
    return translated


def _options(args):
    """Read budget options before the nextest/libtest argument boundary.

    Return the normalized option map and whether a custom config/workspace
    path was supplied. Raise ValueError for duplicate or missing budget
    values and combined short options containing -j or -P.
    """
    names = {'--test-threads': 'threads', '--jobs': 'threads',
             '--profile': 'profile', '--build-jobs': 'build_jobs'}
    found = {}
    custom_config = False
    index = 0
    while index < len(args):
        arg = args[index]
        if arg == '--':
            break
        key, equals, value = arg.partition('=')
        name = names.get(key)
        if arg.startswith(('-j', '-P')) and not arg.startswith('--'):
            name = 'threads' if arg[1] == 'j' else 'profile'
            value = arg[2:].removeprefix('=')
            equals = bool(value)
        elif len(arg) > 2 and arg.startswith(('-v', '-q', '-r', '-h')) and any(c in arg[2:] for c in 'jP'):
            raise ValueError('combined short options containing -j or -P are not supported; separate them')
        if name is not None:
            if name in found:
                raise ValueError(f'{key} was specified more than once')
            if not equals:
                index += 1
                if index == len(args) or args[index] == '--':
                    raise ValueError(f'{key} needs a value')
                value = args[index]
            if not value:
                raise ValueError(f'{key} needs a value')
            found[name] = value
        if key in ('--config-file', '--tool-config-file', '--manifest-path', '--workspace-remap'):
            custom_config = True
        index += 1
    return found, custom_config


def _positive_threads(value):
    """Parse a positive nextest thread count bounded by sys.maxsize.

    Raise ValueError for relative counts, num-cpus, or any unsupported value.
    """
    if not re.fullmatch(r'\+?[0-9]+', value) or not 0 < int(value) <= sys.maxsize:
        raise ValueError('nextest thread overrides require a positive integer for memory admission; '
                         'relative counts and num-cpus are not supported as overrides')
    return int(value)


def _profile_threads(profile):
    """Resolve a profile thread count through .config/nextest.toml inheritance.

    Return None for num-cpus or an inherited default without a count. Raise
    ValueError for unknown/cyclic profiles or unsupported counts; file and
    TOML errors propagate.
    """
    with (ROOT / '.config/nextest.toml').open('rb') as source:
        profiles = tomllib.load(source)['profile']
    visited = set()
    current = profile
    while current is not None:
        if current in visited or current not in profiles:
            raise ValueError(f'unknown or cyclic nextest profile: {profile}')
        visited.add(current)
        config = profiles[current]
        if 'test-threads' in config:
            value = config['test-threads']
            if value == 'num-cpus':
                return None
            if type(value) is not int or not 0 < value <= sys.maxsize:
                raise ValueError(f'unsupported test-threads in profile {current}; use a positive integer or num-cpus')
            return value
        current = config.get('inherits', 'default' if current != 'default' else None)
    return None


def _thread_plan(args, environ, profile):
    """Resolve nextest profile, budget options, thread count, and its source.

    CLI counts override environment counts, then repository profiles apply.
    No-capture forces one thread. Custom config/workspace paths require an
    explicit count; ambiguous or unsupported settings raise ValueError.
    """
    options, custom_config = _options(args)
    if profile is not None and 'profile' in options:
        raise ValueError('--profile was specified more than once')
    selected = options.get('profile', profile if profile is not None else environ.get('NEXTEST_PROFILE', 'local'))
    if 'threads' in options:
        count, source = _positive_threads(options['threads']), 'user-set (nextest CLI)'
    elif 'NEXTEST_TEST_THREADS' in environ:
        count, source = _positive_threads(environ['NEXTEST_TEST_THREADS']), 'user-set (NEXTEST_TEST_THREADS)'
    else:
        if custom_config:
            raise ValueError('custom nextest config/workspace paths require an explicit positive '
                             '--test-threads or NEXTEST_TEST_THREADS value for memory admission')
        count = _profile_threads(selected)
        source = f'profile ({selected})' if count is not None else 'admitted'
    if any(arg in ('--no-capture', '--nocapture') for arg in args):
        count, source = 1, 'user-set (no-capture)'
    return selected, options, count, source


def _prepare_nextest(count, source):
    """Return an admission callback using the resolved nextest process count.

    When count is None, use the admitted Rust test-thread default; otherwise
    include the explicit count in the returned memory estimate.
    """
    def prepare(environ, resources, *, runner=False):
        """Copy the environment and estimate the resolved nextest thread budget.

        Return child settings, their explanations, and the larger of the heavy
        work estimate and nextest thread estimate, in MiB.
        """
        budget = dict(environ)
        if count is not None:
            budget.setdefault('RUST_TEST_THREADS', str(count))
        child, chosen, estimate = host.prepare_heavy_env(budget, resources, runner=runner)
        threads = count if count is not None else int(child['RUST_TEST_THREADS'])
        child['NEXTEST_TEST_THREADS'] = str(threads)
        chosen['NEXTEST_TEST_THREADS'] = (threads, source, count is not None)
        return child, chosen, max(estimate, threads * host.TEST_THREAD_PEAK_MIB)
    return prepare


def run(args=None, profile=None):
    """Run nextest under the shared slot with its final process budget.

    Default to command-line arguments and the local profile unless overridden.
    Return the raw child status, or print FAIL and return 2 for invalid plans
    or OS/value errors during admission and execution.
    """
    args = _strip_separator(sys.argv[1:] if args is None else args)
    try:
        selected, options, count, source = _thread_plan(args, os.environ, profile)
    except (ValueError, OSError, KeyError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr, flush=True)
        return 2
    args = _nextest_args(args)
    if 'profile' not in options:
        args = ['--profile', selected, *args]
    command = ['cargo', 'nextest', 'run', '--no-fail-fast', *args]
    environ = dict(os.environ)
    if 'build_jobs' in options:
        environ['CARGO_BUILD_JOBS'] = options['build_jobs']
    def report(admitted):
        """Print the selected profile, admitted thread count, and nextest command."""
        print(f'nextest profile: {selected}', file=sys.stderr, flush=True)
        print(f'nextest test threads: {admitted["NEXTEST_TEST_THREADS"]} ({source})',
              file=sys.stderr, flush=True)
        print('command: ' + shlex.join(command), file=sys.stderr, flush=True)
    try:
        return slot.run_command(command, env=environ, out=sys.stderr,
                                prepare_heavy_env=_prepare_nextest(count, source),
                                on_admitted=report)
    except (ValueError, OSError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr, flush=True)
        return 2


def run_doc(args=None):
    """Run cargo test --doc under the shared slot, as `just heavy` did.

    Default to command-line arguments. Return the child status, a signal or a
    cancellation as 128 plus its number, or print FAIL and return 2 for other
    OS/value errors during admission and execution.
    """
    args = _strip_separator(sys.argv[1:] if args is None else args)
    command = ['cargo', 'test', '--doc', *args]
    print('doctest step', file=sys.stderr, flush=True)
    print('command: ' + shlex.join(command), file=sys.stderr, flush=True)
    try:
        status = slot.run_command(command, env=os.environ, out=sys.stderr)
    except slot.Cancelled as exc:
        print(exc, file=sys.stderr, flush=True)
        return slot.cancellation_status()
    except (ValueError, OSError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr, flush=True)
        return 2
    return 128 - status if status < 0 else status
