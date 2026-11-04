#!/usr/bin/env bash
# Build the pinned oracle image locally for contract capture on this host.
# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"
set -euo pipefail
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo"
runtime=${OXPINYIN_CAPTURE_RUNTIME:-docker}
command -v "$runtime" >/dev/null || { echo "FAIL: $runtime is not installed" >&2; exit 1; }
case $(uname -m) in
    x86_64) platform=linux/amd64; suffix= ; expected_arch=amd64 ;;
    aarch64 | arm64) platform=linux/arm64; suffix=-arm64; expected_arch=arm64 ;;
    *) echo "FAIL: contract oracle build supports only native x86_64 and aarch64" >&2; exit 2 ;;
esac
py() { uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev "$@"; }
tag=$(py compat image-tag | tail -n 1)
image=ghcr.io/shenghaoc/oxpinyin-oracle:$tag$suffix
# OXPINYIN_CAPTURE_BUILD_FLAGS is for site-specific proxy/network build options.
# shellcheck disable=SC2086
"$runtime" build --platform "$platform" --build-arg "ORACLE_PLATFORM=$platform" \
    ${OXPINYIN_CAPTURE_BUILD_FLAGS:-} -f tools/oracle/Dockerfile.oracle -t "$image" .
actual_arch=$("$runtime" image inspect --format '{{.Architecture}}' "$image")
[[ $actual_arch == "$expected_arch" ]] || { echo "FAIL: built $image for $actual_arch, expected $expected_arch" >&2; exit 1; }
py compat verify-image --engine "$runtime" --platform "$platform" "$image"
printf '%s\n' "$image"
