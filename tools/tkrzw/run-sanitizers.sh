#!/usr/bin/env bash
# run-sanitizers.sh — the tkrzw backend's sanitizer gate.
#
# The backend passes Rust callbacks into tkrzw's visitor dispatch: db_get
# and db_scan install Process-full visitors that hand borrowed key/value
# records to Rust fn pointers valid only for the callback. Panic-unwinding
# across that FFI boundary and borrow-escape of those records are the live
# hazards, so the feature's own test surface runs under AddressSanitizer
# or ThreadSanitizer and any finding fails the gate. There are no compiled
# C/C++ translation units — the backend is pure bindgen over
# tkrzw_langc.h — so the sanitizers instrument the Rust side of the FFI.
#
# rustc has no `undefined` sanitizer -- `-Zsanitizer=` accepts address,
# cfi, dataflow, hwaddress, kcfi, kernel-address, kernel-hwaddress, leak,
# memory, memtag, safestack, shadow-call-stack, thread and realtime, and
# nothing else. With no C targets, UBSan has nothing to instrument here.
#
# LeakSanitizer is off in the address gate by documented choice: the Rust
# harness holds deliberate process-lifetime allocations whose ownership
# crosses the boundary, which LSan misclassifies, and the hazards under
# test, misuse of BORROWED records and unwinding, both stay fatal.
# tools/sanitize.sh refuses standalone tkrzw leak for that reason.
# abort_on_error=1 closes the loophole where a report could print yet the
# run still pass. On Linux, the address gate wraps the test command in
# setarch -R: GitHub runner kernels' mmap entropy exceeds what ASan's shadow
# mapping assumes, and an unprivileged container cannot lower
# vm.mmap_rnd_bits itself. That is an ASan workaround and is not applied to
# ThreadSanitizer, which can fail to set up its shadow memory with address
# randomisation off. Darwin has no setarch and does not need it.
#
# The explicit --target keeps RUSTFLAGS off host build scripts and proc
# macros, which otherwise came out as unloadable sanitized dylibs (E0463);
# only the target graph is sanitized. The prebuilt standard library stays
# uninstrumented: -Zbuild-std is heavier than this gate warrants
# (docs/findings/upstream-divergences.md). ThreadSanitizer names the
# sanitizer ABI mismatch explicitly, because the prebuilt standard library
# is not instrumented.
#
# The library must be sound. Ubuntu's packaged libtkrzw cannot read files
# it writes (docs/findings/tkrzw-distro-compat.md), so a run against it is
# not a result. The gate uses the verified runtime that
# tools/tkrzw/runtime.sh checks (docs/runbooks/cloud.md, "Runtime
# enforcement"): the library setup.sh's environment file names or, with none
# configured, a host libtkrzw that passes tools/tkrzw/distro-probe.sh. Every
# test binary must resolve libtkrzw to it before a test runs.
# The shared cell service snapshots the inherited loader path before
# any directory change, because cargo runs each test binary from the package
# directory. On macOS the just recipe puts Homebrew's tkrzw_dbm_util on
# PATH, and this probe certifies the Homebrew dylib.
#
# NOT covered: libtkrzw's own internals. The library is not rebuilt with
# the sanitizer, so UB inside it is invisible here.
#
# Usage: tools/tkrzw/run-sanitizers.sh
# Exit: 0 clean; non-zero on a sanitizer finding or a build failure.

# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"

set -euo pipefail
REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

# Before any directory change (#1060); no cell build settings enter this build.
"$REPO_ROOT/tools/install-python.sh"
sanitizer_caller_directory=$PWD
inherited_library_path=$(cd "$REPO_ROOT" && uv run --no-project --managed-python python \
    -X lazy_imports=normal -m tools.dev cell-artifacts --library-path "$sanitizer_caller_directory")
cd "$REPO_ROOT"

# Named once, in tools/sanitize.sh.
NIGHTLY=${OXPINYIN_SANITIZER_TOOLCHAIN:-$(sed -n 's/^NIGHTLY=//p' tools/sanitize.sh)}
SANITIZER=${OXPINYIN_SANITIZER:-address}

if ! rustup toolchain list | grep -q "^$NIGHTLY"; then
	printf '%s is not installed.\n' "$NIGHTLY" >&2
	printf '  install it with: rustup toolchain install %s --profile minimal\n' "$NIGHTLY" >&2
	if [ -n "${OXPINYIN_SANITIZER_ALLOW_SKIP:-}" ]; then
		printf '%s\n' "  SKIP: OXPINYIN_SANITIZER_ALLOW_SKIP is set; the gate did NOT run." >&2
		exit 0
	fi
	printf '%s\n' "  ERROR: the sanitizer gate cannot run. Install the toolchain, or set" \
		"  OXPINYIN_SANITIZER_ALLOW_SKIP=1 to allow skipping it." >&2
	exit 1
fi
TARGET=${OXPINYIN_SANITIZER_TARGET:-$(rustc "+$NIGHTLY" -vV | sed -n 's/^host: //p')}

if [ -n "$inherited_library_path" ]; then export LD_LIBRARY_PATH=$inherited_library_path; fi
# No verified runtime configured: setup.sh's own healthy-host decision,
# probe_host, in a subshell that reports only the library it certified.
if [ -z "${OXPINYIN_TKRZW_LIBRARY:-}${OXPINYIN_TKRZW_PREFIX:-}${OXPINYIN_TKRZW_PREFIXES:-}" ]; then
	if ! host_library=$(
		probe=tools/tkrzw/distro-probe.sh
		tkrzw_prefix=
		# shellcheck source=tools/cloud/env.sh
		source tools/cloud/env.sh
		probe_host >&2 && printf '%s' "$tkrzw_host_library"
	); then
		printf '%s\n' "ERROR: no verified libtkrzw; the sanitizer gate did NOT run." \
			"  Source the environment file tools/cloud/setup.sh writes, or provide a host" \
			"  libtkrzw that passes tools/tkrzw/distro-probe.sh (tkrzw_dbm_util on PATH)." \
			"  Ubuntu's packaged libtkrzw fails it: it cannot read files it writes." >&2
		exit 1
	fi
	OXPINYIN_TKRZW_LIBRARY=$host_library
	export OXPINYIN_TKRZW_LIBRARY
fi

case $SANITIZER in
address)
	export RUSTFLAGS="-D warnings -Zsanitizer=address -Cdebuginfo=1"
	export ASAN_OPTIONS=detect_leaks=0:abort_on_error=1
	;;
thread)
	export RUSTFLAGS="-D warnings -Zsanitizer=thread -Cdebuginfo=1 -Cunsafe-allow-abi-mismatch=sanitizer"
	export TSAN_OPTIONS=abort_on_error=1
	;;
leak)
	export RUSTFLAGS="-D warnings -Zsanitizer=leak -Cdebuginfo=1 -Cunsafe-allow-abi-mismatch=sanitizer"
	export LSAN_OPTIONS=abort_on_error=1
	;;
*)
	printf 'unknown sanitizer: %s\n' "$SANITIZER" >&2
	exit 2
	;;
esac

test_command=(cargo "+$NIGHTLY" test --locked --target "$TARGET"
	-p oxpinyin-store --no-default-features --features tkrzw)
if [ "$(uname -s)" = Linux ] && [ "$SANITIZER" = address ]; then
	test_command=(setarch "$(uname -m)" -R "${test_command[@]}")
fi

case $SANITIZER in
address) display=AddressSanitizer ;;
thread) display=ThreadSanitizer ;;
leak) display=LeakSanitizer ;;
esac
printf '%s\n' "--- building the tkrzw feature tests under $display ---"
artifacts=$("${test_command[@]}" --no-run --message-format=json-render-diagnostics)
# Checked where cargo runs them, in the package directory; a binary with
# no tkrzw dependency prints no path.
checked=0
while IFS= read -r executable; do
	if ! resolved=$(cd crates/oxpinyin-store &&
		bash "$REPO_ROOT/tools/tkrzw/runtime.sh" --print-path tkrzw run-sanitizers subject "$executable"); then
		printf 'ERROR: the sanitizer gate did NOT run.\n' >&2
		exit 1
	fi
	if [ -n "$resolved" ]; then
		library=$resolved
		checked=$((checked + 1))
	fi
done < <(printf '%s\n' "$artifacts" | sed -n 's/.*"executable":"\([^"]*\)".*/\1/p')
if [ "$checked" -eq 0 ]; then
	printf 'ERROR: no test binary loads libtkrzw; the sanitizer gate did NOT run.\n' >&2
	exit 1
fi
printf 'libtkrzw: %d test binaries resolve it to %s\n' "$checked" "$library"

if [ "$SANITIZER" = address ]; then
	printf '%s\n' "" "--- Rust under $display (LeakSanitizer off) ---"
else
	printf '%s\n' "" "--- Rust under $display ---"
fi
"${test_command[@]}"

printf '%s\n' "" "sanitizers: clean (libtkrzw's own internals are NOT instrumented)"
