#!/usr/bin/env bash
set -euo pipefail

# Build dependencies: autotools (autoconf, automake, autopoint, libtool),
# a C/C++ toolchain and make, pkg-config, gettext, gnome-common, curl,
# tar, python3, and development headers for GLib 2.0, IBus 1.0,
# Tkrzw and SQLite 3.

LIBPINYIN_TAG=2.11.91
LIBPINYIN_SHA=0c5e80e1200f84fab185d1c5bde458b770a0636c
LIBPINYIN_URL=https://codeload.github.com/libpinyin/libpinyin/tar.gz/refs/tags/2.11.91
LIBPINYIN_ARCHIVE_SHA256=eb25890dab0072eb0744c9ee1bc152051143b7bc23aea2a424792a9b1b84bdcb
IBUS_LIBPINYIN_TAG=1.16.5
IBUS_LIBPINYIN_SHA=2d2cdac0187101aa0cd7ac06694a8340721ddfbb
IBUS_LIBPINYIN_URL=https://codeload.github.com/libpinyin/ibus-libpinyin/tar.gz/refs/tags/1.16.5
IBUS_LIBPINYIN_ARCHIVE_SHA256=ab6d6cc371e4ec0cda1471ef968e9545de69a404958ecfb4e68545ef4b328646
MODEL_URL=https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz
MODEL_SHA256=59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155

work_dir=${TMPDIR:-/tmp}/pinyin-rs-oracle
prefix=
jobs=1

usage() {
	cat <<'EOF'
Usage: build-oracle.sh [OPTIONS]

Build the pinned libpinyin and ibus-libpinyin releases from verified archives.

Options:
  --work-dir DIR  Download and build directory (default: $TMPDIR/pinyin-rs-oracle)
  --prefix DIR    Installation prefix (default: WORK_DIR/prefix)
  --jobs N        Parallel make jobs (default: 1)
  -h, --help      Show this help

Build variables CC, CXX, CFLAGS, CXXFLAGS and LDFLAGS are passed through.
The final stdout line is the absolute path to the built libpinyin shared object.
EOF
}

while (($#)); do
	case $1 in
	--work-dir)
		work_dir=$2
		shift 2
		;;
	--prefix)
		prefix=$2
		shift 2
		;;
	--jobs)
		jobs=$2
		shift 2
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

prefix=${prefix:-"$work_dir/prefix"}

case $jobs in
'' | *[!0-9]* | 0)
	printf '%s\n' '--jobs must be a positive integer' >&2
	exit 2
	;;
esac

for command in curl sha256sum tar autoreconf make pkg-config find; do
	command -v "$command" >/dev/null 2>&1 || {
		printf 'required command not found: %s\n' "$command" >&2
		exit 1
	}
done

if [[ -e $prefix && ! -d $prefix ]]; then
	printf 'installation prefix is not a directory: %s\n' "$prefix" >&2
	exit 1
fi
if [[ -d $prefix && -n $(find "$prefix" -mindepth 1 -print -quit) ]]; then
	printf 'installation prefix must be empty: %s\n' "$prefix" >&2
	exit 1
fi

mkdir -p "$work_dir/downloads" "$work_dir/src" "$prefix"
work_dir=$(cd "$work_dir" && pwd)
prefix=$(cd "$prefix" && pwd)

# Prefer the just-built libraries while retaining pkg-config's system defaults
# for declared build dependencies. Never inherit caller-provided search paths.
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig:$prefix/lib64/pkgconfig"
export LD_LIBRARY_PATH="$prefix/lib:$prefix/lib64"

fetch() {
	local name=$1 url=$2 expected=$3 path=$work_dir/downloads/$1
	if [[ ! -f $path ]]; then
		curl --fail --location --retry 3 --show-error "$url" --output "$path"
	fi
	printf '%s  %s\n' "$expected" "$path" | sha256sum --check --status || {
		printf 'checksum mismatch: %s\n' "$name" >&2
		exit 1
	}
	printf '%s\n' "$path"
}

lib_archive=$(fetch "libpinyin-$LIBPINYIN_TAG.tar.gz" "$LIBPINYIN_URL" "$LIBPINYIN_ARCHIVE_SHA256")
ibus_archive=$(fetch "ibus-libpinyin-$IBUS_LIBPINYIN_TAG.tar.gz" "$IBUS_LIBPINYIN_URL" "$IBUS_LIBPINYIN_ARCHIVE_SHA256")
model_archive=$(fetch model20.text.tar.gz "$MODEL_URL" "$MODEL_SHA256")

rm -rf "$work_dir/src/libpinyin-$LIBPINYIN_TAG" "$work_dir/src/ibus-libpinyin-$IBUS_LIBPINYIN_TAG"
tar -xzf "$lib_archive" -C "$work_dir/src"
tar -xzf "$ibus_archive" -C "$work_dir/src"
tar -xzf "$model_archive" -C "$work_dir/src/libpinyin-$LIBPINYIN_TAG/data"

lib_src=$work_dir/src/libpinyin-$LIBPINYIN_TAG
ibus_src=$work_dir/src/ibus-libpinyin-$IBUS_LIBPINYIN_TAG

(
	cd "$lib_src"
	autoreconf --force --install --verbose
	./configure --prefix="$prefix" --disable-static --with-dbm=Tkrzw
	make -j"$jobs"
	make install
)

(
	cd "$ibus_src"
	autoreconf --force --install --verbose
	./configure --prefix="$prefix" \
		--disable-static \
		--disable-boost \
		--disable-cloud-input-mode \
		--disable-english-input-mode \
		--disable-libnotify \
		--disable-lua-extension
	make -j"$jobs"
)

shared_object=$(find "$prefix" -type f \( -name 'libpinyin.so' -o -name 'libpinyin.so.*' \) -print | sort | head -n 1)
[[ -n $shared_object ]] || {
	printf '%s\n' 'built libpinyin shared object not found' >&2
	exit 1
}

printf 'libpinyin_tag=%s\nlibpinyin_commit=%s\n' "$LIBPINYIN_TAG" "$LIBPINYIN_SHA" >&2
printf 'ibus_libpinyin_tag=%s\nibus_libpinyin_commit=%s\n' "$IBUS_LIBPINYIN_TAG" "$IBUS_LIBPINYIN_SHA" >&2
printf '%s\n' "$shared_object"
