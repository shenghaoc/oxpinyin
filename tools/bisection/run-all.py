#!/usr/bin/env python3
"""Execute the live parity registry. Standard library only; never cache the oracle."""
import argparse
import concurrent.futures
import contextlib
import errno
try:
    import fcntl
except ImportError:
    fcntl = None
import json
import os
from pathlib import Path
import re
import signal
import shutil
import subprocess
import sys
import tempfile
import threading
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
CELLS = ('bdb', 'kc', 'tkrzw')


@contextlib.contextmanager
def scratch_directory(log_dir):
    with tempfile.TemporaryDirectory(prefix='oxpinyin-gates-') as name:
        try:
            yield name
        finally:
            if log_dir:
                for log in Path(name).rglob('*.log'):
                    dest = log_dir / log.relative_to(name)
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(log, dest)


CANCELLED = threading.Event()


class Cancelled(ValueError):
    pass


def check_cancelled():
    if CANCELLED.is_set():
        raise Cancelled('cancelled; child process groups terminated and reaped')


@contextlib.contextmanager
def cancellation_handlers():
    CANCELLED.clear()
    previous = {}
    for name in ('SIGTERM', 'SIGINT', 'SIGHUP'):
        signum = getattr(signal, name, None)
        if signum is not None:
            previous[signum] = signal.signal(signum, lambda *_: CANCELLED.set())
    try:
        yield
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)


def terminate_group(process):
    """Stop the group and reap its leader before the caller releases locks."""
    with contextlib.suppress(ProcessLookupError):
        os.killpg(process.pid, signal.SIGTERM)
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        pass
    # The leader can exit while a descendant still runs.
    with contextlib.suppress(ProcessLookupError):
        os.killpg(process.pid, signal.SIGKILL)
    process.wait()


def wait_child(process, timeout=None):
    deadline = None if timeout is None else time.monotonic() + timeout
    try:
        while True:
            check_cancelled()
            remaining = None if deadline is None else deadline - time.monotonic()
            if remaining is not None and remaining <= 0:
                raise subprocess.TimeoutExpired(process.args, timeout)
            try:
                rc = process.wait(timeout=min(0.1, remaining) if remaining is not None else 0.1)
                check_cancelled()
                return rc
            except subprocess.TimeoutExpired:
                continue
    except BaseException:
        terminate_group(process)
        raise


def positive(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError('must be positive')
    return number


def assignments(values, allowed, parser):
    result = {}
    for value in values:
        key, sep, item = value.partition('=')
        if not sep or key not in allowed or not item:
            parser.error(f'invalid assignment: {value}')
        result[key] = item
    return result


def execute(command, env, cwd, timeout, output, slot=None):
    """Bound the whole process group, including grandchildren retaining stdout."""
    start = time.monotonic()
    with output.open('wb') as log:
        try:
            process = start_child(command, env, slot=slot, cwd=cwd, stdout=log,
                                  stderr=subprocess.STDOUT, start_new_session=True)
        except OSError as exc:
            return 1, time.monotonic() - start, str(exc), False
        expired = False
        try:
            wait_child(process, timeout)
        except subprocess.TimeoutExpired:
            expired = True
    text = output.read_text(errors='replace')
    return process.returncode, time.monotonic() - start, text, expired


def reason_for(rc, output, timeout, expired=False):
    # Only the aggregate deadline is a timeout here; a runner that wraps its own
    # processes in timeout(1) and exits 124 is reported by its own output.
    if expired:
        return f'timeout after {timeout}s (exit {rc})'
    lines = [line.strip() for line in output.splitlines() if line.strip()]
    important = [line for line in lines if re.search(
        r'FAIL|DIVERGEN|SKIP|missing input|fatal:|Assertion .* failed|error:', line)]
    if rc == 0:
        return 'completed'
    # Preserve the concrete failure rather than a shell crash wrapper.
    return (important[-1] if important else (lines[-1] if lines else f'exit {rc}'))[:400]


def normal_tag(tag):
    """`#640`, `640`, `W11`, `row:54` and `Row 54` are the same selector (as contract-diff.py)."""
    tag = str(tag).strip().lstrip('#').lower()
    return re.sub(r'^row[ :]*', 'row:', tag)


def contract_tag_index():
    """contract-diff.py's tag index: {'tags': {tag: [case IDs]}, 'modes': {case ID: facade}}."""
    command = [sys.executable, str(HERE / 'contract-diff.py'), '--list-tags']
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    if result.returncode:
        raise ValueError(result.stderr.strip() or 'contract-diff.py --list-tags failed')
    return json.loads(result.stdout)


def contract_cases_for(requested, index):
    """The contract case IDs the requested tags select, in registration order,
    with the tag(s) that selected each: {case ID: [tag, ...]}."""
    chosen = {}
    for tag in sorted(requested):
        for name in index['tags'].get(tag, []):
            chosen.setdefault(name, []).append(tag)
    return chosen


def timing_of(output):
    """The phase split a runner reports: its last stdout line that is a JSON
    object with a `timing` key (contract-diff.py prints one), else None."""
    for line in reversed(output.splitlines()):
        if line.startswith('{"timing"') or line.startswith('{"timing":'):
            try:
                return json.loads(line)['timing']
            except (ValueError, KeyError):
                return None
    return None


def planned_inputs(row, base_env, substitutions):
    """The input paths a runner will check, as the plan shows them: resolved
    the way run() resolves them, with the row's own env entries applied and
    `{env[NAME]}` read from that environment. A path naming a variable the
    host does not set is shown unresolved; the runner skips on it later."""
    env = dict(base_env)
    shown_substitutions = dict(substitutions, work='<work>')
    try:
        env.update({key: value.format_map(shown_substitutions) for key, value in row.get('env', {}).items()})
    except KeyError:
        pass
    shown_substitutions['env'] = env
    inputs = []
    for item in row.get('input_paths', []):
        try:
            inputs.append(item.format_map(shown_substitutions))
        except KeyError:
            inputs.append(item)
    return inputs or ['none']


def plan_lines(cell, jobs, contract_cases, reasons, index):
    """The selection plan for one cell: each runner, why it runs, the inputs
    it needs and whether it needs the oracle; for a tag-selected contract
    run, the case IDs, why each was selected and their facades."""
    lines = [f'plan ({cell}): {len(jobs)} runner{"" if len(jobs) == 1 else "s"}']
    for row, _, env, substitutions in jobs:
        inputs = planned_inputs(row, env, substitutions)
        lines.append(f"  {row['name']}: {reasons[row['name']]}; oracle: "
                     f"{'yes' if row.get('oracle', True) else 'no'}; inputs: {', '.join(inputs)}")
        if row['name'] == 'contract-diff' and contract_cases:
            modes = {}
            for name in contract_cases:
                mode = index['modes'].get(name, '?')
                modes[mode] = modes.get(mode, 0) + 1
            lines.append('    %d contract case%s; facades: %s' % (
                len(contract_cases), '' if len(contract_cases) == 1 else 's',
                ', '.join('%s %d' % item for item in sorted(modes.items()))))
            names = list(contract_cases)
            shown = names if len(names) <= 50 else names[:3]
            width = max(len(name) for name in shown)
            for name in shown:
                lines.append('    %-*s  %-7s issue %s' % (width, name, index['modes'].get(name, '?'),
                                                        ', '.join(contract_cases[name])))
            if len(names) > 50:
                lines.append('    ... %d more (contract-diff.py --list-tags lists them)' % (len(names) - 3))
    return lines
ARTIFACTS = ('pinyin-capi', 'zhuyin-capi', 'dictool', 'user-rt-test')


def artifact_union(rows):
    """The artifacts the selected runners need, in ARTIFACTS order: the one
    build a cell runs. Every row declares its set; a row without one is a
    registry error, not a request for everything."""
    needed = set()
    for row in rows:
        declared = row.get('artifacts')
        if declared is None:
            raise ValueError(f"runners.json: {row['name']} declares no artifacts list")
        unknown = set(declared) - set(ARTIFACTS)
        if unknown:
            raise ValueError(f"runners.json: {row['name']} names unknown artifacts: {', '.join(sorted(unknown))}")
        needed.update(declared)
    return [artifact for artifact in ARTIFACTS if artifact in needed]
ARTIFACT_ENV = {'pinyin-capi': 'OXPINYIN_CAPI_SO', 'zhuyin-capi': 'OXPINYIN_ZHUYIN_SO',
                'dictool': 'OXPINYIN_DICTOOL', 'user-rt-test': 'OXPINYIN_USER_RT_TEST'}


def needed_artifacts(rows, env):
    """The artifacts the selected rows execute, name -> path, from each row's
    declared `artifacts` list: the same declarations that chose what the cell
    built, so the identity check covers exactly what the runners load or run
    (dictool for import-diff included, not only the {capi}/{zhuyin} paths)."""
    needed = {}
    for row in rows:
        for name in row.get('artifacts', []):
            needed[name] = env[ARTIFACT_ENV[name]]
    return needed


def identity_of(target_dir):
    """The identity cell-artifacts.sh recorded for a target directory, or None."""
    path = Path(target_dir) / 'oxpinyin-cell-identity.json'
    return json.loads(path.read_text()) if path.is_file() else None


def check_identity(identity, root, cell, needed):
    """A hard error unless the target directory's identity is this worktree's,
    this cell's, the dev profile's, and every artifact the run will execute
    (`needed`: name -> path) is the file the record staged, unchanged since.
    A missing record, another owner, or an artifact that changed or vanished
    after the record is a mixed identity, never a run."""
    if identity is None:
        raise ValueError('no artifact identity recorded for this target directory: build it with '
                         'tools/bisection/cell-artifacts.sh (drop --no-build) before running')
    claimed = dict(worktree=identity.get('worktree'), cell=identity.get('cell'), profile=identity.get('profile'))
    wanted = dict(worktree=str(Path(root).resolve()), cell=cell, profile='dev')
    if claimed != wanted:
        raise ValueError('target directory identity mismatch: built for %s, this run is %s' % (
            json.dumps(claimed, sort_keys=True), json.dumps(wanted, sort_keys=True)))
    recorded = identity.get('artifacts', {})
    for name, path in needed.items():
        staged = recorded.get(name)
        if staged is None:
            raise ValueError(f'artifact {name} ({path}) was not staged by the recorded build')
        if not Path(path).exists():
            raise ValueError(f'artifact {name} ({path}) is missing although the record stages it')
        stat = Path(path).stat()
        if (staged.get('path'), staged.get('size'), staged.get('mtime_ns')) != (str(path), stat.st_size, stat.st_mtime_ns):
            raise ValueError(f'artifact {name} ({path}) changed after the recorded build '
                             f'(size/mtime {stat.st_size}/{stat.st_mtime_ns} vs {staged.get("size")}/{staged.get("mtime_ns")}): '
                             'a mixed target directory; rebuild it from this worktree')
# ---------------------------------------------------------------------------
# The host-wide heavy slot. Heavy work (a cell build, a full differential, a
# campaign, a Valgrind run, a release/LTO build) takes one of a few slots
# shared by every worktree and agent on the host, so concurrent heavy work
# serialises instead of running the host out of memory; light work (fmt,
# clippy on one crate, a small case selection, harness tests, the small
# differentials) never waits. The slot is the existing exclusive-resource
# mechanism (a flock on a lock file) at a host-wide location rather than
# beside one checkout: the holder's identity is written into the file so
# anyone waiting sees who holds it, and a file still naming a dead holder is
# reclaimed with a message (the kernel released a dead holder's flock).
# ---------------------------------------------------------------------------
HEAVY_SLOT = 'heavy-slot'
HEAVY_SLOT_HELD = 'OXPINYIN_HEAVY_SLOT_HELD'
# The results table. build_seconds is the cell's one build, shared by every row
# of that cell; lock_wait_seconds is the runner's wait for its exclusive
# resources (#899); slot_wait_seconds is the runner's wait for the heavy slot,
# taken before its locks; build_slot_wait_seconds is the cell build's wait for
# the heavy slot, shared by every row of that cell.
COLUMNS = ('runner', 'cell', 'status', 'seconds', 'build_seconds', 'lock_wait_seconds',
           'slot_wait_seconds', 'build_slot_wait_seconds', 'pin_ref', 'reason')


def host_lock_dir():
    """Where the host-wide locks live: $OXPINYIN_LOCK_DIR, else
    $XDG_CACHE_HOME/oxpinyin/locks, else ~/.cache/oxpinyin/locks (the cache
    tools/cloud/setup.sh already uses). Shared by every worktree and agent."""
    root = os.environ.get('OXPINYIN_LOCK_DIR')
    if not root:
        cache = os.environ.get('XDG_CACHE_HOME') or str(Path.home() / '.cache')
        root = str(Path(cache) / 'oxpinyin/locks')
    path = Path(root)
    if os.environ.get('OXPINYIN_LOCK_DIR') and not path.is_absolute():
        raise ValueError('OXPINYIN_LOCK_DIR must be an absolute path')
    path = path.resolve()
    path.mkdir(parents=True, exist_ok=True)
    return path


def heavy_slot_count():
    """How many heavy slots the host offers: $OXPINYIN_HEAVY_SLOTS, default 1."""
    try:
        return max(1, int(os.environ.get('OXPINYIN_HEAVY_SLOTS', '1')))
    except ValueError:
        return 1


def pid_alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def read_holder(path):
    try:
        return json.loads(path.read_text() or 'null')
    except (OSError, ValueError):
        return None


def describe_holder(holder):
    if not isinstance(holder, dict):
        return 'no holder recorded'
    return (f"pid {holder.get('pid')} in {holder.get('worktree')} since {holder.get('since')}: "
            f"{holder.get('command')}")


def is_heavy(row):
    """A registry row is heavy when it says so (`"heavy": true`); the default
    is light. The cell build is always heavy (cargo decides how much it
    compiles; a no-op build holds the slot for a fraction of a second)."""
    return bool(row.get('heavy'))


def lock_file(file, operation):
    """Retry interrupted locks. Keep other errors and the lock path."""
    while True:
        try:
            fcntl.flock(file, operation)
            return
        except InterruptedError:
            continue
        except BlockingIOError:
            raise
        except OSError as exc:
            if exc.errno == errno.EINTR:
                continue
            raise OSError(exc.errno, exc.strerror, file.name) from exc


@contextlib.contextmanager
def admission_lock(directory):
    with (directory / 'heavy-admission').open('a+') as file:
        lock_file(file, fcntl.LOCK_EX)
        yield
        # Closing this unshared descriptor releases the admission lock.


def process_parent(pid):
    try:
        if sys.platform.startswith('linux'):
            return int(Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[1])
        result = subprocess.run(['ps', '-o', 'ppid=', '-p', str(pid)],
                                capture_output=True, text=True, check=True)
        return int(result.stdout.strip())
    except (OSError, ValueError, IndexError, subprocess.SubprocessError):
        return None


def is_ancestor(pid):
    current, seen = os.getppid(), set()
    while current and current not in seen:
        if current == pid:
            return True
        seen.add(current)
        current = process_parent(current)
    return False


def write_holder(file, holder):
    file.seek(0)
    file.truncate()
    file.write(json.dumps(holder))
    file.flush()


def start_child(command, env, *, slot=None, **kwargs):
    """Transfer an acquired slot to the workload before it executes."""
    descriptors = ()
    if slot is not None:
        descriptors = slot.pass_fds()
        if not slot.inherited:
            command = [sys.executable, str(HERE / 'run-all.py'), '--heavy-child-fd',
                       str(descriptors[0]), '--', *command]
    check_cancelled()
    kwargs.setdefault('start_new_session', True)
    process = subprocess.Popen(command, env=env, pass_fds=descriptors, **kwargs)
    if slot is not None and not slot.inherited:
        slot.handoff()
    return process


def heavy_child(fd, command):
    """Give the inherited descriptor and holder record to the actual child."""
    os.set_inheritable(fd, True)
    marker = json.loads(os.environ[HEAVY_SLOT_HELD])
    directory = Path(marker['lock_dir'])
    with admission_lock(directory):
        with os.fdopen(os.dup(fd), 'r+') as file:
            file.seek(0)
            holder = json.loads(file.read())
            holder.update(pid=os.getpid(), command=' '.join(command))
            write_holder(file, holder)
    marker.update(pid=os.getpid(), fd=fd)
    env = dict(os.environ, **{HEAVY_SLOT_HELD: json.dumps(marker)})
    check_cancelled()
    with subprocess.Popen(command, env=env, pass_fds=(fd,)) as process:
        while True:
            try:
                rc = process.wait(timeout=0.1)
                break
            except subprocess.TimeoutExpired:
                if CANCELLED.is_set():
                    # The group already received the signal. Keep the supervisor
                    # alive so it can reap a command that ignores cancellation.
                    try:
                        rc = process.wait(timeout=2)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        rc = process.wait()
                    break
    if CANCELLED.is_set():
        return 1
    if rc < 0:
        if -rc not in (signal.SIGKILL, signal.SIGSTOP):
            signal.signal(-rc, signal.SIG_DFL)
        os.kill(os.getpid(), -rc)
    return rc


class HeavySlot:
    """Hold one host-wide heavy slot for the duration of a `with` block.

    Acquisition tries every slot without blocking, then waits, polling, and
    prints who holds the slots once at the start of the wait and every 30 s;
    the wait never fails or skips the work. `waited` is the seconds spent
    waiting, for the caller's timing row. `reclaimed` names a dead holder
    whose record this acquisition replaced, if any."""

    def __init__(self, command, out=sys.stderr, poll=0.5):
        self.command, self.out, self.poll = command, out, poll
        self.waited, self.reclaimed, self.slot, self._file = 0.0, None, None, None
        self.started, self.announced = None, None
        self.inherited = False
        self._handed_off = False
        self.holder_pid, self.inherited_fd = None, None

    def _try(self, index):
        file = (self.lock_dir / f'{HEAVY_SLOT}-{index}').open('a+')
        try:
            lock_file(file, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            file.close()
            return None
        except BaseException:
            file.close()
            raise
        return file

    def _inherit(self):
        self.lock_dir = host_lock_dir().resolve()
        marker = os.environ.get(HEAVY_SLOT_HELD)
        if marker is not None:
            try:
                held = json.loads(marker)
                if (not isinstance(held, dict) or not {'lock_dir', 'slot', 'pid'} <= set(held)
                        or set(held) - {'lock_dir', 'slot', 'pid', 'fd'}
                        or not isinstance(held['lock_dir'], str)
                        or not Path(held['lock_dir']).is_absolute()
                        or type(held['slot']) is not int or held['slot'] < 0
                        or type(held['pid']) is not int or held['pid'] <= 0
                        or ('fd' in held and (type(held['fd']) is not int or held['fd'] < 0))):
                    raise ValueError('invalid fields')
                directory = Path(held['lock_dir']).resolve()
            except (ValueError, OSError, RuntimeError) as exc:
                raise ValueError(f'Invalid {HEAVY_SLOT_HELD}: expected a JSON object with '
                                 'an absolute lock_dir, a nonnegative integer slot, and a positive holder pid') from exc
            if directory == self.lock_dir:
                path = directory / f"{HEAVY_SLOT}-{held['slot']}"
                holder = read_holder(path)
                if (isinstance(holder, dict) and holder.get('pid') == held['pid']
                        and pid_alive(held['pid']) and is_ancestor(held['pid'])):
                    self.slot, self.holder_pid = held['slot'], held['pid']
                    self.inherited = True
                    fd = held.get('fd')
                    if fd is not None:
                        try:
                            if os.path.samestat(os.fstat(fd), path.stat()):
                                self.inherited_fd = fd
                        except OSError:
                            pass
                    print(f'heavy slot {self.slot} inherited from ancestor pid {self.holder_pid}: {self.command}',
                          file=self.out, flush=True)
                    return True
                print(f'stale {HEAVY_SLOT_HELD}: pid {held["pid"]}, slot {held["slot"]}; '
                      'acquire a slot normally', file=self.out, flush=True)
            else:
                print(f'stale {HEAVY_SLOT_HELD}: different lock directory; acquire a slot normally',
                      file=self.out, flush=True)
        return False

    def try_acquire(self):
        """Try once. Do not block a worker while the slot is busy."""
        check_cancelled()
        if self._file is not None or self.inherited:
            return True
        if self.started is None:
            self.started = time.monotonic()
            if self._inherit():
                return True
        slots = heavy_slot_count()
        for index in range(slots):
            file = self._try(index)
            if file is not None:
                self.slot, self._file = index, file
                break
        self.waited = time.monotonic() - self.started
        if self._file is None:
            now = time.monotonic()
            if self.announced is None or now - self.announced >= 30:
                holders = [describe_holder(read_holder(self.lock_dir / f'{HEAVY_SLOT}-{i}')) for i in range(slots)]
                print(f'waiting for a heavy slot ({slots} on this host) for: {self.command}; held by: '
                      + '; '.join(holders), file=self.out, flush=True)
                self.announced = now
            return False
        path = Path(self._file.name)
        previous = read_holder(path)
        if isinstance(previous, dict) and previous.get('pid') and not pid_alive(int(previous['pid'])):
            self.reclaimed = previous
            print(f'reclaimed stale heavy slot {self.slot}: {describe_holder(previous)} (pid dead)',
                  file=self.out, flush=True)
        self.holder_pid = os.getpid()
        holder = dict(pid=self.holder_pid, worktree=str(ROOT), command=self.command,
                      since=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()))
        write_holder(self._file, holder)
        if self.waited >= self.poll:
            print(f'heavy slot {self.slot} acquired after {self.waited:.1f}s wait: {self.command}',
                  file=self.out, flush=True)
        return True

    def __enter__(self):
        while not self.try_acquire():
            time.sleep(self.poll)
        return self

    def pass_fds(self):
        fd = self.inherited_fd if self.inherited else self._file.fileno()
        if fd is None:
            return ()
        os.set_inheritable(fd, True)
        return (fd,)

    def child_env(self, env):
        """Copy the environment and mark the slot for this child."""
        marker = dict(lock_dir=str(self.lock_dir), slot=self.slot, pid=self.holder_pid)
        descriptors = self.pass_fds()
        if descriptors:
            marker['fd'] = descriptors[0]
        return dict(env, OXPINYIN_LOCK_DIR=str(self.lock_dir), **{HEAVY_SLOT_HELD: json.dumps(marker)})

    def handoff(self):
        # The child shares this open file description. Never unlock it here.
        # Keep our copy until the child is reaped. Descendants keep theirs.
        self._handed_off = True

    def __exit__(self, *exc):
        if self._file is None:
            return False
        try:
            if not self._handed_off:
                self._file.seek(0)
                self._file.truncate()
                self._file.flush()
        finally:
            self._file.close()
            self._file = None
            self._handed_off = False
        return False


class HeavyAdmission:
    """Prepare heavy work before it enters the worker pool."""

    def __init__(self, command, env, out=sys.stderr, poll=0.5):
        self.slot = HeavySlot(command, out=out, poll=poll)
        self.base_env, self.env, self.poll = env.copy(), None, poll

    def try_acquire(self):
        if not self.slot.try_acquire():
            return False
        self.env = self.slot.child_env(self.base_env)
        return True

    def record(self, result):
        result['slot_wait_seconds'] = round(self.slot.waited, 3)

    def __enter__(self):
        try:
            while not self.try_acquire():
                time.sleep(self.poll)
        except BaseException:
            self.close()
            raise
        return self

    def close(self):
        self.slot.__exit__()

    def __exit__(self, *exc):
        self.close()
        return False


def run_admitted(jobs, workers, run):
    """Submit only admitted jobs. Keep the pool at the requested size."""
    pending, active, results = list(jobs), {}, []
    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
            while pending or active:
                check_cancelled()
                for future in list(active):
                    if future.done():
                        job = active.pop(future)
                        try:
                            result = future.result()
                            if job['admission']:
                                job['admission'].record(result)
                            results.append(result)
                        finally:
                            if job['admission']:
                                job['admission'].close()
                for job in list(pending):
                    if len(active) >= workers:
                        break
                    admission = job['admission']
                    try:
                        ready = admission is None or admission.try_acquire()
                    except (OSError, ValueError) as exc:
                        job['result'].update(status='FAIL', reason=str(exc))
                        admission.record(job['result'])
                        admission.close()
                        results.append(job['result'])
                        pending.remove(job)
                        continue
                    if ready:
                        pending.remove(job)
                        active[pool.submit(run, job)] = job
                if active:
                    concurrent.futures.wait(active, timeout=0.05,
                                            return_when=concurrent.futures.FIRST_COMPLETED)
                elif pending:
                    time.sleep(0.05)
    finally:
        for job in jobs:
            if job['admission']:
                job['admission'].close()
    return results


def run_in_heavy_slot(command):
    """`run-all.py --heavy-slot -- COMMAND...`: run COMMAND holding a heavy
    slot (for campaigns, Valgrind runs and builds started outside run-all).
    The wait is printed; the command's exit status is returned."""
    with HeavyAdmission(' '.join(command), os.environ) as admission:
        slot = admission.slot
        started = time.monotonic()
        with start_child(command, admission.env, slot=slot) as process:
            rc = wait_child(process)
        print(f'heavy slot {slot.slot}: slot_wait_seconds {slot.waited:.1f}, ran {time.monotonic() - started:.1f}s, exit {rc}',
              file=sys.stderr, flush=True)
        return rc


def cell_config(cell, target_root, env):
    configured = env.copy()
    configured['PINYIN_ORACLE_DBM'] = cell
    configured['OXPINYIN_TARGET_ROOT'] = str(target_root)
    command = ['bash', str(HERE / 'cell-artifacts.sh'), '--describe']
    result = subprocess.run(command, env=configured, cwd=ROOT, capture_output=True, text=True)
    if result.returncode:
        raise ValueError(result.stderr.strip() or 'cell configuration failed')
    return json.loads(result.stdout)


def oracle_config(prefix, variant, env):
    command = ['bash', str(HERE / 'cell-artifacts.sh'), '--describe-oracle',
               str(prefix), variant]
    result = subprocess.run(command, env=env, cwd=ROOT, capture_output=True, text=True)
    if result.returncode:
        raise ValueError(result.stderr.strip() or 'oracle configuration failed')
    return json.loads(result.stdout)


def run_main():
    if fcntl is None:
        raise ValueError("run-all.py needs POSIX fcntl; Windows support requires #895 Stage 2 (`just heavy`)")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runners', help='comma-separated registry names (default: every gate)')
    parser.add_argument('--cells', default='bdb', help='comma-separated bdb,kc,tkrzw')
    parser.add_argument('--issue', action='append', default=[],
                        help='issue number, W tag, or row:N; selects the runners and the exact '
                             'contract-diff cases registered under it')
    parser.add_argument('--list', action='store_true', help='list gate and not-a-gate metadata; do not build')
    parser.add_argument('--jobs', type=positive, default=2)
    parser.add_argument('--timeout', type=positive, help='override the registry default for every runner')
    parser.add_argument('--runner-timeout', action='append', default=[], metavar='RUNNER=SECONDS')
    parser.add_argument('--allow-skipped', action='append', default=[], metavar='RUNNER[:CELL]',
                        help='explicitly allow these skips; * allows all skips, never failures')
    parser.add_argument('--oracle-root', type=Path,
                        default=Path(os.environ.get('OXPINYIN_ORACLE_ROOT', Path.home() / '.local/opt/pinyin-oracle')))
    parser.add_argument('--oracle', action='append', default=[], metavar='CELL=PREFIX')
    parser.add_argument('--cell-env', action='append', default=[], metavar='CELL:NAME=VALUE',
                        help='additional per-cell runner inputs, e.g. staged packaging roots')
    parser.add_argument('--ibus-build', action='append', default=[], metavar='CELL=DIR',
                        help='pin-built IBus frontend work directory for import interop')
    parser.add_argument('--data', action='append', default=[], metavar='CELL=DIR',
                        help='subject full tables plus interpolation2.text, matching that cell')
    parser.add_argument('--target-root', type=Path,
                        default=Path(os.environ.get('OXPINYIN_TARGET_ROOT', ROOT / 'target/cells')))
    parser.add_argument('--no-build', action='store_true', help='use existing dev artifacts at the cell targets')
    parser.add_argument('--build-timeout', type=positive, default=900)
    parser.add_argument('--json', type=Path, help='write the same result table as JSON (no raw captures)')
    parser.add_argument('--log-dir', type=Path, help='retain local build/runner logs here instead of deleting scratch')
    if sys.argv[1:2] == ['--heavy-child-fd']:
        return heavy_child(int(sys.argv[2]), sys.argv[4:])
    parser.epilog = ('run-all.py --heavy-slot -- COMMAND...  runs COMMAND holding one host-wide heavy slot '
                     '(for campaigns, Valgrind runs and builds started outside run-all) and exits with its status.')
    if sys.argv[1:2] == ['--heavy-slot']:
        command = sys.argv[2:]
        if command[:1] == ['--']:
            command = command[1:]
        if not command:
            parser.error('--heavy-slot needs a command after --')
        return run_in_heavy_slot(command)
    args = parser.parse_args()
    registry = json.loads((HERE / 'runners.json').read_text())
    names = {row['name'] for row in registry}
    selected = set(args.runners.split(',')) if args.runners else names
    unknown = selected - names
    if unknown:
        parser.error('unknown runners: ' + ', '.join(sorted(unknown)))
    cells = list(dict.fromkeys(args.cells.split(',')))
    if not cells or set(cells) - set(CELLS):
        parser.error('--cells must select bdb,kc,tkrzw')
    requested_issues = {normal_tag(item) for item in args.issue}
    rows = [row for row in registry if row['name'] in selected and
            (not requested_issues or requested_issues.intersection(
                normal_tag(tag) for tag in row['issues']))]
    reasons = {row['name']: ('--issue ' + ', '.join(sorted(requested_issues.intersection(
        normal_tag(tag) for tag in row['issues']))) if requested_issues else
        '--runners' if args.runners else 'default gate set') for row in rows}
    # An issue tag also resolves to the exact contract cases registered under
    # it; those run through the contract-diff runner with --cases. Nothing
    # matching anywhere is an error that names what exists, never the suite.
    contract_cases, index = {}, None
    if requested_issues and 'contract-diff' in selected:
        index = contract_tag_index()
        contract_cases = contract_cases_for(requested_issues, index)
        if contract_cases and all(row['name'] != 'contract-diff' for row in rows):
            rows.append(next(row for row in registry if row['name'] == 'contract-diff'))
            reasons['contract-diff'] = '--issue %s: %d contract case%s' % (
                ', '.join(sorted({tag for tags in contract_cases.values() for tag in tags})),
                len(contract_cases), '' if len(contract_cases) == 1 else 's')
        elif contract_cases:
            reasons['contract-diff'] += '; %d contract case%s' % (
                len(contract_cases), '' if len(contract_cases) == 1 else 's')
    if not rows:
        runner_tags = sorted({normal_tag(tag) for row in registry for tag in row['issues']})
        contract_tags = sorted(tag for tag in (index or contract_tag_index())['tags']
                               if not tag.startswith('family/'))
        parser.error('no runner or contract case carries issue tag(s) %s; runner tags: %s; '
                     'contract tags: %s' % (', '.join(sorted(requested_issues)),
                                            ', '.join(runner_tags), ', '.join(contract_tags)))
    if contract_cases:
        rows = [dict(row, command=row['command'] + ['--', '--cases', ','.join(contract_cases)])
                if row['name'] == 'contract-diff' else row for row in rows]
    if args.list:
        for row in rows:
            print(json.dumps(row, ensure_ascii=False))
        return 0
    measurements = [row['name'] for row in rows if row['kind'] != 'gate']
    if args.runners and measurements:
        parser.error('not-a-gate (measurement recipes are never executed): ' + ', '.join(measurements))
    rows = [row for row in rows if row['kind'] == 'gate']
    if not rows:
        parser.error('selection contains no gates')
    oracle_overrides = assignments(args.oracle, CELLS, parser)
    data_overrides = assignments(args.data, CELLS, parser)
    ibus_builds = assignments(args.ibus_build, CELLS, parser)
    timeouts = assignments(args.runner_timeout, names, parser)
    try:
        timeouts = {name: positive(value) for name, value in timeouts.items()}
    except (ValueError, argparse.ArgumentTypeError):
        parser.error('runner timeouts must be positive integers')
    for allowance in args.allow_skipped:
        runner, _, cell = allowance.partition(':')
        if allowance != '*' and (runner not in names or cell and cell not in CELLS):
            parser.error(f'invalid skip allowance: {allowance}')
    extra_env = {cell: {} for cell in CELLS}
    protected = {'CARGO_TARGET_DIR', 'PINYIN_ORACLE_DBM', 'PINYIN_ORACLE_PREFIX',
                 'OXPINYIN_CAPI_SO', 'OXPINYIN_ZHUYIN_SO', 'OXPINYIN_DICTOOL',
                 'OXPINYIN_SYSTEM_DIR', 'CARGO_PROFILE_DEV_OPT_LEVEL'}
    for value in args.cell_env:
        cell, sep, assignment = value.partition(':')
        key, equals, item = assignment.partition('=')
        if (cell not in CELLS or not sep or not equals or
                not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', key) or key in protected):
            parser.error(f'invalid or reserved cell environment setting: {value}')
        extra_env[cell][key] = item
    base_env = os.environ.copy()
    if len(cells) > 1 and base_env.get('CARGO_TARGET_DIR'):
        parser.error('CARGO_TARGET_DIR is a single-cell override; use --target-root for multiple cells')
    targets = args.target_root.resolve()
    results, jobs = [], []
    build_seconds = {}
    # Locks live beside the checkout rather than in per-cell or per-invocation
    # directories. This protects fixed native outputs across concurrent invocations.
    lock_root = ROOT / 'target/runner-locks'
    lock_root.mkdir(parents=True, exist_ok=True)
    if args.log_dir:
        if args.log_dir.exists() and (not args.log_dir.is_dir() or any(args.log_dir.iterdir())):
            parser.error('--log-dir must be a new or empty directory')
        args.log_dir.mkdir(parents=True, exist_ok=True)
    with scratch_directory(args.log_dir) as scratch_name:
        scratch = Path(scratch_name)
        for cell in cells:
            cfg = cell_config(cell, targets, base_env)
            prefix = Path(oracle_overrides.get(cell) or
                          (base_env.get('PINYIN_ORACLE_PREFIX') if len(cells) == 1 else '') or
                          args.oracle_root / cell).resolve()
            data = Path(data_overrides.get(cell) or
                        (base_env.get('OXPINYIN_SYSTEM_DIR') if len(cells) == 1 else '') or
                        prefix / 'lib/libpinyin/data').resolve()
            env = base_env.copy()
            env.update({key: str(value) for key, value in cfg.items()})
            env.update(PINYIN_ORACLE_PREFIX=str(prefix), OXPINYIN_SYSTEM_DIR=str(data),
                       OXPINYIN_CAPI_SO=str(Path(cfg['CARGO_TARGET_DIR']) / 'debug/libpinyin_capi.so'),
                       OXPINYIN_ZHUYIN_SO=str(Path(cfg['CARGO_TARGET_DIR']) / 'debug/libzhuyin_capi.so'),
                       OXPINYIN_DICTOOL=str(Path(cfg['CARGO_TARGET_DIR']) / 'debug/oxpinyin-dictool'))
            env.update(extra_env[cell])
            env['OXPINYIN_USER_RT_TEST'] = str(Path(cfg['CARGO_TARGET_DIR']) / 'debug/user-dir-round-trip-test')
            env['PINYIN_IBUS_PREFIX'] = str(prefix)
            if cell in ibus_builds:
                env['PINYIN_IBUS_BUILD_DIR'] = str(Path(ibus_builds[cell]).resolve())
            elif len(cells) > 1:
                env.pop('PINYIN_IBUS_BUILD_DIR', None)
            substitutions = dict(root=str(ROOT), prefix=str(prefix), data=str(data), cell=cell,
                                 capi=env['OXPINYIN_CAPI_SO'], zhuyin=env['OXPINYIN_ZHUYIN_SO'])
            build_candidates = []
            oracle_configs = {}
            for row in rows:
                variant = row.get('oracle_variant', 'unpatched')
                if variant not in oracle_configs:
                    oracle_configs[variant] = oracle_config(prefix, variant, env)
                oracle = oracle_configs[variant]
                runner_prefix = Path(oracle['PINYIN_ORACLE_PREFIX'])
                result = dict(runner=row['name'], cell=cell, status='SKIPPED', seconds=0.0,
                              build_seconds=0.0, lock_wait_seconds=0.0, phases=None,
                              slot_wait_seconds=0.0, build_slot_wait_seconds=0.0,
                              pin_ref=oracle['EXPECTED_ORACLE_PIN_REF'].removeprefix('pin_ref='), reason='')
                if cell not in row['cells']:
                    result['reason'] = 'unsupported cell'
                elif row.get('oracle', True):
                    manifest = runner_prefix / 'oracle-pin.txt'
                    patches = runner_prefix / 'oracle-patches.sha256'
                    if not manifest.is_file() or not (runner_prefix / 'lib/libpinyin.so').is_file():
                        result['reason'] = f'missing oracle: {runner_prefix}'
                        if variant != 'unpatched':
                            result['status'] = 'FAIL'
                    elif oracle['EXPECTED_ORACLE_PIN_REF'] not in manifest.read_text().splitlines():
                        result.update(status='FAIL', reason=f'oracle pin/cell mismatch: {manifest}')
                    elif oracle['ORACLE_PATCH_FILE'] and (not patches.is_file() or
                            oracle['ORACLE_PATCH_FILE'] not in patches.read_text()):
                        result.update(status='FAIL', reason=f'missing required oracle patch: {patches}')
                if result['reason']:
                    results.append(result)
                else:
                    build_candidates.append((row, result, runner_prefix))
            if not build_candidates:
                continue
            cell_jobs = []
            for row, result, runner_prefix in build_candidates:
                runner_env = dict(env, PINYIN_ORACLE_PREFIX=str(runner_prefix))
                runner_substitutions = dict(substitutions, prefix=str(runner_prefix))
                cell_jobs.append((row, result, runner_env, runner_substitutions))
            for line in plan_lines(cell, cell_jobs, contract_cases, reasons, index):
                print(line, file=sys.stderr, flush=True)
            # Build the union of what the selected runners declare, once per
            # cell; a selection that needs no subject (pc-version) builds nothing.
            artifacts = artifact_union(row for row, _, _ in build_candidates)
            if not artifacts:
                print(f'No subject artifacts needed: {cell} (no build)', file=sys.stderr, flush=True)
            elif not args.no_build:
                print(f'Building dev opt-level 1 artifacts: {cell}: {", ".join(artifacts)}',
                      file=sys.stderr, flush=True)
                # The build is heavy work: it holds a host-wide slot while cargo runs.
                with HeavyAdmission(f'cell build {cell} in {ROOT}', env) as admission:
                    rc, seconds, output, expired = execute(['bash', str(HERE / 'cell-artifacts.sh'),
                                                            '--artifacts', ','.join(artifacts)], admission.env, ROOT,
                                                           args.build_timeout, scratch / f'build-{cell}.log', slot=admission.slot)
                build_seconds[cell] = round(seconds, 3)
                for _, result, _ in build_candidates:
                    result['build_slot_wait_seconds'] = round(admission.slot.waited, 3)
                if rc or expired:
                    for _, result, _ in build_candidates:
                        result.update(status='FAIL', seconds=round(seconds, 3), build_seconds=build_seconds[cell],
                                      reason='cell build: ' + reason_for(rc, output, args.build_timeout, expired))
                        results.append(result)
                    continue
            # Execute only what this worktree staged into this cell's target.
            needed = needed_artifacts((row for row, _, _ in build_candidates), env)
            # A selection that executes nothing from the cell (pc-version) built
            # nothing above and needs no record; everything else must match it.
            if needed:
                identity = identity_of(cfg['CARGO_TARGET_DIR'])
                check_identity(identity, ROOT, cell, needed)
                head = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, capture_output=True, text=True).stdout.strip()
                source = identity.get('source', {})
                if source.get('head') != head:
                    print(f"Note: {cell} artifacts were built from {source.get('head', '?')[:12]} at "
                          f"{identity.get('built_utc', '?')}; this worktree is at {head[:12]}", file=sys.stderr, flush=True)
            jobs.extend(cell_jobs)

        prepared = []
        for row, result, base, substitutions in jobs:
            timeout = timeouts.get(row['name'], args.timeout or row['timeout'])
            work = scratch / (row['name'] + '-' + result['cell'])
            work.mkdir()
            substitutions = dict(substitutions, work=str(work))
            env = base.copy()
            env.update({key: value.format_map(substitutions) for key, value in row.get('env', {}).items()})
            env['TMPDIR'] = str(work)
            substitutions['env'] = env
            absent = [name for name in row.get('required_env', []) if not env.get(name)]
            if absent:
                result.update(status='SKIPPED', reason='missing required environment: ' + ', '.join(absent))
                results.append(result)
                continue
            command = [part.format_map(substitutions) for part in row['command']]
            missing = [item.format_map(substitutions) for item in row.get('input_paths', [])
                       if not Path(item.format_map(substitutions)).exists()]
            if missing:
                result.update(status='FAIL' if row.get('oracle_variant', 'unpatched') != 'unpatched'
                              else 'SKIPPED', reason='missing required input: ' + ', '.join(missing))
                results.append(result)
                continue
            admission = HeavyAdmission(f"{row['name']} / {result['cell']} in {ROOT}", env) if is_heavy(row) else None
            prepared.append(dict(row=row, result=result, env=env, command=command,
                                 timeout=timeout, work=work, admission=admission))

        def run(job):
            row, result = job['row'], job['result']
            admission = job['admission']
            env = admission.env if admission else job['env']
            timeout = job['timeout']
            with contextlib.ExitStack() as stack:
                # Admission holds the slot before these sorted checkout locks.
                # Light holders never wait for a slot. Heavy holders have one.
                # These waits cannot form a cycle.
                waiting = time.monotonic()
                for resource in sorted(row['exclusive_resources']):
                    lock = stack.enter_context((lock_root / resource).open('a'))
                    while True:
                        check_cancelled()
                        try:
                            lock_file(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                            break
                        except BlockingIOError:
                            time.sleep(0.05)
                lock_wait = time.monotonic() - waiting
                rc, seconds, output, expired = execute(job['command'], env, ROOT, timeout, job['work'] / 'runner.log',
                                                        slot=admission.slot if admission else None)
            # Legacy CI behavior is preserved in the direct live-typing runner;
            # aggregate skips cannot silently turn into passes.
            required_skips = [line for line in output.splitlines() if 'SKIP:' in line and
                              not any(token in line for token in row.get('optional_skips', []))]
            if rc == 0 and required_skips and not expired:
                rc, output = 77, '\n'.join(required_skips)
            result.update(status='FAIL' if expired else 'PASS' if rc == 0 else 'SKIPPED' if rc == 77 else 'FAIL',
                          seconds=round(seconds, 3), build_seconds=build_seconds.get(result['cell'], 0.0),
                          lock_wait_seconds=round(lock_wait, 3),
                          phases=timing_of(output),
                          reason=reason_for(rc, output, timeout, expired))
            print(f"Completed {result['runner']} / {result['cell']}: {result['status']}",
                  file=sys.stderr, flush=True)
            return result

        results.extend(run_admitted(prepared, args.jobs, run))
    order = {row['name']: index for index, row in enumerate(rows)}
    results.sort(key=lambda row: (order[row['runner']], cells.index(row['cell'])))
    print(' | '.join(COLUMNS))
    print('--- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | ---')
    for row in results:
        print(' | '.join(str(row[key]).replace('|', '/').replace('\n', ' ') for key in COLUMNS))
    if args.json:
        args.json.write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n')
    def allowed(row):
        return any(item in args.allow_skipped for item in
                   ('*', row['runner'], row['runner'] + ':' + row['cell']))
    return int(any(row['status'] == 'FAIL' or row['status'] == 'SKIPPED' and not allowed(row)
                   for row in results))


def main():
    with cancellation_handlers():
        return run_main()


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr)
        sys.exit(1)
