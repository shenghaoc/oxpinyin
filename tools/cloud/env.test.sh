#!/usr/bin/env bash
# Exercise setup prerequisites and the real environment writer with synthetic pins.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
oracle_root=$work/"old cache '\$\`"/oracle
model_cache=$work/model20
uv_dir=$work/uv
tkrzw_prefix=$work/tkrzw
tkrzw_local=1
pin=$work/oracle-pin.txt
export OXPINYIN_ORACLE_ENV=$work/oracle.env
source "$(dirname "${BASH_SOURCE[0]}")/../oracle/pin.sh"
platform=$(oracle_platform)
pinval() { sed -n "s/^$1=//p" "$pin"; }
pinval_platform() { pin_platform_field "$pin" "$platform" "$1"; }
log() { printf '%s\n' "$*" >&2; }
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

# Run the real setup entry point with an isolated PATH. Stubs report calls
# through fd 3 into the in-memory result, even when setup redirects stderr.
# No package manager, download, runtime or Python provisioning reaches the host.
(
	setup=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/setup.sh
	stub_bin=$work/setup-bin
	mkdir -p "$stub_bin" "$work/uv-bin" "$work/readonly-uv"
	for tool in bash dirname nproc uname sed cat mkdir mktemp rm base64 sha256sum cut grep basename tar; do
		ln -s "$(command -v "$tool")" "$stub_bin/$tool"
	done
	cat >"$stub_bin/stub" <<'STUB'
#!/bin/bash
case ${0##*/} in
cargo) printf 'cargo 1.97.1\n' ;;
rustup)
	[[ ${SYNTHETIC_TOOLCHAIN_MISSING:-0} == 0 ]] || exit 1
	printf 'rustc 1.97.1\n'
	;;
valgrind)
	[[ ${SYNTHETIC_VALGRIND_MISSING:-0} == 0 ]] || exit 1
	printf 'valgrind-%s\n' "${SYNTHETIC_VALGRIND_VERSION:-3.25.1}"
	;;
findmnt)
	[[ ${SYNTHETIC_EXT4_MODE:-absent} != unknown ]] || exit 1
	if [[ $* == '--raw --noheadings --output TARGET,FSTYPE' ]]; then
		printf '/ overlay\n'
		if [[ ${SYNTHETIC_EXT4_MODE:-absent} != absent ]]; then
			printf '%s ext4\n' "${SYNTHETIC_EXT4_PATH// /\\x20}"
		fi
	elif [[ $# == 5 && $1 == --noheadings && $2 == --output && $3 == FSTYPE && $4 == --target ]]; then
		if [[ ${SYNTHETIC_EXT4_MODE:-absent} == available && $5 == "$SYNTHETIC_EXT4_PATH" ]]; then
			printf 'ext4\n'
		else
			printf 'overlay\n'
		fi
	else
		printf 'ACTION unexpected findmnt %s\n' "$*" >&3
		exit 97
	fi
	;;
podman)
	if [[ $1 == --version ]]; then printf 'podman (synthetic)\n'
	else printf 'ACTION podman %s\n' "$*" >&3; exit 97; fi
	;;
uv)
	if [[ $1 == --version ]]; then printf 'uv %s (synthetic)\n' "${SYNTHETIC_UV_VERSION:-0.13.1}"
	elif [[ $1 == python && $2 == find ]]; then
		# A read-only probe, so it records no ACTION.
		[[ ${SYNTHETIC_PYTHON_PRESENT:-0} == 1 ]] || exit 1
		printf '/synthetic/managed/python\n'
	else
		printf 'ACTION uv %s\n' "$*" >&3
		[[ ${SYNTHETIC_PYTHON_FAILURE:-0} == 0 ]] || exit 97
		if [[ $1 == run ]]; then cat >/dev/null; printf 'Verified synthetic managed Python\n'; fi
	fi
	;;
*) printf 'ACTION %s %s\n' "${0##*/}" "$*" >&3; exit 97 ;;
esac
STUB
	chmod +x "$stub_bin/stub"
	for tool in cargo rustup valgrind uv curl apt-get podman db5.3_dump findmnt; do
		ln -s stub "$stub_bin/$tool"
	done
	run_setup() {
		status=0
		message=$(env -u UV_PYTHON -u PYTHONHOME -u PYTHONPATH \
			PATH="$stub_bin" UV_INSTALL_DIR="${SYNTHETIC_UV_INSTALL_DIR:-$stub_bin}" \
			OXPINYIN_CLOUD_CACHE="$work/setup-cache" OXPINYIN_ORACLE_ENV="$work/setup.env" \
			PINYIN_MODEL_CACHE="$work/setup-model" OXPINYIN_CONTAINER=podman \
			/bin/bash "$setup" "$@" 3>&1 2>&1) || status=$?
	}
	check_setup() {
		local expected_status=$1 expected=$2
		shift 2
		run_setup "$@"
		[[ $status == "$expected_status" && $message == *"$expected"* ]] ||
			fail "setup expected exit $expected_status and '$expected'; got exit $status: $message"
		[[ $message != *'ACTION '* ]] || fail "setup provisioned before checking prerequisites: $message"
	}

	rm "$stub_bin/cargo"
	check_setup 1 'FAIL: cargo is missing; install the rustup-managed toolchain from rust-toolchain.toml (docs/runbooks/setup.md)'
	ln -s stub "$stub_bin/cargo"
	printf 'PASS: missing Cargo fails before provisioning\n'

	export SYNTHETIC_TOOLCHAIN_MISSING=1
	channel=$(sed -n 's/^channel = "\([^"]*\)"/\1/p' "${setup%/tools/cloud/setup.sh}/rust-toolchain.toml")
	check_setup 1 "FAIL: Rust toolchain $channel is not installed; run: rustup toolchain install $channel --component clippy --component rustfmt"
	unset SYNTHETIC_TOOLCHAIN_MISSING
	printf 'PASS: missing repository toolchain fails before provisioning\n'

	if ((EUID != 0)); then
		export SYNTHETIC_VALGRIND_MISSING=1
		check_setup 1 $'missing: valgrind\nrun as root: apt-get install -y valgrind\nFAIL: valgrind is missing and neither root nor sudo is available'
		unset SYNTHETIC_VALGRIND_MISSING
		printf 'PASS: non-root without sudo gets the exact Valgrind install command\n'
	fi

	# Preserve root apt-get and non-root sudo installation/retry paths.
	export SYNTHETIC_VALGRIND_MISSING=1
	installer=apt-get
	if ((EUID != 0)); then ln -s stub "$stub_bin/sudo"; installer='sudo apt-get'; fi
	run_setup
	[[ $status == 1 && $message == *"ACTION $installer install -y valgrind"* && \
		$message == *"ACTION $installer update"* && $message == *'FAIL: could not install valgrind with apt-get'* && \
		$message != *'ACTION uv '* && $message != *'ACTION curl '* ]] || fail "$message"
	if ((EUID != 0)); then rm "$stub_bin/sudo"; fi
	unset SYNTHETIC_VALGRIND_MISSING
	printf 'PASS: Valgrind keeps the %s install/retry path\n' "$installer"

	# Below its minimum, Valgrind is replaced like a missing one.
	export SYNTHETIC_VALGRIND_VERSION=3.21.0
	if ((EUID != 0)); then
		check_setup 1 $'outdated: valgrind-3.21.0 (minimum 3.22.0)\nrun as root: apt-get install -y valgrind\nFAIL: valgrind-3.21.0 is older than 3.22.0 and neither root nor sudo is available'
	fi
	installer=apt-get
	if ((EUID != 0)); then ln -s stub "$stub_bin/sudo"; installer='sudo apt-get'; fi
	run_setup
	[[ $status == 1 && $message == *"ACTION $installer install -y valgrind"* && \
		$message == *'FAIL: could not install valgrind with apt-get'* ]] || fail "$message"
	if ((EUID != 0)); then rm "$stub_bin/sudo"; fi
	unset SYNTHETIC_VALGRIND_VERSION
	printf 'PASS: a Valgrind older than its minimum is installed again\n'

	# Force uv installation into a genuinely unwritable destination. Root
	# bypasses mode bits, so give it a regular file instead of a directory.
	chmod a-w "$work/readonly-uv"
	export SYNTHETIC_UV_INSTALL_DIR="$work/readonly-uv"
	if ((EUID == 0)); then
		SYNTHETIC_UV_INSTALL_DIR=$work/uv-file
		: >"$SYNTHETIC_UV_INSTALL_DIR"
	fi
	check_setup 1 "FAIL: uv installation directory is not writable: $SYNTHETIC_UV_INSTALL_DIR; set UV_INSTALL_DIR to a writable task-owned directory" --python-only
	chmod u+w "$work/readonly-uv"
	printf 'PASS: unwritable uv destination fails before download\n'

	export SYNTHETIC_UV_INSTALL_DIR="$work/uv-bin"
	run_setup --python-only
	[[ $status == 1 && $message == *'ACTION curl '* && $message == *'FAIL: uv download failed; allow github.com and release-assets.githubusercontent.com'* ]] || fail "$message"
	unset SYNTHETIC_UV_INSTALL_DIR
	printf 'PASS: uv download failure names the required release hosts\n'

	export SYNTHETIC_PYTHON_FAILURE=1
	run_setup --python-only
	[[ $status == 1 && $message == *'FAIL: managed CPython 3.15 provisioning failed; check writable UV_PYTHON_INSTALL_DIR/UV_CACHE_DIR and access to github.com and release-assets.githubusercontent.com'* ]] || fail "$message"
	unset SYNTHETIC_PYTHON_FAILURE
	printf 'PASS: Python provisioning failure names writable paths and release hosts\n'

	# uv at or above its minimum is used as it is, by semantic versioning: build metadata does not count,
	# a pre-release of a newer release is newer, and a pre-release of 0.13.1 itself is older than 0.13.1.
	for version in 0.13.1 0.14.2 0.13.1+build.7 0.14.0-beta.1 0.13.2-rc.1+build.7; do
		export SYNTHETIC_UV_VERSION=$version
		run_setup --python-only
		[[ $status == 0 && $message == *"using uv $version (synthetic)"* && $message != *'ACTION curl '* ]] || fail "uv $version: $message"
	done
	for version in 0.13.0 0.13.1-beta.1 0.13.1-rc.1+build.7 0.13.1-0 0.12.9-rc.1; do
		export SYNTHETIC_UV_VERSION=$version
		run_setup --python-only
		[[ $status == 1 && $message == *'ACTION curl '* && $message == *'FAIL: uv download failed'* ]] || fail "uv $version: $message"
	done
	unset SYNTHETIC_UV_VERSION
	printf 'PASS: uv at or above 0.13.1 is kept; an older uv, or a pre-release of 0.13.1, is replaced\n'

	run_setup --cells bdb
	[[ $status == 1 && $message == *'ACTION curl '* && $message == *'downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz'* && \
		$message == *'FAIL: model20 is unavailable; allow downloads.sourceforge.net and its *.dl.sourceforge.net redirect target'* && \
		$message != *'ACTION podman '* ]] || fail "$message"
	printf 'PASS: model20 access failure precedes container pulls and library builds\n'

	# A Valgrind at or above its minimum is kept: setup goes on to the model fetch. A git build
	# (valgrind-3.22.0.GIT) counts as its number, as it does in tools/dev/doctor.py.
	for version in 3.22.0 3.27.1 3.22.0.GIT; do
		export SYNTHETIC_VALGRIND_VERSION=$version
		run_setup --cells bdb
		[[ $status == 1 && $message == *'FAIL: model20 is unavailable'* && $message != *'ACTION apt-get '* && \
			$message != *'ACTION sudo '* ]] || fail "valgrind $version: $message"
	done
	unset SYNTHETIC_VALGRIND_VERSION
	printf 'PASS: a Valgrind at or above its minimum is not reinstalled\n'

	# Match the fetcher's existing synthetic cache contract. It must not
	# require a network probe when a verified model extraction is present.
	mkdir -p "$work/setup-model/extracted"
	for name in art culture economy gb_char gbk_char geology history life merged nature opengram people punct science society sport technology; do
		: >"$work/setup-model/extracted/$name.table"
	done
	: >"$work/setup-model/extracted/interpolation2.text"
	printf 'sha256=59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155\n' >"$work/setup-model/verified"
	run_setup --cells bdb
	[[ $status == 1 && $message == *'verified model cache present; skipping fetch'* && \
		$message != *'ACTION curl '* && $message == *'ACTION podman info'* ]] || fail "$message"
	printf 'PASS: a verified model cache avoids network access\n'

	# Python-only must not inherit the full setup's Cargo/Valgrind requirements.
	rm "$stub_bin/cargo" "$stub_bin/rustup" "$stub_bin/valgrind"
	run_setup --python-only
	[[ $status == 0 && $message == *'stopping after Python provisioning (--python-only)'* && \
		$message == *'ACTION uv python install cpython@3.15'* && $message != *'ACTION curl '* && \
		$message != *'ACTION apt-get '* && $message != *'ACTION podman '* ]] || fail "$message"
	printf 'PASS: Python-only keeps its Python-only prerequisite scope\n'

	# A managed 3.15-series interpreter that is already installed is kept.
	export SYNTHETIC_PYTHON_PRESENT=1
	run_setup --python-only
	[[ $status == 0 && $message == *'ACTION uv run '* && $message != *'ACTION uv python install'* ]] || fail "$message"
	unset SYNTHETIC_PYTHON_PRESENT
	printf 'PASS: an installed managed interpreter is not installed again\n'

	# The measurement report must not gate a successful non-measurement setup.
	# A mount outside HOME/checkout/TMPDIR/cache is discovered, including spaces.
	export SYNTHETIC_EXT4_PATH="$work/ext4 mount" SYNTHETIC_EXT4_MODE=available
	mkdir -p "$SYNTHETIC_EXT4_PATH"
	run_setup --python-only
	[[ $status == 0 && $message == *"ext4 path (readable, writable, searchable): $SYNTHETIC_EXT4_PATH"* && \
		$message == *'Measurement paths still need individual checks with findmnt -T <path>: libraries, data, profiles, work directory and TMPDIR.'* && \
		$message == *'does not verify all measured files are on ext4'* ]] || fail "$message"
	printf 'PASS: accessible ext4 mount reported without promising measurement readiness\n'

	for mode in absent overmounted; do
		export SYNTHETIC_EXT4_MODE=$mode
		run_setup --python-only
		[[ $status == 0 && $message == *'ext4 paths: none accessible'* && \
			$message == *'measurement gates cannot run without an ext4 path.'* && \
			$message != *'ext4 path (readable, writable, searchable):'* ]] || fail "$mode: $message"
	done
	printf 'PASS: absent and overmounted ext4 keep non-measurement setup successful\n'

	export SYNTHETIC_EXT4_MODE=available
	rm -r "$SYNTHETIC_EXT4_PATH"
	run_setup --python-only
	[[ $status == 0 && $message == *'ext4 paths: none accessible'* ]] || fail "$message"
	printf 'PASS: inaccessible mount target is not reported as an available path\n'

	export SYNTHETIC_EXT4_MODE=unknown
	run_setup --python-only
	[[ $status == 0 && $message == *'ext4 paths: unknown (mount detection failed)'* ]] || fail "$message"
	rm "$stub_bin/findmnt"
	run_setup --python-only
	[[ $status == 0 && $message == *'ext4 paths: unknown (findmnt unavailable)'* ]] || fail "$message"
	unset SYNTHETIC_EXT4_MODE SYNTHETIC_EXT4_PATH
	printf 'PASS: failed or unavailable findmnt reports unknown without failing setup\n'
)

fixture() {
	local dbm=$1 lib file prefix=$oracle_root/$1
	mkdir -p "$prefix/lib/libpinyin/data"
	for lib in libpinyin libzhuyin; do
		printf '%s %s\n' "$dbm" "$lib" >"$prefix/lib/$lib.so"
		printf '%s_sha256_%s%s=%s\n' "$lib" "$dbm" "$(oracle_platform_suffix "$platform")" \
			"$(sha256sum "$prefix/lib/$lib.so" | cut -d' ' -f1)" >>"$pin"
	done
	printf '%s\n' "$dbm" >"$prefix/lib/libpinyin/data/table.conf"
	(cd "$prefix" && sha256sum lib/libpinyin/data/table.conf >oracle-data.sha256)
	printf 'data_manifest_sha256_%s%s=%s\n' "$dbm" "$(oracle_platform_suffix "$platform")" \
		"$(sha256sum "$prefix/oracle-data.sha256" | cut -d' ' -f1)" >>"$pin"
	for file in bigram.db phrase_index.bin pinyin_index.bin punct.bin addon_phrase_index.bin addon_pinyin_index.bin; do
		printf '%s %s\n' "$dbm" "$file" >"$prefix/lib/libpinyin/data/$file"
	done
	(cd "$prefix" && sha256sum lib/libpinyin/data/{bigram.db,phrase_index.bin,pinyin_index.bin,punct.bin,addon_phrase_index.bin,addon_pinyin_index.bin} >oracle-data-unstable.sha256)
	printf 'data_unstable_manifest_sha256=%s\n' "$(sha256sum "$prefix/oracle-data-unstable.sha256" | cut -d' ' -f1)" >"$prefix/oracle-pin.txt"
}

select_cell() (
	export PINYIN_ORACLE_DBM=$1
	source "$OXPINYIN_ORACLE_ENV" || return
	[[ $PINYIN_ORACLE_PREFIX == "$2" && $OXPINYIN_LIBPINYIN_DATA_DIR == "$2/lib/libpinyin/data" ]]
)

for dbm in bdb kc tkrzw; do fixture "$dbm"; done

for dbm in bdb kc tkrzw; do
	data_ok "$dbm" || fail "good $dbm prefix was refused"
done
printf 'PASS: good prefixes with their own unstable digests accepted\n'

refused_data() {
	local file=$1 message
	if message=$(data_ok bdb 2>&1); then fail "damaged or missing $file was accepted"; fi
	[[ $message == *"$file"* ]] || fail "diagnostic did not name $file: $message"
	printf 'PASS: damaged or missing %s refused with its name\n' "$file"
}
for file in bigram.db phrase_index.bin pinyin_index.bin punct.bin addon_phrase_index.bin addon_pinyin_index.bin; do
	printf 'corrupt\n' >"$oracle_root/bdb/lib/libpinyin/data/$file"
	refused_data "$file"
	printf 'bdb %s\n' "$file" >"$oracle_root/bdb/lib/libpinyin/data/$file"
done
manifest=$(cat "$oracle_root/bdb/oracle-data-unstable.sha256")
printf '\n' >>"$oracle_root/bdb/oracle-data-unstable.sha256"
refused_data oracle-data-unstable.sha256
rm "$oracle_root/bdb/oracle-data-unstable.sha256"
refused_data oracle-data-unstable.sha256
printf '%s\n' "$manifest" >"$oracle_root/bdb/oracle-data-unstable.sha256"
data_ok bdb || fail 'restored prefix was refused'

cells=(bdb)
write_oracle_env
if message=$(select_cell kc "$oracle_root/kc" 2>&1); then
	fail 'never-provisioned kc was accepted'
fi
[[ $message == 'cell kc was not provisioned by this environment file' ]] || fail "$message"
printf 'PASS: never-provisioned cell keeps the existing error\n'

cells=(bdb kc tkrzw)
write_oracle_env
cells=(bdb)
tkrzw_local=0
write_oracle_env
for dbm in bdb kc tkrzw; do
	select_cell "$dbm" "$oracle_root/$dbm" || fail "$dbm lost after a bdb-only run"
done
printf 'PASS: kc and tkrzw survive a bdb-only run\n'

(
	LD_LIBRARY_PATH= PKG_CONFIG_PATH=
	export PINYIN_ORACLE_DBM=tkrzw
	source "$OXPINYIN_ORACLE_ENV"
	[[ $OXPINYIN_TKRZW_PREFIX == "$tkrzw_prefix" ]] || fail "missing trusted runtime prefix"
	[[ $LD_LIBRARY_PATH == "$tkrzw_prefix/lib" && $PKG_CONFIG_PATH == "$tkrzw_prefix/lib/pkgconfig" && $PATH == "$tkrzw_prefix/bin:"* ]]
) || fail 'retained tkrzw lost its local runtime paths'
printf 'PASS: retained tkrzw keeps its local runtime paths\n'

# A subset run can use another cache. Old cells must keep their own prefix.
old_root=$oracle_root
oracle_root=$work/new-cache/oracle
mkdir -p "$oracle_root"
cp -a "$old_root/bdb" "$oracle_root/bdb"
write_oracle_env
select_cell bdb "$oracle_root/bdb"
for dbm in kc tkrzw; do select_cell "$dbm" "$old_root/$dbm"; done
printf 'PASS: another cache preserves each retained prefix\n'

restore_env() {
	(oracle_root=$old_root; cells=(bdb kc tkrzw); tkrzw_local=1; write_oracle_env)
}
removed() {
	local dbm=$1 check=$2 file=${3:-} message
	message=$(write_oracle_env 2>&1)
	[[ $(printf '%s\n' "$message" | grep -c "^$dbm:") == 1 && $message == *"$dbm: removed from environment file; $check check failed"* ]] || fail "$message"
	[[ -z $file || $message == *"$file"* ]] || fail "removed cell diagnostic did not name $file: $message"
	if message=$(select_cell "$dbm" "$old_root/$dbm" 2>&1); then fail "invalid $dbm was retained"; fi
	[[ $message == "cell $dbm was not provisioned by this environment file" ]] || fail "$message"
	if grep -q "^export OXPINYIN_ORACLE_.*_${dbm^^}=" "$OXPINYIN_ORACLE_ENV"; then fail "stale $dbm assignment"; fi
	printf 'PASS: invalid %s removed with one %s diagnostic\n' "$dbm" "$check"
}

for lib in libpinyin libzhuyin; do
	printf 'corrupt\n' >"$old_root/kc/lib/$lib.so"
	removed kc 'oracle-pin.txt library hash'
	select_cell tkrzw "$old_root/tkrzw"
	printf 'kc %s\n' "$lib" >"$old_root/kc/lib/$lib.so"
	restore_env
done
printf '\n' >>"$old_root/tkrzw/oracle-data.sha256"
removed tkrzw 'data manifest'
(cd "$old_root/tkrzw" && sha256sum lib/libpinyin/data/table.conf >oracle-data.sha256)
restore_env
printf 'corrupt\n' >"$old_root/tkrzw/lib/libpinyin/data/table.conf"
removed tkrzw 'data manifest'
printf 'tkrzw\n' >"$old_root/tkrzw/lib/libpinyin/data/table.conf"
restore_env
printf 'corrupt\n' >"$old_root/kc/lib/libpinyin/data/bigram.db"
removed kc 'data manifest' 'lib/libpinyin/data/bigram.db'
printf 'kc bigram.db\n' >"$old_root/kc/lib/libpinyin/data/bigram.db"
restore_env

# Observe the old complete file at the exact rename boundary, and source the
# complete replacement there. Keep observations in memory, never in captures.
before=$(cat "$OXPINYIN_ORACLE_ENV")
mv() {
	[[ $1 == "$OXPINYIN_ORACLE_ENV.new" && $2 == "$OXPINYIN_ORACLE_ENV" ]] || fail 'unexpected rename'
	[[ $(cat "$2") == "$before" ]] || fail 'environment changed before rename'
	for dbm in bdb kc tkrzw; do
		OXPINYIN_ORACLE_ENV=$1 select_cell "$dbm" "${old_root}/$dbm" || fail 'incomplete staging file'
	done
	command mv "$@"
}
(oracle_root=$old_root; write_oracle_env)
unset -f mv
[[ ! -e $OXPINYIN_ORACLE_ENV.new ]] || fail 'staging file left after rename'
printf 'PASS: complete replacement published by one atomic rename\n'

before=$(cat "$OXPINYIN_ORACLE_ENV")
mv() { return 75; }
status=0
(write_oracle_env) || status=$?
unset -f mv
[[ $status == 75 && $(cat "$OXPINYIN_ORACLE_ENV") == "$before" && ! -e $OXPINYIN_ORACLE_ENV.new ]] || fail 'failed rename changed the environment or left staging behind'
printf 'PASS: failed rename leaves the old file intact and cleans staging\n'

# Model setup's tkrzw decision with the real probe helpers. Cached utilities
# are healthy; the synthetic host can be broken or healthy. No provisioning.
(
	cache_a=$tkrzw_prefix
	cache_b=$work/cache-b/tkrzw
	host_prefix=$work/host-tkrzw
	for prefix in "$cache_a" "$cache_b" "$host_prefix"; do
		mkdir -p "$prefix/bin" "$prefix/lib"
		: >"$prefix/lib/libtkrzw.so.1"
		if [[ $prefix == "$host_prefix" ]]; then
			printf '#!/bin/sh\n[ "$LD_LIBRARY_PATH" = "$SYNTHETIC_HOST_LIB" ] || exit 2\nexit "${SYNTHETIC_HOST_STATUS:-1}"\n' >"$prefix/bin/tkrzw_dbm_util"
		else
			printf '#!/bin/sh\nexit 0\n' >"$prefix/bin/tkrzw_dbm_util"
		fi
		chmod +x "$prefix/bin/tkrzw_dbm_util"
	done
	probe=$work/tkrzw-probe.sh
	printf '#!/bin/sh\ntkrzw_dbm_util\n' >"$probe"
	ldd() {
		local so=${1%/bin/tkrzw_dbm_util}/lib/libtkrzw.so.1
		[[ $1 != "$host_prefix/bin/tkrzw_dbm_util" ]] || so=${SYNTHETIC_HOST_SO:-$so}
		printf 'libtkrzw.so.1 => %s (0x1234)\n' "$so"
	}
	base_path=$host_prefix/bin:$PATH
	PATH=$base_path
	LD_LIBRARY_PATH=$host_prefix/lib
	export PINYIN_ORACLE_DBM=tkrzw SYNTHETIC_HOST_STATUS=1 SYNTHETIC_HOST_LIB=$host_prefix/lib
	source "$OXPINYIN_ORACLE_ENV"
	[[ $PATH == "$cache_a/bin:"* && $LD_LIBRARY_PATH == "$cache_a/lib:"* ]] || fail 'cache A was not inherited'
	legacy_env=$work/legacy-a.env
	sed '/^export OXPINYIN_TKRZW_PREFIXES=/d' "$OXPINYIN_ORACLE_ENV" >"$legacy_env"
	tkrzw_prefix=$cache_b
	PATH=$cache_b/bin:$PATH
	LD_LIBRARY_PATH=$cache_b/lib:$LD_LIBRARY_PATH
	tkrzw_local=0
	if probe_host; then :; else tkrzw_local=1; fi
	[[ $tkrzw_local == 1 ]] || fail 'cache A was mistaken for the host when provisioning cache B'
	printf 'PASS: retained cache A and current cache B cannot pass the host probe\n'
	(
		unset OXPINYIN_TKRZW_PREFIXES
		OXPINYIN_ORACLE_ENV=$legacy_env
		PATH=$base_path
		LD_LIBRARY_PATH=$host_prefix/lib
		source "$OXPINYIN_ORACLE_ENV"
		if probe_host; then fail 'legacy environment cache A was mistaken for the host'; fi
	)
	printf 'PASS: legacy environment files also exclude their cached runtime\n'

	oracle_root=$work/cache-b/oracle
	mkdir -p "$oracle_root"
	cp -a "$old_root/tkrzw" "$oracle_root/tkrzw"
	cells=(tkrzw)
	write_oracle_env
	(
		PATH=$base_path
		LD_LIBRARY_PATH=$host_prefix/lib
		source "$OXPINYIN_ORACLE_ENV"
		[[ $PATH == "$cache_b/bin:"* && $LD_LIBRARY_PATH == "$cache_b/lib:"* ]] || fail 'cache B runtime paths missing in a fresh shell'
	)
	printf 'PASS: cache B environment restores its runtime in a fresh shell\n'

	# Sourcing another environment must keep the earlier cache excluded too.
	source "$OXPINYIN_ORACLE_ENV"
	tkrzw_prefix=$work/cache-c/tkrzw
	export SYNTHETIC_HOST_STATUS=0
	message=$(probe_host) || fail 'healthy real host library was rejected'
	[[ $message == "host check: $host_prefix/bin/tkrzw_dbm_util resolves libtkrzw.so.1 to $host_prefix/lib/libtkrzw.so.1" ]] || fail "$message"
	printf 'PASS: all sourced caches excluded while a healthy host still passes\n'

	(
		# A healthy host needs no cache build, but must publish one exact
		# verified library identity rather than trusting a whole system prefix.
		probe_host >/dev/null || fail 'healthy host probe failed'
		tkrzw_local=0
		[[ ! -e $tkrzw_prefix ]] || fail 'host-only fixture unexpectedly has a cache'
		OXPINYIN_ORACLE_ENV=$work/host-only.env
		write_oracle_env
		[[ -z $(tkrzw_env_prefix "$OXPINYIN_ORACLE_ENV") ]] || fail 'host identity became a cache prefix'
		unset OXPINYIN_TKRZW_LIBRARY OXPINYIN_TKRZW_PREFIX
		source "$OXPINYIN_ORACLE_ENV"
		[[ $OXPINYIN_TKRZW_LIBRARY == "$host_prefix/lib/libtkrzw.so.1" ]] || fail 'verified host identity lost'
		[[ -z ${OXPINYIN_TKRZW_PREFIX:-} ]] || fail 'host identity became a trusted system prefix'
		resolver_bin=$work/host-resolver-bin
		mkdir -p "$resolver_bin" "$host_prefix/lib/other"
		: >"$host_prefix/lib/other/libtkrzw.so.1"
		cat >"$resolver_bin/ldd" <<'RESOLVER'
#!/bin/sh
printf 'libtkrzw.so.1 => %s (0x1234)\n' "$SYNTHETIC_RESOLVED_LIBRARY"
RESOLVER
		chmod +x "$resolver_bin/ldd"
		export PATH="$resolver_bin:$PATH" SYNTHETIC_RESOLVED_LIBRARY="$host_prefix/lib/libtkrzw.so.1"
		runtime=$(cd "$(dirname "${BASH_SOURCE[0]}")/../tkrzw" && pwd)/runtime.sh
		bash "$runtime" bdb host-env-test subject "$host_prefix/bin/tkrzw_dbm_util" || fail 'verified host library refused'
		printf 'PASS: healthy host without a cache exports an accepted exact runtime identity\n'
		SYNTHETIC_RESOLVED_LIBRARY=$host_prefix/lib/other/libtkrzw.so.1
		if message=$(bash "$runtime" bdb host-env-test subject "$host_prefix/bin/tkrzw_dbm_util" 2>&1); then
			fail 'another library under the same system prefix was accepted'
		fi
		[[ $message == *"$SYNTHETIC_RESOLVED_LIBRARY"* ]] || fail "wrong host refusal diagnostic: $message"
		printf 'PASS: host trust rejects another library under the same system prefix\n'
	)

	# Even a host executable can resolve a cached library through its RPATH.
	SYNTHETIC_HOST_SO=$cache_a/lib/libtkrzw.so.1
	if probe_host; then fail 'host executable resolved cache A but was accepted'; fi
	printf 'PASS: a cached library cannot count as host through executable RPATH\n'
)
