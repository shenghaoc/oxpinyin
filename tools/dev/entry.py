"""Validate the interpreter before running a managed Python command."""
import sys


def require_normal_imports():
    """Reject missing interpreter support or an unexpected import mode."""
    _get_lazy_imports = getattr(sys, 'get_lazy_imports', None)
    if _get_lazy_imports is None:
        print('repository Python: sys.get_lazy_imports is missing; '
              'run the interpreter in .python-version with -X lazy_imports=normal', file=sys.stderr)
        raise SystemExit(2)
    _mode = _get_lazy_imports()
    if _mode != 'normal':
        print(f'repository Python: lazy import mode is {_mode!r}; '
              'run with -X lazy_imports=normal (or PYTHON_LAZY_IMPORTS=normal)', file=sys.stderr)
        raise SystemExit(2)
