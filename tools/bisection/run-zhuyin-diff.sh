#!/usr/bin/env bash
set -euo pipefail

# run-zhuyin-diff.sh — build and run the libzhuyin differential driver against
# the pinned libzhuyin oracle and the oxpinyin libzhuyin.so.15, diffing the
# logs.  The two sides read DIFFERENT systemdirs: the oracle side reads the
# pin-built C++ tables (its own prefix's lib/libpinyin/data); the Rust side
# reads an oxpinyin-native converted systemdir (redb tables +
# interpolation2.text) that carries the same model20 data.
#
# Non-vacuous verification: the driver must produce real output for BOTH
# libraries (not empty), so a behaviour change shows up as a diff; the
# revert-and-check proof is run by the four-check gate.
#
# Usage: run-zhuyin-diff.sh
#
# Env:
#   ZHUYIN_ORACLE_PREFIX  pin-built libzhuyin-oracle prefix (REQUIRED — the
#                         oracle .so lives at $PREFIX/lib/libzhuyin.so.15.0.0
#                         and its data at $PREFIX/lib/libpinyin/data). Not
#                         defaulted to /tmp: CI must build it with
#                         build-oracle.sh (patched) and point here. The script
#                         STOPS if this is unset, mirroring the pinyin suite.
#   ZHUYIN_ORACLE_SO     pin-built libzhuyin.so.15 (default $PREFIX/lib/libzhuyin.so.15.0.0)
#   ZHUYIN_ORACLE_DATA   pin-built oracle's C++ data dir (default $PREFIX/lib/libpinyin/data)
#   ZHUYIN_RUST_SO       the oxpinyin libzhuyin.so.15 (default $REPO_ROOT/target/debug/libzhuyin_capi.so)
#   ZHUYIN_RUST_DATA     an oxpinyin-native converted systemdir (REQUIRED —
#                         the oxpinyin-converted redb tables + interpolation2.text).
#   ZHUYIN_USER_DIR      caller-supplied user-data dir for the RUST side. If
#                         set, the script uses it as-is and does NOT remove it;
#                         otherwise each side gets its own fresh scratch dir
#                         (removed on exit).

cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"

if [[ -z "${ZHUYIN_ORACLE_PREFIX:-}" || ! -d "$ZHUYIN_ORACLE_PREFIX" ]]; then
    echo "SKIP: ZHUYIN_ORACLE_PREFIX is unset or not a directory" >&2
    echo "  build the pin-built libzhuyin oracle (tools/oracle/build-oracle.sh patched" >&2
    echo "  with --enable-libzhuyin) and set ZHUYIN_ORACLE_PREFIX to its prefix." >&2
    exit 3
fi

ORACLE_SO="${ZHUYIN_ORACLE_SO:-$ZHUYIN_ORACLE_PREFIX/lib/libzhuyin.so.15.0.0}"
ORACLE_DATA="${ZHUYIN_ORACLE_DATA:-$ZHUYIN_ORACLE_PREFIX/lib/libpinyin/data}"
RUST_SO="${ZHUYIN_RUST_SO:-$REPO_ROOT/target/debug/libzhuyin_capi.so}"
RUST_DATA="${ZHUYIN_RUST_DATA:-}"
# Each side gets its own user dir so neither run observes user-state files the
# other creates (libzhuyin writes user.conf / the user store under here). A
# caller-supplied ZHUYIN_USER_DIR pins the RUST side and is never removed; the
# scratch dirs this script creates are cleaned on exit.
ORACLE_USER_DIR="$(mktemp -d)"
if [[ -n "${ZHUYIN_USER_DIR:-}" ]]; then
    RUST_USER_DIR="$ZHUYIN_USER_DIR"
    mkdir -p "$RUST_USER_DIR"
    trap 'rm -rf "$ORACLE_USER_DIR"' EXIT
else
    RUST_USER_DIR="$(mktemp -d)"
    trap 'rm -rf "$ORACLE_USER_DIR" "$RUST_USER_DIR"' EXIT
fi

if [[ -z "$RUST_DATA" || ! -d "$RUST_DATA" ]]; then
    echo "SKIP: ZHUYIN_RUST_DATA is unset or not a directory" >&2
    echo "  point it at an oxpinyin-native converted systemdir (pinyin_index.redb," >&2
    echo "  phrase_index.redb, bigram.redb, punct.redb + interpolation2.text)." >&2
    exit 3
fi

for p in "$ORACLE_SO" "$RUST_SO"; do
    [[ -f "$p" ]] || { echo "so not found: $p" >&2; exit 1; }
done
for d in "$ORACLE_DATA" "$RUST_DATA"; do
    [[ -f "$d/interpolation2.text" ]] || { echo "data dir missing interpolation2.text: $d" >&2; exit 1; }
done

echo "== building zhuyin-diff =="
cc -O2 -Wall -Wextra -o "$SCRIPT_DIR/zhuyin-diff" "$SCRIPT_DIR/zhuyin-diff.c" -ldl || {
    echo "build failed" >&2; exit 1; }

echo "== running against oracle (data=$ORACLE_DATA) =="
# The oracle writes a benign init diagnostic ("open user.conf failed") to
# stderr on a fresh user dir, so stderr is captured per side rather than
# merged into the compared stdout log — but it is NOT discarded: a failing
# side emits its captured diagnostics (the driver's own failure reason)
# before the script exits.
if ! "$SCRIPT_DIR/zhuyin-diff" "$ORACLE_SO" "$ORACLE_DATA" "$ORACLE_USER_DIR" \
        > "$SCRIPT_DIR/zhuyin-oracle.log" 2> "$SCRIPT_DIR/zhuyin-oracle.stderr"; then
    echo "FAIL: the oracle driver exited nonzero" >&2
    cat "$SCRIPT_DIR/zhuyin-oracle.stderr" >&2
    exit 1
fi
rm -f "$SCRIPT_DIR/zhuyin-oracle.stderr"
echo "oracle log lines: $(wc -l < "$SCRIPT_DIR/zhuyin-oracle.log")"

echo "== running against rust facade (data=$RUST_DATA) =="
if ! "$SCRIPT_DIR/zhuyin-diff" "$RUST_SO" "$RUST_DATA" "$RUST_USER_DIR" \
        > "$SCRIPT_DIR/zhuyin-rust.log" 2> "$SCRIPT_DIR/zhuyin-rust.stderr"; then
    echo "FAIL: the rust driver exited nonzero" >&2
    cat "$SCRIPT_DIR/zhuyin-rust.stderr" >&2
    exit 1
fi
rm -f "$SCRIPT_DIR/zhuyin-rust.stderr"
echo "rust log lines: $(wc -l < "$SCRIPT_DIR/zhuyin-rust.log")"

# Both sides must produce a non-empty log for the diff to mean anything: an
# empty log means the side failed to init or crashed before writing output, so
# a byte-identical comparison would be vacuous. Fail loudly instead.
if [[ ! -s "$SCRIPT_DIR/zhuyin-oracle.log" || ! -s "$SCRIPT_DIR/zhuyin-rust.log" ]]; then
    echo "FAIL: one side produced no output (empty log)" >&2
    echo "  oracle: $SCRIPT_DIR/zhuyin-oracle.log ($(wc -l < "$SCRIPT_DIR/zhuyin-oracle.log") lines)" >&2
    echo "  rust:   $SCRIPT_DIR/zhuyin-rust.log ($(wc -l < "$SCRIPT_DIR/zhuyin-rust.log") lines)" >&2
    exit 1
fi

echo "== diff (oracle vs rust) =="
if diff -u "$SCRIPT_DIR/zhuyin-oracle.log" "$SCRIPT_DIR/zhuyin-rust.log" > "$SCRIPT_DIR/zhuyin.diff"; then
    echo "IDENTICAL"
    exit 0
else
    echo "DIFF FOUND (see $SCRIPT_DIR/zhuyin.diff)"
    exit 2
fi
