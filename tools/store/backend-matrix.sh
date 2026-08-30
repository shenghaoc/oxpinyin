#!/usr/bin/env bash
# backend-matrix.sh — prove the exactly-one-backend invariant.
#
# The four oxpinyin store backends (kyotocabinet, redb, lmdb, tkrzw) are
# peer implementations behind one trait surface, and every build has
# exactly one of them. This script drives that invariant end-to-end:
#
#   1. Each of the four valid selections is a green `cargo check
#      --workspace`.
#   2. Every one of the six pairwise combinations, and a three-way
#      combination, refuses to compile with the `compile_error!` message
#      from `crates/oxpinyin-store/src/lib.rs`.
#   3. The zero-backend build refuses with the same guard.
#
# Runs `cargo check`, not `cargo build`, so libtkrzw/liblmdb/etc. do not
# have to be linkable in the environment — but the compile-error checks
# are still meaningful (they fire in the store crate, whose feature
# combinations are the same for check and build).
#
# Exit: 0 = all valid selections passed and every invalid selection was
# refused by the guard; non-zero on any deviation.

set -u
cd "$(dirname "$0")/../.."

# One securely-created temp file for every cargo-check log; trap ensures
# it is removed on any exit path (success, failure, ^C). Predictable
# names in a shared /tmp are a classic symlink-race surface, even for a
# dev-tool: mktemp gives an unpredictable path and O_EXCL semantics.
LOG="$(mktemp -t backend-matrix.XXXXXXXX)"
trap 'rm -f "$LOG"' EXIT

pass=0
fail=0

# Every valid selection must compile.
for peer in "" \
    "--no-default-features --features kyotocabinet" \
    "--no-default-features --features redb" \
    "--no-default-features --features lmdb" \
    "--no-default-features --features tkrzw"; do
    label=${peer:-default (KC)}
    printf '── valid: %s\n' "$label"
    if cargo check --locked -p oxpinyin-store $peer >"$LOG" 2>&1; then
        printf '   PASS\n'
        pass=$((pass + 1))
    else
        printf '   FAIL — expected a clean build; tail:\n'
        tail -20 "$LOG" | sed 's/^/     /'
        fail=$((fail + 1))
    fi
done

# Every invalid combination must be refused by the compile_error guard.
# Six pairs plus a three-way plus a zero-backend case.
for combo in \
    "kyotocabinet,redb" \
    "kyotocabinet,lmdb" \
    "kyotocabinet,tkrzw" \
    "redb,lmdb" \
    "redb,tkrzw" \
    "lmdb,tkrzw" \
    "kyotocabinet,redb,lmdb"; do
    printf '── invalid: --features %s\n' "$combo"
    if cargo check --locked -p oxpinyin-store --no-default-features --features "$combo" \
        >"$LOG" 2>&1; then
        printf '   FAIL — the guard did not fire, build succeeded\n'
        fail=$((fail + 1))
    elif grep -q 'more than one store backend selected' "$LOG"; then
        printf '   PASS (refused by the exactly-one-backend guard)\n'
        pass=$((pass + 1))
    else
        printf '   FAIL — build refused for another reason:\n'
        grep -E 'compile_error|^error' "$LOG" | head -3 | sed 's/^/     /'
        fail=$((fail + 1))
    fi
done

printf '── invalid: --no-default-features (zero backends)\n'
if cargo check --locked -p oxpinyin-store --no-default-features >"$LOG" 2>&1; then
    printf '   FAIL — the guard did not fire, build succeeded\n'
    fail=$((fail + 1))
elif grep -q 'no store backend selected' "$LOG"; then
    printf '   PASS (refused by the exactly-one-backend guard)\n'
    pass=$((pass + 1))
else
    printf '   FAIL — build refused for another reason:\n'
    grep -E 'compile_error|^error' "$LOG" | head -3 | sed 's/^/     /'
    fail=$((fail + 1))
fi

rm -f "$LOG"

printf '\nsummary: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ] && exit 0 || exit 1
