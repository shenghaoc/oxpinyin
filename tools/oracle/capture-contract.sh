#!/usr/bin/env bash
# Capture contract-diff references from the pin, in the Debian stable pin environment.
#
#   bash tools/oracle/capture-contract.sh CELL [capture options]
#
# CELL is bdb, kc or tkrzw. The options are those of `contract-diff.py capture`
# (--cases, --issue, --family, --cases-prefix, --runs, --case-timeout, --out). References
# are written to fixtures/contract-refs unless --out names another directory under the
# checkout. The pin is the published oracle image (`just compat image-tag`), pulled when
# absent and never built here; the system data is `just fetch-contract-data`'s libpinyin-data.
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
py() { uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev "$@"; }
tag=$(py compat image-tag | tail -n 1)
reference=ghcr.io/shenghaoc/oxpinyin-oracle:$tag
recipe=$(sha256sum tools/oracle/Dockerfile.capture tools/oracle/versions.py tools/oracle/version-manifest.json .python-version | sha256sum | cut -c1-12)
capture=oxpinyin-capture:$tag-$recipe
"$runtime" image inspect "$reference" >/dev/null 2>&1 || "$runtime" pull "$reference"
# OXPINYIN_CAPTURE_BUILD_FLAGS carries site flags for the image build, such as --network host
# or --secret id=cacert,src=... behind a TLS-intercepting egress proxy (no quoting inside).
# shellcheck disable=SC2086
"$runtime" image inspect "$capture" >/dev/null 2>&1 || "$runtime" build ${OXPINYIN_CAPTURE_BUILD_FLAGS:-} -f tools/oracle/Dockerfile.capture --build-arg "REFERENCE_IMAGE=$reference" -t "$capture" .
data=$(py fetch-contract-data | tail -n 1)
image=$("$runtime" image inspect --format '{{.Id}}' "$capture")
mkdir -p "$out"
# The scratch directory is on the host disk, not the container overlay: a few cases (dir_chunk) follow
# the file system their scratch lives on, and the reference records which one.
scratch="$repo/target/capture-scratch"
mkdir -p "$scratch"
exec "$runtime" run --rm --network none --user "$(id -u):$(id -g)" \
    -e HOME=/tmp -e TMPDIR=/scratch -e PYTHONPYCACHEPREFIX=/tmp/pyc \
    -e "OXPINYIN_CAPTURE_IMAGE=$image" -e OXPINYIN_TKRZW_LIBRARY=/usr/lib/"$(uname -m)"-linux-gnu/libtkrzw.so.1 \
    -v "$repo:/work:ro" -v "$repo/$out:/work/$out:rw" -v "$data:/data:ro" -v "$scratch:/scratch:rw" -w /work \
    "$capture" "python$(sed -n 's/^cpython@\([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' .python-version)" -X lazy_imports=normal tools/bisection/contract-diff.py capture "$cell" \
    "/opt/reference/$cell" --data /data --out "/work/$out" "${args[@]}"
