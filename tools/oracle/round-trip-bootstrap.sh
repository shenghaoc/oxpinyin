#!/usr/bin/env bash
# Bootstrap: the supplied round-trip image has neither uv nor managed Python.
# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"
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

} > /work/target/rt-oracle.log 2>&1
bash tools/cloud/setup.sh --python-only >> /work/target/rt-oracle.log 2>&1 || exit "$?"
exec uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev compat docker-round-trip-inside
