"""Host command execution through the shared cancellation and supervision services."""
import os
from pathlib import Path
import sys

from . import slot
from .native import launch_status, shell_status
from .runners import Finished
from .measure import bench

ROOT = Path(__file__).resolve().parents[2]
PYTHON = [sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev']


def run(command, *, env=None, cwd=None, stdout=None, stdin=None, stderr=None, heavy=False, check=True):
    """Launch a command, preserve its shell status, and reap it on cancellation."""
    command = [str(part) for part in command]
    environment = dict(os.environ if env is None else env)
    sys.stdout.flush()
    sys.stderr.flush()
    try:
        if heavy:
            # slot.run_command owns all heavy admission, including inheritance.
            if stdin is not None or stderr is not None:
                raise ValueError('heavy launches use inherited stdin/stderr')
            code = shell_status(slot.run_command(command, env=environment, cwd=cwd, stdout=stdout))
        else:
            code = bench.run(command, cwd=cwd, env=environment, stdin=stdin, stdout=stdout,
                             stderr=stderr, check_status=False)
    except OSError as error:
        print(f'{command[0]}: {error.strerror}', file=sys.stderr)
        code = launch_status(error)
    if check and code:
        raise Finished(code)
    return code


def capture(command, *, env=None, cwd=None, check=True, stderr=True):
    """Capture a small host command with the shared cancellable process adapter."""
    done = bench.capture(command, env=env, cwd=cwd)
    if stderr and done.stderr:
        bench.write(done.stderr, error=True)
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
