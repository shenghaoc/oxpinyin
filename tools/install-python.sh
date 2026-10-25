#!/usr/bin/env bash
# Install the managed interpreter that .python-version names, only when absent.
# `just python`, tools/cloud/setup.sh, CI and tools/oracle/Dockerfile.compat call
# this, so the version is stated once, in .python-version (#1230).
#
# uv has no download keyed +gil, so `uv python install` with no arguments fails on
# the file's GIL request ("No download found for request"). The plain request for
# the same series installs the GIL build, and selection keeps the file's request:
# this script derives the install request from the file by dropping +gil. It is the
# one place that does (the PowerShell `python` recipe in the justfile cannot run it).
#
# Run it with uv on PATH. It resolves the file next to its own directory, so the
# working directory does not matter.

# shellcheck source=lib/bash-floor.sh
. "${0%/*}/lib/bash-floor.sh"

set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
request=$(<"$root/.python-version")
install=${request%+gil}

cd "$root"
if ! uv python find --managed-python >/dev/null 2>&1; then
	uv python install "$install"
fi
if ! uv python find --managed-python >/dev/null; then
	echo "$request does not resolve after installing $install" >&2
	exit 1
fi
