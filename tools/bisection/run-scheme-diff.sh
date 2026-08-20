#!/usr/bin/env bash
# run-scheme-diff.sh <double|bopomofo|full> [scheme-number]
#
# W13/W15 per-scheme differential. Drives the same C-ABI sequence into
# libpinyin_capi.so and the pin-built libpinyin.so and diffs the logs.
# The scheme number selects the double-pinyin scheme in double mode,
# the zhuyin keyboard in bopomofo mode (1 STANDARD, 2 HSU, 3 IBM,
# 4 GINYIEH, 5 ETEN, 6 ETEN26, 8 HSU_DVORAK, 9 DACHEN_CP26; default 1),
# or the full-pinyin scheme in full mode (1 HANYU, 2 LUOMA,
# 3 SECONDARY_ZHUYIN; default 1).
#
# Env-gated on the pin-built oracle exactly like the import/train diffs:
# PINYIN_ORACLE_PREFIX (default $HOME/.local/opt/pinyin-oracle).
#
# The capi side is built from the workspace. To compare candidate lists
# against real model data, set W13_CAPI_SYSTEM to a directory containing
# pinyin_index.redb/phrase_index.redb/bigram.redb and interpolation2.text;
# otherwise the committed fixtures/w3 mini tables are used and candidate
# lists are expected to differ (the oracle side is the pin model).
#
# Exit codes: 0 = identical or skipped; 1 = build/run failure; 2 = divergence.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

SCHEME="${1:-double}"
SCHEME_NUMBER="${2:-}"

echo "--- building scheme-diff driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o scheme-diff scheme-diff.c -ldl
if [[ "$SCHEME" == "full" ]]; then
    if [[ -n "$SCHEME_NUMBER" && ! "$SCHEME_NUMBER" =~ ^(1|2|3)$ ]]; then
        echo "fatal: full scheme must be 1, 2 or 3 (got '$SCHEME_NUMBER')"
        exit 1
    fi
    SCHEME_ARGS=("${SCHEME_NUMBER:-1}")
elif [[ "$SCHEME" == "bopomofo" ]]; then
    gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o chewing-diff chewing-diff.c -ldl
fi
if [[ "$SCHEME" == "full" ]]; then
    gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o fullpin-diff fullpin-diff.c -ldl
fi
echo "build: ok"

echo "--- building oxpinyin-capi ---"
cargo build -p oxpinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
CAPI_SO="$REPO_ROOT/target/debug/libpinyin_capi.so"
if [[ ! -f "$CAPI_SO" ]]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [[ ! -f "$PREFIX/oracle-pin.txt" || ! -f "$ORACLE_SO" ]]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    echo "  build it with tools/oracle/build-oracle.sh and set PINYIN_ORACLE_PREFIX"
    exit 0
fi
if ! grep -q '^pin_ref=libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c' \
    "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin"
    exit 0
fi

if [[ "$SCHEME" == "full" ]]; then
    if [[ -n "$SCHEME_NUMBER" && ! "$SCHEME_NUMBER" =~ ^(1|2|3)$ ]]; then
        echo "fatal: full scheme must be 1, 2 or 3 (got '$SCHEME_NUMBER')"
        exit 1
    fi
    SCHEME_ARGS=("${SCHEME_NUMBER:-1}")
elif [[ "$SCHEME" == "bopomofo" ]]; then
    # Only the implemented keyboards the driver sweeps: 1 STANDARD,
    # 2 HSU, 3 IBM, 4 GINYIEH, 5 ETEN, 6 ETEN26, 8 HSU_DVORAK,
    # 9 DACHEN_CP26.  7 (STANDARD_DVORAK) aborts the oracle and is
    # never sent; 30 belongs to the double-pinyin setter.
    if [[ -n "$SCHEME_NUMBER" && ! "$SCHEME_NUMBER" =~ ^(1|2|3|4|5|6|8|9)$ ]]; then
        echo "fatal: bopomofo scheme must be one of 1 2 3 4 5 6 8 9 (got '$SCHEME_NUMBER')"
        exit 1
    fi
    SCHEME_ARGS=("${SCHEME_NUMBER:-1}")
else
    if [[ -n "$SCHEME_NUMBER" ]]; then
        SCHEME_ARGS=("$SCHEME_NUMBER")
    else
        SCHEME_ARGS=()
    fi
fi

USER_DIR="$(mktemp -d)"
SYSTEM_TMP=""
trap 'rm -rf "$USER_DIR" "$SYSTEM_TMP"' EXIT
export SCHEME_DIFF_USER_DIR="$USER_DIR"

CAPI_DATA="$REPO_ROOT/fixtures/w3"
if [[ -n "${W13_CAPI_SYSTEM:-}" ]]; then
    SYSTEM_TMP="$(mktemp -d)"
    cp "$W13_CAPI_SYSTEM"/pinyin_index.redb \
       "$W13_CAPI_SYSTEM"/phrase_index.redb \
       "$W13_CAPI_SYSTEM"/bigram.redb "$SYSTEM_TMP"/ 2>/dev/null || true
    if [[ -n "${W13_INTERPOLATION:-}" && -f "$W13_INTERPOLATION" ]]; then
        cp "$W13_INTERPOLATION" "$SYSTEM_TMP/interpolation2.text"
    fi
    CAPI_DATA="$SYSTEM_TMP"
fi

echo "--- capi side ---"
CAPI_LOG="$(mktemp)"
CAPI_ERR="$(mktemp)"
if [[ "$SCHEME" == "full" ]]; then
    CAPI_DRIVER=./fullpin-diff
    DRIVER_ARGS=("$CAPI_SO" "$CAPI_DATA" "${SCHEME_ARGS[@]}")
elif [[ "$SCHEME" == "bopomofo" ]]; then
    CAPI_DRIVER=./chewing-diff
    DRIVER_ARGS=("$CAPI_SO" "$CAPI_DATA" "${SCHEME_ARGS[@]}")
else
    CAPI_DRIVER=./scheme-diff
    DRIVER_ARGS=("$CAPI_SO" "$CAPI_DATA" "${SCHEME_ARGS[@]}")
fi
if ! "$CAPI_DRIVER" "${DRIVER_ARGS[@]}" \
    > "$CAPI_LOG" 2> "$CAPI_ERR"; then
    echo "FAIL: scheme-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    cat "$CAPI_ERR"
    rm -f "$CAPI_LOG" "$CAPI_ERR"
    exit 1
fi
echo "oxpinyin-capi: ok"

echo "--- oracle side ---"
ORACLE_LOG="$(mktemp)"
ORACLE_ERR="$(mktemp)"
if [[ "$SCHEME" == "full" ]]; then
    ORACLE_DRIVER=./fullpin-diff
    DRIVER_ARGS=("$ORACLE_SO" "$ORACLE_DATA" "${SCHEME_ARGS[@]}")
elif [[ "$SCHEME" == "bopomofo" ]]; then
    ORACLE_DRIVER=./chewing-diff
    DRIVER_ARGS=("$ORACLE_SO" "$ORACLE_DATA" "${SCHEME_ARGS[@]}")
else
    ORACLE_DRIVER=./scheme-diff
    DRIVER_ARGS=("$ORACLE_SO" "$ORACLE_DATA" "${SCHEME_ARGS[@]}")
fi
if ! "$ORACLE_DRIVER" "${DRIVER_ARGS[@]}" \
    > "$ORACLE_LOG" 2> "$ORACLE_ERR"; then
    echo "FAIL: scheme-diff crashed against oracle"
    cat "$ORACLE_LOG"
    cat "$ORACLE_ERR"
    rm -f "$CAPI_LOG" "$ORACLE_LOG" "$CAPI_ERR" "$ORACLE_ERR"
    exit 1
fi
echo "oracle: ok"

echo "--- differential ---"
if [[ "${SCHEME_DIFF_PARSE_AUX_ONLY:-0}" = "1" ]]; then
    filter_log() { grep -Ev 'candidate|sentence' "$1"; }
    if diff -u <(filter_log "$ORACLE_LOG") <(filter_log "$CAPI_LOG") > /dev/null; then
        echo "PARSE_AUX_IDENTICAL"
        rm -f "$CAPI_LOG" "$ORACLE_LOG" "$CAPI_ERR" "$ORACLE_ERR"
        exit 0
    fi
    echo "DIVERGENCE (parse/aux surface)"
    diff -u <(filter_log "$ORACLE_LOG") <(filter_log "$CAPI_LOG") || true
    rm -f "$CAPI_LOG" "$ORACLE_LOG" "$CAPI_ERR" "$ORACLE_ERR"
    exit 2
fi

if diff -u "$ORACLE_LOG" "$CAPI_LOG" > /dev/null; then
    echo "IDENTICAL"
    rm -f "$CAPI_LOG" "$ORACLE_LOG" "$CAPI_ERR" "$ORACLE_ERR"
    exit 0
fi

echo "DIVERGENCE"
diff -u "$ORACLE_LOG" "$CAPI_LOG" || true
rm -f "$CAPI_LOG" "$ORACLE_LOG" "$CAPI_ERR" "$ORACLE_ERR"
exit 2
