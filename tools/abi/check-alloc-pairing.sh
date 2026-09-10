#!/usr/bin/env bash
# check-alloc-pairing.sh — the allocator-pairing gate for the two C ABIs.
#
# oxpinyin ships `libpinyin.so.15` / `libzhuyin.so.15` as drop-in
# replacements. Every pointer the ABI hands a consumer is owned by exactly
# one side, and released by exactly one function; until this gate there was
# nothing that held the library to that. `check-exports.sh` freezes WHICH
# symbols cross the boundary; this one freezes WHAT CROSSES WITH THEM.
#
# The declaration is `crates/oxpinyin-capi/libpinyin.alloc` and
# `crates/oxpinyin-zhuyin-capi/libzhuyin.alloc` — one line per pointer-shaped
# slot, naming its class and, for a handle, its destructor. Three checks
# stand on it:
#
#   1. STATIC — coverage of the frozen header. Every pointer return and every
#      `T **` out-parameter `pinyin.h` / `zhuyin.h` declares is in the
#      register exactly once, and nothing is in the register that the header
#      does not declare. This is the check that fails when a new allocating
#      entry point is added and nobody says who frees it.
#
#   2. STATIC — the class agrees with the C type, and the deallocator exists.
#      A non-const `gchar **` / `char **` is `g_free`; a `gchar ***` is
#      `g_strfreev`; a `const T **` is `borrowed`; a pointer return is a
#      handle or borrowed. Every `handle:<fn>` names a symbol the version
#      script exports, so no handle is declared with an unreachable
#      destructor.
#
#   3. DYNAMIC — the declaration is true. A C++ consumer per ABI
#      (alloc-pairing-*.cc) drives the real `.so`, releases every slot with
#      the register's deallocator, and runs under AddressSanitizer's
#      LeakSanitizer. A slot the declared deallocator does not actually
#      release is a leak; a borrowed pointer wrongly declared owned is an
#      invalid free. The driver also reports per-slot coverage: the run is
#      rejected unless every registered slot was reached with a non-NULL
#      pointer, because a gate that never exercises the allocation it checks
#      is green for the wrong reason.
#
# And a negative control. Each driver is built a second time with
# -DOXPINYIN_ALLOC_PAIRING_LEAK, which drops the frees, and that build is
# REQUIRED to fail with a LeakSanitizer report. Without it a sanitizer that
# silently stopped working would leave the gate passing forever.
#
# No new dependency: the CI container's `g++` already carries libasan/liblsan
# (verified on debian:testing with the test job's apt set), the Rust side is
# not instrumented and not rebuilt, and ASan's malloc interposition covers
# the whole process — including the library's libc `malloc` in
# `ffi::owned_cstr` and Rust's `Box` behind the handles.
#
# Linux only: LeakSanitizer's ptrace-based stop-the-world does not run under
# macOS's sandbox, and the C ABIs are Linux-first anyway (AGENTS.md).
#
# Usage: tools/abi/check-alloc-pairing.sh [--static-only]
# Exit: 0 when both ABIs pass every check; non-zero with the offending slots
# named.

set -euo pipefail
cd "$(dirname "$0")/../.."
REPO_ROOT=$(pwd)

static_only=0
if [[ "${1:-}" == "--static-only" ]]; then
	static_only=1
fi

if [[ "$(uname -s)" != Linux && $static_only -eq 0 ]]; then
	echo "fatal: the dynamic half needs Linux (LeakSanitizer); re-run with --static-only" >&2
	exit 2
fi

status=0
fail() {
	echo "  FAIL: $*"
	status=1
}

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# ── Header parsing ────────────────────────────────────────────────────
#
# One C declaration per line, with comments and `#if 0` blocks removed. The
# same extraction recovers exactly the 79/52 symbols the two version scripts
# freeze, which is what makes it trustworthy enough to gate on.
# `[[:blank:]]` rather than `\t`: BSD sed reads `\t` as a literal `t` and
# would mangle every identifier containing one.
flatten_header() {
	sed -e 's://.*::' "$1" |
		awk 'BEGIN { c = 0 }
		     { line = $0
		       while (1) {
		         if (c) { i = index(line, "*/"); if (i == 0) { line = ""; break }
		                  line = substr(line, i + 2); c = 0 }
		         i = index(line, "/*"); if (i == 0) break
		         rest = substr(line, i + 2); j = index(rest, "*/")
		         if (j == 0) { line = substr(line, 1, i - 1); c = 1; break }
		         line = substr(line, 1, i - 1) substr(rest, j + 2)
		       }
		       print line }' |
		awk '/^[ \t]*#[ \t]*if[ \t]+0[ \t]*$/ { d = 1; next }
		     d && /^[ \t]*#[ \t]*(if|ifdef|ifndef)/ { n++; next }
		     d && /^[ \t]*#[ \t]*endif/ { if (n > 0) n--; else d = 0; next }
		     d { next }
		     /^[ \t]*#/ { next }
		     { print }' |
		tr '\n' ' ' | tr ';' '\n' |
		sed -e 's/[[:blank:]][[:blank:]]*/ /g' -e 's/^ //' -e 's/ $//' |
		grep -E '^[A-Za-z_][A-Za-z0-9_ ]*\*? *[a-z_]+ *\(' || true
}

# `<symbol> <slot> <shape>` for every pointer-shaped slot the header declares.
# Shapes are the C types, not the register's classes; check 2 compares them.
header_slots() {
	flatten_header "$1" | while IFS= read -r decl; do
		name=$(printf '%s\n' "$decl" | sed -E 's/\(.*//' |
			sed -E 's/.*[^A-Za-z0-9_]([A-Za-z_][A-Za-z0-9_]*) *$/\1/')
		ret=$(printf '%s\n' "$decl" | sed -E 's/\(.*//')
		case "$ret" in *'*'*) printf '%s return ptr-return\n' "$name" ;; esac
		printf '%s\n' "$decl" | sed -E 's/^[^(]*\(//; s/\) *$//' | tr ',' '\n' |
			sed 's/^ *//; s/ *$//' | while IFS= read -r param; do
			slot=$(printf '%s\n' "$param" |
				sed -E 's/.*[^A-Za-z0-9_]([A-Za-z_][A-Za-z0-9_]*) *$/\1/')
			case "$param" in
			*'***'*) printf '%s %s strv\n' "$name" "$slot" ;;
			'const '*'**'*) printf '%s %s const-pp\n' "$name" "$slot" ;;
			*'char **'*) printf '%s %s char-pp\n' "$name" "$slot" ;;
			*'**'*) printf '%s %s opaque-pp\n' "$name" "$slot" ;;
			esac
		done
	done
}

# `<symbol> <slot> <class> <on-false>` from a register file — the whole
# entry, so the dynamic half can compare all four fields against what the
# driver actually did rather than just the symbol and slot name.
register_slots() {
	awk '!/^[[:space:]]*#/ && NF >= 4 { print $1, $2, $3, $4 }' "$1"
}

# The `global:` names of a version script.
ver_globals() {
	awk '/global:/ { on = 1; next } /local:/ { on = 0 }
	     on && /;/ { gsub(/[[:space:];]/, ""); print }' "$1" | sort -u
}

# ── Check 1 and 2: the register against the header and the version script ──
check_static() {
	local abi=$1 header=$2 reg=$3 ver=$4
	local before=$status
	echo "--- $abi: register against $(basename "$header") ---"

	header_slots "$header" | sort > "$WORK/$abi.header"
	register_slots "$reg" | sort > "$WORK/$abi.register"
	ver_globals "$ver" > "$WORK/$abi.ver"

	local n_header n_register
	n_header=$(wc -l < "$WORK/$abi.header" | tr -d ' ')
	n_register=$(wc -l < "$WORK/$abi.register" | tr -d ' ')
	echo "  header slots: $n_header, register entries: $n_register"

	cut -d' ' -f1,2 < "$WORK/$abi.header" | sort > "$WORK/$abi.header.keys"
	cut -d' ' -f1,2 < "$WORK/$abi.register" | sort > "$WORK/$abi.register.keys"

	local missing extra dup
	missing=$(comm -23 "$WORK/$abi.header.keys" "$WORK/$abi.register.keys")
	extra=$(comm -13 "$WORK/$abi.header.keys" "$WORK/$abi.register.keys")
	dup=$(uniq -d "$WORK/$abi.register.keys")
	if [[ -n $missing ]]; then
		echo "  slots the header declares but the register does not classify:"
		printf '%s\n' "$missing" | sed 's/^/    /'
		fail "unclassified ABI slot"
	fi
	if [[ -n $extra ]]; then
		echo "  register entries with no matching slot in the header:"
		printf '%s\n' "$extra" | sed 's/^/    /'
		fail "stale register entry"
	fi
	if [[ -n $dup ]]; then
		echo "  duplicate register entries:"
		printf '%s\n' "$dup" | sed 's/^/    /'
		fail "duplicate register entry"
	fi

	# The class must agree with the C type the header gives the slot.
	while read -r symbol slot shape; do
		local class
		class=$(awk -v s="$symbol" -v n="$slot" \
			'!/^[[:space:]]*#/ && $1 == s && $2 == n { print $3 }' "$reg")
		[[ -z $class ]] && continue
		case "$shape:$class" in
		ptr-return:handle:* | ptr-return:borrowed) ;;
		strv:g_strfreev) ;;
		const-pp:borrowed) ;;
		char-pp:g_free) ;;
		opaque-pp:borrowed) ;;
		*) fail "$symbol/$slot is '$shape' in the header but '$class' in the register" ;;
		esac
	done < "$WORK/$abi.header"

	# Every registered symbol, and every destructor a handle names, must be
	# a symbol the version script actually exports.
	local symbol class dtor
	while read -r symbol _ class _; do
		grep -qx "$symbol" "$WORK/$abi.ver" ||
			fail "$symbol is in the register but not in $(basename "$ver")"
		case "$class" in
		handle:*)
			dtor=${class#handle:}
			grep -qx "$dtor" "$WORK/$abi.ver" ||
				fail "$symbol declares destructor '$dtor', which $(basename "$ver") does not export"
			;;
		esac
	done < "$WORK/$abi.register"

	((status == before)) &&
		echo "  OK: every header slot classified, every class and destructor sound"
	return 0
}

# ── Fixture data ──────────────────────────────────────────────────────
#
# The committed W3 mini fixture, one real drop-in directory per backend, plus
# the stub interpolation2.text the public `*_init` needs to parse (the
# committed tables carry none). Mirrors tools/bisection/run-cpp-smoke.sh;
# duplicated rather than shared so that adding this gate cannot break the
# existing one, and kept in step by review.
prepare_fixtures() {
	local fix_root="$REPO_ROOT/fixtures/w3" ext=""
	if [[ -n ${OXPINYIN_CAPI_BACKEND_EXT:-} ]]; then
		case "$OXPINYIN_CAPI_BACKEND_EXT" in
		kct | redb | tkt | lmdb) ext=$OXPINYIN_CAPI_BACKEND_EXT ;;
		*)
			echo "fatal: OXPINYIN_CAPI_BACKEND_EXT='$OXPINYIN_CAPI_BACKEND_EXT' is not one of: kct redb tkt lmdb" >&2
			exit 2
			;;
		esac
	else
		local candidate
		for candidate in tkt kct redb lmdb; do
			[[ -d $fix_root/$candidate ]] && ext=$candidate && break
		done
	fi
	if [[ -z $ext || ! -d $fix_root/$ext ]]; then
		echo "fatal: no per-backend fixture directory under $fix_root" >&2
		exit 2
	fi
	mkdir -p "$WORK/sys"
	cp "$fix_root/$ext"/* "$WORK/sys/"
	printf '%s\n' '\data model interpolation' '\1-gram' '\item 1 ok count 1' \
		> "$WORK/sys/interpolation2.text"
}

# ── Check 3: the drivers under AddressSanitizer/LeakSanitizer ─────────
#
# `detect_leaks=1` is the default on Linux but is spelled out: the whole
# check is that one option. `-O1 -fno-omit-frame-pointer` keeps the leak
# reports' stacks readable without making the build slow.
build_driver() {
	local out=$1 src=$2 include=$3 lib=$4 leak=$5
	local defines=()
	((leak)) && defines=(-DOXPINYIN_ALLOC_PAIRING_LEAK)
	g++ -std=c++17 -Wall -Wextra -Werror -O1 -g -fno-omit-frame-pointer \
		-fsanitize=address \
		${GLIB_CFLAGS[@]+"${GLIB_CFLAGS[@]}"} \
		-I"$REPO_ROOT/$include" \
		"$REPO_ROOT/$src" \
		-L"$CAPI_DIR" -Wl,-rpath,"$CAPI_DIR" \
		"-l$lib" ${GLIB_LIBS[@]+"${GLIB_LIBS[@]}"} \
		${defines[@]+"${defines[@]}"} \
		-o "$out"
}

check_dynamic() {
	local abi=$1 src=$2 include=$3 lib=$4 reg=$5
	local before=$status
	echo "--- $abi: $(basename "$src") under AddressSanitizer/LeakSanitizer ---"
	register_slots "$reg" | sort > "$WORK/$abi.register"

	local user="$WORK/$abi-user"
	rm -rf "$user" && mkdir -p "$user"
	build_driver "$WORK/$abi-driver" "$src" "$include" "$lib" 0

	local out rc=0
	out=$(ASAN_OPTIONS=detect_leaks=1:abort_on_error=0 \
		"$WORK/$abi-driver" "$WORK/sys" "$user" --coverage 2>&1) || rc=$?
	if ((rc != 0)); then
		printf '%s\n' "$out" | sed 's/^/    /'
		fail "the sanitized run exited $rc (0 expected)"
		return
	fi

	# The driver echoes the register's own class and note back with each
	# slot, plus whether it reached the slot and whether it probed the
	# false-return contract:
	#   SLOT <symbol> <slot> <class> <on-false> hit|miss <probe state>
	printf '%s\n' "$out" | awk '$1 == "SLOT" { print $2, $3, $4, $5, $6, $7 }' | sort \
		> "$WORK/$abi.coverage"

	local missed
	missed=$(awk '$5 != "hit" { print $1, $2 }' "$WORK/$abi.coverage")
	if [[ -n $missed ]]; then
		echo "  slots the driver never reached with a live pointer:"
		printf '%s\n' "$missed" | sed 's/^/    /'
		fail "unexercised slot — the pairing was not actually tested"
	fi

	# Every reachable false-return contract must have been probed. A note
	# the driver never exercised is prose again, which is what this gate
	# replaced; `false-unreachable` is the one way out, and it is an
	# explicit claim in the register rather than a silent omission.
	local unprobed
	unprobed=$(awk '$6 == "unchecked" { print $1, $2, "(" $4 ")" }' "$WORK/$abi.coverage")
	if [[ -n $unprobed ]]; then
		echo "  false-return contracts the driver never probed:"
		printf '%s\n' "$unprobed" | sed 's/^/    /'
		fail "unprobed false-return contract"
	fi

	# The whole entry, not just the key: a class flipped from a handle to
	# borrowed, a renamed destructor, or an edited note now fails here
	# instead of passing because the symbol and slot name still line up.
	local drift
	drift=$(comm -3 <(cut -d' ' -f1,2,3,4 < "$WORK/$abi.coverage" | sort) \
		"$WORK/$abi.register")
	if [[ -n $drift ]]; then
		echo "  driver slot table and register disagree (<driver only / register only>):"
		printf '%s\n' "$drift" | sed 's/^/    /'
		fail "driver/register drift"
	fi
	((status == before)) &&
		echo "  OK: $(wc -l < "$WORK/$abi.coverage" | tr -d ' ') slots exercised and released, no leak"

	# The negative control. A gate that cannot fail is not a gate.
	rm -rf "$user" && mkdir -p "$user"
	build_driver "$WORK/$abi-driver-leak" "$src" "$include" "$lib" 1
	local leak_out leak_rc=0
	leak_out=$(ASAN_OPTIONS=detect_leaks=1:abort_on_error=0 \
		"$WORK/$abi-driver-leak" "$WORK/sys" "$user" 2>&1) || leak_rc=$?
	if ((leak_rc == 0)) || ! printf '%s\n' "$leak_out" | grep -q 'LeakSanitizer'; then
		echo "  the deliberately-leaking build exited $leak_rc without a LeakSanitizer report"
		fail "negative control did not fire — the leak instrument is not working"
	else
		echo "  OK: negative control fired (exit $leak_rc, LeakSanitizer reported)"
	fi
}

# ── Run ───────────────────────────────────────────────────────────────

check_static libpinyin \
	crates/oxpinyin-capi/pinyin.h \
	crates/oxpinyin-capi/libpinyin.alloc \
	crates/oxpinyin-capi/libpinyin.ver
check_static libzhuyin \
	crates/oxpinyin-zhuyin-capi/zhuyin.h \
	crates/oxpinyin-zhuyin-capi/libzhuyin.alloc \
	crates/oxpinyin-zhuyin-capi/libzhuyin.ver

if ((static_only)); then
	if ((status)); then
		echo "FAIL: the allocator-pairing register does not match the frozen ABI"
	else
		echo "PASS: static half only (--static-only); the dynamic half was not run"
	fi
	exit $status
fi

echo "--- building oxpinyin-capi and oxpinyin-zhuyin-capi ---"
cargo build --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi
CAPI_DIR="$REPO_ROOT/target/debug"
built=(libpinyin_capi.so libzhuyin_capi.so)
soname=(libpinyin.so.15 libzhuyin.so.15)
for i in 0 1; do
	[[ -f $CAPI_DIR/${built[i]} ]] || {
		echo "fatal: $CAPI_DIR/${built[i]} not built" >&2
		exit 1
	}
	# Both cdylibs carry the drop-in SONAME, so anything linking them records
	# that name in DT_NEEDED; the symlink makes rpath resolve it to the build
	# under test rather than to a system libpinyin (run-cpp-smoke.sh's law).
	ln -sf "${built[i]}" "$CAPI_DIR/${soname[i]}"
done

# Arrays, not strings: these expand to several arguments each, and an
# unquoted string would also glob. GLIB_CFLAGS honours an environment
# override for constrained builders, as run-cpp-smoke.sh does.
read -r -a GLIB_CFLAGS <<< "${GLIB_CFLAGS:-$(pkg-config --cflags glib-2.0 2>/dev/null)}"
read -r -a GLIB_LIBS <<< "$(pkg-config --libs glib-2.0 2>/dev/null)"
prepare_fixtures

check_dynamic libpinyin \
	tools/abi/alloc-pairing-pinyin.cc crates/oxpinyin-capi pinyin_capi \
	crates/oxpinyin-capi/libpinyin.alloc
check_dynamic libzhuyin \
	tools/abi/alloc-pairing-zhuyin.cc crates/oxpinyin-zhuyin-capi zhuyin_capi \
	crates/oxpinyin-zhuyin-capi/libzhuyin.alloc

if ((status)); then
	echo "FAIL: the allocator-pairing contract is not held"
else
	echo "PASS: every ABI allocation is classified, exercised and released by its declared deallocator"
fi
exit $status
