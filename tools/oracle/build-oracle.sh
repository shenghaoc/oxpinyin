#!/usr/bin/env bash
set -euo pipefail

# Build dependencies: autotools (autoconf, automake, autopoint, libtool),
# a C/C++ toolchain and make, pkg-config, gettext, gnome-common, curl,
# tar, python3, and development headers for GLib 2.0, IBus 1.0, SQLite 3,
# and the DBM backend selected by --dbm (Berkeley DB by default, or
# Tkrzw / Kyoto Cabinet).

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
# The parity oracle defaults to Berkeley DB: the reference build is a bare
# `./configure`, which selects BerkeleyDB (configure.ac:94 at the pin) —
# human ruling 2026-09-27 UTC, following Q1. tkrzw and kc stay selectable
# with --dbm. The frozen capture goldens and the pinyin-oracle crate's
# EXPECTED_PIN_REF are still the tkrzw cell (+dbm-tkrzw); build those
# prefixes with `--dbm tkrzw` (tools/capture/run-capture.sh,
# tools/profile/run-w8-cycle.sh, docs/testing/capture-fixtures.md).
dbm=bdb
enable_libzhuyin=0
# full: libraries, ibus-libpinyin and the model-derived data (the default).
# libs: libraries, headers and oracle-pin.txt only — no model, no data. This
#   is what CI builds and publishes (docs/findings/model-provenance.md).
# data: regenerate lib/libpinyin/data into a prefix an earlier libs build
#   (or a pulled oracle image) already provides, then check it against the
#   recorded manifest.
mode=full

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
  --enable-libzhuyin   Also build libzhuyin (configure --enable-libzhuyin): installs
                       lib/libzhuyin.so beside libpinyin.so, which the zhuyin,
                       candidate-assembly, two-context and open-counter
                       differentials need.
  --dbm NAME           DBM backend libpinyin is configured with: bdb
                       (default — the bare ./configure reference build),
                       kc, or tkrzw. Recorded in the pin ref (+dbm-<name>)
                       and the oracle-pin.txt dbm= field. Differentials
                       select the matching cell with PINYIN_ORACLE_DBM
                       (tools/bisection/oracle-cell.sh). The capture
                       goldens and the pinyin-oracle crate need
                       --dbm tkrzw (their frozen pin ref is +dbm-tkrzw).
  --libs-only          Build only the libraries, headers, pkg-config files and
                       oracle-pin.txt: no model, no ibus-libpinyin, no data.
                       The result is byte-reproducible (see below) and is what
                       the oracle image carries.
  --regen-data         Into an existing --libs-only prefix, build the pin's
                       data from the model (--model-dir or the pinned
                       download) in a staging area beside the prefix and
                       compare data_manifest_sha256 with the one recorded in
                       tools/oracle/oracle-pin.txt. Only on a match is
                       lib/libpinyin/data (with the data manifests and the
                       prefix's oracle-pin.txt) swapped in; on a mismatch the
                       prefix is left untouched and the script fails.
  -h, --help           Show this help

Build variables CC, CXX, CFLAGS, CXXFLAGS and LDFLAGS are passed through.
The library build is made reproducible: SOURCE_DATE_EPOCH is the pin's commit
time, the source path is remapped (-ffile-prefix-map, -fdebug-prefix-map) to a
fixed name, and the ELF build ID is omitted. The libraries then hash the same
wherever the work directory is, provided the toolchain and --prefix are the
same (tools/oracle/Dockerfile.oracle pins the toolchain; tools/oracle/
check-reproducible.sh proves it).
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
	--enable-libzhuyin)
		enable_libzhuyin=1
		shift
		;;
	--dbm)
		dbm=$2
		shift 2
		;;
	--libs-only)
		[[ $mode == full ]] || {
			printf '%s\n' '--libs-only and --regen-data are mutually exclusive' >&2
			exit 2
		}
		mode=libs
		shift
		;;
	--regen-data)
		[[ $mode == full ]] || {
			printf '%s\n' '--libs-only and --regen-data are mutually exclusive' >&2
			exit 2
		}
		mode=data
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
if [[ $mode == data ]]; then
	# Data is regenerated into a prefix that already holds the pin's libraries.
	if [[ -n $apply_patches_dir ]]; then
		printf '%s\n' '--regen-data cannot be combined with --apply-patches' >&2
		exit 2
	fi
	if [[ ! -f $prefix/oracle-pin.txt ]] || ! grep -qx "pin_ref=$ORACLE_PIN_REF" "$prefix/oracle-pin.txt"; then
		printf 'FAIL: %s is not a libs-only prefix of this pin and dbm\n' "$prefix" >&2
		printf '  expected pin_ref=%s in %s/oracle-pin.txt\n' "$ORACLE_PIN_REF" "$prefix" >&2
		exit 1
	fi
elif [[ -d $prefix && -n $(find "$prefix" -mindepth 1 -print -quit) ]]; then
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
# Intentional: the pinned build recipe isolates its own generation prefix.
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
ibus_src=
[[ $mode == full ]] && ibus_src=$(fetch_commit ibus-libpinyin "$IBUS_LIBPINYIN_GIT_URL" "$IBUS_LIBPINYIN_SHA")

# Reproducible-build settings (issue #818). The pin's commit time is the
# build timestamp, so it is a function of the pin and nothing else. The
# source path is rewritten to a fixed name in every place the compiler
# records it (__FILE__, debug info, macro expansion), so the library hash
# does not depend on --work-dir. The build ID would hash the path-bearing
# sections' layout and is of no use to a parity oracle, so it is omitted.
# CFLAGS/CXXFLAGS fall back to autoconf's own defaults (-g -O2) when the
# caller sets none, so the optimisation level is that of the bare
# ./configure reference build.
SOURCE_DATE_EPOCH=$(git -C "$lib_src" log -1 --format=%ct)
export SOURCE_DATE_EPOCH
export TZ=UTC LC_ALL=C
repro_map="-ffile-prefix-map=$lib_src=/usr/src/libpinyin -fdebug-prefix-map=$lib_src=/usr/src/libpinyin"
export CFLAGS="${CFLAGS--g -O2} $repro_map"
export CXXFLAGS="${CXXFLAGS--g -O2} $repro_map"
export LDFLAGS="${LDFLAGS:+$LDFLAGS }-Wl,--build-id=none"

# The model data is the 18-file model20 export that build-oracle.sh drops into
# libpinyin's data dir (make install then ships it to $prefix/lib/libpinyin/data).
# Prefer a pre-extracted export from tools/model/fetch-model.sh when given, so a
# rebuild of the oracle does not re-download the archive. The SHA-256 was already
# verified against MODEL_SHA256 before that export was produced; applying it here
# changes WHERE the bytes come from, never WHETHER they are checked. With no
# --model-dir the pinned archive is downloaded and verified, exactly as before.
if [[ $mode != libs ]]; then
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

# --regen-data generates into a staging area next to the prefix (same
# filesystem) and touches the prefix only after the regenerated manifest has
# been checked against the recorded one: a failed run leaves it as it was.
# Installing with DESTDIR keeps the real prefix in the installed binaries'
# run paths, so what is published is exactly what an in-place install makes.
stage=
content_root=$prefix
out_dir=$prefix
if [[ $mode == data ]]; then
	stage=$(mktemp -d "$prefix.stage.XXXXXX")
	trap 'rm -rf "$stage"' EXIT
	content_root=$stage$prefix
	out_dir=$stage/out
	mkdir -p "$out_dir"
fi

(
	cd "$lib_src"
	autoreconf --force --install --verbose
	configure_args=(--prefix="$prefix" --disable-static --with-dbm="$dbm_name")
	((enable_libzhuyin)) && configure_args+=(--enable-libzhuyin)
	./configure "${configure_args[@]}"
	case $mode in
	full)
		make -j"$jobs"
		make install
		;;
	libs)
		# src/ holds both libraries and the installed headers; the .pc
		# files are the top-level pkgconfig_DATA. Nothing here reads data/.
		make -C src -j"$jobs"
		make -C src install
		make install-pkgconfigDATA
		;;
	data)
		# Everything is built so utils/ links the freshly built libraries,
		# but only data/ and the data-build utils are installed (into the
		# staging area): the prefix's libraries stay the ones it was
		# provisioned with.
		make -j"$jobs"
		make -C utils install DESTDIR="$stage"
		make -C data install DESTDIR="$stage"
		;;
	esac
)

if [[ $mode == full ]]; then
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
fi

shared_object=$(find "$prefix" -type f \( -name 'libpinyin.so' -o -name 'libpinyin.so.*' \) -print | sort | head -n 1)
[[ -n $shared_object ]] || {
	printf '%s\n' 'built libpinyin shared object not found' >&2
	exit 1
}

header=$prefix/include/libpinyin-$LIBPINYIN_VERSION/pinyin.h
[[ -f $header ]] || {
	printf '%s\n' 'installed oracle header not found' >&2
	exit 1
}
read -r header_sha256 _ < <(sha256sum "$header")
read -r shared_object_sha256 _ < <(sha256sum "$shared_object")
zhuyin_sha256=
if [[ -f $prefix/lib/libzhuyin.so ]]; then
	read -r zhuyin_sha256 _ < <(sha256sum "$prefix/lib/libzhuyin.so")
fi

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
data_dir=$content_root/lib/libpinyin/data
data_manifest=$out_dir/oracle-data.sha256
data_unstable_manifest=$out_dir/oracle-data-unstable.sha256
data_lines=
if [[ $mode != libs ]]; then
	[[ -d $data_dir ]] || {
		printf '%s\n' 'installed oracle data directory not found' >&2
		exit 1
	}
	(
		cd "$content_root"
		find lib/libpinyin/data -type f -print0 | sort -z |
			grep -zvF -e "$(printf '/%s\n' "${DATA_UNSTABLE_FILES[@]}")" |
			xargs -0 sha256sum
	) >"$data_manifest"
	(
		cd "$content_root"
		for f in "${DATA_UNSTABLE_FILES[@]}"; do
			[[ -f lib/libpinyin/data/$f ]] || {
				printf 'generated data file not found: %s\n' "$f" >&2
				exit 1
			}
			sha256sum "lib/libpinyin/data/$f"
		done
	) >"$data_unstable_manifest"
	read -r data_manifest_sha256 _ < <(sha256sum "$data_manifest")
	read -r data_unstable_manifest_sha256 _ < <(sha256sum "$data_unstable_manifest")
	data_lines="data_manifest_sha256=$data_manifest_sha256
data_unstable_manifest_sha256=$data_unstable_manifest_sha256"
fi

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
EOF
	[[ -z $zhuyin_sha256 ]] || printf 'zhuyin_shared_object_sha256=%s\n' "$zhuyin_sha256"
	[[ -z $data_lines ]] || printf '%s\n' "$data_lines"
	if [[ -n $patch_manifest ]]; then
		printf 'patches_manifest_sha256=%s\n' "$patch_manifest_sha256"
	fi
} >"$out_dir/oracle-pin.txt.new"
mv "$out_dir/oracle-pin.txt.new" "$out_dir/oracle-pin.txt"

# --regen-data: the regenerated stable data must be the recorded one. The
# recorded value is a pure function of the pin (see above), so a mismatch is
# a wrong model, a wrong pin or a broken build — never host noise.
if [[ $mode == data ]]; then
	case $(uname -m) in
	x86_64) platform_suffix= ;;
	aarch64 | arm64) platform_suffix=_linux_arm64 ;;
	*)
		printf 'FAIL: no frozen oracle data for architecture %s\n' "$(uname -m)" >&2
		exit 1
		;;
	esac
	recorded=$(sed -n "s/^data_manifest_sha256_$dbm${platform_suffix}=//p" "$(dirname "${BASH_SOURCE[0]}")/oracle-pin.txt")
	if [[ -z $recorded ]]; then
		printf 'FAIL: tools/oracle/oracle-pin.txt records no data_manifest_sha256_%s%s\n' "$dbm" "$platform_suffix" >&2
		exit 1
	fi
	if [[ $recorded != "$data_manifest_sha256" ]]; then
		printf 'FAIL: data_manifest_sha256 mismatch\n  recorded:    %s\n  regenerated: %s\n' "$recorded" "$data_manifest_sha256" >&2
		exit 1
	fi
	printf 'data_manifest_sha256 matches the recorded value\n' >&2
	# Verified: now, and only now, swap the data and the data-build utilities
	# into the prefix and publish the manifests and pin file with them.
	rm -rf "$prefix/lib/libpinyin/data"
	cp -a "$content_root/." "$prefix/"
	mv "$out_dir/oracle-data.sha256" "$out_dir/oracle-data-unstable.sha256" "$prefix/"
	mv "$out_dir/oracle-pin.txt" "$prefix/oracle-pin.txt"
fi

printf 'libpinyin_tag=%s\nlibpinyin_commit=%s\n' "$LIBPINYIN_VERSION" "$LIBPINYIN_SHA" >&2
printf 'ibus_libpinyin_tag=%s\nibus_libpinyin_commit=%s\n' "$IBUS_LIBPINYIN_TAG" "$IBUS_LIBPINYIN_SHA" >&2
printf '%s\n' "$shared_object"
