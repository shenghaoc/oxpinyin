#!/usr/bin/env bash
# run-rss-diag.sh — the RSS diagnosis matrix for one backend.
#
# Two cells on ONE data directory, the drop-in configuration:
#   L  the pin-built libpinyin .so
#   S  the oxpinyin C ABI as the shipping recipe produces it
#      (tools/packaging/release-stage.sh, i.e. `--features <backend>,shipped`)
# Both open libpinyin's own install data/, so nothing oxpinyin-generated is
# involved and the two cells differ only in the implementation under test.
#
# Rounds are round-robin (L, S, L, S, ...) rather than blocked, so a drift
# in host state over the capture window lands on both cells equally instead
# of on whichever one ran second.
#
# Two kinds of round are taken, and they must not be mixed:
#   * measurement rounds run with RSS_DIAG_DIR unset. Writing the /proc
#     text dumps allocates, which would move the very RSS the malloc_trim
#     delta is measuring.
#   * one mapping round per cell runs with RSS_DIAG_DIR set, to capture
#     /proc/self/maps and malloc_info for the attribution question.
#
# Usage:
#   run-rss-diag.sh --lp <libpinyin.so> --ox <oxpinyin.so> --data <dir> \
#                   [--out DIR] [--rounds N] [--cycles N]
set -euo pipefail
# Resolve before the cd: usage() reads the script by path, and $0 is
# relative to the caller's directory, not ours.
SELF="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"

LP=""; OX=""; DATA=""; OUT=""; ROUNDS=10; CYCLES=8; CPU="${PERF_CPU:-0}"

usage() { sed -n '2,22p' "$SELF" >&2; exit 2; }

while (($#)); do
    case $1 in
        --lp) LP=$2; shift 2 ;;
        --ox) OX=$2; shift 2 ;;
        --data) DATA=$2; shift 2 ;;
        --out) OUT=$2; shift 2 ;;
        --rounds)
            # Reaches `((r <= ROUNDS))` below, which evaluates its operand
            # as an arithmetic expression: anything but a plain decimal is
            # refused here rather than evaluated there.
            case ${2-} in
                ''|*[!0-9]*) echo "--rounds must be a decimal integer: ${2-}" >&2; usage ;;
            esac
            ROUNDS=$2; shift 2 ;;
        --cycles) CYCLES=$2; shift 2 ;;
        -h|--help) usage ;;
        *) echo "unknown argument: $1" >&2; usage ;;
    esac
done
[ -n "$LP" ] && [ -n "$OX" ] && [ -n "$DATA" ] || usage
OUT="${OUT:-/tmp/rss-diag}"
mkdir -p "$OUT" "$OUT/maps"

# Rebuild when the binary is missing, not executable, or older than its
# source. A stale binary silently measures a different harness than the
# tree describes, which is exactly the provenance gap the identity table
# below exists to close.
if [ ! -x "$SCRIPT_DIR/bisect" ] || [ "$SCRIPT_DIR/bisect.c" -nt "$SCRIPT_DIR/bisect" ]; then
    gcc -std=gnu11 -Wall -Wextra -Werror -O2 \
        -o "$SCRIPT_DIR/bisect" "$SCRIPT_DIR/bisect.c" -ldl
fi

# Identity of everything the numbers depend on, captured before any of them
# are taken: a record whose subject cannot be named is not a record.
{
    printf 'captured_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf 'kernel=%s\n' "$(uname -srm)"
    printf 'glibc=%s\n' "$(ldd --version | head -n 1)"
    printf 'nproc=%s\n' "$(nproc)"
    printf 'rounds=%s cycles=%s cpu=%s\n' "$ROUNDS" "$CYCLES" "$CPU"
    printf 'data_dir=%s\n' "$DATA"
    # The harness is part of the subject: a capture is not reproducible
    # from the .so hashes alone.
    printf 'repo_revision=%s%s\n' \
        "$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || echo unknown)" \
        "$(git -C "$REPO_ROOT" diff --quiet HEAD 2>/dev/null || echo ' (working tree dirty)')"
    printf 'bisect=%s sha256=%s\n' \
        "$SCRIPT_DIR/bisect" "$(sha256sum "$SCRIPT_DIR/bisect" | cut -d' ' -f1)"
    printf 'bisect_c_sha256=%s\n' \
        "$(sha256sum "$SCRIPT_DIR/bisect.c" | cut -d' ' -f1)"
    for so in "$LP" "$OX"; do
        printf 'so=%s sha256=%s size=%s\n' \
            "$so" "$(sha256sum "$so" | cut -d' ' -f1)" "$(stat -c %s "$so")"
        printf '  needed=%s\n' \
            "$(readelf -d "$so" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | tr '\n' ' ')"
    done
} > "$OUT/identity.txt"

run_one() {
    local cell=$1 so=$2 round=$3
    local load_before load_after
    load_before=$(cut -d' ' -f1-3 /proc/loadavg)
    local line
    line=$(PERF_BACKEND="$cell" PERF_MODE=rss-diag PERF_CYCLES="$CYCLES" \
        taskset -c "$CPU" "$SCRIPT_DIR/bisect" --perf "$so" "$DATA")
    load_after=$(cut -d' ' -f1-3 /proc/loadavg)
    printf '{"cell":"%s","round":%d,"loadavg_before":"%s","loadavg_after":"%s","result":%s}\n' \
        "$cell" "$round" "$load_before" "$load_after" "$line"
}

: > "$OUT/rounds.jsonl"
for ((r = 1; r <= ROUNDS; r++)); do
    run_one L "$LP" "$r" >> "$OUT/rounds.jsonl"
    run_one S "$OX" "$r" >> "$OUT/rounds.jsonl"
done

# The mapping round, separate and last: RSS_DIAG_DIR makes bisect write
# /proc/self/maps, smaps_rollup and malloc_info at each snapshot point.
for pair in "L:$LP" "S:$OX"; do
    cell=${pair%%:*}; so=${pair#*:}
    PERF_BACKEND="$cell" PERF_MODE=rss-diag PERF_CYCLES="$CYCLES" \
        RSS_DIAG_DIR="$OUT/maps" RSS_DIAG_TAG="$cell" \
        taskset -c "$CPU" "$SCRIPT_DIR/bisect" --perf "$so" "$DATA" \
        > "$OUT/maps/$cell-round.json"
done

printf 'wrote %s\n' "$OUT"
