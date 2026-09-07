#!/usr/bin/env bash
# check-exports.sh — the exported-symbol gate for the two C ABIs.
#
# The drop-in contract is the pin's version scripts: `libpinyin.ver` (79
# `pinyin_*` symbols) and `libzhuyin.ver` (52 `zhuyin_*` symbols), both
# checked in verbatim beside the crates. This script builds the two
# cdylibs and proves that the set of exported `pinyin_*` / `zhuyin_*`
# symbols is EXACTLY the script's `global:` list — nothing missing (a
# consumer would fail to link) and nothing extra (a symbol the pin does
# not export, which the compatibility policy treats as worse than a link
# error because it can be present and wrong).
#
# Every other exported symbol is listed and, outside the fixture hooks
# below, fails the gate too: a Rust cdylib exports only `#[no_mangle]`
# items, so anything unexpected is a leak.
#
# Two modes, because the export set is feature-dependent:
#   (default)   the development build, which carries the two
#               `oxpinyin_*` fixture hooks the ABI tests drive
#               (`#[cfg(not(feature = "shipped"))]` in oxpinyin-capi);
#   --shipped   `--features shipped`, the packaged artifact, where the
#               hooks are compiled out and only the .ver set may remain.
#
# Linux reads the dynamic symbol table (`nm -D --defined-only`); macOS is
# supported for local runs (`nm -gU`, leading underscore stripped). The
# shipped identity (SONAME, symbol versions) is not this script's concern
# — see crates/oxpinyin-capi/build.rs and docs/findings/drop-in-abi-identity.md.
#
# Exit: 0 = both libraries export exactly their .ver set; non-zero on any
# deviation, with the missing/extra symbols named.

set -euo pipefail
cd "$(dirname "$0")/../.."

shipped=0
if [[ "${1:-}" == "--shipped" ]]; then
	shipped=1
fi

features=()
if ((shipped)); then
	features=(--features shipped)
fi

echo "--- building oxpinyin-capi and oxpinyin-zhuyin-capi ($( ((shipped)) && echo "--features shipped" || echo default)) ---"
cargo build --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi ${features[@]+"${features[@]}"}

case "$(uname -s)" in
Linux)
	ext=so
	exported() { nm -D --defined-only "$1" | awk '$2 ~ /^[TtWw]$/ { print $3 }'; }
	;;
Darwin)
	ext=dylib
	exported() { nm -gU "$1" | awk '$2 == "T" { sub(/^_/, "", $3); print $3 }'; }
	;;
*)
	echo "fatal: unsupported host $(uname -s)" >&2
	exit 2
	;;
esac

# The `global:` names of a version script, one per line, sorted.
ver_globals() {
	awk '/global:/ { on = 1; next } /local:/ { on = 0 } on && /;/ { gsub(/[[:space:];]/, ""); print }' "$1" | sort -u
}

status=0
check() {
	local name=$1 lib=$2 ver=$3 prefix=$4
	shift 4
	local allowed_extra=("$@")
	echo "--- $name: $lib against $ver ---"
	[[ -f $lib ]] || {
		echo "fatal: $lib not built" >&2
		exit 1
	}
	local expected actual others
	expected=$(ver_globals "$ver")
	actual=$(exported "$lib" | grep -E "^${prefix}_" | sort -u || true)
	others=$(exported "$lib" | grep -vE "^${prefix}_" | sort -u || true)

	local missing extra
	missing=$(comm -23 <(printf '%s\n' "$expected") <(printf '%s\n' "$actual"))
	extra=$(comm -13 <(printf '%s\n' "$expected") <(printf '%s\n' "$actual"))
	local n_expected n_actual
	n_expected=$(printf '%s\n' "$expected" | grep -c . || true)
	n_actual=$(printf '%s\n' "$actual" | grep -c . || true)
	echo "${prefix}_* exported: $n_actual, in .ver: $n_expected"
	if [[ -n $missing ]]; then
		echo "MISSING (in .ver, not exported):"
		printf '%s\n' "$missing" | sed 's/^/  /'
		status=1
	fi
	if [[ -n $extra ]]; then
		echo "EXTRA (exported, not in .ver):"
		printf '%s\n' "$extra" | sed 's/^/  /'
		status=1
	fi
	local sym leak=0
	for sym in $others; do
		local ok=0 a
		for a in ${allowed_extra[@]+"${allowed_extra[@]}"}; do
			[[ $sym == "$a" ]] && ok=1
		done
		if ((ok)); then
			echo "allowed non-ABI export: $sym"
		else
			echo "LEAKED non-ABI export: $sym"
			leak=1
		fi
	done
	((leak)) && status=1
	if [[ -z $missing && -z $extra && $leak == 0 ]]; then
		echo "OK: exactly the .ver set"
	fi
}

hooks=()
if ((!shipped)); then
	# The ABI tests' fixture hooks, compiled out under --features shipped
	# (crates/oxpinyin-capi/src/context.rs).
	hooks=(oxpinyin_init_for_fixtures oxpinyin_test_set_user_bigram)
fi
check libpinyin "target/debug/libpinyin_capi.$ext" crates/oxpinyin-capi/libpinyin.ver pinyin ${hooks[@]+"${hooks[@]}"}
check libzhuyin "target/debug/libzhuyin_capi.$ext" crates/oxpinyin-zhuyin-capi/libzhuyin.ver zhuyin

if ((status)); then
	echo "FAIL: the exported symbol set is not the pin's version script"
else
	echo "PASS: both libraries export exactly their version script's set"
fi
exit $status
