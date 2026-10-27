"""Compat command execution through the existing cancellation and admission services."""
import os
from pathlib import Path
import subprocess
import sys

from .. import slot
from ..native import launch_status, shell_status
from ..runners import Finished
from ..measure import bench

ROOT = Path(__file__).resolve().parents[3]
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


def tee(command, path, *, env=None, cwd=None, heavy=False, append=False):
    """Mirror merged child output to a local file while the existing launcher supervises it."""
    import threading
    read_fd, write_fd = os.pipe()
    with os.fdopen(read_fd, 'rb') as reader, os.fdopen(write_fd, 'wb') as writer, \
            Path(path).open('ab' if append else 'wb') as output:
        def copy():
            for data in iter(lambda: reader.read1(65536), b''):
                output.write(data)
                output.flush()
                try:
                    sys.stdout.buffer.write(data)
                    sys.stdout.buffer.flush()
                except BrokenPipeError:
                    pass
        thread = threading.Thread(target=copy)
        thread.start()
        try:
            if heavy:
                command = [sys.executable, '-c',
                           'import os,sys; os.dup2(1,2); os.execvpe(sys.argv[1],sys.argv[1:],os.environ)',
                           *map(str, command)]
                return run(command, env=env, cwd=cwd, stdout=writer, heavy=True, check=False)
            return run(command, env=env, cwd=cwd, stdout=writer, stderr=subprocess.STDOUT, check=False)
        finally:
            writer.close()
            thread.join()
