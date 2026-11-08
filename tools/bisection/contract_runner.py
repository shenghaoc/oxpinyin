"""Contract-driver execution, with one state owner per loaded driver.

The driver passes its namespace so existing function/path/timing patches are
read at call time. Mutable pool settings and cancellation belong to Runner;
set_state is their only writer. No case registry or native library is imported.
"""
import contextlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import threading
import time

try:
    from . import contract_reference as reference
except ImportError:  # standalone contract driver
    import contract_reference as reference

_UNSET = object()
STATE_NAMES = {'WORKER_ENV': 'worker_env', 'WORKER_FDS': 'worker_fds', 'CANCELLED': 'cancelled'}


class Cancelled(Exception):
    """The caller cancelled the driver; no case verdict is available."""


class Runner:
    def __init__(self, driver):
        self.driver = driver
        self.worker_state = threading.local()
        self.platform_skip = object()
        self.set_state(worker_env={}, worker_fds=(), cancelled=False)

    def set_state(self, *, worker_env=_UNSET, worker_fds=_UNSET, cancelled=_UNSET):
        """Publish/restore admission settings or cancellation in this owner only."""
        if worker_env is not _UNSET:
            self.worker_env = worker_env
        if worker_fds is not _UNSET:
            self.worker_fds = worker_fds
        if cancelled is not _UNSET:
            self.cancelled = cancelled

    def setting(self, name):
        """A temporary driver patch overrides the owned setting while installed.

        Normally these names are exposed by the driver's __getattr__, without
        duplicate globals. patch.object/patch.dict can still install an override.
        """
        return self.driver.get(name, getattr(self, STATE_NAMES[name]))

    def check_cancelled(self):
        """Raise Cancelled when the controller's shared cancellation flag is set."""
        if self.setting('CANCELLED'):
            raise Cancelled('cancelled; worker group terminated and reaped')

    @contextlib.contextmanager
    def cancellation_handlers(self):
        """Defer signals until the active worker can be stopped and reaped.

        Reset CANCELLED on entry and temporarily convert SIGTERM, SIGINT and
        SIGHUP into that shared flag. Restore previous handlers on exit; worker
        code must check the flag and reap processes. Signal installation requires
        Python's main thread."""
        self.set_state(cancelled=False)
        previous = {}

        def cancel(signum, frame):
            """Mark controller cancellation without performing process cleanup in the signal handler."""
            self.set_state(cancelled=True)

        # Python cannot handle SIGKILL of the driver.
        for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            previous[signum] = signal.signal(signum, cancel)
        try:
            yield
        finally:
            for signum, handler in previous.items():
                signal.signal(signum, handler)

    def run_worker(self, mode, so, data, name, scratch, extra_env=None, timeout=None, entry=None):
        """Run an isolated worker and return its normalized observation.

        Use the current interpreter with entry (script plus leading arguments) or
        WORKER, inherited environment plus WORKER_ENV/extra_env, scratch as TMPDIR,
        and WORKER_FDS. Check cancellation and the tkrzw guard before spawning a
        new process session. timeout is seconds from pre-spawn setup; expiry kills
        the group, drains pipes and raises TimeoutExpired with captured bytes.
        None disables the deadline.

        Use the last stdout JSON object, replace invalid stdout UTF-8, preserve
        invalid stderr bytes with surrogateescape, normalize scratch paths, and
        summarize/remove new user directories. Include measurement marker fields
        without interpreting them. On post-spawn exit close pipes and append phase,
        wall and teardown seconds to TIMINGS; errors before completion terminate
        and reap the process group. Malformed JSON and filesystem errors propagate."""
        env = dict(os.environ, **self.setting('WORKER_ENV'))
        env['TMPDIR'] = str(scratch)
        env.update(extra_env or {})
        before = set(os.listdir(scratch))
        spawned = time.monotonic()
        env['CONTRACT_DIFF_SPAWNED'] = repr(spawned)
        # `entry` is the script and its leading arguments; the default is read
        # per call so a patched WORKER takes effect.
        command = [sys.executable, *map(str, entry or (self.driver['WORKER'],)), mode, str(so), str(data), name]
        self.driver['check_cancelled']()
        self.driver['check_tkrzw'](self.driver['selected_cell'](), 'contract-diff.py',
                    env.get('OXPINYIN_RUNTIME_SIDE', 'subject'), so, env=env)
        proc = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                env=env, start_new_session=True, pass_fds=self.setting('WORKER_FDS'))
        stdout, stderr = b'', b''
        completed = False
        deadline = None if timeout is None else spawned + timeout
        try:
            while True:
                self.driver['check_cancelled']()
                remaining = None if deadline is None else deadline - time.monotonic()
                if remaining is not None and remaining <= 0:
                    # Descendants can hold the output pipes after the leader dies.
                    with contextlib.suppress(ProcessLookupError):
                        os.killpg(proc.pid, signal.SIGKILL)
                    stdout, stderr = proc.communicate()
                    completed = True
                    self.driver['check_cancelled']()
                    raise subprocess.TimeoutExpired(command, timeout, output=stdout, stderr=stderr)
                try:
                    stdout, stderr = proc.communicate(timeout=min(0.1, remaining) if remaining is not None else 0.1)
                    self.driver['check_cancelled']()
                    break
                except subprocess.TimeoutExpired as error:
                    stdout, stderr = error.output or b'', error.stderr or b''
            stdout_text = stdout.decode('utf-8', 'replace')
            # Keep invalid UTF-8 apart from a real U+FFFD: the pin writes path bytes.
            stderr_text = stderr.decode('utf-8', 'surrogateescape')
            # Replace the generated root before normalizing user/sys names.
            stderr_text = stderr_text.replace(str(scratch), 'SCRATCH')
            lines = [json.loads(line) for line in stdout_text.splitlines() if line.startswith('{')]
            observation = dict(exit=proc.returncode, result=lines[-1] if lines else None,
                        userfiles=self.driver['user_dir_listing'](scratch, before),
                        measurement_roots=[line.removeprefix('J-ROOT ') for line in stdout_text.splitlines() if line.startswith('J-ROOT ')],
                        measurement_abort_flush=('J-GCOV-ABORT-BEGIN' in stdout_text.splitlines()
                                                 and 'J-GCOV-ABORT-END' in stdout_text.splitlines()),
                        stderr_lines=len(stderr_text.splitlines()),
                        stderr=re.sub(r'(user|sys)-(\udcff)?[A-Za-z0-9_]+', r'\1-\2X', stderr_text))
            self.driver['check_cancelled']()
            completed = True
            return observation
        finally:
            if not completed:
                # This also covers KeyboardInterrupt and exceptions while waiting.
                self.driver['terminate_group'](proc)
            proc.stdout.close()
            proc.stderr.close()
            phases = [json.loads(line[2:]) for line in stdout.decode('utf-8', 'replace').splitlines()
                      if line.startswith('T {')]
            timing = dict(case=name, so=str(so), **(phases[-1] if phases else {}))
            timing['wall'] = time.monotonic() - spawned
            timing['teardown'] = timing['wall'] - sum(timing.get(k, 0.0) for k in ('startup', 'load', 'fixture', 'execute'))
            side = getattr(self.driver['WORKER_STATE'], 'side', None)
            if side is not None:
                timing['side'] = side
            self.driver['TIMINGS'].append(timing)

    def run_case(self, spec, pin_so, subject_so, data, name, scratch, timeout):
        """The local-only live handoff: one deadline for both sides; incomplete observations cannot be a match.

        Kept for `verify --reverse-cross-open` (docs/runbooks/contract-references.md): the pin reads
        what oxpinyin wrote, which the frozen references cannot check.

        Run pin before subject and return both observations; timeout is in seconds.
        Pass fatal-criticals when requested and track the worker side per thread.
        A timeout becomes TIMEOUT and prevents later ordinary execution; an absent
        second result becomes NOT-RUN. Other errors propagate.

        For the two issue1114 collision cases, retain separate profiles, compare
        saved bigram bytes with only the timestamp normalized, and cross-reload
        both donor profiles in fresh workers under the same deadline. Mismatch or
        reload failure updates the corresponding observation; successful reloads
        add filtered cross_reload results. The #929 logical cases reuse the same
        handoff and deadline, requiring exact key/value rows in both directions,
        without the #1114 byte criterion. The lazy-user-bigram cases compare
        both empty and trained save snapshots directly, with only timestamp
        bytes normalized. Profile cleanup belongs to the caller."""
        deadline = time.monotonic() + timeout
        results = []
        fatal_env = {'G_DEBUG': 'fatal-criticals'} if spec['fatal'] else None
        handoff_929 = name in ('save-929-logical-cross-open-pinyin', 'save-929-logical-cross-open-zhuyin')
        lazy_bigram = name in ('lazy-user-bigram-pinyin', 'lazy-user-bigram-zhuyin')
        handoff = handoff_929 or name in ('issue1114-collision-load-save-pinyin', 'issue1114-collision-load-save-zhuyin')
        profile_key = 'CONTRACT_929_PROFILE' if handoff_929 else 'CONTRACT_1114_PROFILE'
        reload_key = 'CONTRACT_929_RELOAD' if handoff_929 else 'CONTRACT_1114_RELOAD'
        profiles = {}
        if handoff or lazy_bigram:
            for side in ('pin', 'subject'):
                profiles[side] = tempfile.mkdtemp(prefix='handoff-' + side + '-', dir=scratch)
        for side, so in (('pin', pin_so), ('subject', subject_so)):
            if side == 'pin' and spec.get('pin_not_run') is not None:
                results.append(dict(exit='NOT-RUN', result=None, userfiles=[], stderr_lines=0, stderr=''))
                continue
            previous_side = getattr(self.driver['WORKER_STATE'], 'side', None)
            self.driver['WORKER_STATE'].side = side
            remaining = deadline - time.monotonic()
            try:
                if remaining <= 0:
                    raise subprocess.TimeoutExpired(str(so), timeout)
                extra_env = (dict(fatal_env or {}, OXPINYIN_RUNTIME_SIDE=side)
                             if self.driver['selected_cell']() == 'tkrzw' else fatal_env)
                if handoff:
                    extra_env = dict(extra_env or {}, **{profile_key: profiles[side]})
                if lazy_bigram:
                    extra_env = dict(extra_env or {}, CONTRACT_BIGRAM_SNAPSHOTS=profiles[side])
                result = self.driver['run_worker'](spec['mode'], so.resolve(), data, name, scratch,
                                    extra_env=extra_env, timeout=remaining)
            except subprocess.TimeoutExpired as error:
                results.append(dict(exit='TIMEOUT', result=None, userfiles=[],
                                    stderr_lines=len((error.stderr or b'').splitlines()),
                                    stderr=(error.stderr or b'').decode('utf-8', 'surrogateescape')))
                break
            finally:
                self.driver['WORKER_STATE'].side = previous_side
            results.append(result)
        if len(results) == 1:
            results.append(dict(exit='NOT-RUN', result=None, userfiles=[], stderr_lines=0, stderr=''))
        if lazy_bigram and all(result['exit'] == 0 for result in results):
            for filename in ('empty.db', 'trained.db'):
                raw = [Path(profiles[side], filename).read_bytes() for side in ('pin', 'subject')]
                if self.driver['tkrzw_saved_bigram_bytes'](raw[0]) != self.driver['tkrzw_saved_bigram_bytes'](raw[1]):
                    results[0].update(exit=1, result={'error': filename + ' differs outside META_OFFSET_TIMESTAMP'})
                    return results
            for result in results:
                result['result']['saved_byte_comparisons'] = ['empty.db', 'trained.db']
        if handoff and all(result['exit'] == 0 for result in results):
            if not handoff_929:
                # Direct byte equality is the gate, not digest equality. Bucket
                # pointers and record links stay exact along with every payload byte.
                raw = [Path(profiles[side], 'user_bigram.db').read_bytes() for side in ('pin', 'subject')]
                if self.driver['tkrzw_saved_bigram_bytes'](raw[0]) != self.driver['tkrzw_saved_bigram_bytes'](raw[1]):
                    results[0].update(exit=1, result={'error': 'saved bytes differ outside META_OFFSET_TIMESTAMP'})
                    return results
            # Fresh workers isolate the two libraries. Run both donor -> receiver
            # directions under the same case deadline, retaining donor files.
            for index, (donor, receiver, so) in enumerate((('pin', 'subject', subject_so),
                                                         ('subject', 'pin', pin_so))):
                previous_side = getattr(self.driver['WORKER_STATE'], 'side', None)
                self.driver['WORKER_STATE'].side = receiver
                try:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise subprocess.TimeoutExpired(str(so), timeout)
                    reload = self.driver['run_worker'](spec['mode'], so.resolve(), data, name, scratch,
                                        extra_env=dict(OXPINYIN_RUNTIME_SIDE=receiver,
                                                       **{profile_key: profiles[donor], reload_key: '1'}),
                                        timeout=remaining)
                except subprocess.TimeoutExpired:
                    results[index].update(exit='TIMEOUT', result=None)
                    break
                finally:
                    self.driver['WORKER_STATE'].side = previous_side
                if reload['exit'] != 0 or reload['result'] is None:
                    results[index].update(exit=reload['exit'] or 1, result=None,
                                          stderr=reload['stderr'], stderr_lines=reload['stderr_lines'])
                else:
                    if handoff_929 and self.driver['observed'](reload['result']) != self.driver['observed'](results[index]['result']):
                        results[index].update(exit=1, result={'error': 'cross-open changed logical records'})
                    else:
                        results[index]['result']['cross_reload'] = self.driver['observed'](reload['result'])
        return results

    def _handoff(self, name):
        """(is a handoff case, is the #929 logical one, profile env key, reload env key)."""
        handoff_929 = name in ('save-929-logical-cross-open-pinyin', 'save-929-logical-cross-open-zhuyin')
        handoff = handoff_929 or name in ('issue1114-collision-load-save-pinyin', 'issue1114-collision-load-save-zhuyin')
        return (handoff, handoff_929, 'CONTRACT_929_PROFILE' if handoff_929 else 'CONTRACT_1114_PROFILE',
                'CONTRACT_929_RELOAD' if handoff_929 else 'CONTRACT_1114_RELOAD')

    def _one_side(self, side, spec, so, data, name, scratch, deadline, timeout, extra_env):
        """One worker for SIDE under the case deadline; a timeout is an observation."""
        previous_side = getattr(self.driver['WORKER_STATE'], 'side', None)
        self.driver['WORKER_STATE'].side = side
        try:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise subprocess.TimeoutExpired(str(so), timeout)
            if self.driver['selected_cell']() == 'tkrzw':
                extra_env = dict(extra_env or {}, OXPINYIN_RUNTIME_SIDE=side)
            observation = self.driver['run_worker'](spec['mode'], so.resolve(), data, name, scratch,
                                                    extra_env=extra_env, timeout=remaining)
            # A handoff profile directory has a generated name, like the user and system ones.
            observation['stderr'] = re.sub(r'handoff-(pin|subject)-[A-Za-z0-9_]+', r'handoff-\1-X',
                                           observation['stderr'])
            # So is any other mkdtemp name a case makes (`wrap-q83kle1_`): prefix, dash, eight characters.
            observation['stderr'] = re.sub(r'(SCRATCH/[A-Za-z0-9_.]+-)[a-z0-9_]{8}(?![a-z0-9_])', r'\1X',
                                           observation['stderr'])
            return observation
        except subprocess.TimeoutExpired as error:
            return dict(exit='TIMEOUT', result=None, userfiles=[],
                        stderr_lines=len((error.stderr or b'').splitlines()),
                        stderr=(error.stderr or b'').decode('utf-8', 'surrogateescape'))
        finally:
            self.driver['WORKER_STATE'].side = previous_side

    def capture_case(self, spec, pin_so, data, name, scratch, timeout):
        """Run one case on the pin alone and return (observation, profile, package_data).

        PROFILE is the pin-written donor profile of a handoff case as {file: base64}, else
        None. PACKAGE_DATA says the case read the system data directory. A pin_not_run case
        does not run the pin. A handoff case then lets the pin read its own profile in a fresh
        worker: `cross_reload` is that observation, which a verifying subject must reproduce
        from the same file (the #929 case also requires it to equal the donor's own)."""
        if spec.get('pin_not_run') is not None:
            # No worker runs, so nothing shows whether the case reads the package data. Count it as
            # reading it (tkrzw only): the conservative reading, checked by the subject at verify.
            return dict(exit='NOT-RUN', result=None, userfiles=[], stderr_lines=0, stderr=''), None, True
        deadline = time.monotonic() + timeout
        fatal_env = {'G_DEBUG': 'fatal-criticals'} if spec['fatal'] else {}
        handoff, handoff_929, profile_key, reload_key = self._handoff(name)
        used = os.path.join(tempfile.mkdtemp(prefix='data-use-', dir=scratch), 'used')
        env = dict(fatal_env, CONTRACT_DATA_USE=used)
        profile = None
        lazy_bigram = name in ('lazy-user-bigram-pinyin', 'lazy-user-bigram-zhuyin')
        if handoff or lazy_bigram:
            profile = tempfile.mkdtemp(prefix='handoff-pin-', dir=scratch)
            if handoff:
                env[profile_key] = profile
            else:
                env['CONTRACT_BIGRAM_SNAPSHOTS'] = profile
        observation = self._one_side('pin', spec, pin_so, data, name, scratch, deadline, timeout, env)
        if lazy_bigram and observation['exit'] == 0:
            observation['result']['saved_byte_comparisons'] = ['empty.db', 'trained.db']
        package = os.path.exists(used)
        # The stored profile is the one the pin wrote, taken before the pin's own reload below
        # opens it (a reopen can rewrite the container's clock field).
        encoded = reference.encode_profile(profile) if profile else None
        if handoff and observation['exit'] == 0:
            reload = self._one_side('pin', spec, pin_so, data, name, scratch, deadline, timeout,
                                    {profile_key: profile, reload_key: '1'})
            if reload['exit'] != 0 or reload['result'] is None:
                observation.update(exit=reload['exit'] or 1, result=None, stderr=reload['stderr'],
                                   stderr_lines=reload['stderr_lines'])
            elif handoff_929 and self.driver['observed'](reload['result']) != self.driver['observed'](observation['result']):
                observation.update(exit=1, result={'error': 'cross-open changed logical records'})
            else:
                observation['result']['cross_reload'] = self.driver['observed'](reload['result'])
        return observation, encoded, package

    def verify_case(self, spec, row, subject_so, data, name, scratch, timeout):
        """Run one case on the subject alone and return [stored pin observation, subject observation].

        A handoff case also lets the subject read the pin-written profile the reference
        holds, an ordinary fixture here, and attaches what it observed as `cross_reload`.
        The #1114 case compares the saved bigram bytes with the pin's, timestamp masked."""
        identity = row.get('identity', {})
        pin = dict(row['pin'], capture_glibc=identity.get('environment', {}).get('glibc'),
                   capture_glib=identity.get('environment', {}).get('glib'))
        if spec.get('pin_not_run') is not None:
            pin = dict(exit='NOT-RUN', result=None, userfiles=[], stderr_lines=0, stderr='')
        deadline = time.monotonic() + timeout
        handoff, handoff_929, profile_key, reload_key = self._handoff(name)
        env = {'G_DEBUG': 'fatal-criticals'} if spec['fatal'] else {}
        own = None
        lazy_bigram = name in ('lazy-user-bigram-pinyin', 'lazy-user-bigram-zhuyin')
        if handoff or lazy_bigram:
            own = tempfile.mkdtemp(prefix='handoff-subject-', dir=scratch)
            if handoff:
                env[profile_key] = own
            else:
                env['CONTRACT_BIGRAM_SNAPSHOTS'] = own
        subject = self._one_side('subject', spec, subject_so, data, name, scratch, deadline, timeout, env or None)
        if lazy_bigram and subject['exit'] == 0 and pin['exit'] == 0:
            donor = tempfile.mkdtemp(prefix='handoff-pin-', dir=scratch)
            reference.decode_profile(row['profile'], donor)
            for filename in ('empty.db', 'trained.db'):
                saved = [self.driver['tkrzw_saved_bigram_bytes'](Path(directory, filename).read_bytes())
                         for directory in (donor, own)]
                if saved[0] != saved[1]:
                    subject.update(exit=1, result={'error': filename + ' differs outside META_OFFSET_TIMESTAMP'})
                    return [pin, subject]
            subject['result']['saved_byte_comparisons'] = ['empty.db', 'trained.db']
        if handoff and subject['exit'] == 0 and pin['exit'] == 0:
            donor = tempfile.mkdtemp(prefix='handoff-pin-', dir=scratch)
            reference.decode_profile(row['profile'], donor)
            if not handoff_929:
                masked = [self.driver['tkrzw_saved_bigram_bytes'](Path(directory, 'user_bigram.db').read_bytes())
                          for directory in (donor, own)]
                if masked[0] != masked[1]:
                    subject.update(exit=1, result={'error': 'saved bytes differ outside META_OFFSET_TIMESTAMP'})
                    return [pin, subject]
            reload = self._one_side('subject', spec, subject_so, data, name, scratch, deadline, timeout,
                                    {profile_key: donor, reload_key: '1'})
            if reload['exit'] != 0 or reload['result'] is None:
                subject.update(exit=reload['exit'] or 1, result=None, stderr=reload['stderr'],
                               stderr_lines=reload['stderr_lines'])
            elif handoff_929 and self.driver['observed'](reload['result']) != self.driver['observed'](
                    {key: value for key, value in pin['result'].items() if key != 'cross_reload'}):
                subject.update(exit=1, result={'error': 'cross-open changed logical records'})
            else:
                subject['result']['cross_reload'] = self.driver['observed'](reload['result'])
        return [pin, subject]

    @contextlib.contextmanager
    def case_jobs(self, requested):
        """Yield the case count of this run.

        The caller must join case threads and reap worker groups before
        leaving the context."""
        yield requested

    @contextlib.contextmanager
    def case_results(self, args, names, data, scratch, jobs):
        """Isolate cases and consume their results in the original selection order.

        Use a separate scratch directory per case. Serial execution yields a lazy
        map; parallel execution schedules a thread pool. Each item is (name, result), with PLATFORM_SKIP for excluded platforms
        and None for excluded cells. On a parallel-body exception mark cancellation;
        always wait for pool shutdown and cancel pending futures on that path's exit."""
        def observe(name):
            """Check case eligibility, look up its reference and run the subject in a per-case directory.

            A missing or incompatible reference is returned as the result, not raised."""
            self.driver['check_cancelled']()
            spec = self.driver['CASES'][name]
            if self.driver['platform_skips'](spec) and not getattr(args, 'run_platform_excluded', False):
                return name, self.driver['PLATFORM_SKIP']
            if args.cell not in spec['cells']:
                return name, None
            store = args.references
            # The subject alone, against the stored pin observation.
            try:
                row = store.row(spec)
            except reference.ReferenceError as error:
                return name, error
            subject_so = args.zhuyin_so if spec['mode'] == 'zhuyin' else args.pinyin_so
            assert subject_so and Path(subject_so).is_file(), '%s needs %s' % (
                name, '--zhuyin-so' if spec['mode'] == 'zhuyin' else 'PINYIN_SO')
            with tempfile.TemporaryDirectory(prefix='case-', dir=scratch) as isolated:
                pair = self.verify_case(spec, row, Path(subject_so), data, name, isolated, args.case_timeout)
                return name, pair
        if jobs == 1:
            yield map(observe, names)
            return
        from concurrent.futures import ThreadPoolExecutor
        pool = ThreadPoolExecutor(max_workers=jobs)
        try:
            # map schedules the pool concurrently but yields in input order. Only
            # the controller writes observations, verdicts and diagnostics.
            yield pool.map(observe, names)
        except BaseException:
            self.set_state(cancelled=True)
            raise
        finally:
            pool.shutdown(wait=True, cancel_futures=True)
