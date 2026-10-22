#!/usr/bin/env bash
# Live parity aggregate; registry and orchestration use Python's standard library.
set -euo pipefail
for name in UV_PYTHON PYTHONHOME PYTHONPATH; do
	if [[ -n ${!name:-} ]]; then
		printf 'FAIL: %s is not supported; unset it before running repository Python\n' "$name" >&2
		exit 2
	fi
done
command -v uv >/dev/null 2>&1 || {
	printf 'missing uv 0.13.1 or newer; install it and managed CPython 3.15.0+gil (docs/runbooks/setup.md)\n' >&2
	exit 127
}
exec uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal "$(dirname "${BASH_SOURCE[0]}")/run-all.py" "$@"
