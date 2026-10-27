#!/usr/bin/env bash
# External interface: preserve the documented `bash tools/oracle/compat.sh` CLI.
# Interpreter overrides must be refused before uv selects Python.

# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"
set -euo pipefail
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo"
for variable in UV_PYTHON UV_PYTHON_VERSION UV_PYTHON_IMPLEMENTATION UV_PYTHON_VARIANT UV_PYTHON_ARCH UV_PYTHON_BIN_DIR UV_PYTHON_PREFERENCE PYTHONHOME PYTHONPATH; do
    [[ -z ${!variable:-} ]] || { printf 'FAIL: %s is not supported; unset it before running repository Python\n' "$variable" >&2; exit 1; }
done
command -v uv >/dev/null || { echo 'FAIL: missing uv 0.13.1 or newer and the managed interpreter in .python-version' >&2; exit 1; }
exec uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev compat "$@"
