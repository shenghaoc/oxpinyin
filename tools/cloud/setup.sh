#!/usr/bin/env bash
set -euo pipefail

# Provision a Claude Code cloud session (a root container behind a
# TLS-intercepting egress proxy) so every verification gate can run in it
# (issue #818). Idempotent: each step checks its result and skips when it
# already holds, so a second run does nothing but the checks.
#
#   0. install valgrind from the distribution archive if it is missing
#   0b. require the backend dump utilities the stateful differentials call:
#      db5.3_dump (bdb) and kchashmgr (kc); install them with apt-get when
#      root, otherwise print the apt-get line and exit 1 (never sudo)
#   1. pick the container runtime ($OXPINYIN_CONTAINER, else podman, else
#      docker). docker: start dockerd if it is not up (detached, waits for
#      `docker info`) and trust the session proxy CA for the daemon
#      (/root/.ccr/README.md); podman runs rootless and needs neither
#   2. pull the pinned debian:testing digest
#   3. oracle libraries for bdb, kc and tkrzw: pull
#      ghcr.io/shenghaoc/oxpinyin-oracle when it exists, otherwise build
#      them with tools/oracle/build-oracle.sh --libs-only inside the pinned
#      image; either way each hash must equal tools/oracle/oracle-pin.txt
#   3b. tkrzw cell: tools/tkrzw/distro-probe.sh against the host libtkrzw;
#      when it reports a defect (Ubuntu's -flto/-Bsymbolic-functions
#      build) or finds none, build upstream tkrzw 9ee8416 (1.0.34) from source
#      into <cache>/tkrzw, refuse it unless the probe prints `RESULT :
#      healthy`, and put it first on PKG_CONFIG_PATH, LD_LIBRARY_PATH and
#      PATH in the environment file; check with ldd that the tkrzw oracle
#      library resolves libtkrzw.so.1 to it
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
#
# $OXPINYIN_CONTAINER names the container command (default: podman if on
# PATH, else docker).

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
pin=$repo/tools/oracle/oracle-pin.txt
cache=${OXPINYIN_CLOUD_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/oxpinyin}
cells=(bdb kc tkrzw)
jobs=$(nproc 2>/dev/null || echo 2)
pull=1
proxy_ca=${OXPINYIN_PROXY_CA:-/root/.ccr/ca-bundle.crt}
proxy_ca_single=/root/.ccr/agent-proxy-ca.crt
# Upstream tkrzw 1.0.34 (9ee8416): its sentinel fix makes defect 1 immune to
# LTO (docs/findings/tkrzw-distro-compat.md).
tkrzw_url=https://github.com/estraier/tkrzw
tkrzw_rev=9ee84164cf45e441aa8186418c03a02c9b89f08b

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

# ---- 0b. backend dump utilities -----------------------------------------

# contract-diff.py and the requested-row training gates call db5.3_dump
# (bdb) and kchashmgr (kc); tkrzw_dbm_util (tkrzw) comes with the tkrzw
# step below. Root installs them as the valgrind step does; anyone else
# gets the exact command, because this script never runs sudo.
missing_tool=() missing_pkg=()
for dbm in "${cells[@]}"; do
	case $dbm in
	bdb) tool=db5.3_dump pkg=db5.3-util ;;
	kc) tool=kchashmgr pkg=kyotocabinet-utils ;;
	*) continue ;;
	esac
	command -v "$tool" >/dev/null 2>&1 || { missing_tool+=("$tool"); missing_pkg+=("$pkg"); }
done
if ((${#missing_tool[@]})); then
	if ((EUID == 0)); then
		log "installing ${missing_pkg[*]} from the distribution archive"
		apt-get install -y "${missing_pkg[@]}" >/dev/null 2>&1 ||
			{ apt-get update >/dev/null 2>&1 && apt-get install -y "${missing_pkg[@]}" >/dev/null; } ||
			die "could not install ${missing_pkg[*]} with apt-get"
	else
		printf 'missing: %s\nrun as root: apt-get install -y %s\n' "${missing_tool[*]}" "${missing_pkg[*]}" >&2
		exit 1
	fi
fi

# ---- 1. container runtime -----------------------------------------------

# $OXPINYIN_CONTAINER, else podman, else docker. Every container call below
# goes through $ctr; only docker needs a daemon, and podman runs rootless.
ctr=${OXPINYIN_CONTAINER:-}
if [[ -z $ctr ]]; then
	if command -v podman >/dev/null 2>&1; then
		ctr=podman
	else
		ctr=docker
	fi
fi
command -v "$ctr" >/dev/null 2>&1 || die "container command $ctr is not installed"
ctr_ready() { "$ctr" info >/dev/null 2>&1; }

if ctr_ready; then
	log "$ctr ready"
elif [[ $(basename "$ctr") == docker ]]; then
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
		ctr_ready && break
		sleep 1
	done
	ctr_ready || {
		tail -20 /var/log/dockerd.log >&2
		die 'dockerd did not become ready in 60 s'
	}
else
	"$ctr" info >&2 || true
	die "$ctr info failed; there is no daemon to start for it"
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

if "$ctr" image inspect "$base_image" >/dev/null 2>&1; then
	log "base image present: ${base_image#*@}"
else
	log "pulling $base_image"
	retry "$ctr" pull --quiet --platform "$platform" "$base_image" >/dev/null
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
	pull_err=$cache/pull-published.err
	if "$ctr" pull --quiet --platform "$platform" "$published" >/dev/null 2>"$pull_err"; then
		cid=$("$ctr" create --platform "$platform" "$published" /none)
		for dbm in "${need[@]}"; do
			rm -rf "${oracle_root:?}/$dbm"
			"$ctr" cp "$cid:/opt/pinyin-oracle/$dbm" "$oracle_root/$dbm" >/dev/null
		done
		"$ctr" rm "$cid" >/dev/null
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
		# Keep the pull's own words: "not published yet" and "could not
		# reach the registry" both fall back to the build, but only the
		# first is expected.
		if grep -Eiq 'manifest unknown|manifest .*not found|name unknown|404 not found|: not found$' "$pull_err"; then
			log 'published oracle image: manifest unknown / not found; building the libraries'
		else
			log 'published oracle image: pull FAILED (network, auth or proxy, not "not found"); building the libraries'
		fi
		log "pull error (last lines, full text in $pull_err):"
		tail -n 5 "$pull_err" | while IFS= read -r line; do log "  $line"; done
	fi
fi

build_deps_image() {
	"$ctr" image inspect "$deps_image" >/dev/null 2>&1 && return 0
	log "building $deps_image (the pinned toolchain)"
	local flags=("${net_flags[@]}")
	if [[ -n $proxy ]]; then
		flags+=(--build-arg "https_proxy=$proxy" --build-arg "HTTPS_PROXY=$proxy" --secret "id=cacert,src=$proxy_ca")
	fi
	DOCKER_BUILDKIT=1 "$ctr" build --platform "$platform" "${flags[@]}" --target deps \
		-f "$repo/tools/oracle/Dockerfile.oracle" -t "$deps_image" "$repo/tools/oracle" >&2
}

# in_image ARGS...: run build-oracle.sh in the pinned image. The oracle
# root appears at /opt/pinyin-oracle, the path the published image uses, so
# a prefix means the same thing wherever it was produced. Every mount
# carries the shared SELinux relabel `z` (not the private `Z`): later
# in_image calls mount the same paths again, and on a host without SELinux
# the option does nothing.
in_image() {
	"$ctr" run --rm --platform "$platform" "${net_flags[@]}" "${env_flags[@]}" \
		-v "$repo/tools/oracle/build-oracle.sh:/usr/local/bin/build-oracle.sh:ro,z" \
		-v "$repo/tools/oracle/oracle-pin.txt:/usr/local/bin/oracle-pin.txt:ro,z" \
		-v "$oracle_root:/opt/pinyin-oracle:z" \
		-v "$model_cache:/model-cache:ro,z" \
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

# ---- 3b. a sound libtkrzw -----------------------------------------------

# Ubuntu builds tkrzw with -flto and -Bsymbolic-functions, which breaks its
# pointer-identity protocol (docs/findings/tkrzw-distro-compat.md); the pin
# oracle and the subject both load whatever libtkrzw.so.1 the host offers.
# tools/tkrzw/distro-probe.sh decides whether the host's is usable; if not
# (or there is none) upstream tkrzw is built into <cache>/tkrzw and put first
# on PKG_CONFIG_PATH, LD_LIBRARY_PATH and PATH by the environment file.
probe=$repo/tools/tkrzw/distro-probe.sh
tkrzw_prefix=$cache/tkrzw
tkrzw_local=0

# tkrzw_so FILE: where ldd, in the caller's environment, resolves FILE's
# libtkrzw.so.1 (empty if it does not).
tkrzw_so() {
	ldd "$1" 2>/dev/null | sed -n 's/^[[:space:]]*libtkrzw\.so\.1 => \(\/[^ ]*\).*/\1/p'
}

# without_prefix LIST: the colon list LIST minus every entry inside
# $tkrzw_prefix. A shell that sourced the environment file carries the
# cached build on PATH and LD_LIBRARY_PATH; the host probe must not see it.
without_prefix() {
	local entry out=() IFS=:
	for entry in $1; do
		[[ $entry == "$tkrzw_prefix" || $entry == "$tkrzw_prefix"/* ]] || out+=("$entry")
	done
	printf '%s' "${out[*]}"
}

# probe_host: the probe against the host's own libtkrzw, and an ldd check
# that the probe's tkrzw_dbm_util resolved libtkrzw.so.1 outside the cache.
probe_host() {
	local host_path host_ld util so
	host_path=$(without_prefix "$PATH")
	host_ld=$(without_prefix "${LD_LIBRARY_PATH:-}")
	env PATH="$host_path" LD_LIBRARY_PATH="$host_ld" sh "$probe" || return 1
	util=$(PATH=$host_path command -v tkrzw_dbm_util) || return 1
	so=$(LD_LIBRARY_PATH=$host_ld tkrzw_so "$util")
	printf 'host check: %s resolves libtkrzw.so.1 to %s\n' "$util" "${so:-nothing}"
	[[ -n $so && $so != "$tkrzw_prefix"/* ]]
}

# probe_built: the probe against the cached build, then an ldd check, in the
# same environment, that its tkrzw_dbm_util resolves libtkrzw.so.1 to the
# cache. (The probe's own path line needs readelf, which is optional.)
probe_built() {
	local built_path=$tkrzw_prefix/bin:$PATH
	local built_ld=$tkrzw_prefix/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH} out
	out=$(PATH=$built_path LD_LIBRARY_PATH=$built_ld sh "$probe" 2>&1) || return 1
	[[ $out == *'RESULT : healthy'* ]] || return 1
	[[ $(PATH=$built_path LD_LIBRARY_PATH=$built_ld tkrzw_so "$tkrzw_prefix/bin/tkrzw_dbm_util") == \
		"$tkrzw_prefix/lib/libtkrzw.so.1" ]]
}

if [[ " ${cells[*]} " == *' tkrzw '* ]]; then
	if probe_host >"$cache/tkrzw-host-probe.log" 2>&1; then
		log 'tkrzw: the host libtkrzw passes distro-probe.sh'
	else
		log "tkrzw: the host libtkrzw is broken or missing (probe: $cache/tkrzw-host-probe.log)"
		tkrzw_local=1
		if [[ -f $tkrzw_prefix/REV && $(<"$tkrzw_prefix/REV") == "$tkrzw_rev" ]] && probe_built; then
			log "tkrzw: cached build ${tkrzw_rev:0:7} passes distro-probe.sh"
		else
			for tool in g++ make; do
				command -v "$tool" >/dev/null 2>&1 ||
					die "building tkrzw needs $tool (apt-get install -y build-essential)"
			done
			log "tkrzw: building upstream ${tkrzw_rev:0:7}"
			rm -rf "${tkrzw_prefix:?}" "$cache/tkrzw-src"
			mkdir -p "$cache/tkrzw-src"
			(
				cd "$cache/tkrzw-src"
				git init -q .
				retry git fetch -q --depth=1 "$tkrzw_url" "$tkrzw_rev"
				git checkout -q FETCH_HEAD
				# No LTO and no -Bsymbolic-functions may come in from the environment.
				env -u CFLAGS -u CXXFLAGS -u LDFLAGS ./configure --prefix="$tkrzw_prefix" &&
					make -j"$jobs" && make install
			) >"$cache/tkrzw-build.log" 2>&1 || {
				tail -40 "$cache/tkrzw-build.log" >&2
				die "the tkrzw build failed (full log: $cache/tkrzw-build.log)"
			}
			rm -rf "$cache/tkrzw-src"
			if ! probe_built; then
				PATH=$tkrzw_prefix/bin:$PATH LD_LIBRARY_PATH=$tkrzw_prefix/lib sh "$probe" >&2 || true
				die "the built tkrzw does not print 'RESULT : healthy' (refusing to use it)"
			fi
			printf '%s\n' "$tkrzw_rev" >"$tkrzw_prefix/REV"
		fi
		# Both sides must resolve libtkrzw.so.1 to the cache: the oracle
		# library here, an oxpinyin tkrzw binary in docs/runbooks/cloud.md.
		resolved=$(LD_LIBRARY_PATH=$tkrzw_prefix/lib tkrzw_so "$oracle_root/tkrzw/lib/libpinyin.so")
		[[ $resolved == "$tkrzw_prefix/lib/libtkrzw.so.1" ]] ||
			die "the tkrzw oracle libpinyin.so resolves libtkrzw.so.1 to '${resolved:-nothing}', not $tkrzw_prefix/lib"
		log "tkrzw: oracle libpinyin.so resolves libtkrzw.so.1 to $resolved"
	fi
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
	if ((tkrzw_local)); then
		printf 'export PKG_CONFIG_PATH=%s\n' "$(printf '%q' "$tkrzw_prefix/lib/pkgconfig")\${PKG_CONFIG_PATH:+:\$PKG_CONFIG_PATH}"
		printf 'export LD_LIBRARY_PATH=%s\n' "$(printf '%q' "$tkrzw_prefix/lib")\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"
		printf 'export PATH=%s\n' "$(printf '%q' "$tkrzw_prefix/bin"):\$PATH"
	fi
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
