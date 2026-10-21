"""Hard-limit transport for compat.sh. Admission lives only in tools.dev.slot.

The named container is created without starting its workload. Its reported hard
limit is checked before start. The reservation and creation fence precede even
create; runtime clients inherit the fence across launcher SIGKILL. Only a
confirmed exit permits release. An interrupted create is deliberately retained.
"""
import argparse
import contextlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time

from tools.dev import host, slot


def call(runtime, args, *, pass_fds=(), console=None, cancellable=True):
    with contextlib.ExitStack() as stack:
        output = stack.enter_context(open(console, 'wb')) if console else None
        process = subprocess.Popen([runtime, *args], pass_fds=pass_fds,
                                   start_new_session=True,
                                   stdout=(subprocess.PIPE if output else
                                           None if args[0] == 'start' else subprocess.DEVNULL),
                                   stderr=subprocess.STDOUT if output else None)
        def copy():
            mirror_stdout = True
            while data := process.stdout.read1(65536):
                output.write(data)
                output.flush()
                if mirror_stdout:
                    try:
                        sys.stdout.buffer.write(data)
                        sys.stdout.buffer.flush()
                    except BrokenPipeError:
                        # A consumer such as head may leave early. Keep the
                        # sole pipe reader alive and preserve the full console.
                        mirror_stdout = False
        thread = None
        if output:
            thread = threading.Thread(target=copy)
            thread.start()
        try:
            return slot.wait_child(process) if cancellable else process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            slot.terminate_group(process)
            raise
        finally:
            if thread:
                thread.join()
                process.stdout.close()


def verify_limit(record):
    row = host.inspect_container(record)
    try:
        if record['kind'] == 'container':
            actual = row['configuration']['resources']['memoryInBytes']
            labels = row['configuration']['labels']
        else:
            actual = row['HostConfig']['Memory']
            swap = row['HostConfig']['MemorySwap']
            if swap != actual:
                raise ValueError('runtime did not report the requested memory+swap cap')
            labels = row['Config']['Labels']
        if (type(actual) is not int or actual != record['memory_mib'] * 1024 * 1024
                or labels.get('io.oxpinyin.memory-reservation') != record['token']):
            raise ValueError('hard memory limit or reservation identity mismatch')
    except (TypeError, KeyError) as exc:
        raise ValueError('runtime cannot report its hard memory limit') from exc


# Docker/Podman can accept a configured limit on a host without its controller.
# Gate the payload on the effective kernel limit too. This shell is not an
# admission implementation: it reads no host budget, slots or reservations.
LIMIT_GATE = '''limit=$1; shift
found=0
for path in /sys/fs/cgroup/memory.max /sys/fs/cgroup/memory/memory.limit_in_bytes; do
    if test -r "$path"; then
        read -r actual < "$path"
        case $actual in ''|max|*[!0-9]*) continue ;; esac
        if test "$actual" -gt 0 && test "$actual" -le "$limit"; then found=1; fi
    fi
done
if test "$found" != 1; then
    echo 'FAIL: runtime cannot enforce/report the requested hard memory limit' >&2
    exit 1
fi
exec "$@"
'''

SMOKE = '''printf 'Memory smoke guest: '; uname -sm
grep MemTotal /proc/meminfo
printf 'Memory smoke alive; hard limit: %s MiB\\n' "$1"
sleep 12
python3 -c 'import sys; bytearray(int(sys.argv[1]) * 2 * 1024 * 1024)' "$1"
status=$?
if test "$status" != 137; then
    echo "FAIL: over-limit allocation exit $status (expected 137)" >&2
    exit 1
fi
echo 'Memory smoke: over-limit allocation killed (137)'
'''


def run(runtime, kind, name, memory_mib, command, *, console=None, remove=False,
        exit_confirmed=None, out=sys.stderr):
    reservation = slot.ContainerReservation(runtime, kind, name, memory_mib, out=out)
    reservation.acquire()
    created = start_attempted = False
    try:
        # The separator keeps runtime options and payload boundaries explicit.
        if '--' in command:
            boundary = command.index('--')
            options, payload = command[:boundary], command[boundary + 1:]
        else:
            options, payload = [], command
        if not payload:
            raise ValueError('container image required')
        args = ['create', '--name', name, '--memory', f'{memory_mib}M',
                '--label', f'io.oxpinyin.memory-reservation={reservation.record["token"]}']
        if kind != 'container':
            args += ['--memory-swap', f'{memory_mib}M', '--entrypoint', '/bin/sh']
            payload = [payload[0], '-c', LIMIT_GATE, '--', str(memory_mib * 1024 * 1024), *payload[1:]]
        args += [*options, *payload]
        rc = call(runtime, args, pass_fds=reservation.pass_fds())
        if rc:
            released = reservation.create_failed()
            outcome = 'reservation released' if released else 'runtime state unknown; reservation retained'
            raise ValueError(f'runtime create failed ({rc}); {outcome}')
        created = True
        reservation.created()
        verify_limit(reservation.record)
        slot.check_cancelled()
        print(f'container hard memory limit verified: {memory_mib} MiB ({kind})', file=out, flush=True)
        start_attempted = True
        return call(runtime, ['start', '--attach', name],
                    pass_fds=reservation.pass_fds(), console=console)
    finally:
        try:
            if created:
                # Always stop/wait, even after an attached client exits. A
                # disconnected CLI alone is not evidence of container exit.
                try:
                    call(runtime, ['stop', name], pass_fds=reservation.pass_fds(), cancellable=False)
                except (OSError, subprocess.SubprocessError) as exc:
                    print(f'container stop uncertain: {exc}', file=out, flush=True)
                if not start_attempted:
                    # A settled create failed validation before any start.
                    # Delete the inert object, then confirm absence as usual.
                    call(runtime, ['rm', name], cancellable=False)
                deadline = time.monotonic() + 30
                while host.container_liveness(reservation.record) == 'live' and time.monotonic() < deadline:
                    time.sleep(0.1)
                if not reservation.release():
                    raise ValueError('container exit unconfirmed; native reservation retained')
                if exit_confirmed is not None:
                    Path(exit_confirmed).touch()
                if remove:
                    call(runtime, ['rm', name], cancellable=False)
        finally:
            reservation.close()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime', required=True)
    parser.add_argument('--name', required=True)
    parser.add_argument('--memory-mib', default=os.environ.get('OXPINYIN_CONTAINER_MEMORY_MIB'))
    parser.add_argument('--console')
    parser.add_argument('--remove', action='store_true')
    parser.add_argument('--smoke', action='store_true')
    parser.add_argument('--exit-confirmed')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    try:
        size = host.container_memory_mib(args.memory_mib)
        runtime = shutil.which(args.runtime)
        if runtime is None or Path(runtime).name not in ('container', 'docker', 'podman'):
            raise ValueError('runtime must enforce memory limits: container, docker or podman')
        kind = Path(runtime).name
        command = args.command[1:] if args.command[:1] == ['--'] else args.command
        if not command:
            raise ValueError('container command required')
        if args.smoke:
            command += ['bash', '-c', SMOKE, '--', str(size)]
        with slot.cancellation_handlers():
            rc = run(str(Path(runtime).absolute()), kind, args.name, size, command,
                     console=args.console, remove=args.remove, exit_confirmed=args.exit_confirmed)
        return 128 - rc if rc < 0 else rc
    except (OSError, ValueError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr, flush=True)
        return 130 if isinstance(exc, slot.Cancelled) else 1


if __name__ == '__main__':
    raise SystemExit(main())
