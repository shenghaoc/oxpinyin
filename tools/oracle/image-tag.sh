#!/usr/bin/env bash
set -euo pipefail

# Print the immutable tag of the published oracle image (issue #818).
#
#   <libpinyin commit, 12 hex>-<base image tag, slugged>-<recipe and record, 12 hex>
#
# The tag names every input that changes the library hashes: the pin, the base
# image codename (all from oracle-pin.txt) and the recipe itself (SHA-256 of
# tools/oracle/Dockerfile.oracle, build-oracle.sh and the version-check
# inputs, followed by oracle-pin.txt). The record holds the library hashes,
# so a rebuild that records new ones gets a new tag instead of being skipped
# because the old tag exists.
# The base is a rolling stable tag, never a digest or a snapshot date; a move
# to the next Debian release is a deliberate codename change and retags the
# image. The workflow, tools/cloud/setup.sh and docs/runbooks/oracle.md all
# take the tag from this script.
#
# Usage: image-tag.sh

dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
pin=$dir/oracle-pin.txt

field() { sed -n "s/^$1=//p" "$pin"; }

commit=$(field libpinyin_commit)
base=$(field base_image)
if [[ -z $commit || -z $base ]]; then
	printf 'FAIL: %s lacks libpinyin_commit or base_image\n' "$pin" >&2
	exit 1
fi
if [[ $base == *@sha256:* ]]; then
	printf 'FAIL: base_image %s is digest-pinned; the catalog tracks a codename tag\n' "$base" >&2
	exit 1
fi
if grep -q '^apt_snapshot=' "$pin"; then
	printf 'FAIL: %s still records an apt snapshot; snapshots are not pins\n' "$pin" >&2
	exit 1
fi
slug=$(printf '%s' "$base" | sed 's/[^A-Za-z0-9_.-]/-/g')
# The Dockerfile hard-codes the same base; refuse a disagreement.
grep -qF "$base" "$dir/Dockerfile.oracle" || {
	printf 'FAIL: Dockerfile.oracle does not use base_image %s\n' "$base" >&2
	exit 1
}
hash() {
	if command -v sha256sum >/dev/null 2>&1; then sha256sum; else shasum -a 256; fi
}
recipe=$(cat "$dir/Dockerfile.oracle" "$dir/build-oracle.sh" \
	"$dir/version-manifest.json" "$dir/versions.py" \
	"$dir/../tkrzw/distro-probe.sh" "$pin" | hash | cut -c1-12)
printf '%s-%s-%s\n' "${commit:0:12}" "$slug" "$recipe"
