#!/usr/bin/env bash
# run-dropin-fedora-kc.sh — build oxpinyin (Kyoto Cabinet default) inside a
# Fedora rawhide container and run the R1 drop-in differential against the
# container's own KC-built libpinyin and installed libpinyin-data.
#
# Everything — build, oracle, data — lives inside the container, so no
# cross-environment ABI question arises. Uses podman; the container gets
# its own cargo target dir under target-fedora/ so host builds are
# untouched. The distro rustc is not used: rustup installs the workspace's
# pinned toolchain (rust-toolchain.toml).
#
# Usage: tools/bisection/run-dropin-fedora-kc.sh
# Exit: the differential's exit (0 IDENTICAL, 1 DIVERGE, 2 setup failure).

set -euo pipefail
cd "$(dirname "$0")/../.."

exec podman run --rm --security-opt label=disable \
    -v "$(pwd)":/src \
    docker.io/library/fedora:rawhide \
    bash -c '
        set -euo pipefail
        dnf install -y --quiet gcc gcc-c++ make pkg-config clang-devel \
            kyotocabinet-devel libpinyin libpinyin-data libpinyin-devel diffutils \
            curl ca-certificates >/dev/null
        curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs \
            | sh -s -- -y --profile minimal --default-toolchain none -q
        . "$HOME/.cargo/env"
        cd /src
        export CARGO_TARGET_DIR=/src/target-fedora
        cargo build --release -p oxpinyin-capi
        ORACLE_LIB=$(ls /usr/lib64/libpinyin.so.15* | head -1) \
        SUBJECT_LIB=$CARGO_TARGET_DIR/release/libpinyin_capi.so \
        OXPINYIN_SYSTEM_DIR=$(pkg-config --variable=pkgdatadir libpinyin)/data \
            tools/bisection/run-pred-order-kc-dropin.sh
    '
