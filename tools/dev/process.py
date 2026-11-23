"""Host command execution through the shared cancellation and supervision services."""
import os
from pathlib import Path
import sys

from . import pool, slot
from .native import launch_status, shell_status
from .runners import Finished, write

ROOT = Path(__file__).resolve().parents[2]


def run(command, *, env=None, cwd=None, stdout=None, stdin=None, stderr=None, check=True):
    """Launch a command, preserve its shell status, and reap it on cancellation."""
    command = [str(part) for part in command]
    environment = dict(os.environ if env is None else env)
    sys.stdout.flush()
    sys.stderr.flush()
    try:
        code = shell_status(slot.run_command(command, env=environment, cwd=cwd, stdout=stdout,
                                             stdin=stdin, stderr=stderr))
    except OSError as error:
        print(f'{command[0]}: {error.strerror}', file=sys.stderr)
        code = launch_status(error)
    if check and code:
        raise Finished(code)
    return code


def capture(command, *, env=None, cwd=None, check=True, stderr=True, merge_stderr=False):
    """Capture a small host command with the shared cancellable process adapter."""
    command = [str(part) for part in command]
    sys.stdout.flush()
    sys.stderr.flush()
    context = pool.UnitContext('measure', slot.CANCELLED,
                               environment=dict(os.environ if env is None else env))
    done = context.launch(command, cwd=cwd, merge_stderr=merge_stderr)
    done = done._replace(status=shell_status(done.status))
    if stderr and done.stderr:
        write(done.stderr, error=True)
    if check and done.status:
        raise Finished(done.status)
    return done


def text(command, **kwargs):
    """Capture command substitution text, stripping only trailing newlines."""
    return capture(command, **kwargs).stdout.decode().rstrip('\n')


def fail(message, status=1):
    """Print a diagnostic and terminate the current recipe with STATUS."""
    print(message, file=sys.stderr)
    raise Finished(status)
