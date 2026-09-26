#!/usr/bin/env bash
# pc-version.sh — print the drop-in's libpinyin version: the `.pc` Version,
# the `libpinyin-<version>` include subdirectory and the version the release
# packages Provide.
#
# The one source of truth is the oracle pin record's `libpinyin_tag`
# (tools/oracle/oracle-pin.txt) — the version the pin's configure.ac stamps
# into its own libpinyin.pc. Both capi build scripts read the same line
# (crates/oxpinyin-capi/build_pin_version.rs) and fail the build when their
# Cargo.toml [package.metadata.capi] tables disagree with it, so a pin bump
# that edits the record moves every consumer of the version together.
#
# Usage: tools/packaging/pc-version.sh
# Exits 0 printing the version; 1 when the record has no well-formed line.

set -euo pipefail
RECORD="$(cd "$(dirname "$0")/../oracle" && pwd)/oracle-pin.txt"
version="$(sed -n 's/^libpinyin_tag=//p' "$RECORD")"
if ! printf '%s\n' "$version" | grep -Eqx '[0-9]+\.[0-9]+\.[0-9]+'; then
  echo "error: $RECORD: no well-formed libpinyin_tag=<major>.<minor>.<micro> line" >&2
  exit 1
fi
printf '%s\n' "$version"
