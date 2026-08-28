#!/usr/bin/env bash
set -euo pipefail

# Fetch the checksum-pinned libpinyin model archive into a build cache.
#
# Mirrors the fetch used by tools/oracle/build-oracle.sh (curl, SHA-256, tar)
# but writes only to an ignored cache: the archive, its extraction, and any
# converted form must not enter the repository, a release, or an installer.
# See docs/findings/model-provenance.md.
#
# This script does not feed frequencies into the language model.
#
# Pinned identity (do not alter; from model-provenance.md):
#   URL     https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz
#   SHA-256 59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155
#
# Network: downloads.sourceforge.net (HTTPS, SourceForge may redirect to a
# mirror). Environments that run this script must allow that host; there is
# no vendored copy and no alternate URL.
#
# No mirror: docs/findings/model-provenance.md classifies the model20 archive
# (interpolation2.text + the .table files) as NOT redistributable, so this
# script never points at a project-owned release asset or any copy the project
# publishes. This is a LOCAL-ONLY workflow: no CI job downloads the archive —
# a developer machine fetches it once into the build cache below, verified
# against MODEL_SHA256, and build-oracle.sh reuses that cache via --model-dir
# so subsequent local oracle rebuilds never re-fetch.
#
# Default cache: <repo>/target/model20/
#   downloads/model20.text.tar.gz
#   extracted/{interpolation2.text + seventeen .table files}
#
# The last stdout line is the absolute path of the extracted directory.

MODEL_URL=https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz
MODEL_SHA256=59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155
ARCHIVE_NAME=model20.text.tar.gz

# Inventory from docs/findings/model-provenance.md (eighteen files).
EXPECTED_FILES=(
	art.table
	culture.table
	economy.table
	gb_char.table
	gbk_char.table
	geology.table
	history.table
	interpolation2.text
	life.table
	merged.table
	nature.table
	opengram.table
	people.table
	punct.table
	science.table
	society.table
	sport.table
	technology.table
)

script_dir=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)

cache_dir=${PINYIN_MODEL_CACHE:-"$repo_root/target/model20"}
url=${PINYIN_MODEL_URL:-$MODEL_URL}
local_archive=
force=0

usage() {
	cat <<'EOF'
Usage: fetch-model.sh [OPTIONS]

Download the checksum-pinned model20.text.tar.gz, verify SHA-256, and
extract interpolation2.text plus the seventeen .table files into a build
cache. Idempotent: a verified extraction is left untouched.

Options:
  --cache-dir DIR  Build cache (default: <repo>/target/model20, or
                   $PINYIN_MODEL_CACHE)
  --url URL        Archive URL (default: the pinned SourceForge URL, or
                   $PINYIN_MODEL_URL)
  --archive PATH   Use a local archive instead of downloading
  --force          Re-download (if needed) and re-extract even if verified
  -h, --help       Show this help

The cache must be a gitignored path inside the work tree, or a path
outside the repository. The last stdout line is the extracted directory.
EOF
}

while (($#)); do
	case $1 in
	--cache-dir)
		cache_dir=$2
		shift 2
		;;
	--url)
		url=$2
		shift 2
		;;
	--archive)
		local_archive=$2
		shift 2
		;;
	--force)
		force=1
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	*)
		printf 'unknown option: %s\n' "$1" >&2
		usage >&2
		exit 2
		;;
	esac
done

require_command() {
	command -v "$1" >/dev/null 2>&1 || {
		printf 'required command not found: %s\n' "$1" >&2
		exit 1
	}
}

require_command tar
if [[ -z $local_archive ]]; then
	require_command curl
fi
if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
	printf 'required command not found: sha256sum or shasum\n' >&2
	exit 1
fi

abs_path() {
	case $1 in
	/*) printf '%s\n' "$1" ;;
	*) printf '%s/%s\n' "$PWD" "$1" ;;
	esac
}

cache_dir=$(abs_path "$cache_dir")
repo_root=$(abs_path "$repo_root")
extracted_dir=$cache_dir/extracted
downloads_dir=$cache_dir/downloads
marker=$cache_dir/verified

# Refuse to write model bytes where git would track them.
case $cache_dir in
"$repo_root" | "$repo_root"/*)
	if (cd "$repo_root" && git rev-parse --is-inside-work-tree >/dev/null 2>&1); then
		if ! (cd "$repo_root" && git check-ignore -q -- "$cache_dir"); then
			printf 'refusing to write model bytes to a path git would track: %s\n' "$cache_dir" >&2
			printf 'use a build cache under target/ (the default) or another ignored path\n' >&2
			exit 1
		fi
	else
		case $cache_dir in
		"$repo_root/target" | "$repo_root/target"/*) ;;
		*)
			printf 'refusing to write model bytes under the repository without git ignore checks: %s\n' "$cache_dir" >&2
			exit 1
			;;
		esac
	fi
	;;
esac

file_sha256() {
	local path=$1 sum
	if command -v sha256sum >/dev/null 2>&1; then
		sum=$(sha256sum -- "$path" | awk '{print $1}')
	else
		sum=$(shasum -a 256 -- "$path" | awk '{print $1}')
	fi
	printf '%s\n' "$sum"
}

extraction_complete() {
	local dir=$1 name
	[[ -d $dir ]] || return 1
	for name in "${EXPECTED_FILES[@]}"; do
		[[ -f $dir/$name ]] || return 1
	done
	return 0
}

marker_matches() {
	[[ -f $marker ]] || return 1
	grep -qx "sha256=$MODEL_SHA256" "$marker"
}

if ((force == 0)) && extraction_complete "$extracted_dir" && marker_matches; then
	printf 'verified model cache present; skipping fetch\n' >&2
	printf '%s\n' "$extracted_dir"
	exit 0
fi

if [[ -n $local_archive ]]; then
	[[ -f $local_archive ]] || {
		printf 'local archive is not a file: %s\n' "$local_archive" >&2
		exit 1
	}
	src=$(abs_path "$local_archive")
else
	mkdir -p "$downloads_dir"
	src=$downloads_dir/$ARCHIVE_NAME
	if ((force == 1)) && [[ -f $src ]]; then
		rm -f -- "$src"
	fi
	if [[ ! -f $src ]]; then
		printf 'downloading %s\n' "$url" >&2
		if ! curl --fail --location --retry 3 --connect-timeout 30 --max-time 600 \
			--show-error --output "$src.part" "$url"; then
			rm -f -- "$src.part"
			printf 'failed to download %s from %s\n' "$ARCHIVE_NAME" "$url" >&2
			exit 1
		fi
		mv -- "$src.part" "$src"
	fi
fi

actual=$(file_sha256 "$src")
if [[ $actual != "$MODEL_SHA256" ]]; then
	printf 'checksum mismatch: %s\n  expected: %s\n  actual:   %s\n' \
		"$ARCHIVE_NAME" "$MODEL_SHA256" "$actual" >&2
	exit 1
fi

mkdir -p "$cache_dir"
tmp=$(mktemp -d "$cache_dir/.extract.XXXXXX")
if ! tar -xzf "$src" -C "$tmp"; then
	rm -rf -- "$tmp"
	printf 'failed to extract %s\n' "$src" >&2
	exit 1
fi

missing=0
for name in "${EXPECTED_FILES[@]}"; do
	if [[ ! -f $tmp/$name ]]; then
		printf 'extracted archive is missing %s\n' "$name" >&2
		missing=1
	fi
done
if ((missing)); then
	rm -rf -- "$tmp"
	exit 1
fi

rm -rf -- "$extracted_dir"
mv -- "$tmp" "$extracted_dir"

cat >"$marker" <<EOF
schema=pinyin-model-cache-v1
url=$MODEL_URL
sha256=$MODEL_SHA256
EOF

printf '%s\n' "$extracted_dir"
