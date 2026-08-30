#!/usr/bin/env bash
# run-dropin-nixos-kc.sh — build oxpinyin (Kyoto Cabinet default) inside a
# nixos/nix container and run the R1 drop-in differential against
# nixpkgs's own KC-built libpinyin and its installed data.
#
# Why this leg exists: NixOS packages libpinyin against Kyoto Cabinet
# (like Fedora), so it confirms the KC compat path is portable beyond
# RPM-based layouts — /nix/store paths, profile symlinks, no /usr/lib.
#
# Nix-specific shape, verified against the image before writing:
# - The oracle is addressed through the PROFILE symlink
#   (/root/.nix-profile/lib/libpinyin.so.15), never a /nix/store find —
#   the store holds two matches (the package and the user-environment
#   link), so a find is ambiguous by construction.
# - The data dir is derived from the resolved library: nixpkgs installs
#   it at $(libdir)/libpinyin/data inside the package, upstream's layout.
# - rustup toolchains cannot run here (prebuilt glibc binaries need
#   /lib64/ld-linux, which a pure Nix image does not have), so the build
#   uses nixpkgs' rustc/cargo. nixpkgs-unstable currently trails the
#   workspace's rust-version pin (1.95 vs 1.97.1), so the build passes
#   --ignore-rust-version: the pin declares minimum SUPPORT, edition 2024
#   needs only 1.85+, and the differential itself gates the artifact.
# - bindgen finds libclang via LIBCLANG_PATH; the Kyoto Cabinet headers
#   and library ride OXPINYIN_KC_INCLUDE_DIR/LIB_DIR (which also bakes
#   the profile-lib rpath into the subject, so dlopen resolves
#   libkyotocabinet inside the container).
#
# Usage: tools/bisection/run-dropin-nixos-kc.sh
# Exit: the differential's exit (0 IDENTICAL, 1 DIVERGE, 2 setup failure).

set -euo pipefail
cd "$(dirname "$0")/../.."

# The named /nix volume caches the downloaded closures across runs —
# without it every invocation re-fetches the whole toolchain.
exec podman run --rm --security-opt label=disable \
    -v "$(pwd)":/src \
    -v oxpinyin-nix-store:/nix \
    docker.io/nixos/nix \
    bash -c '
        set -euo pipefail
        nix-env -iA nixpkgs.gcc nixpkgs.libclang nixpkgs.kyotocabinet \
            nixpkgs.libpinyin nixpkgs.rustc nixpkgs.cargo \
            nixpkgs.gnused nixpkgs.diffutils >/dev/null 2>&1 \
            || { echo "nix-env install failed" >&2; exit 2; }
        export PATH="/root/.nix-profile/bin:$PATH"
        # nixpkgs libclang does not link its shared library into the
        # profile; point bindgen at the store copy directly.
        LIBCLANG_SO=$(find /nix/store -maxdepth 4 -name "libclang.so*" \
            -not -path "*user-environment*" 2>/dev/null | head -1)
        [ -n "$LIBCLANG_SO" ] || { echo "no libclang.so in the store" >&2; exit 2; }
        export LIBCLANG_PATH=$(dirname "$LIBCLANG_SO")
        # Raw libclang carries no default sysroot on Nix: hand bindgen the
        # glibc dev headers (in the store via gcc'"'"'s closure) explicitly.
        GLIBC_HEADER=$(find /nix/store -maxdepth 4 -name assert.h \
            -path "*glibc*" 2>/dev/null | head -1)
        [ -n "$GLIBC_HEADER" ] || { echo "no glibc dev headers in the store" >&2; exit 2; }
        GLIBC_INC=$(dirname "$GLIBC_HEADER")
        [ -d "$GLIBC_INC" ] || { echo "no glibc dev headers in the store" >&2; exit 2; }
        export BINDGEN_EXTRA_CLANG_ARGS="-isystem $GLIBC_INC"
        export OXPINYIN_KC_INCLUDE_DIR=/root/.nix-profile/include
        export OXPINYIN_KC_LIB_DIR=/root/.nix-profile/lib
        command -v cc >/dev/null || export CC=gcc
        cd /src
        export CARGO_TARGET_DIR=/src/target-nixos
        cargo build --release -p oxpinyin-capi --ignore-rust-version \
            || { echo "subject build failed" >&2; exit 2; }
        ORACLE_LIB=/root/.nix-profile/lib/libpinyin.so.15
        [ -e "$ORACLE_LIB" ] || { echo "profile lacks libpinyin.so.15" >&2; exit 2; }
        DATA_DIR="$(dirname "$(readlink -f "$ORACLE_LIB")")/libpinyin/data"
        echo "nixpkgs libpinyin: $(readlink -f "$ORACLE_LIB")"
        ORACLE_LIB="$ORACLE_LIB" \
        SUBJECT_LIB=$CARGO_TARGET_DIR/release/libpinyin_capi.so \
        OXPINYIN_SYSTEM_DIR="$DATA_DIR" \
            tools/bisection/run-pred-order-nixos-dropin.sh
    '
