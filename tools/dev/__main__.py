"""Thin entry point for `python -m tools.dev`."""
import sys

# Normal import mode is required before any application import (PEP 810).
# This check imports nothing from tools.dev; status 2 is the CLI's usage error.
_get_lazy_imports = getattr(sys, 'get_lazy_imports', None)
if _get_lazy_imports is None:
    print('python -m tools.dev: sys.get_lazy_imports is missing; '
          'run with cpython@3.15.0+gil and -X lazy_imports=normal', file=sys.stderr)
    raise SystemExit(2)
_mode = _get_lazy_imports()
if _mode != 'normal':
    print(f'python -m tools.dev: lazy import mode is {_mode!r}; '
          'run with -X lazy_imports=normal (or PYTHON_LAZY_IMPORTS=normal)', file=sys.stderr)
    raise SystemExit(2)

from .cli import main

raise SystemExit(main())
