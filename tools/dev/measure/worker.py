"""Build and supervise the existing C startup worker; pipes only, no captures."""
import contextlib
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import subprocess
import sys
import time

from .. import slot
from . import native
from ...bisection import startup_profile

ROOT = Path(__file__).resolve().parents[3]
POINTS = ('start', 'lib_load', 'init', 'config', 'alloc', 'first_candidate', 'end')


def sha256(raw):
    return hashlib.sha256(raw).hexdigest()


def exports(facade):
    crate = 'oxpinyin-capi' if facade == 'pinyin' else 'oxpinyin-zhuyin-capi'
    return re.findall(r'\b' + facade + r'_\w+(?=\s*;)',
                      (ROOT / 'crates' / crate / ('lib' + facade + '.ver')).read_text())


def build(work, env, pass_fds):
    source, executable = ROOT / 'tools/workers/startup.c', work / 'startup'
    command = ['cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2', '-g',
               '-o', str(executable), str(source)]
    if sys.platform != 'darwin':
        command.append('-ldl')
    result = execute(command, env=env, pass_fds=pass_fds)
    if result['status']:
        raise ValueError('startup worker build failed: ' + result['stderr'].decode(errors='replace'))
    record = dict(path=str(executable), sha256=sha256(executable.read_bytes()),
                  bytes=executable.stat().st_size, command=command,
                  source_sha256=sha256(source.read_bytes()))
    return executable, record


def check_identity(executable, record):
    required = ('path', 'sha256', 'bytes', 'command', 'source_sha256')
    if any(not record.get(key) for key in required):
        raise ValueError('worker executable identity field is missing')
    if (record['path'] != str(executable) or record['sha256'] != sha256(executable.read_bytes()) or
            record['bytes'] != executable.stat().st_size or
            record['source_sha256'] != sha256((ROOT / 'tools/workers/startup.c').read_bytes())):
        raise ValueError('worker executable/source does not match its identity record')


def execute(command, *, env, pass_fds=(), memory=False, cpu=None, timeout=120):
    """Drain both streams and a separate checkpoint pipe; reap before releasing resources."""
    checkpoint_read, checkpoint_write = os.pipe()
    ack_read, ack_write = os.pipe()
    env = dict(env)
    if memory:
        env['OXPINYIN_STARTUP_HANDSHAKE'] = f'{checkpoint_write},{ack_read}'
    fds = (*pass_fds, checkpoint_write, ack_read) if memory else pass_fds
    started = time.monotonic_ns()
    try:
        with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
                              start_new_session=True, pass_fds=fds) as process:
            os.close(checkpoint_write)
            checkpoint_write = None
            os.close(ack_read)
            ack_read = None
            streams = {'stdout': bytearray(), 'stderr': bytearray(), 'checkpoint': bytearray()}
            checkpoints = {}
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ, 'stdout')
                selector.register(process.stderr, selectors.EVENT_READ, 'stderr')
                if memory:
                    selector.register(checkpoint_read, selectors.EVENT_READ, 'checkpoint')
                try:
                    if cpu is not None:
                        # Affinity is inherited from the controller, set before spawning by its caller.
                        if os.sched_getaffinity(process.pid) != {cpu}:
                            raise ValueError('worker did not inherit the selected CPU affinity')
                    while selector.get_map() or process.poll() is None:
                        slot.check_cancelled()
                        if time.monotonic_ns() - started >= timeout * 1e9:
                            raise ValueError('startup worker timed out')
                        for key, _ in selector.select(.1):
                            block = os.read(key.fd, 65536)
                            if not block:
                                selector.unregister(key.fileobj)
                                continue
                            streams[key.data].extend(block)
                            if key.data == 'checkpoint':
                                buffered = streams['checkpoint']
                                while b'\n' in buffered:
                                    line, _, rest = buffered.partition(b'\n')
                                    buffered[:] = rest
                                    expected = ('point ' + POINTS[len(checkpoints)]).encode() if len(checkpoints) < len(POINTS) else b''
                                    if line != expected:
                                        raise ValueError('worker checkpoint sequence changed: ' + repr(line))
                                    checkpoints[POINTS[len(checkpoints)]] = native.read_memory(process.pid)
                                    os.write(ack_write, b'\n')
                    process.wait()
                    slot.check_cancelled()
                    if memory and process.returncode == 0 and tuple(checkpoints) != POINTS:
                        raise ValueError('worker checkpoints are incomplete')
                except BaseException:
                    slot.terminate_group(process)
                    raise
            elapsed = time.monotonic_ns() - started
            return dict(status=process.returncode, pid=process.pid, stdout=bytes(streams['stdout']),
                        stderr=bytes(streams['stderr']), checkpoints=checkpoints, process_wall=elapsed)
    finally:
        for fd in (checkpoint_read, checkpoint_write, ack_read, ack_write):
            if fd is not None:
                os.close(fd)


def parse(stdout):
    if not stdout.endswith(b'\n'):
        raise ValueError('unterminated worker output')
    lines = stdout.splitlines()
    documents = [json.loads(line) for line in lines]
    units = [line for line, row in zip(lines, documents) if row.get('startup') == 'units']
    if len(units) != 1 or json.loads(units[0]).get('stopped_at') != 'end':
        raise ValueError('worker did not complete U1–U6 exactly once')
    # Hash the exact canonical record including its terminating newline, without reserializing JSON.
    return documents, sha256(units[0] + b'\n')


def userfiles(directory):
    """Compare raw non-database bytes and the complete filename set. No DB row reader."""
    result = {}
    for path in sorted(directory.rglob('*')):
        if path.is_symlink() or (not path.is_dir() and not path.is_file()):
            raise ValueError('non-regular user file: ' + str(path))
        if not path.is_file():
            continue
        raw = path.read_bytes()
        # BDB native files, Kyoto Cabinet and tkrzw stores are verified through
        # public-API reopen observations. Their containers carry native identities.
        database = (path.suffix in ('.db', '.tkt', '.tkh', '.kch', '.kct') or
                    path.name.endswith('_index.bin') or
                    raw[12:16] in (b'\x62\x31\x05\x00', b'\x61\x15\x06\x00') or
                    raw.startswith((b'KC\n', b'Tkrzw')))
        result[path.relative_to(directory).as_posix()] = dict(database=True) if database else dict(raw_hex=raw.hex())
    return result


@contextlib.contextmanager
def checked_profile(profile, witness):
    manifest = profile.parent / 'profile.manifest'
    if manifest.is_file():
        startup_profile.check(profile, manifest)
    elif any(profile.iterdir()):
        raise ValueError('nonempty source profile has no manifest: ' + str(profile))
    identity = startup_profile.identity(profile)
    witness_digest = sha256(witness.read_bytes()) if witness is not None else None
    try:
        yield
    finally:
        if manifest.is_file():
            startup_profile.check(profile, manifest)
        if startup_profile.identity(profile) != identity:
            raise ValueError('source profile was changed')
        if witness is not None and sha256(witness.read_bytes()) != witness_digest:
            raise ValueError('source witness was changed')


@contextlib.contextmanager
def sample_directory(work):
    # Sequential fresh copies at one pathname preserve raw stderr path bytes.
    # Never reuse directory contents or a live process. A stale directory refuses.
    path = work / 'sample'
    path.mkdir()
    try:
        yield path
    finally:
        shutil.rmtree(path)


class Worker:
    def __init__(self, executable, identity, data, work, env, pass_fds, cpu, bind):
        self.executable, self.identity, self.data, self.work = executable, identity, data, work
        self.env, self.pass_fds, self.cpu, self.bind = env, pass_fds, cpu, bind

    def run(self, library, facade, profile, *, mode='measure', witness=None, memory=False, probe=None, scope=None):
        check_identity(self.executable, self.identity)
        with checked_profile(profile, witness), sample_directory(self.work) as temporary:
            user = temporary / 'user'
            shutil.copytree(profile, user)
            argument = ','.join(POINTS) if memory else '-'
            if mode == 'verify' and witness is not None:
                argument = str(witness)
            if probe is not None:
                argument = str(probe)
            command = [str(self.executable), mode, facade, str(library), str(self.data), str(user), self.bind, argument]
            if scope:
                command = ['valgrind', '--tool=callgrind', '--collect-atstart=no', '--cache-sim=no',
                           '--branch-sim=no', '--dump-instr=yes', '--compress-pos=no', '--demangle=yes', '--read-inline-info=yes',
                           '--callgrind-out-file=/dev/stderr',
                           '--vgdb=no', '--toggle-collect=startup_' + scope + '_unit', *command]
            env = dict(self.env, OXPINYIN_STARTUP_EXPORTS=','.join(exports(facade)))
            env.pop('OXPINYIN_STARTUP_STEADY', None)
            env.pop('OXPINYIN_STARTUP_LOADER', None)
            if scope in ('first_candidate', 'steady'):
                env['OXPINYIN_STARTUP_LOADER'] = '1'
            if scope == 'steady':
                env['OXPINYIN_STARTUP_STEADY'] = '1'
            result = execute(command, env=env, pass_fds=self.pass_fds, memory=memory, cpu=self.cpu,
                             timeout=1800 if scope else 120)
            if probe is not None:
                return dict(status=result['status'],
                            outcome='crashed' if result['status'] < 0 else 'refused' if result['status'] else 'answered',
                            stdout_sha256=sha256(result['stdout']), stderr_sha256=sha256(result['stderr']),
                            stdout_bytes=len(result['stdout']), stderr_bytes=len(result['stderr']))
            if result['status']:
                raise ValueError(f"startup worker failed ({result['status']}): {result['stderr'][-3000:]!r}")
            if mode == 'persist':
                documents = [json.loads(line) for line in result['stdout'].splitlines()]
                digest = None
            else:
                documents, digest = parse(result['stdout'])
            result.update(documents=documents, digest=digest, userfiles=userfiles(user), command=command)
            return result
