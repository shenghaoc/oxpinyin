#!/usr/bin/env bash
# Is a debug-info build the same code as the shipped one?
#
# Profilers need symbols; the release profile ships without them. Any
# profile-vs-timing comparison therefore rests on an assumption -- that
# adding debug info changed only debug info -- and that assumption is
# both testable and, on this workspace, false in a way worth knowing.
#
#   bash tools/env-probe/probe-debuginfo-neutrality.sh [backend-feature]
#
# Default backend is redb, the pure-Rust peer, so the probe runs on a
# host with no DBM development packages installed.
#
# The comparison itself (full normalized instruction text for the
# multiset; hash-stripped symbol identity plus instruction-body pairing
# for the relocation count) lives in
# tools/bisection/debuginfo-neutrality-lib.sh and is shared with the
# differential driver's stage 2, so the two cannot drift.
#
# Two things this exists to stop you getting wrong.
#
# 1. RUSTFLAGS="-Cdebuginfo=1" IS SILENTLY USELESS HERE. Cargo passes
#    -Cstrip=debuginfo whenever the profile itself asks for no debug
#    info, so the flag emits DWARF and cargo strips it straight back out.
#    Measured on rustc 1.97.1: the RUSTFLAGS-only artifact carries zero
#    .debug_* sections and is byte-identical to the stock one. Debug info
#    has to come from the profile -- CARGO_PROFILE_RELEASE_DEBUG=1 -- or
#    from RUSTFLAGS with CARGO_PROFILE_RELEASE_STRIP=none alongside it.
#
# 2. ENABLING IT IS NOT NEUTRAL, BUT IT IS NOT CODEGEN EITHER. Measured on
#    2026-09-08 UTC, oxpinyin-capi with and without debug info has the
#    same instruction multiset but different .text bytes: the profile's
#    debug level feeds cargo's crate metadata hash, hence every mangled
#    symbol name, hence link order. Instruction counts are unaffected by
#    that. SIMULATED cache and branch figures are not -- layout is
#    exactly what they model.
#
# So the probe classifies rather than passing or failing on a hash:
#
#   (a) identical .text bytes            -> fully neutral
#   (b) identical instruction multiset   -> Ir safe, simulated cache and
#       at different addresses             branch figures layout-shifted
#   (c) different instruction multiset   -> different code. Stop.

set -euo pipefail

BACKEND=${1:-redb}
PKG=${PKG:-oxpinyin-capi}
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
LIB="$REPO/tools/bisection/debuginfo-neutrality-lib.sh"
# shellcheck source=../bisection/debuginfo-neutrality-lib.sh
source "$LIB"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

cd "$REPO"
echo "=== debug-info neutrality: $PKG, backend $BACKEND ==="
echo "captured_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "rustc: $(rustc --version)"
echo

SO_REL=$(sed -n 's/^name = "\(.*\)"/\1/p' crates/"$PKG"/Cargo.toml | head -1)
# [lib] name wins over [package] name for the artifact filename.
LIB_NAME=$(awk '/^\[lib\]/{f=1;next} /^\[/{f=0} f&&/^name *=/{gsub(/[",]/,"");print $3}' crates/"$PKG"/Cargo.toml)
SO="target/release/lib${LIB_NAME:-$SO_REL}.so"

build() { # tag  env-assignments...
    local tag=$1; shift
    touch crates/"$PKG"/src/lib.rs
    env "$@" cargo build --locked --release -p "$PKG" \
        --no-default-features --features "$BACKEND" >/dev/null 2>&1
    cp "$SO" "$WORK/$tag.so"
    printf '%-10s %-12s debug sections: %s\n' "$tag" \
        "$(stat -c%s "$WORK/$tag.so") B" \
        "$(readelf -S "$WORK/$tag.so" | grep -c debug_ || true)"
}

# A control that makes point 1 above visible rather than asserted.
build stock       CARGO_TERM_QUIET=true
build rustflags   RUSTFLAGS=-Cdebuginfo=1
build profile     CARGO_PROFILE_RELEASE_DEBUG=1
echo

text_of() { objcopy -O binary --only-section=.text "$WORK/$1.so" "$WORK/$1.text"; }
for t in stock rustflags profile; do text_of "$t"; done

echo "=== .text ==="
for t in stock rustflags profile; do
    printf '%-10s %10s B  sha256 %s\n' "$t" \
        "$(stat -c%s "$WORK/$t.text")" \
        "$(sha256sum "$WORK/$t.text" | cut -c1-16)…"
done
echo

if cmp -s "$WORK/stock.so" "$WORK/rustflags.so"; then
    echo "CONFIRMED: RUSTFLAGS=-Cdebuginfo=1 produced a byte-identical"
    echo "           artifact — cargo stripped it. Use the profile."
else
    echo "NOTE: RUSTFLAGS=-Cdebuginfo=1 did change the artifact on this"
    echo "      toolchain; the strip-back behaviour above may have been fixed."
fi
echo

echo "=== verdict: stock vs profile-debug ==="
compare_debuginfo_neutrality "$WORK/stock.so" "$WORK/profile.so"
