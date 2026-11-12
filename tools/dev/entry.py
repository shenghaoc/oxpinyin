"""Validate the interpreter before running a managed Python command."""
import sys


def require_utf8_mode():
    """Reject missing interpreter support or disabled UTF-8 mode."""
    mode = getattr(sys.flags, 'utf8_mode', None)
    if mode is None:
        print('repository Python: sys.flags.utf8_mode is missing; '
              'run the interpreter in .python-version with -X utf8', file=sys.stderr)
        raise SystemExit(2)
    if mode != 1:
        print(f'repository Python: UTF-8 mode is {mode!r}; '
              'run with -X utf8 (or PYTHONUTF8=1)', file=sys.stderr)
        raise SystemExit(2)
