#!/usr/bin/env bash
set -euo pipefail

# Print the immutable tag of the published oracle image (issue #818).
#
#   <libpinyin commit, 12 hex>-<base image digest, 12 hex>-<apt snapshot>-<recipe, 12 hex>
#
# The tag names every input that changes the library hashes: the pin, the base
# image digest, the apt snapshot timestamp (all from oracle-pin.txt) and the
# recipe itself (SHA-256 of tools/oracle/Dockerfile.oracle followed by
# tools/oracle/build-oracle.sh). Change any of them and the tag changes, so a
# published tag always holds the libraries its recipe produced and is never
# pushed twice. The workflow, tools/cloud/setup.sh and docs/runbooks/oracle.md
# all take the tag from this script.
#
# Usage: image-tag.sh

dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
pin=$dir/oracle-pin.txt

field() { sed -n "s/^$1=//p" "$pin"; }

commit=$(field libpinyin_commit)
base=$(field base_image)
snapshot=$(field apt_snapshot)
digest=${base##*@sha256:}
if [[ -z $commit || -z $snapshot || $digest == "$base" ]]; then
	printf 'FAIL: %s lacks libpinyin_commit, apt_snapshot or a digest-pinned base_image\n' "$pin" >&2
	exit 1
fi
# The Dockerfile hard-codes the same base and snapshot; refuse a disagreement.
grep -qF "$base" "$dir/Dockerfile.oracle" || {
	printf 'FAIL: Dockerfile.oracle does not use base_image %s\n' "$base" >&2
	exit 1
}
grep -qF "$snapshot" "$dir/Dockerfile.oracle" || {
	printf 'FAIL: Dockerfile.oracle does not use apt_snapshot %s\n' "$snapshot" >&2
	exit 1
}
recipe=$(cat "$dir/Dockerfile.oracle" "$dir/build-oracle.sh" | sha256sum | cut -c1-12)
printf '%s-%s-%s-%s\n' "${commit:0:12}" "${digest:0:12}" "$snapshot" "$recipe"
