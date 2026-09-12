#!/usr/bin/env bash
set -euo pipefail

# Build dependencies: autotools (autoconf, automake, autopoint, libtool),
# a C/C++ toolchain and make, pkg-config, gettext, gnome-common, curl,
# tar, python3, and development headers for GLib 2.0, IBus 1.0, SQLite 3,
# and the DBM backend selected by --dbm (Tkrzw, Kyoto Cabinet or
# Berkeley DB).

# Both upstreams are fetched by commit SHA and verified by commit SHA. For
# libpinyin this is forced: no release tag carries this pin (2.11.92 is
# untagged upstream, so the former tag-tarball form cannot pin it). For
# ibus-libpinyin a tag exists (1.16.5, a lightweight tag pointing at
# IBUS_LIBPINYIN_SHA), but the two forms pin different things: an archive
# SHA-256 is byte-level identity of one tarball, and GitHub regenerates
# archive tarballs and has changed its compression before, so a still-valid
# pin can start failing closed at the checksum with no upstream change; a
# commit SHA is identity of the git tree itself and is stable across any
# re-serving of it. Verifying the two upstreams the same way also keeps
# the provisioning mirror symmetric (issue #369). The model archive stays
# on its SHA-256: it is a plain file download, not a git tree.
# LIBPINYIN_VERSION is the version configure.ac
# reports; it names the installed header dir and the pin ref, and the
# manifest key keeps its schema name `libpinyin_tag` for compatibility
# with existing manifests.
LIBPINYIN_VERSION=2.11.92
LIBPINYIN_SHA=074a2219c90feaf962d0d24f034514033ece5f99
LIBPINYIN_GIT_URL=https://github.com/libpinyin/libpinyin.git
IBUS_LIBPINYIN_TAG=1.16.5
IBUS_LIBPINYIN_SHA=2d2cdac0187101aa0cd7ac06694a8340721ddfbb
IBUS_LIBPINYIN_GIT_URL=https://github.com/libpinyin/ibus-libpinyin.git
MODEL_URL=https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz
MODEL_SHA256=59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155
# The pinned model20 export inventory (interpolation2.text + seventeen .table
# files). Keep in lockstep with tools/model/fetch-model.sh and
# crates/pinyin-oracle/src/model_cache.rs (EXPECTED_MODEL_FILES).
MODEL_FILES=(
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
work_dir=${TMPDIR:-/tmp}/oxpinyin-oracle
prefix=
jobs=1
apply_patches_dir=
model_dir=
dbm=tkrzw

usage() {
	cat <<'EOF'
Usage: build-oracle.sh [OPTIONS]

Build the pinned libpinyin and the pinned ibus-libpinyin, each fetched by
commit SHA (git fetch --depth=1) and verified by git rev-parse.

Options:
  --work-dir DIR       Download and build directory (default: $TMPDIR/oxpinyin-oracle)
  --prefix DIR         Installation prefix (default: WORK_DIR/prefix)
  --jobs N             Parallel make jobs (default: 1)
  --apply-patches DIR  Apply every *.patch in DIR to libpinyin source before
                       autoreconf; each patch's SHA-256 is folded into pin_ref
                       and recorded in oracle-pin.txt so the patched build is
                       distinguishable from the unpatched pin. The install
                       prefix must also differ from the pinned build.
  --model-dir DIR      Use an already-extracted, SHA-verified model export (the
                       product of tools/model/fetch-model.sh) instead of
                       downloading the pinned archive. DIR must contain all
                       eighteen export files. Reusing a cache does not relax the
                       SHA-256 check: the archive that produced DIR was verified
                       against MODEL_SHA256 before extraction. Only the source
                       of the bytes changes, never whether they are checked.
  --dbm NAME           DBM backend libpinyin is configured with: tkrzw
                       (default), kc, or bdb. Recorded in the pin ref
                       (+dbm-<name>) and the oracle-pin.txt dbm= field. For
                       bench-only oracle prefixes; the parity oracle stays
                       tkrzw.
  -h, --help           Show this help

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
	--apply-patches)
		apply_patches_dir=$2
		shift 2
		;;
	--model-dir)
		model_dir=$2
		shift 2
		;;
	--dbm)
		dbm=$2
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

if [[ -n $apply_patches_dir && -z $prefix ]]; then
	printf '%s
' '--apply-patches requires an explicit --prefix: a patched build must not land in the unpatched default prefix' >&2
	exit 2
fi

prefix=${prefix:-"$work_dir/prefix"}

case $jobs in
'' | *[!0-9]* | 0)
	printf '%s\n' '--jobs must be a positive integer' >&2
	exit 2
	;;
esac

# The dbm suffix in the pin ref is the flag value itself (tkrzw/kc/bdb);
# dbm_name is the spelling libpinyin's configure expects. Both must stay in
# step with PINYIN_BENCH_DBM in crates/pinyin-oracle/build.rs.
case $dbm in
tkrzw)
	dbm_name=Tkrzw
	;;
kc)
	dbm_name=KyotoCabinet
	;;
bdb)
	dbm_name=BerkeleyDB
	;;
*)
	printf '%s\n' '--dbm must be one of: tkrzw, kc, bdb' >&2
	exit 2
	;;
esac
ORACLE_PIN_REF="libpinyin-$LIBPINYIN_VERSION-$LIBPINYIN_SHA+model20-$MODEL_SHA256+dbm-$dbm"

for command in curl git sha256sum tar autoreconf make pkg-config find sort xargs patch; do
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

if [[ -n $model_dir ]]; then
	if [[ ! -d $model_dir ]]; then
		printf '%s\n' "--model-dir is not a directory: $model_dir" >&2
		exit 1
	fi
	missing=
	for f in "${MODEL_FILES[@]}"; do
		[[ -f "$model_dir/$f" ]] || missing="$missing $f"
	done
	if [[ -n $missing ]]; then
		printf 'FAIL: --model-dir is missing:%s\n' "$missing" >&2
		printf 'DIR must hold the verified model20 export (run tools/model/fetch-model.sh)\n' >&2
		exit 1
	fi
	# Require provenance: fetch-model.sh writes a marker recording the
	# SHA-256 of the archive that produced the extraction. Without it the
	# bytes could be any eighteen files with the right names. The marker
	# sits one level above the extracted dir (cache_dir/verified for
	# cache_dir/extracted/).
	marker=$model_dir/../verified
	if [[ ! -f $marker ]] || ! grep -qx "sha256=$MODEL_SHA256" "$marker"; then
		printf 'FAIL: --model-dir lacks a matching provenance marker\n' >&2
		printf '  expected sha256=%s in %s\n' "$MODEL_SHA256" "$marker" >&2
		printf '  run tools/model/fetch-model.sh to produce a verified cache\n' >&2
		exit 1
	fi
fi

mkdir -p "$work_dir/downloads" "$work_dir/src" "$prefix"
work_dir=$(cd "$work_dir" && pwd)
prefix=$(cd "$prefix" && pwd)

# Prefer the just-built libraries while retaining pkg-config's system defaults
# for declared build dependencies. Never inherit caller-provided search paths
# for those prefixes — EXCEPT a caller-supplied base such as
# PKG_CONFIG_PATH=/usr/local/lib/pkgconfig for a from-tarball libtkrzw:
# not every distro's pkg-config searches /usr/local by default, so dropping
# an inherited entry would break exactly that consumer.
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig:$prefix/lib64/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
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

# Fetch one upstream at a pinned commit into a fresh shallow checkout and
# verify that the checked-out HEAD is exactly the pinned SHA. The source
# directory is named by the commit SHA, never by the version or tag: the
# version names the release line, the SHA names the tree that was built.
fetch_commit() {
	local name=$1 url=$2 sha=$3 src=$work_dir/src/$1-$3
	rm -rf "$src"
	mkdir -p "$src"
	git init --quiet "$src"
	if ! git -C "$src" fetch --quiet --depth=1 "$url" "$sha"; then
		printf 'git fetch of %s %s failed\n' "$name" "$sha" >&2
		exit 1
	fi
	git -C "$src" checkout --quiet --detach FETCH_HEAD
	local fetched_sha
	fetched_sha=$(git -C "$src" rev-parse HEAD)
	if [[ $fetched_sha != "$sha" ]]; then
		printf '%s commit mismatch: fetched %s, expected %s\n' "$name" "$fetched_sha" "$sha" >&2
		exit 1
	fi
	printf '%s\n' "$src"
}

lib_src=$(fetch_commit libpinyin "$LIBPINYIN_GIT_URL" "$LIBPINYIN_SHA")
ibus_src=$(fetch_commit ibus-libpinyin "$IBUS_LIBPINYIN_GIT_URL" "$IBUS_LIBPINYIN_SHA")

# The model data is the 18-file model20 export that build-oracle.sh drops into
# libpinyin's data dir (make install then ships it to $prefix/lib/libpinyin/data).
# Prefer a pre-extracted export from tools/model/fetch-model.sh when given, so a
# rebuild of the oracle does not re-download the archive. The SHA-256 was already
# verified against MODEL_SHA256 before that export was produced; applying it here
# changes WHERE the bytes come from, never WHETHER they are checked. With no
# --model-dir the pinned archive is downloaded and verified, exactly as before.
model_data_dir=$lib_src/data
mkdir -p "$model_data_dir"
if [[ -n $model_dir ]]; then
	model_dir=$(cd "$model_dir" && pwd)
	# Copy the validated inventory only: a stray extra file in the cache dir
	# must not ride into the pinned oracle's data dir, and a subdirectory
	# would abort cp outright. The 18 names were checked above, so nothing
	# here can be missing.
	for f in "${MODEL_FILES[@]}"; do
		cp "$model_dir/$f" "$model_data_dir/$f"
	done
	printf 'reusing pre-extracted model export from %s\n' "$model_dir" >&2
else
	model_archive=$(fetch model20.text.tar.gz "$MODEL_URL" "$MODEL_SHA256")
	tar -xzf "$model_archive" -C "$model_data_dir"
fi

patch_manifest=
if [[ -n $apply_patches_dir ]]; then
	if [[ ! -d $apply_patches_dir ]]; then
		printf '%s\n' "--apply-patches: not a directory: $apply_patches_dir" >&2
		exit 1
	fi
	apply_patches_dir=$(cd "$apply_patches_dir" && pwd)
	mapfile -t patches < <(find "$apply_patches_dir" -maxdepth 1 -type f -name '*.patch' -print | sort)
	if ((${#patches[@]} == 0)); then
		printf '%s\n' "--apply-patches: no *.patch files in $apply_patches_dir" >&2
		exit 1
	fi
	(
		cd "$lib_src"
		for p in "${patches[@]}"; do
			printf 'applying patch: %s\n' "$p" >&2
			patch -p1 --forward --no-backup-if-mismatch <"$p"
		done
	)
	patch_manifest=$prefix/oracle-patches.sha256
	(
		cd "$apply_patches_dir"
		# shellcheck disable=SC2016  # sha256sum wants literal names.
		find . -maxdepth 1 -type f -name '*.patch' -print0 | sort -z | xargs -0 sha256sum
	) >"$patch_manifest"
	read -r patch_manifest_sha256 _ < <(sha256sum "$patch_manifest")
	ORACLE_PIN_REF="$ORACLE_PIN_REF+patches-$patch_manifest_sha256"
fi

(
	cd "$lib_src"
	autoreconf --force --install --verbose
	./configure --prefix="$prefix" --disable-static --with-dbm="$dbm_name"
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

header=$prefix/include/libpinyin-$LIBPINYIN_VERSION/pinyin.h
data_dir=$prefix/lib/libpinyin/data
data_manifest=$prefix/oracle-data.sha256
data_unstable_manifest=$prefix/oracle-data-unstable.sha256
[[ -f $header && -d $data_dir ]] || {
	printf '%s\n' 'installed oracle header or data directory not found' >&2
	exit 1
}
# The data payload is split into two manifests. libpinyin's generated data
# is not reproducible at a fixed pin: two clean builds of the same pin, same
# model, same container image, differ on exactly the six files produced
# through the DBM-backed generation path (see
# docs/findings/oracle-data-reproducibility.md). Those six go into the
# informational manifest, which is tamper-evident within one prefix but
# never comparable across builds. Everything else — the seventeen files the
# reproducible manifest gates — is byte-identical across clean builds and forms the reproducible gate: oracle-data.sha256, and its
# data_manifest_sha256 line in oracle-pin.txt, is a pure function of the
# pin, so two prefixes of the same pin agree on it.
DATA_UNSTABLE_FILES=(
	addon_phrase_index.bin
	addon_pinyin_index.bin
	bigram.db
	phrase_index.bin
	pinyin_index.bin
	punct.bin
)
(
	cd "$prefix"
	find lib/libpinyin/data -type f -print0 | sort -z |
		grep -zvF -e "$(printf '/%s\n' "${DATA_UNSTABLE_FILES[@]}")" |
		xargs -0 sha256sum
) >"$data_manifest"
(
	cd "$prefix"
	for f in "${DATA_UNSTABLE_FILES[@]}"; do
		[[ -f lib/libpinyin/data/$f ]] || {
			printf 'generated data file not found: %s\n' "$f" >&2
			exit 1
		}
		sha256sum "lib/libpinyin/data/$f"
	done
) >"$data_unstable_manifest"
read -r header_sha256 _ < <(sha256sum "$header")
read -r shared_object_sha256 _ < <(sha256sum "$shared_object")
read -r data_manifest_sha256 _ < <(sha256sum "$data_manifest")
read -r data_unstable_manifest_sha256 _ < <(sha256sum "$data_unstable_manifest")
{
	cat <<EOF
schema=pinyin-oracle-v1
pin_ref=$ORACLE_PIN_REF
libpinyin_tag=$LIBPINYIN_VERSION
libpinyin_commit=$LIBPINYIN_SHA
ibus_libpinyin_tag=$IBUS_LIBPINYIN_TAG
ibus_libpinyin_commit=$IBUS_LIBPINYIN_SHA
model_sha256=$MODEL_SHA256
dbm=$dbm_name
header_sha256=$header_sha256
shared_object_sha256=$shared_object_sha256
data_manifest_sha256=$data_manifest_sha256
data_unstable_manifest_sha256=$data_unstable_manifest_sha256
EOF
	if [[ -n $patch_manifest ]]; then
		printf 'patches_manifest_sha256=%s\n' "$patch_manifest_sha256"
	fi
} >"$prefix/oracle-pin.txt"

printf 'libpinyin_tag=%s\nlibpinyin_commit=%s\n' "$LIBPINYIN_VERSION" "$LIBPINYIN_SHA" >&2
printf 'ibus_libpinyin_tag=%s\nibus_libpinyin_commit=%s\n' "$IBUS_LIBPINYIN_TAG" "$IBUS_LIBPINYIN_SHA" >&2
printf '%s\n' "$shared_object"
