"""Validate the interpreter before running a managed Python command."""
import sys


def require_utf8_mode():
    """Reject missing interpreter support or disabled UTF-8 mode."""
    mode = sys.flags.utf8_mode
    if mode != 1:
        print(f'repository Python: UTF-8 mode is {mode!r}; '
              'run with -X utf8 (or PYTHONUTF8=1)', file=sys.stderr)
        raise SystemExit(2)
