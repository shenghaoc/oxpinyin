#!/usr/bin/env bash
# run-dropin-debian-tkrzw.sh — build oxpinyin (tkrzw backend) inside a
# Debian testing container and run the R1 drop-in differential against the
# container's own tkrzw-built libpinyin and installed libpinyin-data.
#
# Debian switched libpinyin to tkrzw in 2.11.91-1 (the same healthy
# packaging the store-backends CI already builds against), so this is the
# tkrzw twin of run-dropin-fedora-kc.sh; see that script for the shape.
#
# Usage: tools/bisection/run-dropin-debian-tkrzw.sh
# Exit: the differential's exit (0 IDENTICAL, 1 DIVERGE, 2 setup failure).

set -euo pipefail
cd "$(dirname "$0")/../.."

exec podman run --rm --security-opt label=disable \
    -v "$(pwd)":/src \
    docker.io/library/debian:testing \
    bash -c '
        set -euo pipefail
        export DEBIAN_FRONTEND=noninteractive
        apt-get update -qq
        apt-get install -y --no-install-recommends -qq \
            gcc g++ make pkg-config libclang-dev libtkrzw-dev \
            liblzma-dev liblz4-dev libzstd-dev zlib1g-dev \
            libpinyin15 libpinyin-data libpinyin-dev diffutils \
            curl ca-certificates >/dev/null
        curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs \
            | sh -s -- -y --profile minimal --default-toolchain none -q
        . "$HOME/.cargo/env"
        cd /src
        export CARGO_TARGET_DIR=/src/target-debian
        cargo build --release -p oxpinyin-capi --no-default-features --features tkrzw
        ORACLE_LIB=$(ls /usr/lib/*/libpinyin.so.15* /usr/lib/libpinyin.so.15* 2>/dev/null | head -1) \
        SUBJECT_LIB=$CARGO_TARGET_DIR/release/libpinyin_capi.so \
        OXPINYIN_SYSTEM_DIR=$(pkg-config --variable=pkgdatadir libpinyin)/data \
            tools/bisection/run-pred-order-tkrzw-dropin.sh
    '
