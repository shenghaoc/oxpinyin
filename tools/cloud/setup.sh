#!/usr/bin/env bash
# Bootstrap: uv and the managed interpreter must exist before tools.dev.provision.

# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"
set -euo pipefail
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
uv_minimum=0.13.1
SECONDS=0
log() { printf '[%4ds] %s\n' "$SECONDS" "$*" >&2; }
die() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
python_request=$(<"$repo/.python-version")
for name in UV_PYTHON PYTHONHOME PYTHONPATH; do
    [[ -z ${!name:-} ]] || die "$name is not supported; unset it before running repository Python"
done
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

# uv.toml sets the floor (required-version = ">=0.13.1"). Keep a uv at or
# above it; install the checksummed 0.13.1 only when uv is missing or older.
# uv prints a semantic version, so a pre-release of the floor (0.13.1-beta.1) is older.
uv_at_least_floor() {
	local version
	version=$(uv --version 2>/dev/null) || return 1
	[[ $version == uv\ * ]] && semver_at_least "$version" "$uv_minimum"
}

uv_install_dir=${UV_INSTALL_DIR:-$HOME/.local/bin}
kept_uv=1
if ! command -v uv >/dev/null 2>&1 || ! uv_at_least_floor ||
	[[ -n ${UV_INSTALL_DIR:-} && $(command -v uv) != "$uv_install_dir/uv" ]]; then
	kept_uv=0
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
fi
uv_at_least_floor ||
	die "uv $uv_minimum or newer required; found: $(uv --version 2>/dev/null || echo missing)"
# Keep a managed interpreter that is already there; install one only when none is.
if ! "$repo/tools/install-python.sh"; then
	((kept_uv)) && log "using $(uv --version)"
	die 'managed Python provisioning failed; check writable UV_PYTHON_INSTALL_DIR/UV_CACHE_DIR and access to github.com and release-assets.githubusercontent.com (docs/runbooks/cloud.md)'
fi
OXPINYIN_CLOUD_BOOTSTRAP_SECONDS=$SECONDS OXPINYIN_CLOUD_BOOTSTRAP_KEPT_UV=$kept_uv \
exec uv run --no-project --managed-python --python "$python_request" python -X lazy_imports=normal \
    "$repo/tools/dev/__main__.py" provision "$@"
