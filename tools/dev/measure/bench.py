"""Shared services for the measurement and profile runners (#1290).

The shell runners each repeated the same machinery around the `bisect --perf`
harness. It lives here once: refusing a count that is not a plain decimal, the
function-order guard that runs before any worker starts, the libtkrzw runtime
check on every object a worker opens, building the harness, launching a measured
worker pinned to a CPU with the object's directory first on the loader path, and
a build under the heavy slot. Recipes (recipes.py) compose these.

Measured launches are serial. A timed or resident-set sample belongs to its arm
only if nothing else runs beside it, so `round_robin` runs one process at a time
and interleaves the arms, spreading host drift over all of them. Builds and
untimed collections are not measurements: `build_all` runs independent builds as
units of one admitted pool. Standard library only.
"""
import contextlib
import dataclasses
import functools
import importlib.util
import io
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import traceback
import types
from collections.abc import Callable, Iterator, Mapping, Sequence

from .. import host, pool, slot
from ..runners import Finished, emit, status as shell_status, write
from tools.tkrzw.runtime import RuntimeRefused, check

ROOT = Path(__file__).resolve().parents[3]
BISECT = ROOT / 'tools/bisection'
HARNESS_SOURCE = BISECT / 'bisect.c'
# The flags every harness build uses; recipes add -g or drop -Werror as their script did.
STRICT_FLAGS = ('-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2')
FUNCTION_ORDER_OPTIONS = ('--function-order', '--unordered-arm')
MACH_O_MAGIC = (b'\xcf\xfa\xed\xfe', b'\xce\xfa\xed\xfe', b'\xfe\xed\xfa\xcf', b'\xfe\xed\xfa\xce')


def fail(message: str = '', code: int = 1):
    """Stop the run: MESSAGE on stderr when there is one, CODE as the exit status."""
    if message:
        emit(message, error=True)
    raise Finished(code)


def setting(env: Mapping[str, str], name: str, default: str) -> str:
    """The variable's value, or DEFAULT when it is unset or empty (the shell's ${NAME:-DEFAULT})."""
    return env.get(name) or default


class Count(int):
    """A validated decimal count that prints as it was written.

    Loops see the number; the banners the runners print show the text they were
    given, so a leading zero (`08`) stays in the line as it always did.
    """

    def __new__(cls, text: str):
        count = super().__new__(cls, int(text, 10))
        count.text = text
        return count

    def __str__(self) -> str:
        return self.text

    __repr__ = __str__


def decimal_count(label: str, name: str, value: str) -> Count:
    """Return VALUE as a count; refuse anything but plain decimal digits with status 2.

    A leading zero is decimal (`08` is 8, as `seq` reads it), never octal, and no
    text is evaluated as an expression, which is what a shell arithmetic loop
    did with the same value.
    """
    if not re.fullmatch('[0-9]+', value):
        fail(f"{label}: {name} must be a decimal count (got '{value}')", 2)
    return Count(value)


def count(label: str, env: Mapping[str, str], name: str, default: str) -> Count:
    """The decimal count in env[NAME], or DEFAULT when unset or empty."""
    return decimal_count(label, name, setting(env, name, default))


def function_order_arguments(argv: Sequence[str]) -> list[str]:
    """Validate the legacy runners' only options: `--function-order ARM/FACADE=POLICY` and `--unordered-arm ARM`.

    Each takes a non-empty value; a missing or empty value stops with status 2 and
    no message, anything else with status 2 naming it. Return the flat option list
    for `check_function_order`.
    """
    options, rest = [], list(argv)
    while rest:
        flag = rest.pop(0)
        if flag not in FUNCTION_ORDER_OPTIONS:
            fail(f'unknown function-order argument: {flag}', 2)
        if not rest or not rest[0]:
            raise Finished(2)
        options += [flag, rest.pop(0)]
    return options


@functools.cache
def script(relative: str):
    """Load a repository script by its path below the checkout.

    Scripts with a hyphen in their name cannot be imported; this is how the
    report and policy tools are reused without a second copy of their logic.
    """
    path = ROOT / relative
    spec = importlib.util.spec_from_file_location(re.sub(r'\W', '_', path.stem), path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@contextlib.contextmanager
def exported(variables: Mapping[str, str] | None) -> Iterator[None]:
    """Give the body the process environment VARIABLES (when given), as a script's exports reached a child.

    A tool run in-process sees os.environ, not the dictionary a recipe built for its
    children; this puts one in place of the other for the length of the call.
    """
    if variables is None:
        yield
        return
    saved = dict(os.environ)
    os.environ.clear()
    os.environ.update(variables)
    try:
        yield
    finally:
        os.environ.clear()
        os.environ.update(saved)


def check_function_order(libraries: Sequence[tuple[str, Path]], options: Sequence[str], *,
                         references: Sequence[str] = (), arm: str | None = None,
                         env: Mapping[str, str] | None = None) -> None:
    """Refuse an unordered shipping library before any output is created or worker started.

    LIBRARIES are ('ARM/FACADE', object) pairs in the order the runner named them;
    REFERENCES are the arms that are measured but never ordered; ARM names the
    arm for unqualified 'FACADE' keys; ENV is the environment the script had
    exported by then, when that is more than the caller's own. A refusal prints
    the policy tool's own message and stops with status 1, as `function_order.py`
    did when run alone.
    """
    argv = []
    if arm is not None:
        argv += ['--arm', arm]
    for key, path in libraries:
        argv += ['--library', f'{key}={path}']
    for reference in references:
        argv += ['--reference-arm', reference]
    try:
        with exported(env):
            script('tools/packaging/function_order.py').main([*argv, *options])
    except SystemExit as refusal:
        raise Finished(refusal.code if isinstance(refusal.code, int) else 1) from None


def runtime_check(label: str, cell: str, side: str, obj, *, env: Mapping[str, str],
                  cwd: Path | None = None) -> None:
    """Refuse OBJ unless every libtkrzw it resolves is the runtime setup verified.

    The check is tools/tkrzw/runtime.sh's, through its Python adapter, run in the
    environment and directory the worker will have. CELL is advisory; SIDE
    decides only the diagnostic's wording. A refusal is a failed run.
    """
    try:
        check(cell, label, side, obj, env=dict(env), cwd=cwd)
    except RuntimeRefused as error:
        fail('REFUSED: ' + str(error))


def native(path: Path) -> bool:
    """True for an ELF or Mach-O file: only a native executable carries its own libtkrzw dependencies."""
    try:
        with open(path, 'rb') as stream:
            magic = stream.read(4)
    except OSError:
        return False
    return magic == b'\x7fELF' or magic in MACH_O_MAGIC


def guard(label: str, cell: str, side: str, obj, command: Sequence, *, env: Mapping[str, str],
          cwd: Path | None = None) -> None:
    """Check OBJ and, when it is native, the executable that will open it.

    A dlopen subject and a directly linked worker can each bring libtkrzw. A
    profiler or shell wrapper in front of the worker is the executable checked
    here; the subject it profiles is OBJ.
    """
    runtime_check(label, cell, side, obj, env=env, cwd=cwd)
    executable = shutil.which(str(command[0]), path=env.get('PATH')) if command else None
    if executable and executable != str(obj) and native(Path(executable)):
        runtime_check(label, cell, side, executable, env=env, cwd=cwd)


def loader_path(directory, inherited: str | None) -> str:
    """The object's directory first, then the inherited LD_LIBRARY_PATH when it has one."""
    return str(directory) + (':' + inherited if inherited else '')


def pinned(command: Sequence, cpu, *, required: bool = True,
           which: Callable[[str], str | None] | None = None) -> list[str]:
    """Prefix COMMAND with `taskset -c CPU`.

    REQUIRED always pins, so a missing taskset fails the run; otherwise the
    command is pinned only when taskset exists, as on images without util-linux.
    WHICH finds an executable (default shutil.which); tests supply their own.
    """
    prefix = ['taskset', '-c', str(cpu)] if required or (which or shutil.which)('taskset') else []
    return [*prefix, *map(str, command)]


@contextlib.contextmanager
def working_directory(path) -> Iterator[None]:
    """Run the body in PATH, as a script that `cd`s into its own directory did."""
    previous = os.getcwd()
    os.chdir(path)
    try:
        yield
    finally:
        os.chdir(previous)


def find_named(root, name: str, *, any_type: bool = False) -> Path | None:
    """The first entry named NAME below ROOT that is a file or a symlink, else None.

    The equivalent of `find ROOT -name NAME ( -type f -o -type l ) -print -quit`
    (ANY_TYPE drops the type test), visiting entries in sorted order so the answer
    does not depend on the filesystem. A missing ROOT has none.
    """
    try:
        entries = sorted(os.scandir(root), key=lambda entry: entry.name)
    except OSError:
        return None
    for entry in entries:
        directory = entry.is_dir(follow_symlinks=False)
        if entry.name == name and (any_type or entry.is_symlink() or entry.is_file(follow_symlinks=False)):
            return Path(entry.path)
        if directory:
            found = find_named(entry.path, name, any_type=any_type)
            if found is not None:
                return found
    return None


def find_library(root, name: str = 'libpinyin.so') -> Path:
    """The library NAME below ROOT, or stop with the runners' message."""
    found = find_named(root, name)
    if found is None:
        fail(f'fatal: {name} not found under {root}')
    return found


def utc_now() -> str:
    """The time as `date -u +%Y-%m-%dT%H:%M:%SZ` prints it."""
    return time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())


def append(path, data: bytes) -> None:
    """Append DATA to PATH, creating it: the `>>` redirection of a worker's output."""
    with open(path, 'ab') as stream:
        stream.write(data)


def copy_file(source, destination) -> None:
    """Copy SOURCE over DESTINATION as `cp` does, content and mode; a failure ends the run with status 1.

    The messages are cp's own for the two ways a copy fails: no source to stat, or no file to create.
    """
    try:
        shutil.copy(source, destination)
    except OSError as error:
        if not os.path.exists(source):
            fail(f"cp: cannot stat '{source}': No such file or directory")
        fail(f"cp: cannot create regular file '{destination}': {error.strerror}")


def tee(path, text: str) -> None:
    """Print TEXT and keep it in PATH: the `{ …; } | tee PATH` of a runner's identity tables."""
    sys.stdout.write(text)
    sys.stdout.flush()
    Path(path).write_text(text)


def capture(command: Sequence, *, env: Mapping[str, str] | None = None, cwd=None,
            merge_stderr: bool = False) -> pool.Completed:
    """Run COMMAND with its output captured; return Completed(status, stdout, stderr).

    The status is the shell's (128 plus the signal). The command runs in its own
    process group through pool.py's unit launcher: it is polled for cancellation
    and reaped, with its descendants, before any error propagates. A command that
    does not exist raises FileNotFoundError, which the shared failure ladder
    (runners.guarded) reports with status 127.
    """
    command = [str(part) for part in command]
    sys.stdout.flush()
    sys.stderr.flush()
    context = pool.UnitContext('measure', slot.CANCELLED,
                               environment=dict(os.environ if env is None else env))
    done = context.launch(command, cwd=cwd, merge_stderr=merge_stderr)
    return done._replace(status=shell_status(done.status))


def run(command: Sequence, *, env: Mapping[str, str] | None = None, cwd=None, stdout=None,
        check_status: bool = True) -> int:
    """Run COMMAND on the console (STDOUT may redirect it) and return the shell status.

    A nonzero status ends the run with that status unless CHECK_STATUS is false.
    Cancellation terminates and reaps the command's process group first.
    """
    command = [str(part) for part in command]
    sys.stdout.flush()
    sys.stderr.flush()
    with subprocess.Popen(command, env=None if env is None else dict(env), cwd=cwd, stdout=stdout,
                          start_new_session=True) as process:
        code = shell_status(slot.wait_child(process))
    if check_status and code:
        raise Finished(code)
    return code


def heavy(command: Sequence, *, env: Mapping[str, str] | None = None, stdout=None,
          check_status: bool = True) -> int:
    """Run COMMAND holding one heavy slot and return its shell status.

    The former `run-all.py --heavy-slot --`: admission waits are printed, the run
    ends with the slot's trailer line on stderr, and a nonzero status ends the
    runner with it unless CHECK_STATUS is false.
    """
    code = shell_status(slot.run_command([str(part) for part in command], env=env, stdout=stdout, report=True))
    if check_status and code:
        raise Finished(code)
    return code


def build_all(builds: Sequence[tuple[str, Callable[[pool.UnitContext, int], bytes | None]]],
              label: str, *, env: Mapping[str, str] | None = None) -> None:
    """Run independent BUILDS, (name, function(context, jobs)) pairs, as units of one admitted pool.

    Builds are not measurements, so nothing needs them one at a time. The pool takes
    the heavy slot once and plans from the CPUs and memory admission grants
    (pool.plan, a rustc allowance per job): how many builds overlap and the Cargo jobs
    each one builds with, so a host with many CPUs and little memory builds them one
    by one with fewer jobs, as the heavy-slot builds did. An explicit CARGO_BUILD_JOBS
    stays as given and is charged in full (AGENTS.md, rule 8): only the overlap
    adapts, and a host that cannot hold one build at that count refuses it. A unit
    returns the bytes its build printed, replayed on stderr in build order whatever
    order they finish in; the earliest failure ends the run with its status.
    """
    environment = dict(os.environ if env is None else env)

    def replay(name, output):
        if output:
            write(output, error=True)

    def planned(function):
        return lambda context: function(context, int(context.environment[pool.POOL_JOBS]))

    prepare = pool.admission_preparer(len(builds), None, 0, host.RUSTC_PEAK_MIB,
                                      explicit=environment.get('CARGO_BUILD_JOBS'), job_name='cargo job')
    pool.run_admitted([(name, planned(function)) for name, function in builds], label,
                      cancelled=slot.CANCELLED.is_set, prepare=prepare, env=environment, emit=replay,
                      cancel_error=slot.Cancelled, out=sys.stderr)


def cargo(context: pool.UnitContext, command: Sequence, jobs: int) -> bytes:
    """Run a cargo COMMAND as a unit of `build_all` and return its merged output.

    The unit's admitted environment applies with CARGO_BUILD_JOBS set to the jobs
    the pool planned for it unless the caller set one. A failed build replays its
    output and ends the run with its status.
    """
    environment = dict(context.environment)
    environment.setdefault('CARGO_BUILD_JOBS', str(jobs))
    done = context.launch([str(part) for part in command], env=environment, merge_stderr=True)
    if done.status:
        write(done.stdout, error=True)
        raise Finished(shell_status(done.status))
    return done.stdout


def harness_build(output, *, source=HARNESS_SOURCE, compiler: str = 'gcc',
                  flags: Sequence[str] = STRICT_FLAGS, libraries: Sequence[str] = ('-ldl',)) -> list[str]:
    """The compile command for the `bisect` harness: COMPILER FLAGS -o OUTPUT SOURCE LIBRARIES."""
    return [compiler, *flags, '-o', str(output), str(source), *libraries]


@dataclasses.dataclass(frozen=True)
class Arm:
    """One measured cell: PERF_BACKEND's label, what the harness opens, and where its loader looks first.

    LIBDIR None leaves LD_LIBRARY_PATH as the caller had it.
    """
    label: str
    so: Path
    data: Path
    libdir: Path | None = None


@dataclasses.dataclass
class Bench:
    """How a runner launches the `bisect --perf` harness (one per recipe run).

    LABEL is the legacy script name that still names the runner in every refusal;
    CELL is the runtime check's advisory backend; REQUIRED says whether taskset
    must exist or pins only when it does. Every launch is checked first, in the
    environment and directory it will have, and runs alone.
    """
    label: str
    env: Mapping[str, str]
    harness: Path
    cpu: str
    cell: str = 'auto'
    required: bool = True

    def environment(self, arm: Arm, mode: str, cycles, repeats=None, extra: Mapping[str, str] | None = None) -> dict:
        """The worker's environment: the caller's plus the loader path and the PERF_* settings."""
        env = dict(self.env)
        if arm.libdir is not None:
            env['LD_LIBRARY_PATH'] = loader_path(arm.libdir, self.env.get('LD_LIBRARY_PATH'))
        env.update(PERF_BACKEND=arm.label, PERF_MODE=mode, PERF_CYCLES=str(cycles))
        if repeats is not None:
            env['PERF_REPEATS'] = str(repeats)
        env.update(extra or {})
        return env

    def standalone(self, side: str, arm: Arm, cell: str | None = None) -> None:
        """Check ARM's object once as SIDE (`pin` or `subject`) with its loader path, before any launch."""
        env = dict(self.env)
        if arm.libdir is not None:
            env['LD_LIBRARY_PATH'] = loader_path(arm.libdir, self.env.get('LD_LIBRARY_PATH'))
        runtime_check(self.label, cell or self.cell, side, arm.so, env=env)

    def launch(self, arm: Arm, mode: str, cycles, *, repeats=None, extra: Mapping[str, str] | None = None,
               prefix: Sequence = (), diagnostics=None) -> pool.Completed:
        """Check ARM, then run the harness on it pinned to the CPU and return what it printed.

        PREFIX is a profiler in front of the harness (valgrind and its options). The shell
        sent a worker's whole chain, its libtkrzw check included, to the worker's stderr
        file; DIAGNOSTICS names that file, and a refusal is appended there and not printed.
        """
        env = self.environment(arm, mode, cycles, repeats, extra)
        command = [*map(str, prefix), str(self.harness), '--perf', str(arm.so), str(arm.data)]
        if diagnostics is None:
            guard(self.label, self.cell, arm.label, arm.so, command, env=env)
        else:
            refusal = io.StringIO()
            try:
                with contextlib.redirect_stderr(refusal):
                    guard(self.label, self.cell, arm.label, arm.so, command, env=env)
            except Finished:
                append(diagnostics, refusal.getvalue().encode())
                raise
        return capture(pinned(command, self.cpu, required=self.required), env=env)

    def sample(self, arm: Arm, mode: str, out, err, cycles, *, repeats=None, extra: Mapping[str, str] | None = None) -> None:
        """Launch one worker, appending its stdout to OUT and its stderr to ERR; its failure ends the run.

        Both files exist before the check runs, as the redirections created them.
        """
        append(out, b'')
        append(err, b'')
        done = self.launch(arm, mode, cycles, repeats=repeats, extra=extra, diagnostics=err)
        append(out, done.stdout)
        append(err, done.stderr)
        if done.status:
            raise Finished(done.status)


def round_robin(rounds: int, order: Sequence[tuple[Arm, str]], step: Callable[[Arm, str], None]) -> None:
    """Take ROUNDS rounds; each runs STEP(arm, mode) for every entry of ORDER, in order.

    Serial on purpose: every step is a timed or resident-set sample, and a process
    running beside it, this campaign's or another's, would measure the host and
    not the arm. Interleaving the arms puts a drift in host state on all of them.
    """
    for _ in range(rounds):
        for arm, mode in order:
            step(arm, mode)


@contextlib.contextmanager
def as_script(path) -> Iterator[None]:
    """Run the body as the process the interpreter would have started for the script at PATH.

    argparse names its program from sys.argv[0], and from `python -m` when __main__
    has a module spec; a report script run in-process still calls itself by its file name.
    """
    argv, main = sys.argv, sys.modules.get('__main__')
    sys.argv = [str(path)]
    sys.modules['__main__'] = types.ModuleType('__main__')
    try:
        yield
    finally:
        sys.argv = argv
        if main is None:
            del sys.modules['__main__']
        else:
            sys.modules['__main__'] = main


def report(relative: str, argv: Sequence[str]) -> tuple[int, str]:
    """Run a report script's main(ARGV) and return (status, what it printed).

    The failures are the interpreter's own. A script it cannot open is named on
    stderr and is status 2; a SystemExit message goes to stderr and is status 1;
    an exception prints its traceback and is status 1. The output stays whole so a
    caller can write it where the shell redirected it, whatever the status.
    """
    path = ROOT / relative
    try:
        path.open('rb').close()
    except OSError as error:
        emit(f"{sys.executable}: can't open file {str(path)!r}: [Errno {error.errno}] {error.strerror}", error=True)
        return 2, ''
    buffer = io.StringIO()
    code = 0
    with contextlib.redirect_stdout(buffer), as_script(path):
        try:
            code = script(relative).main(list(argv)) or 0
        except SystemExit as error:
            code = error.code
            if isinstance(code, str):
                emit(code, error=True)
                code = 1
            code = code or 0
        except (Finished, slot.Cancelled, pool.UnitStopped, pool.UnitsCancelled):
            raise
        except Exception:
            traceback.print_exc()
            code = 1
    return code, buffer.getvalue()
