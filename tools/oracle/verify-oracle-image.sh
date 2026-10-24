#!/usr/bin/env bash
set -euo pipefail

# Check a built oracle image against tools/oracle/oracle-pin.txt (issue #818).
#
# For each cell (bdb, kc, tkrzw) the image's libpinyin.so and libzhuyin.so
# must hash to the recorded recipe values, and the image must carry NO
# model-derived data (docs/findings/model-provenance.md): no
# lib/libpinyin/data directory and no *.bin, *.table, *.text or bigram.db
# file anywhere under /opt/pinyin-oracle.
#
# Usage: verify-oracle-image.sh [--engine docker|podman] [--platform PLATFORM] IMAGE

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
# shellcheck source=pin.sh
source "$repo/tools/oracle/pin.sh"
engine=
image=
platform=linux/amd64

while (($#)); do
	case $1 in
	--engine)
		engine=$2
		shift 2
		;;
	--platform)
		platform=$2
		shift 2
		;;
	-h | --help)
		sed -n '4,14p' "${BASH_SOURCE[0]}"
		exit 0
		;;
	-*)
		printf 'unknown option: %s\n' "$1" >&2
		exit 2
		;;
	*)
		image=$1
		shift
		;;
	esac
done
[[ -n $image ]] || {
	printf 'usage: verify-oracle-image.sh [--engine docker|podman] [--platform PLATFORM] IMAGE\n' >&2
	exit 2
}
case $platform in
linux/amd64 | linux/arm64) ;;
*)
	printf 'unsupported platform: %s (use linux/amd64 or linux/arm64)\n' "$platform" >&2
	exit 2
	;;
esac
if [[ -z $engine ]]; then
	for e in docker podman; do
		command -v "$e" >/dev/null 2>&1 && engine=$e && break
	done
fi
[[ -n $engine ]] || {
	printf '%s\n' 'neither docker nor podman found' >&2
	exit 1
}

scratch=$(mktemp -d)
cid=
trap '[[ -z $cid ]] || "$engine" rm -f "$cid" >/dev/null 2>&1 || true; rm -rf "$scratch"' EXIT

# The image is FROM scratch: it has no command, so give create a dummy one.
cid=$("$engine" create --platform "$platform" "$image" /none)
"$engine" cp "$cid:/opt/pinyin-oracle" "$scratch/oracle" >/dev/null

status=0
for dbm in bdb kc tkrzw; do
	for lib in libpinyin libzhuyin; do
		recorded=$(pin_platform_field "$repo/tools/oracle/oracle-pin.txt" "$platform" "${lib}_sha256_$dbm")
		actual=$(sha256sum "$scratch/oracle/$dbm/lib/$lib.so" | cut -d' ' -f1)
		printf '%s %s %s\n' "$dbm" "$lib" "$actual"
		if [[ -z $recorded || $recorded != "$actual" ]]; then
			printf 'FAIL: %s %s is %s, oracle-pin.txt records %s\n' "$dbm" "$lib" "$actual" "${recorded:-nothing}" >&2
			status=1
		fi
	done
done

leaks=$(find "$scratch/oracle" \( -path '*/lib/libpinyin/data' -o -name '*.bin' -o -name '*.table' -o -name '*.text' -o -name 'bigram.db' \) -print)
if [[ -n $leaks ]]; then
	printf 'FAIL: model-derived data in the image:\n%s\n' "$leaks" >&2
	status=1
fi
exit "$status"
