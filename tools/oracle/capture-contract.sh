#!/usr/bin/env bash
# Capture contract-diff references from the pin, in the Debian stable pin environment.
#
#   bash tools/oracle/capture-contract.sh CELL [capture options]
#
# CELL is bdb, kc or tkrzw. The options are those of `contract-diff.py capture`
# (--cases, --issue, --family, --cases-prefix, --runs, --case-timeout, --out). References
# are written to fixtures/contract-refs unless --out names another directory under the
# checkout. The pin is the published oracle image (`just compat image-tag`), pulled when
# absent; build it locally with build-contract-image.sh when unpublished.
# An investigative live comparison is a capture into a temporary --out, then
# `just runner contract-diff verify CELL ... --references <that directory>`.

# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"
set -euo pipefail
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo"
cell=${1:?usage: capture-contract.sh CELL [capture options]}
shift
# --out DIR (or --out=DIR) names the reference directory, relative to the checkout, which is
# mounted writable; the checkout itself is read-only. Every other option passes through.
out=fixtures/contract-refs
args=()
while (($#)); do
    case $1 in
    --out) [[ $# -ge 2 ]] || { echo "FAIL: --out needs a directory" >&2; exit 2; }; out=$2; shift 2 ;;
    --out=*) out=${1#--out=}; shift ;;
    *) args+=("$1"); shift ;;
    esac
done
case $out in
'' | /* | .. | ../* | */.. | */../*) echo "FAIL: --out must be a relative path under the checkout without ..: $out" >&2; exit 2 ;;
esac
out=${out%/}
case $cell in bdb | kc | tkrzw) ;; *) echo "FAIL: unknown backend: $cell" >&2; exit 2 ;; esac
runtime=${OXPINYIN_CAPTURE_RUNTIME:-docker}
command -v "$runtime" >/dev/null || { echo "FAIL: $runtime is not installed" >&2; exit 1; }
case $(uname -m) in
    x86_64) platform=linux/amd64; suffix=; expected_arch=amd64; multiarch=x86_64-linux-gnu ;;
    aarch64 | arm64) platform=linux/arm64; suffix=-arm64; expected_arch=arm64; multiarch=aarch64-linux-gnu ;;
    *) echo "FAIL: contract capture supports only native x86_64 and aarch64" >&2; exit 2 ;;
esac
py() { uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev "$@"; }
tag=$(py compat image-tag | tail -n 1)
reference=ghcr.io/shenghaoc/oxpinyin-oracle:$tag$suffix
recipe=$(sha256sum tools/oracle/Dockerfile.capture tools/oracle/versions.py tools/oracle/version-manifest.json .python-version | sha256sum | cut -c1-12)
capture=oxpinyin-capture:$tag$suffix-$recipe
if ! "$runtime" image inspect "$reference" >/dev/null 2>&1; then
    if ! "$runtime" pull --platform "$platform" "$reference"; then
        echo "FAIL: $reference is unavailable; build it with: bash tools/oracle/build-contract-image.sh" >&2
        exit 1
    fi
fi
actual_arch=$("$runtime" image inspect --format '{{.Architecture}}' "$reference")
[[ $actual_arch == "$expected_arch" ]] || { echo "FAIL: $reference is $actual_arch, expected native $expected_arch" >&2; exit 1; }
case $runtime in
    *podman) build_pull=(--pull=never); run_user=() ;;
    *) build_pull=(--pull=false); run_user=(--user "$(id -u):$(id -g)") ;;
esac
# OXPINYIN_CAPTURE_BUILD_FLAGS carries site flags for the image build, such as --network host
# or --secret id=cacert,src=... behind a TLS-intercepting egress proxy (no quoting inside).
# shellcheck disable=SC2086
"$runtime" image inspect "$capture" >/dev/null 2>&1 || "$runtime" build --platform "$platform" "${build_pull[@]}" ${OXPINYIN_CAPTURE_BUILD_FLAGS:-} -f tools/oracle/Dockerfile.capture --build-arg "REFERENCE_IMAGE=$reference" -t "$capture" .
actual_arch=$("$runtime" image inspect --format '{{.Architecture}}' "$capture")
[[ $actual_arch == "$expected_arch" ]] || { echo "FAIL: $capture is $actual_arch, expected native $expected_arch" >&2; exit 1; }
data=$(py fetch-contract-data | tail -n 1)
image=$("$runtime" image inspect --format '{{.Id}}' "$capture")
mkdir -p "$out"
exec "$runtime" run --rm --platform "$platform" --network none "${run_user[@]}" \
    -e HOME=/tmp -e TMPDIR=/tmp -e PYTHONPYCACHEPREFIX=/tmp/pyc \
    -e "OXPINYIN_CAPTURE_IMAGE=$image" -e "OXPINYIN_TKRZW_LIBRARY=/usr/lib/$multiarch/libtkrzw.so.1" \
    -v "$repo:/work:ro" -v "$repo/$out:/work/$out:rw" -v "$data:/data:ro" -w /work \
    "$capture" "python$(sed -n 's/^\([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' .python-version)" -X lazy_imports=normal tools/bisection/contract-diff.py capture "$cell" \
    "/opt/reference/$cell" --data /data --out "/work/$out" "${args[@]}"
