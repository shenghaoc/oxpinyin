#!/usr/bin/env bash
# Live parity aggregate; registry and orchestration use Python's standard library.
set -euo pipefail
exec python3 "$(dirname "${BASH_SOURCE[0]}")/run-all.py" "$@"
