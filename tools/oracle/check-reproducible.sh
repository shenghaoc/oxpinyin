#!/usr/bin/env bash

# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"

set -euo pipefail

# Reproducibility gate for the pin-built oracle libraries (issue #818).
#
# For each DBM backend (bdb, kc, tkrzw) this builds libpinyin.so and
# libzhuyin.so twice, in the pinned build image, from clean, DIFFERENT work
# directories and DIFFERENT install prefixes, and fails unless both builds
# hash identically, and equally to the values tools/oracle/oracle-pin.txt
# records. Prints one line per backend and library:
#
#   <dbm> <library> <sha256>
#
# Usage: check-reproducible.sh [--engine docker|podman|container] [--image IMAGE] [--jobs N]
#                              [--platform linux/amd64|linux/arm64]
#
# Without --image the `deps` stage of tools/oracle/Dockerfile.oracle is built
# first (the Debian trixie pin-reference environment). The builds fetch the
# two pinned upstreams over the network; no model is needed (--libs-only).

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
# shellcheck source=pin.sh
source "$repo/tools/oracle/pin.sh"
engine=
image=
jobs=$(nproc 2>/dev/null || echo 2)
platform=linux/amd64

while (($#)); do
	case $1 in
	--engine)
		engine=$2
		shift 2
		;;
	--image)
		image=$2
		shift 2
		;;
	--jobs)
		jobs=$2
		shift 2
		;;
	--platform)
		platform=$2
		shift 2
		;;
	-h | --help)
		sed -n "4,22p" "${BASH_SOURCE[0]}"
		exit 0
		;;
	*)
		printf 'unknown option: %s\n' "$1" >&2
		exit 2
		;;
	esac
done

case $platform in
linux/amd64 | linux/arm64) ;;
*)
	printf 'unsupported platform: %s (use linux/amd64 or linux/arm64)\n' "$platform" >&2
	exit 2
	;;
esac
if [[ -z $engine ]]; then
	for e in docker podman container; do
		command -v "$e" >/dev/null 2>&1 && engine=$e && break
	done
fi
[[ -n $engine ]] || {
	printf '%s\n' 'no docker, podman or container runtime found' >&2
	exit 1
}
# SELinux hosts refuse a bind-mounted script without a relabel. Apple
# container's default per-container limit is below the KC C++ build's peak;
# give its two parallel builds room.
run_flags=(--rm)
[[ $engine == podman ]] && run_flags+=(--security-opt label=disable)
[[ $engine == container ]] && run_flags+=(--memory 6G)

# Behind a session proxy (a cloud session, tools/cloud/setup.sh) containers
# cannot reach the proxy at its loopback address or trust its CA: they run
# with --network host, the proxy in their environment and the CA mounted,
# and the deps build gets the proxy as a build arg and the CA as the
# BuildKit secret that Dockerfile.oracle reads.
build_flags=()
proxy=${HTTPS_PROXY:-${https_proxy:-}}
proxy_ca=${OXPINYIN_PROXY_CA:-/root/.ccr/ca-bundle.crt}
if [[ $engine == docker && -n $proxy && -s $proxy_ca ]]; then
	run_flags+=(--network host -e "https_proxy=$proxy" -e "HTTPS_PROXY=$proxy"
		-e GIT_SSL_CAINFO=/run/ca.crt -e SSL_CERT_FILE=/run/ca.crt -e CURL_CA_BUNDLE=/run/ca.crt
		-v "$proxy_ca:/run/ca.crt:ro")
	build_flags=(--network host --build-arg "https_proxy=$proxy" --build-arg "HTTPS_PROXY=$proxy"
		--secret "id=cacert,src=$proxy_ca")
fi

if [[ -z $image ]]; then
	image=oxpinyin-oracle-deps:check
	platform_build=()
	[[ $platform == linux/amd64 ]] || platform_build=(--build-arg "ORACLE_PLATFORM=$platform")
	"$engine" build --platform "$platform" "${platform_build[@]}" "${build_flags[@]}" --target deps -f "$repo/tools/oracle/Dockerfile.oracle" -t "$image" "$repo" >&2
fi

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

# build_one TAG WORKDIR PREFIX DBM: one clean build, then the library hashes.
build_one() {
	local tag=$1 work=$2 prefix=$3 dbm=$4
	"$engine" run -i --platform "$platform" "${run_flags[@]}" \
		-v "$repo/tools/oracle/build-oracle.sh:/usr/local/bin/build-oracle.sh:ro" \
		"$image" bash -s -- "$dbm" "$work" "$prefix" "$jobs" <<'INCONTAINER' >"$scratch/$tag.out"
set -eu
build-oracle.sh --libs-only --enable-libzhuyin --dbm "$1" \
	--work-dir "$2" --prefix "$3" --jobs "$4" >/dev/null 2>/tmp/build.log ||
	{ tail -40 /tmp/build.log >&2; exit 1; }
for lib in libpinyin libzhuyin; do
	printf "%s %s\n" "$lib" "$(sha256sum "$3/lib/$lib.so" | cut -d" " -f1)"
done
INCONTAINER
}

status=0
for dbm in bdb kc tkrzw; do
	build_one "$dbm-a" /build/a "/opt/oracle-a/$dbm" "$dbm" &
	pid_a=$!
	build_one "$dbm-b" /var/tmp/elsewhere/b "/opt/oracle-b/$dbm" "$dbm" &
	pid_b=$!
	# Each build's own exit status counts: a build that fails after printing
	# its hashes must not pass because the other one succeeded.
	build_failed=0
	wait "$pid_a" || build_failed=1
	wait "$pid_b" || build_failed=1
	if ((build_failed)); then
		printf 'FAIL: %s: a build exited non-zero\n' "$dbm" >&2
		status=1
		continue
	fi
	if cmp -s "$scratch/$dbm-a.out" "$scratch/$dbm-b.out"; then
		while read -r lib sum; do
			printf '%s %s %s\n' "$dbm" "$lib" "$sum"
			recorded=$(pin_platform_field "$repo/tools/oracle/oracle-pin.txt" "$platform" "${lib}_sha256_$dbm")
			if [[ $recorded != "$sum" ]]; then
				printf 'FAIL: %s %s is %s, tools/oracle/oracle-pin.txt records %s\n' "$dbm" "$lib" "$sum" "${recorded:-nothing}" >&2
				status=1
			fi
		done <"$scratch/$dbm-a.out"
	else
		printf 'FAIL: %s builds differ\n' "$dbm" >&2
		diff "$scratch/$dbm-a.out" "$scratch/$dbm-b.out" >&2 || true
		status=1
	fi
done
exit "$status"
