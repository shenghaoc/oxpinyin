#!/usr/bin/env bash

# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"

set -euo pipefail

# Provision a Claude Code cloud session (a root container behind a
# TLS-intercepting egress proxy) for verification gates; measurement also
# requires ext4 paths (reported, not provisioned, by this script)
# (issue #818). Idempotent: each step checks
# its result and skips when it already holds. A tool already at or above its
# minimum is never reinstalled: tools.dev doctor (`just doctor`) is the one
# check of every tool's minimum, and this script repeats only the two it
# provisions, uv and Valgrind (doctor.test.py fails if they drift from
# doctor's table).
#
#   0. require Cargo and the repository toolchain; normal cloud setup
#      keeps a Valgrind at or above its minimum and otherwise installs one
#      with apt-get as root/through sudo
#   0a. install uv 0.13.1 only when uv 0.13.1 or newer is missing, and a
#       managed CPython 3.15-series GIL build only when none is present
#   0b. require the backend dump utilities the stateful differentials call:
#      db5.3_dump (bdb) and kchashmgr (kc); install them with apt-get when
#      root, otherwise print the apt-get line and exit 1 (never sudo)
#   1. pick the container runtime ($OXPINYIN_CONTAINER, else podman, else
#      docker). docker: start dockerd if it is not up (detached, waits for
#      `docker info`) and trust the session proxy CA for the daemon
#      (/root/.ccr/README.md); podman runs rootless and needs neither
#   2. pull the debian:trixie pin-reference base image
#   3. oracle libraries for bdb, kc and tkrzw: pull
#      ghcr.io/shenghaoc/oxpinyin-oracle when it exists, otherwise build
#      them with tools/oracle/build-oracle.sh --libs-only inside the
#      trixie pin-reference image; either way each hash must equal
#      tools/oracle/oracle-pin.txt
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
#        setup.sh --python-only
#
#   --python-only    provision uv and managed Python, then stop; cannot combine
#                    with --cells, --cache-dir, --jobs, --no-pull or --in-container
#   --cache-dir DIR  oracle prefixes and the model20 cache live here
#                    (default ${XDG_CACHE_HOME:-$HOME/.cache}/oxpinyin, or
#                    $OXPINYIN_CLOUD_CACHE)
#   --cells LIST     cells to provision (default all three)
#   --jobs N         parallel make jobs (default nproc)
#   --no-pull        build the libraries even if the published image exists
#
# $OXPINYIN_CONTAINER names the container command (default: podman if on
# PATH, else docker).
#
# $OXPINYIN_PROXY_CA names the proxy's CA bundle. Without it, the session's
# /root/.ccr/ca-bundle.crt is used; with HTTPS_PROXY set and neither present,
# the CA the host trusts is used (SSL_CERT_FILE, CURL_CA_BUNDLE, then the
# system bundle), or setup fails at step 0. A present, non-empty system bundle
# does not show that the proxy's interception certificate is trusted: the
# fallback only avoids failing early, and a TLS failure there is real.
# Containers mount a private copy (mode 0600, in this run's temporary
# directory, removed on exit) with `:ro,z`; the selected file itself is never
# bind-mounted or relabelled.
#
# Test-only overrides, read by tools/cloud/env.test.sh and not for use
# otherwise: $OXPINYIN_CCR_DIR replaces /root/.ccr, and
# $OXPINYIN_SYSTEM_CA_BUNDLES (colon-separated) replaces the system bundle
# search (/etc/ssl/certs/ca-certificates.crt, /etc/pki/tls/certs/ca-bundle.crt,
# /etc/ssl/cert.pem). Unset, both leave those defaults, and setup behaves as
# it would without them.

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
pin=$repo/tools/oracle/oracle-pin.txt
# shellcheck source=../oracle/pin.sh
source "$repo/tools/oracle/pin.sh"
cache=${OXPINYIN_CLOUD_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/oxpinyin}
cells=(bdb kc tkrzw)
jobs=$(nproc 2>/dev/null || echo 2)
pull=1
inside=0
python_only=0
oracle_options=0
# OXPINYIN_CCR_DIR is a test-only override (see the header).
ccr_dir=${OXPINYIN_CCR_DIR:-/root/.ccr}
proxy_ca=${OXPINYIN_PROXY_CA:-$ccr_dir/ca-bundle.crt}
proxy_ca_single=$ccr_dir/agent-proxy-ca.crt
# Minimums this script provisions to (see the header). Keep them equal to
# tools/dev/doctor.py; doctor.test.py compares the two.
uv_minimum=0.13.1
valgrind_minimum=3.22.0
# Upstream tkrzw 1.0.34 (9ee8416): its sentinel fix makes defect 1 immune to
# LTO (docs/findings/tkrzw-distro-compat.md).
# Exact on purpose (#1074): the oracle side is the reference itself, not a tool.
tkrzw_url=https://github.com/estraier/tkrzw
tkrzw_rev=9ee84164cf45e441aa8186418c03a02c9b89f08b

while (($#)); do
	case $1 in
	--python-only)
		python_only=1
		shift
		;;
	--cache-dir)
		oracle_options=1
		cache=$2
		shift 2
		;;
	--cells)
		oracle_options=1
		read -r -a cells <<<"$2"
		shift 2
		;;
	--jobs)
		oracle_options=1
		jobs=$2
		shift 2
		;;
	--in-container)
		oracle_options=1
		inside=1
		shift
		;;
	--no-pull)
		oracle_options=1
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

if ((python_only && oracle_options)); then
	printf '%s\n' '--python-only cannot be combined with --cells, --cache-dir, --jobs, --no-pull or --in-container' >&2
	exit 2
fi

[[ $(uname -s) == Linux ]] || {
	printf 'Oracle provisioning requires Linux; run subject, reference and harness together in a Linux environment. See docs/runbooks/setup.md.\n' >&2
	exit 2
}
platform=$(oracle_platform) || {
	printf 'No frozen oracle hashes for architecture %s; supported platforms are linux/amd64 and linux/arm64. See docs/runbooks/setup.md.\n' "$(uname -m)" >&2
	exit 2
}
[[ -n $(pin_platform_field "$pin" "$platform" libpinyin_sha256_bdb) ]] || {
	printf 'No frozen oracle hashes for %s in %s. See docs/runbooks/setup.md.\n' "$platform" "$pin" >&2
	exit 2
}
if ((inside)) && [[ ${OXPINYIN_COMPAT_CONTAINER:-} != 1 || ! -d /opt/reference ]]; then
	printf '%s\n' '--in-container requires the controlled compatibility image; no host provisioning attempted' >&2
	exit 2
fi
((${#cells[@]})) || { echo 'at least one cell is required' >&2; exit 2; }

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
pinval_platform() { pin_platform_field "$pin" "$platform" "$1"; }

# Report candidates, not measurement readiness: a writable ext4 directory
# does not establish where a runner's libraries, data or profiles will live.
# Raw findmnt mount targets use hex escapes. Check each target with -T too:
# another filesystem may be mounted over an ext4 target.
report_ext4() {
	local mounts path fs found=0 unknown=0
	local -a candidates=("$repo" "$HOME" "${TMPDIR:-/tmp}" "$cache")
	local -A seen=()
	if ! command -v findmnt >/dev/null 2>&1; then
		printf 'ext4 paths: unknown (findmnt unavailable); measurement gates require a verified ext4 path.\n'
		return 0
	fi
	if mounts=$(findmnt --raw --noheadings --output TARGET,FSTYPE 2>/dev/null); then
		while read -r path fs; do
			[[ $fs == ext4 ]] || continue
			printf -v path '%b' "$path"
			candidates+=("$path")
		done <<<"$mounts"
	else
		unknown=1
	fi
	for path in "${candidates[@]}"; do
		[[ -d $path && -r $path && -w $path && -x $path ]] || continue
		[[ ! ${seen[$path]+yes} ]] || continue
		seen[$path]=1
		if ! fs=$(findmnt --noheadings --output FSTYPE --target "$path" 2>/dev/null); then
			unknown=1
			continue
		fi
		[[ $fs == ext4 ]] || continue
		printf 'ext4 path (readable, writable, searchable): %s\n' "$path"
		found=1
	done
	if ((found)); then
		printf 'Measurement paths still need individual checks with findmnt -T <path>: libraries, data, profiles, work directory and TMPDIR.\n'
	elif ((unknown)); then
		printf 'ext4 paths: unknown (mount detection failed); measurement gates require a verified ext4 path.\n'
	else
		printf 'ext4 paths: none accessible among mount targets, checkout, HOME, TMPDIR and cache; measurement gates cannot run without an ext4 path.\n'
	fi
	printf 'This report does not verify all measured files are on ext4; non-measurement setup may continue.\n'
}

# version_at_least TEXT MINIMUM: the first dotted version in TEXT (major.minor
# or major.minor.patch) is MINIMUM or newer. Numbers only, for tools whose
# versions are not semantic (Valgrind: valgrind-3.22.0, and valgrind-3.22.0.GIT
# for a git build): a development build counts as its number, as it does in
# tools/dev/doctor.py. Plain Bash, because nothing it checks has been installed
# yet when it first runs.
version_at_least() {
	local text=$1 minimum=$2 i
	local -a found needed
	[[ $text =~ ([0-9]+)\.([0-9]+)(\.([0-9]+))? ]] || return 1
	found=("$((10#${BASH_REMATCH[1]}))" "$((10#${BASH_REMATCH[2]}))" "$((10#${BASH_REMATCH[4]:-0}))")
	IFS=. read -r -a needed <<<"$minimum"
	for i in 0 1 2; do
		((found[i] == ${needed[i]:-0})) && continue
		((found[i] > ${needed[i]:-0})) && return 0
		return 1
	done
	return 0
}

# semver_at_least TEXT MINIMUM: the semantic version after the program name in
# TEXT (`uv 0.13.1 (...)`: MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]) is MINIMUM, a
# plain MAJOR.MINOR.PATCH, or newer. A pre-release is older than the release it
# precedes (0.13.1-beta.1 is below 0.13.1) and newer than every lower release
# (0.14.0-beta.1 is above 0.13.1); build metadata does not count. Text that is
# not a semantic version is not at the minimum.
semver_at_least() {
	local text=$1 minimum=$2 prerelease i
	local -a found needed
	[[ $text =~ ^[^[:space:]]+[[:space:]]+([0-9]+)\.([0-9]+)\.([0-9]+)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?([[:space:]]|$) ]] || return 1
	found=("$((10#${BASH_REMATCH[1]}))" "$((10#${BASH_REMATCH[2]}))" "$((10#${BASH_REMATCH[3]}))")
	prerelease=${BASH_REMATCH[4]}
	IFS=. read -r -a needed <<<"$minimum"
	for i in 0 1 2; do
		((found[i] == needed[i])) && continue
		((found[i] > needed[i])) && return 0
		return 1
	done
	[[ -z $prerelease ]]
}

# ---- 0a. uv and the managed interpreter ---------------------------------
# Every repository-owned Python call uses the interpreter .python-version names,
# the one place its version is stated. tools/install-python.sh installs it (uv has
# no +gil archive, so the install request is the file's without +gil).
python_request=$(<"$repo/.python-version")
for name in UV_PYTHON PYTHONHOME PYTHONPATH; do
	if [[ -n ${!name:-} ]]; then
		die "$name is not supported; unset it before running repository Python"
	fi
done

# Fail before Python downloads or oracle builds. rustup run uses only an
# installed toolchain (unlike invoking a proxy, which may install one).
# Python-only tooling deliberately has no Rust or Valgrind prerequisite.
if ((!python_only)); then
	command -v cargo >/dev/null 2>&1 ||
		die 'cargo is missing; install the rustup-managed toolchain from rust-toolchain.toml (docs/runbooks/setup.md)'
	command -v rustup >/dev/null 2>&1 ||
		die 'rustup is missing; install the repository toolchain via docs/runbooks/setup.md'
	toolchain=$(sed -n 's/^channel = "\([^"]*\)"/\1/p' "$repo/rust-toolchain.toml")
	[[ -n $toolchain ]] || die 'no channel in rust-toolchain.toml'
	for tool in rustc cargo rustfmt clippy-driver; do
		rustup run "$toolchain" "$tool" --version >/dev/null 2>&1 ||
			die "Rust toolchain $toolchain is not installed; run: rustup toolchain install $toolchain --component clippy --component rustfmt"
	done
	if ((!inside)); then
		# Install (or upgrade through the distribution) only a Valgrind that is
		# missing or older than its minimum.
		valgrind_found() { valgrind --version 2>/dev/null; }
		if ! version_at_least "$(valgrind_found)" "$valgrind_minimum"; then
			if valgrind_found >/dev/null; then
				valgrind_state="$(valgrind_found) is older than $valgrind_minimum"
				valgrind_note="outdated: $(valgrind_found) (minimum $valgrind_minimum)"
			else
				valgrind_state='valgrind is missing'
				valgrind_note='missing: valgrind'
			fi
			sudo_cmd=()
			if ((EUID != 0)); then
				if ! command -v sudo >/dev/null 2>&1; then
					printf '%s\nrun as root: apt-get install -y valgrind\n' "$valgrind_note" >&2
					die "$valgrind_state and neither root nor sudo is available"
				fi
				sudo_cmd=(sudo)
			fi
			command -v apt-get >/dev/null 2>&1 ||
				die "$valgrind_state and apt-get is unavailable; ask the environment owner to install Valgrind $valgrind_minimum or newer"
			log 'installing valgrind from the distribution archive'
			"${sudo_cmd[@]}" apt-get install -y valgrind >/dev/null 2>&1 ||
				{ "${sudo_cmd[@]}" apt-get update >/dev/null 2>&1 &&
					"${sudo_cmd[@]}" apt-get install -y valgrind >/dev/null; } ||
				die 'could not install valgrind with apt-get'
			version_at_least "$(valgrind_found)" "$valgrind_minimum" ||
				die "valgrind $valgrind_minimum or newer required; the distribution archive provides: $(valgrind_found || echo none)"
		fi
		valgrind_version=$(valgrind --version)
	fi
fi

# uv.toml sets the floor (required-version = ">=0.13.1"). Keep a uv at or
# above it; install the checksummed 0.13.1 only when uv is missing or older.
# uv prints a semantic version, so a pre-release of the floor (0.13.1-beta.1) is older.
uv_at_least_floor() {
	local version
	version=$(uv --version 2>/dev/null) || return 1
	[[ $version == uv\ * ]] && semver_at_least "$version" "$uv_minimum"
}

uv_install_dir=${UV_INSTALL_DIR:-$HOME/.local/bin}
if ! command -v uv >/dev/null 2>&1 || ! uv_at_least_floor ||
	[[ -n ${UV_INSTALL_DIR:-} && $(command -v uv) != "$uv_install_dir/uv" ]]; then
	# Probe the actual destination before fetching the archive. This catches
	# read-only cloud mounts as well as ordinary directory permissions.
	uv_write_error="uv installation directory is not writable: $uv_install_dir; set UV_INSTALL_DIR to a writable task-owned directory"
	mkdir -p "$uv_install_dir" 2>/dev/null || die "$uv_write_error"
	uv_probe=$(mktemp "$uv_install_dir/.oxpinyin-write.XXXXXX" 2>/dev/null) || die "$uv_write_error"
	rm -f "$uv_probe"
	for tool in uv uvx; do
		[[ ! -e $uv_install_dir/$tool || -w $uv_install_dir/$tool ]] || die "$uv_write_error"
	done
	command -v curl >/dev/null 2>&1 || die 'installing uv needs curl; ask the environment owner to install it'
	log 'installing uv 0.13.1'
	(
		# Published in https://github.com/astral-sh/uv/releases/download/0.13.1/sha256.sum.
		# Exact on purpose (#1074): a checksum is only known for a known version.
		case $(uname -m) in
		x86_64)
			uv_target=x86_64-unknown-linux-gnu
			uv_sha256=eadd9d27c134ab5ac1ffd4473b97a741407f3ea238288f03121fc4deddf90220
			;;
		aarch64 | arm64)
			uv_target=aarch64-unknown-linux-gnu
			uv_sha256=9041c73c99fdc5892cf128140beb354eca9231d502068cc36e6cb53a8e59ad14
			;;
		*)
			printf 'no uv target for architecture %s\n' "$(uname -m)" >&2
			exit 1
			;;
		esac
		# Keep the archive in memory. Write only its verified executables.
		uv_payload=$(curl --fail --location --retry 3 "https://github.com/astral-sh/uv/releases/download/0.13.1/uv-$uv_target.tar.gz" | base64) ||
			die 'uv download failed; allow github.com and release-assets.githubusercontent.com (docs/runbooks/cloud.md)'
		uv_actual=$(printf '%s' "$uv_payload" | base64 -d | sha256sum)
		[[ ${uv_actual%% *} == "$uv_sha256" ]] || { printf 'uv archive checksum mismatch\n' >&2; exit 1; }
		uv_tmp=$(mktemp -d)
		trap 'rm -rf "$uv_tmp"' EXIT
		trap 'exit 1' HUP INT TERM
		printf '%s' "$uv_payload" | base64 -d | tar -xz -C "$uv_tmp" "uv-$uv_target/uv" "uv-$uv_target/uvx"
		mkdir -p "$uv_install_dir"
		install -m755 "$uv_tmp/uv-$uv_target/uv" "$uv_tmp/uv-$uv_target/uvx" "$uv_install_dir/"
		printf 'uv archive SHA-256 verified: %s\n' "$uv_sha256"
	)
	export PATH="$uv_install_dir:$PATH"
	hash -r
else
	log "using $(uv --version)"
fi
uv_at_least_floor ||
	die "uv $uv_minimum or newer required; found: $(uv --version 2>/dev/null || echo missing)"
# shellcheck disable=SC2034  # read by tools/cloud/env.sh's environment writer.
uv_dir=$(dirname "$(command -v uv)")
# Keep a managed interpreter that is already there; install one only when none is.
"$repo/tools/install-python.sh" ||
	die 'managed Python provisioning failed; check writable UV_PYTHON_INSTALL_DIR/UV_CACHE_DIR and access to github.com and release-assets.githubusercontent.com (docs/runbooks/cloud.md)'
uv run --no-project --managed-python --python "$python_request" python -X lazy_imports=normal - "$python_request" <<'PYTHON'
from pathlib import Path
import subprocess
import sys
import sysconfig
assert sys.implementation.name == "cpython"
assert sys.version_info.releaselevel == "final" and (3, 15, 0) <= sys.version_info[:3] < (3, 16, 0)
assert sysconfig.get_config_var("Py_GIL_DISABLED") == 0
assert sys._is_gil_enabled()
managed = subprocess.check_output(
    ["uv", "python", "find", "--managed-python", sys.argv[1]],
    text=True).strip()
assert Path(sys.executable).resolve() == Path(managed).resolve()
print("Verified managed CPython", sys.version.split()[0], "GIL build:", Path(managed).resolve())
PYTHON

if ((python_only)); then
	report_ext4
	log 'stopping after Python provisioning (--python-only)'
	exit 0
fi

# ---- 0. the proxy CA ----------------------------------------------------

# An ordinary proxy needs no custom CA. A TLS-intercepting one does, and
# containers do not inherit the host's trust, so the CA is chosen here, before
# any download or build: an explicit OXPINYIN_PROXY_CA, else the session's
# bundle, else (proxy set) the CA the host already trusts, else a failure that
# names OXPINYIN_PROXY_CA. Verification is never disabled.
proxy=${HTTPS_PROXY:-${https_proxy:-}}
custom_ca=0
if [[ -n ${OXPINYIN_PROXY_CA:-} ]]; then
	[[ -s $proxy_ca ]] || die "OXPINYIN_PROXY_CA bundle is missing: $proxy_ca"
	custom_ca=1
	log "proxy CA: $proxy_ca (OXPINYIN_PROXY_CA)"
elif [[ -s $proxy_ca ]]; then
	custom_ca=1
	log "proxy CA: $proxy_ca (session bundle)"
elif [[ -n $proxy ]] && ((!inside)); then
	# OXPINYIN_SYSTEM_CA_BUNDLES is a test-only override (see the header).
	IFS=: read -r -a system_bundles <<<"${OXPINYIN_SYSTEM_CA_BUNDLES-/etc/ssl/certs/ca-certificates.crt:/etc/pki/tls/certs/ca-bundle.crt:/etc/ssl/cert.pem}"
	labels=(SSL_CERT_FILE CURL_CA_BUNDLE)
	paths=("${SSL_CERT_FILE:-}" "${CURL_CA_BUNDLE:-}")
	for bundle in "${system_bundles[@]}"; do
		labels+=("system bundle")
		paths+=("$bundle")
	done
	for i in "${!paths[@]}"; do
		if [[ -n ${paths[i]} && -s ${paths[i]} ]]; then
			proxy_ca=${paths[i]}
			custom_ca=1
			log "proxy CA: HTTPS_PROXY is set and $ccr_dir/ca-bundle.crt is absent; using the host-trusted ${labels[i]}: $proxy_ca"
			log "proxy CA: a present, non-empty bundle does not show that the proxy's interception certificate is trusted; if TLS then fails, set OXPINYIN_PROXY_CA to the proxy's CA bundle"
			break
		fi
	done
	((custom_ca)) || die "HTTPS_PROXY is set but there is no proxy CA bundle: $ccr_dir/ca-bundle.crt is absent and the host trusts none (SSL_CERT_FILE, CURL_CA_BUNDLE, system bundle); set OXPINYIN_PROXY_CA to the proxy's CA bundle"
fi

# Containers mount a private copy of the selected bundle, never the host's own
# file: the shared SELinux relabel (`z`) rewrites the label of whatever it
# mounts, and a system CA file is not setup's to relabel. The copy lives in
# this run's temporary directory, mode 0600, and is removed on exit.
ca_mount=$proxy_ca
ca_tmp=
if ((custom_ca && !inside)); then
	ca_tmp=$(mktemp -d)
	trap 'rm -rf "$ca_tmp"' EXIT
	trap 'exit 1' HUP INT TERM
	ca_mount=$ca_tmp/ca.crt
	(umask 077 && cat "$proxy_ca" >"$ca_mount") || die "could not copy the proxy CA bundle: $proxy_ca"
	log "proxy CA: containers mount a private copy at $ca_mount (mode 0600, removed on exit)"
fi

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
model_cache=${PINYIN_MODEL_CACHE:-$cache/model20}
mkdir -p "$model_cache"
model_cache=$(cd "$model_cache" && pwd)
oracle_root=$cache/oracle
mkdir -p "$oracle_root"

base_image=$(pinval base_image)
[[ -n $base_image ]] || die "no base_image in $pin"
# The published tag is derived from the whole recipe by one script, the same
# one the workflow and docs/runbooks/oracle.md use. The amd64 tags are the
# historical names; arm64 publishes sibling -arm64 tags.
published_base=ghcr.io/shenghaoc/oxpinyin-oracle:$(uv run --no-project --managed-python --python "$python_request" python -X lazy_imports=normal "$repo/tools/dev/__main__.py" compat image-tag)
case $platform in
linux/amd64)
	published=$published_base
	published_deps=$published_base-deps
	;;
linux/arm64)
	published=$published_base-arm64
	published_deps=$published_base-deps-arm64
	;;
*)
	die "no published oracle tag scheme for $platform"
	;;
esac
# The key also hashes oracle-pin.txt: when a rolling trixie update changes the
# recorded library hashes, a deps image cached from the older packages must
# not be reused.
deps_image=oxpinyin-oracle-deps:$(
	cat "$repo/tools/oracle/Dockerfile.oracle" "$repo/tools/oracle/versions.py" \
		"$repo/tools/oracle/version-manifest.json" "$repo/tools/tkrzw/distro-probe.sh" \
		"$pin" | sha256sum | cut -c1-12
)

if ((!inside)); then
	# Python has been provisioned above; preserve the selected runtime in this shell.
	eval "$(uv run --no-project --managed-python --python "$python_request" python -X lazy_imports=normal "$repo/tools/dev/__main__.py" compat select-runtime --shell "${OXPINYIN_CONTAINER:-}")"
	# shellcheck disable=SC2154  # emitted by the shared runtime selector.
	[[ $(basename "$ctr") != container ]] || die 'normal cloud provisioning requires a Linux cloud runtime; use compat.sh for the Apple adapter'
else
	printf 'Runtime: supplied compatibility container (no nested runtime)\n'
fi

# ---- 0b. backend dump utilities -----------------------------------------

# contract-diff.py and the requested-row training gates call db5.3_dump
# (bdb) and kchashmgr (kc); tkrzw_dbm_util (tkrzw) comes with the tkrzw
# step below. Root containers install them explicitly; anyone else
# gets the exact command; this backend-utility step never runs sudo.
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

# Resolve model access before container pulls or library builds. Replacing
# invalid libraries also replaces their data directory. The existing fetcher
# accepts its verified cache/local archive without network access.
# shellcheck source=env.sh
source "$repo/tools/cloud/env.sh"
for dbm in "${cells[@]}"; do
	if ! libs_ok "$dbm" || ! data_ok "$dbm"; then
		"$repo/tools/model/fetch-model.sh" --cache-dir "$model_cache" >/dev/null ||
			die 'model20 is unavailable; allow downloads.sourceforge.net and its *.dl.sourceforge.net redirect target, or supply a verified local model cache (docs/runbooks/cloud.md)'
		break
	fi
done

# The compatibility launcher supplies a controlled image and Linux volume.
# This mode provisions and executes without a nested runtime or host libraries.
if ((inside)); then
	[[ ${OXPINYIN_COMPAT_CONTAINER:-} == 1 && -d /opt/reference ]] || die '--in-container requires the compatibility image'
	ln -s "$oracle_root" /opt/pinyin-oracle
	ln -s "$model_cache" /model-cache
else
# ---- 1. container runtime -----------------------------------------------

# $OXPINYIN_CONTAINER, else podman, else docker. Every container call below
# goes through $ctr; only docker needs a daemon, and podman runs rootless.
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
	for ((attempt = 1; attempt <= 60; attempt++)); do
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
	)
	if ((custom_ca)); then
		env_flags+=(-e GIT_SSL_CAINFO=/run/ca.crt -e SSL_CERT_FILE=/run/ca.crt
			-e CURL_CA_BUNDLE=/run/ca.crt -v "$ca_mount:/run/ca.crt:ro,z")
	fi
fi

fi

# ---- 3. oracle libraries ------------------------------------------------

if ((inside)); then
	for dbm in "${cells[@]}"; do
		if ! libs_ok "$dbm"; then
			rm -rf "${oracle_root:?}/$dbm"
			cp -a "/opt/reference/$dbm" "$oracle_root/$dbm"
		fi
		if ! libs_ok "$dbm"; then
			for lib in libpinyin libzhuyin; do
				printf 'MISMATCH %s %s\n  recorded: %s\n  reference: %s\n' "$dbm" "$lib" \
					"$(pinval_platform "${lib}_sha256_$dbm")" \
					"$(sha256sum "$oracle_root/$dbm/lib/$lib.so" | cut -d' ' -f1)" >&2
			done
			exit 3
		fi
	done
fi

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
	((inside)) && return 0
	"$ctr" image inspect "$deps_image" >/dev/null 2>&1 && return 0
	# Same deps stage as the oracle recipe, published by the normal image workflow.
	# Pulling it avoids a cold package install on data regeneration.
	if ((pull)); then
		if "$ctr" pull --quiet --platform "$platform" "$published_deps" > /dev/null 2>"$cache/pull-deps.err"; then
			"$ctr" tag "$published_deps" "$deps_image"
			return 0
		fi
		log "recorded deps image unavailable; building locally (details: $cache/pull-deps.err)"
		tail -n 5 "$cache/pull-deps.err" >&2
	fi
	if ! "$ctr" image inspect "$base_image" >/dev/null 2>&1; then
		log "pulling $base_image for the local fallback build"
		retry "$ctr" pull --quiet --platform "$platform" "$base_image" >/dev/null
	fi
	log "building $deps_image (the trixie pin-reference toolchain)"
	local flags=("${net_flags[@]}")
	[[ $platform == linux/amd64 ]] || flags+=(--build-arg "ORACLE_PLATFORM=$platform")
	if [[ -n $proxy ]]; then
		flags+=(--build-arg "https_proxy=$proxy" --build-arg "HTTPS_PROXY=$proxy")
		if ((custom_ca)); then flags+=(--secret "id=cacert,src=$proxy_ca"); fi
	fi
	DOCKER_BUILDKIT=1 "$ctr" build --platform "$platform" "${flags[@]}" --target deps \
		-f "$repo/tools/oracle/Dockerfile.oracle" -t "$deps_image" "$repo" >&2
}

# in_image_cmd ARGS...: set in_image_argv to the command that runs build-oracle.sh
# in the pin-reference image. The oracle root appears at /opt/pinyin-oracle, the
# path the published image uses, so a prefix means the same thing wherever it was
# produced. Every mount carries the shared SELinux relabel `z` (not the private
# `Z`): later runs mount the same paths again, and on a host without SELinux the
# option does nothing. in_image_flags adds container flags.
in_image_flags=()
in_image_cmd() {
	if ((inside)); then
		in_image_argv=("$repo/tools/oracle/build-oracle.sh" "$@")
		return
	fi
	in_image_argv=("$ctr" run --rm --platform "$platform" "${net_flags[@]}" "${env_flags[@]}" "${in_image_flags[@]}"
		-v "$repo/tools/oracle/build-oracle.sh:/usr/local/bin/build-oracle.sh:ro,z"
		-v "$repo/tools/oracle/oracle-pin.txt:/usr/local/bin/oracle-pin.txt:ro,z"
		-v "$oracle_root:/opt/pinyin-oracle:z"
		-v "$model_cache:/model-cache:ro,z"
		"$deps_image" build-oracle.sh "$@")
}
in_image() {
	in_image_cmd "$@"
	"${in_image_argv[@]}"
}

# pool_phase LABEL NAMES LOG ARGS...: run build-oracle.sh ARGS once per cell, the
# cells at once under the heavy slot (#1233). tools/dev/pool.py plans the overlap
# from the budget the slot admits, so workers x make jobs stays within the CPUs
# and memory; {item}, {jobs} in ARGS, LOG and the container flags are filled in
# per cell. Each container carries this invocation's random {token} in its name
# and label and records its ID in {cidfile}; a failed cell, or a signal, stops
# the rest and the pool removes only the containers it recorded and labelled.
pool_phase() {
	local label=$1 names=$2 log=$3
	local -a owner=()
	shift 3
	in_image_flags=()
	if ((!inside)); then
		in_image_flags=(--name "oxpinyin-setup-{token}-$label-{item}" --label 'oxpinyin.setup={token}'
			--cidfile '{cidfile}' --cpus '{jobs}')
		owner=(--container-runtime "$(printf '%q' "$ctr")" --container-label oxpinyin.setup)
	fi
	in_image_cmd "$@"
	(cd "$repo" && uv run --no-project --managed-python python -X lazy_imports=normal \
		-m tools.dev pool run --label "$label" --items "$names" --cpus "$jobs" --log "$log" "${owner[@]}" \
		-- "${in_image_argv[@]}")
}
csv() { local IFS=,; printf '%s' "$*"; }

mkdir -p "$model_cache"

if ((${#need[@]})); then
	build_deps_image
	for dbm in "${need[@]}"; do rm -rf "${oracle_root:?}/$dbm"; done
	pool_phase libraries "$(csv "${need[@]}")" "$cache/build-{item}.log" \
		--libs-only --enable-libzhuyin --dbm '{item}' --work-dir '/build/{item}' \
		--prefix '/opt/pinyin-oracle/{item}' --jobs '{jobs}' ||
		die "the library build failed or was cancelled (logs: $cache/build-<cell>.log)"
fi

status=0
for dbm in "${cells[@]}"; do
	for lib in libpinyin libzhuyin; do
		want=$(pinval_platform "${lib}_sha256_$dbm")
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
		# shellcheck disable=SC2034  # read by tools/cloud/env.sh's environment writer.
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
				# Intentional: diagnose the newly built cache in isolation.
				PATH=$tkrzw_prefix/bin:$PATH LD_LIBRARY_PATH=$tkrzw_prefix/lib sh "$probe" >&2 || true
				die "the built tkrzw does not print 'RESULT : healthy' (refusing to use it)"
			fi
			printf '%s\n' "$tkrzw_rev" >"$tkrzw_prefix/REV"
		fi
		# Both sides must resolve libtkrzw.so.1 to the cache: the oracle
		# library here, an oxpinyin tkrzw binary in docs/runbooks/cloud.md.
		# Intentional: verify this cache in isolation from inherited runtimes.
		resolved=$(LD_LIBRARY_PATH=$tkrzw_prefix/lib tkrzw_so "$oracle_root/tkrzw/lib/libpinyin.so")
		[[ $resolved == "$tkrzw_prefix/lib/libtkrzw.so.1" ]] ||
			die "the tkrzw oracle libpinyin.so resolves libtkrzw.so.1 to '${resolved:-nothing}', not $tkrzw_prefix/lib"
		log "tkrzw: oracle libpinyin.so resolves libtkrzw.so.1 to $resolved"
	fi
fi

# ---- 4. model20 and the data --------------------------------------------

data_need=()
for dbm in "${cells[@]}"; do
	if data_ok "$dbm"; then
		log "$dbm: data already matches the recorded manifest"
	else
		data_need+=("$dbm")
	fi
done

if ((${#data_need[@]})); then
	log "fetching model20 for ${data_need[*]}"
	"$repo/tools/model/fetch-model.sh" --cache-dir "$model_cache" >/dev/null
	build_deps_image
	pool_phase data "$(csv "${data_need[@]}")" "$cache/data-{item}.log" \
		--regen-data --dbm '{item}' --prefix '/opt/pinyin-oracle/{item}' \
		--model-dir /model-cache/extracted --work-dir '/build/{item}-data' --jobs '{jobs}' ||
		die "--regen-data failed or was cancelled (logs: $cache/data-<cell>.log)"
	for dbm in "${data_need[@]}"; do
		data_ok "$dbm" || die "$dbm: the regenerated data does not match the recorded manifest"
	done
fi

# ---- 5. report ----------------------------------------------------------

report_ext4
if ((!inside)); then printf 'Valgrind: %s\n' "$valgrind_version"; fi

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
write_oracle_env
