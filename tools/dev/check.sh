#!/usr/bin/env bash
# First setup acceptance: compile, link and execute existing native-library tests.

# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
case ${1:-bdb} in
bdb|tkrzw|kyotocabinet) backend=${1:-bdb} ;;
*) echo 'usage: tools/dev/check.sh [bdb|tkrzw|kyotocabinet]' >&2; exit 2 ;;
esac
for tool in cargo rustup cc pkg-config; do
	command -v "$tool" >/dev/null || { echo "missing $tool; see docs/runbooks/setup.md" >&2; exit 1; }
done
printf 'System: '; uname -sm
rustup show active-toolchain
rustc -vV
printf 'Backend: %s; target directory: %s\n' "$backend" "${CARGO_TARGET_DIR:-target}"
pkg-config --modversion libclang 2>/dev/null || echo 'libclang: discovered by bindgen during build (a .pc file is optional)'
case $backend in
bdb) printf 'Berkeley DB header: '; printf '#include <db.h>\nDB_VERSION_STRING\n' | cc ${OXPINYIN_BDB_INCLUDE_DIR:+-I"$OXPINYIN_BDB_INCLUDE_DIR"} -E -P - | tail -1 ;;
*) pkg-config --modversion "$backend" ;;
esac
for input in PINYIN_MODEL_DIR PINYIN_ORACLE_PREFIX; do
	path=${!input:-}
	if [[ -z $path ]]; then
		printf '%s: not selected (optional for setup)\n' "$input"
	elif [[ -d $path ]]; then
		printf '%s: directory present: %s (not hash/parity verification)\n' "$input" "$path"
	else
		printf '%s: MISSING directory: %s (optional for setup)\n' "$input" "$path"
	fi
done
args=(--locked --no-default-features --features "$backend" -p oxpinyin-store)
if [[ $(uname -s) == Linux ]]; then
	pkg-config --modversion glib-2.0
	args+=(-p oxpinyin-capi)
fi
cargo test "${args[@]}"
printf '\nSetup check passed: native backend tests linked and ran. This is not oracle parity.\n'
