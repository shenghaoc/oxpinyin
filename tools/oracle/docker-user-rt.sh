#!/usr/bin/env bash
# docker-user-rt.sh — one container, the whole drop-in task-9 verification:
# build the pin oracle (tkrzw), then run the user-dir round-trip
# differential both directions (tools/oracle/user-dir-round-trip.sh).
#
# Usage:
#   docker-user-rt.sh [IMAGE]
#
#   IMAGE  base image (default oxpinyin-validate:latest — Debian testing,
#          rust toolchain, libtkrzw-dev + libkyotocabinet-dev, cargo-c).
#
# Host requirements: the model20 cache (tools/model/fetch-model.sh) under
# <repo>/target/model20/extracted, and a sha-faithful libpinyin git mirror
# under <repo>/target/mirrors/libpinyin.git (a local clone of the pin
# commit). Both are host-side: this machine's container egress proved
# unreliable for github and the Debian CDN, so the container takes its
# big inputs through the mount. ibus-libpinyin is skipped in the oracle
# build (github): its build installs nothing the prefix consumes — the
# differential needs libpinyin's .so, headers and data only — and the
# libpinyin commit itself is still fetched and verified by SHA from the
# mirror.
#
# apt (the small dev packages the base image lacks) runs with fast-fail
# timeouts and retries; everything else is network-free.
set -euo pipefail

image=${1:-oxpinyin-validate:latest}
script_dir=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$script_dir/../.." && pwd)

[[ -d $repo/target/model20/extracted ]] || {
	printf 'model20 cache missing; run: bash tools/model/fetch-model.sh\n' >&2
	exit 1
}
if [[ ! -d $repo/target/mirrors/libpinyin.git ]]; then
	printf 'libpinyin mirror missing; creating from a clone of the pin\n' >&2
	mkdir -p "$repo/target/mirrors"
	git clone https://github.com/libpinyin/libpinyin "$repo/target/mirrors/libpinyin.git"
fi

# shellcheck disable=SC2016
docker run --rm --network host \
	-v "$repo:/work" \
	-v "$HOME/.cargo/registry:/root/.cargo/registry" \
	-w /work -e CARGO_TARGET_DIR=/tmp/target -e HOME=/root \
	"$image" bash -lc '
set -uo pipefail
export DEBIAN_FRONTEND=noninteractive
A="apt-get -o Acquire::http::Timeout=20 -o Acquire::https::Timeout=20 -o Acquire::Retries=8"
{
  echo "== apt (fast-fail retries) ==";
  for i in 1 2 3 4 5 6 7 8 9 10; do
	$A update -qq >/dev/null 2>&1 && { echo UPDATE-OK; break; }
	echo "update retry $i"; sleep 5
  done
  for i in 1 2 3 4 5 6 7 8 9 10; do
	$A install -y --no-install-recommends \
	  liblz4-dev libzstd-dev zlib1g-dev liblzma-dev \
	  autoconf automake libtool libglib2.0-dev wget git pkg-config python3 \
	  >/dev/null 2>&1 && { echo APT-DONE; break; }
	echo "install retry $i"; sleep 5
  done
  command -v git >/dev/null || { echo "git never installed — container egress too degraded"; exit 1; }
  git config --global --add safe.directory "*"

  echo "== oracle build (file:// mirror; ibus skipped: github) ==";
  cp tools/oracle/build-oracle.sh /tmp/bo.sh
  python3 - <<PYEOF
import re
s = open("/tmp/bo.sh").read()
s = s.replace("LIBPINYIN_GIT_URL=https://github.com/libpinyin/libpinyin.git",
              "LIBPINYIN_GIT_URL=file:///work/target/mirrors/libpinyin.git")
s = s.replace(
  "ibus_src=\$(fetch_commit ibus-libpinyin \"\$IBUS_LIBPINYIN_GIT_URL\" \"\$IBUS_LIBPINYIN_SHA\")",
  "ibus_src=ibus-skipped-github-unreachable")
s = re.sub(r"\(\n\tcd \"\\\$ibus_src\"\n.*?\n\)\n",
           "echo \"ibus build skipped: github unreachable this run\"\n",
           s, count=1, flags=re.S)
open("/tmp/bo.sh", "w").write(s)
PYEOF
  bash /tmp/bo.sh \
	--work-dir /tmp/oracle-work \
	--prefix /tmp/oracle-prefix \
	--model-dir /work/target/model20/extracted \
	--jobs "$(nproc)"
  echo "ORACLE-BUILD-EXIT:$?"

  echo "== round trip ==";
  bash tools/oracle/user-dir-round-trip.sh /tmp/oracle-prefix nihao nisha nihaoa
  echo "ROUNDTRIP-EXIT:$?"
} > /work/target/rt-oracle.log 2>&1
echo "CONTAINER-EXIT:$?"
tail -5 /work/target/rt-oracle.log
'
