"""Python adapter to the single shell runtime check in runtime.sh."""

import os
from pathlib import Path
import subprocess
import sys


class RuntimeRefused(RuntimeError):
    pass


def check(cell, runner, side, obj, *, env=None, cwd=None):
    """Check the actual object/environment/cwd, independent of a cell label."""
    script = Path(__file__).resolve().with_suffix('.sh')
    try:
        result = subprocess.run(['bash', str(script), '--print-path', cell, runner, side, str(obj)],
                                env=env, cwd=cwd, capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise RuntimeRefused(f'{runner}: {side}: libtkrzw resolved to <unknown>: {error}') from error
    if result.returncode:
        message = result.stderr.strip()
        raise RuntimeRefused(message.removeprefix('REFUSED: ') or
                             f'{runner}: {side}: runtime resolver failed for {obj}')
    path = result.stdout.strip()
    return Path(path) if path else None


def main():
    script = Path(__file__).resolve().with_suffix('.sh')
    os.execvp('bash', ['bash', str(script), *sys.argv[1:]])


if __name__ == '__main__':
    main()
