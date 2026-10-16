#!/usr/bin/env bash
set -euo pipefail

# Provision a Claude Code cloud session (a root container behind a
# TLS-intercepting egress proxy) so every verification gate can run in it
# (issue #818). Idempotent: each step checks its result and skips when it
# already holds, so a second run does nothing but the checks.
#
#   0. install valgrind from the distribution archive if it is missing
#   1. start dockerd if it is not up (detached, waits for `docker info`);
#      trust the session proxy CA for the daemon (/root/.ccr/README.md)
#   2. pull the pinned debian:testing digest
#   3. oracle libraries for bdb, kc and tkrzw: pull
#      ghcr.io/shenghaoc/oxpinyin-oracle when it exists, otherwise build
#      them with tools/oracle/build-oracle.sh --libs-only inside the pinned
#      image; either way each hash must equal tools/oracle/oracle-pin.txt
#   4. model20 via tools/model/fetch-model.sh into a local cache, then
#      build-oracle.sh --regen-data per cell, which fails unless the data
#      manifest equals the recorded one
#   5. write ~/.oxpinyin-oracle.env (OXPINYIN_ORACLE_ENV overrides), then
#      one line per cell on stdout:
#        <dbm> prefix=<dir> libpinyin=<sha256> libzhuyin=<sha256> data manifest OK
#
# Progress goes to stderr. A library hash that differs from the recorded
# one stops the script with exit 3 after printing both: that means the
# recipe is not reproducible on this host, which is the result to report.
#
# Usage: setup.sh [--cache-dir DIR] [--cells "bdb kc tkrzw"] [--jobs N] [--no-pull]
#
#   --cache-dir DIR  oracle prefixes and the model20 cache live here
#                    (default ${XDG_CACHE_HOME:-$HOME/.cache}/oxpinyin, or
#                    $OXPINYIN_CLOUD_CACHE)
#   --cells LIST     cells to provision (default all three)
#   --jobs N         parallel make jobs (default nproc)
#   --no-pull        build the libraries even if the published image exists

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
pin=$repo/tools/oracle/oracle-pin.txt
cache=${OXPINYIN_CLOUD_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/oxpinyin}
cells=(bdb kc tkrzw)
jobs=$(nproc 2>/dev/null || echo 2)
pull=1
proxy_ca=${OXPINYIN_PROXY_CA:-/root/.ccr/ca-bundle.crt}
proxy_ca_single=/root/.ccr/agent-proxy-ca.crt

while (($#)); do
	case $1 in
	--cache-dir)
		cache=$2
		shift 2
		;;
	--cells)
		read -r -a cells <<<"$2"
		shift 2
		;;
	--jobs)
		jobs=$2
		shift 2
		;;
	--no-pull)
		pull=0
		shift
		;;
	-h | --help)
		awk 'NR > 3 && /^#/ { print; next } NR > 3 { exit }' "${BASH_SOURCE[0]}"
		exit 0
		;;
	*)
		printf 'unknown option: %s\n' "$1" >&2
		exit 2
		;;
	esac
done

for dbm in "${cells[@]}"; do
	case $dbm in
	bdb | kc | tkrzw) ;;
	*)
		printf 'unknown cell: %s (one of bdb kc tkrzw)\n' "$dbm" >&2
		exit 2
		;;
	esac
done

log() { printf '[%4ds] %s\n' "$SECONDS" "$*" >&2; }
die() {
	printf 'FAIL: %s\n' "$*" >&2
	exit 1
}
pinval() { sed -n "s/^$1=//p" "$pin"; }

# retry CMD...: registries answer 429 to anonymous pulls from a shared
# egress address; back off 2, 4, 8, 16 s before giving up.
retry() {
	local delay
	for delay in 2 4 8 16; do
		"$@" && return 0
		log "retrying in ${delay}s: $*"
		sleep "$delay"
	done
	"$@"
}

mkdir -p "$cache"
cache=$(cd "$cache" && pwd)
model_cache=$cache/model20
oracle_root=$cache/oracle
mkdir -p "$oracle_root"

base_image=$(pinval base_image)
[[ -n $base_image ]] || die "no base_image in $pin"
# The published tag is derived from the whole recipe by one script, the same
# one the workflow and docs/runbooks/oracle.md use.
published=ghcr.io/shenghaoc/oxpinyin-oracle:$("$repo/tools/oracle/image-tag.sh")
platform=$(pinval platform)
[[ -n $platform ]] || die "no platform in $pin"
deps_image=oxpinyin-oracle-deps:$(sha256sum "$repo/tools/oracle/Dockerfile.oracle" | cut -c1-12)

proxy=${HTTPS_PROXY:-${https_proxy:-}}
if [[ -n $proxy && ! -s $proxy_ca ]]; then
	die "HTTPS_PROXY is set but its CA bundle $proxy_ca is missing (set OXPINYIN_PROXY_CA)"
fi

# ---- 0. valgrind --------------------------------------------------------

# The same-run callgrind measurements (docs/runbooks/benches.md) need
# valgrind, and each one reports the version it used. Install it from the
# distribution archive when it is missing.
if ! valgrind --version >/dev/null 2>&1; then
	sudo_cmd=()
	((EUID == 0)) || sudo_cmd=(sudo)
	log 'installing valgrind from the distribution archive'
	"${sudo_cmd[@]}" apt-get install -y valgrind >/dev/null 2>&1 ||
		{ "${sudo_cmd[@]}" apt-get update >/dev/null 2>&1 &&
			"${sudo_cmd[@]}" apt-get install -y valgrind >/dev/null; } ||
		die 'could not install valgrind with apt-get'
fi
valgrind_version=$(valgrind --version)

# ---- 1. dockerd ---------------------------------------------------------

docker_ready() { docker info >/dev/null 2>&1; }

if docker_ready; then
	log 'dockerd already up'
else
	command -v dockerd >/dev/null 2>&1 || die 'dockerd is not installed'
	# The daemon pulls images itself, through the session proxy, so it must
	# trust the proxy CA. It reads the system store when it starts.
	if [[ -n $proxy && -s $proxy_ca_single ]] &&
		! cmp -s "$proxy_ca_single" /usr/local/share/ca-certificates/ccr-agent-proxy.crt 2>/dev/null; then
		mkdir -p /usr/local/share/ca-certificates
		cp "$proxy_ca_single" /usr/local/share/ca-certificates/ccr-agent-proxy.crt
		update-ca-certificates >/dev/null 2>&1 || log 'update-ca-certificates failed; the daemon may not trust the proxy'
	fi
	log 'starting dockerd'
	setsid nohup dockerd >/var/log/dockerd.log 2>&1 </dev/null &
	for _ in $(seq 1 60); do
		docker_ready && break
		sleep 1
	done
	docker_ready || {
		tail -20 /var/log/dockerd.log >&2
		die 'dockerd did not become ready in 60 s'
	}
fi

# Containers cannot reach the proxy at its loopback address and do not trust
# its CA: they run with --network host, the proxy in their environment and
# the CA mounted (/root/.ccr/README.md, "docker build / docker run").
net_flags=(--network host)
env_flags=()
if [[ -n $proxy ]]; then
	env_flags=(
		-e "https_proxy=$proxy" -e "HTTPS_PROXY=$proxy"
		-e "no_proxy=${NO_PROXY:-${no_proxy:-}}" -e "NO_PROXY=${NO_PROXY:-${no_proxy:-}}"
		-e GIT_SSL_CAINFO=/run/ca.crt -e SSL_CERT_FILE=/run/ca.crt -e CURL_CA_BUNDLE=/run/ca.crt
		-v "$proxy_ca:/run/ca.crt:ro"
	)
fi

# ---- 2. pinned base image -----------------------------------------------

if docker image inspect "$base_image" >/dev/null 2>&1; then
	log "base image present: ${base_image#*@}"
else
	log "pulling $base_image"
	retry docker pull --quiet --platform "$platform" "$base_image" >/dev/null
fi

# ---- 3. oracle libraries ------------------------------------------------

# libs_ok DBM: both libraries of the cell hash to the recorded recipe values.
libs_ok() {
	local dbm=$1 lib want
	for lib in libpinyin libzhuyin; do
		want=$(pinval "${lib}_sha256_$dbm")
		[[ -f $oracle_root/$dbm/lib/$lib.so ]] || return 1
		[[ $(sha256sum "$oracle_root/$dbm/lib/$lib.so" | cut -d' ' -f1) == "$want" ]] || return 1
	done
}

# data_ok DBM: the regenerated data is present, intact and the recorded one.
data_ok() {
	local dbm=$1 want
	want=$(pinval "data_manifest_sha256_$dbm")
	[[ -f $oracle_root/$dbm/oracle-data.sha256 ]] || return 1
	[[ $(sha256sum "$oracle_root/$dbm/oracle-data.sha256" | cut -d' ' -f1) == "$want" ]] || return 1
	(cd "$oracle_root/$dbm" && sha256sum --check --quiet oracle-data.sha256 >/dev/null 2>&1)
}

need=()
for dbm in "${cells[@]}"; do
	if libs_ok "$dbm"; then
		log "$dbm: libraries already match oracle-pin.txt"
	else
		need+=("$dbm")
	fi
done

if ((${#need[@]} && pull)); then
	log "trying $published"
	if docker pull --quiet --platform "$platform" "$published" >/dev/null 2>&1; then
		cid=$(docker create --platform "$platform" "$published" /none)
		for dbm in "${need[@]}"; do
			rm -rf "${oracle_root:?}/$dbm"
			docker cp "$cid:/opt/pinyin-oracle/$dbm" "$oracle_root/$dbm" >/dev/null
		done
		docker rm "$cid" >/dev/null
		# A pulled image is trusted only as far as its hashes match the pin:
		# a cell that does not is discarded and built below.
		still=()
		for dbm in "${need[@]}"; do
			if libs_ok "$dbm"; then
				log "$dbm: pulled libraries match oracle-pin.txt"
			else
				log "$dbm: pulled libraries do not match oracle-pin.txt; building instead"
				rm -rf "${oracle_root:?}/$dbm"
				still+=("$dbm")
			fi
		done
		need=("${still[@]}")
	else
		log 'no published oracle image; building the libraries'
	fi
fi

build_deps_image() {
	docker image inspect "$deps_image" >/dev/null 2>&1 && return 0
	log "building $deps_image (the pinned toolchain)"
	local flags=("${net_flags[@]}")
	if [[ -n $proxy ]]; then
		flags+=(--build-arg "https_proxy=$proxy" --build-arg "HTTPS_PROXY=$proxy" --secret "id=cacert,src=$proxy_ca")
	fi
	DOCKER_BUILDKIT=1 docker build --platform "$platform" "${flags[@]}" --target deps \
		-f "$repo/tools/oracle/Dockerfile.oracle" -t "$deps_image" "$repo/tools/oracle" >&2
}

# in_image ARGS...: run build-oracle.sh in the pinned image. The oracle
# root appears at /opt/pinyin-oracle, the path the published image uses, so
# a prefix means the same thing wherever it was produced.
in_image() {
	docker run --rm --platform "$platform" "${net_flags[@]}" "${env_flags[@]}" \
		-v "$repo/tools/oracle/build-oracle.sh:/usr/local/bin/build-oracle.sh:ro" \
		-v "$repo/tools/oracle/oracle-pin.txt:/usr/local/bin/oracle-pin.txt:ro" \
		-v "$oracle_root:/opt/pinyin-oracle" \
		-v "$model_cache:/model-cache:ro" \
		"$deps_image" build-oracle.sh "$@"
}

mkdir -p "$model_cache"

for dbm in "${need[@]}"; do
	build_deps_image
	log "$dbm: building the libraries"
	rm -rf "${oracle_root:?}/$dbm"
	in_image --libs-only --enable-libzhuyin --dbm "$dbm" \
		--work-dir "/build/$dbm" --prefix "/opt/pinyin-oracle/$dbm" \
		--jobs "$jobs" >/dev/null 2>"$cache/build-$dbm.log" ||
		{
			tail -40 "$cache/build-$dbm.log" >&2
			die "$dbm: the library build failed (full log: $cache/build-$dbm.log)"
		}
done

status=0
for dbm in "${cells[@]}"; do
	for lib in libpinyin libzhuyin; do
		want=$(pinval "${lib}_sha256_$dbm")
		got=$(sha256sum "$oracle_root/$dbm/lib/$lib.so" | cut -d' ' -f1)
		if [[ $got != "$want" ]]; then
			printf 'MISMATCH %s %s\n  recorded: %s\n  built:    %s\n' "$dbm" "$lib" "$want" "$got" >&2
			status=3
		fi
	done
done
if ((status)); then
	printf '%s\n' 'The recipe did not reproduce the recorded hashes on this host; the built' \
		"libraries are left in $oracle_root for readelf/cmp against a reference." >&2
	exit "$status"
fi

# ---- 4. model20 and the data --------------------------------------------

for dbm in "${cells[@]}"; do
	if data_ok "$dbm"; then
		log "$dbm: data already matches the recorded manifest"
		continue
	fi
	log "$dbm: fetching model20 and regenerating the data"
	"$repo/tools/model/fetch-model.sh" --cache-dir "$model_cache" >/dev/null
	build_deps_image
	in_image --regen-data --dbm "$dbm" --prefix "/opt/pinyin-oracle/$dbm" \
		--model-dir /model-cache/extracted --work-dir "/build/$dbm-data" \
		--jobs "$jobs" >/dev/null 2>"$cache/data-$dbm.log" ||
		{
			tail -40 "$cache/data-$dbm.log" >&2
			die "$dbm: --regen-data failed (full log: $cache/data-$dbm.log)"
		}
	data_ok "$dbm" || die "$dbm: the regenerated data does not match the recorded manifest"
done

# ---- 5. report ----------------------------------------------------------

for dbm in "${cells[@]}"; do
	# pkg-config files record the prefix; point them at this location.
	sed -i "s|^prefix=.*|prefix=$oracle_root/$dbm|" "$oracle_root/$dbm"/lib/pkgconfig/*.pc
	printf '%s prefix=%s libpinyin=%s libzhuyin=%s data manifest OK\n' "$dbm" "$oracle_root/$dbm" \
		"$(sha256sum "$oracle_root/$dbm/lib/libpinyin.so" | cut -d' ' -f1)" \
		"$(sha256sum "$oracle_root/$dbm/lib/libzhuyin.so" | cut -d' ' -f1)"
done
# The environment file a lane sources before a runner. Per-cell variables
# name every provisioned cell; PINYIN_ORACLE_DBM (the caller's value, else
# bdb, else the first cell) selects the one the runners see.
env_file=${OXPINYIN_ORACLE_ENV:-$HOME/.oxpinyin-oracle.env}
default_cell=${cells[0]}
for dbm in "${cells[@]}"; do [[ $dbm == bdb ]] && default_cell=bdb; done
{
	printf '# Written by tools/cloud/setup.sh; source it, do not edit it.\n'
	printf 'export PINYIN_MODEL_DIR=%q\n' "$model_cache/extracted"
	for dbm in "${cells[@]}"; do
		up=${dbm^^}
		printf 'export OXPINYIN_ORACLE_PREFIX_%s=%q\n' "$up" "$oracle_root/$dbm"
		printf 'export OXPINYIN_ORACLE_DATA_%s=%q\n' "$up" "$oracle_root/$dbm/lib/libpinyin/data"
	done
	# The selected cell (PINYIN_ORACLE_DBM, default the reference cell)
	# decides the prefix and data directory every time the file is sourced.
	printf "export PINYIN_ORACLE_DBM=\${PINYIN_ORACLE_DBM:-%s}\n" "$default_cell"
	cat <<'SELECT'
_oxp_cell=${PINYIN_ORACLE_DBM^^}
_oxp_prefix=OXPINYIN_ORACLE_PREFIX_$_oxp_cell
_oxp_data=OXPINYIN_ORACLE_DATA_$_oxp_cell
export PINYIN_ORACLE_PREFIX=${!_oxp_prefix:?cell not provisioned by tools/cloud/setup.sh}
export OXPINYIN_LIBPINYIN_DATA_DIR=${!_oxp_data}
unset _oxp_cell _oxp_prefix _oxp_data
SELECT
} >"$env_file.new"
mv "$env_file.new" "$env_file"
log "valgrind: $valgrind_version"
log "wrote $env_file; source it before a runner"
