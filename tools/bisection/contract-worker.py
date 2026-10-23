#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Worker entry for contract-diff.py: one fresh process per side per case.

`python contract-diff.py --worker ...` compiled the whole driver on every
start: a script run as `__main__` is never bytecode-cached, and the compile
was most of a worker's cost (91 ms of 127 ms on the baseline host, against
28 ms for executing the body and registering every case). This stub loads
the same file through the import system, so its bytecode is cached in
`__pycache__` (validated against the source's mtime and size; with
PYTHONDONTWRITEBYTECODE set Python compiles as before), then calls the
same `worker` with the same arguments:

    contract-worker.py MODE SO DATA CASE

Nothing else changes: the process is still fresh per side per case, the
environment (TMPDIR, CONTRACT_DIFF_CELL, G_DEBUG, AXIS_C_*) is inherited,
and the driver's `__file__` is the same path.
"""
import importlib.util
from pathlib import Path
import sys

_spec = importlib.util.spec_from_file_location('contract_diff', Path(__file__).with_name('contract-diff.py'))
_module = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_module)
_module.worker(*sys.argv[1:])
