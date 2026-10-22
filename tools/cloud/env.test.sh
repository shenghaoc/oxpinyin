#!/usr/bin/env bash
# Exercise the real environment writer and verification with synthetic pins.
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
pinval() { sed -n "s/^$1=//p" "$pin"; }
log() { printf '%s\n' "$*" >&2; }
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

fixture() {
	local dbm=$1 lib prefix=$oracle_root/$1
	mkdir -p "$prefix/lib/libpinyin/data"
	for lib in libpinyin libzhuyin; do
		printf '%s %s\n' "$dbm" "$lib" >"$prefix/lib/$lib.so"
		printf '%s_sha256_%s=%s\n' "$lib" "$dbm" "$(sha256sum "$prefix/lib/$lib.so" | cut -d' ' -f1)" >>"$pin"
	done
	printf '%s\n' "$dbm" >"$prefix/lib/libpinyin/data/table.conf"
	(cd "$prefix" && sha256sum lib/libpinyin/data/table.conf >oracle-data.sha256)
	printf 'data_manifest_sha256_%s=%s\n' "$dbm" "$(sha256sum "$prefix/oracle-data.sha256" | cut -d' ' -f1)" >>"$pin"
}

select_cell() (
	export PINYIN_ORACLE_DBM=$1
	source "$OXPINYIN_ORACLE_ENV" || return
	[[ $PINYIN_ORACLE_PREFIX == "$2" && $OXPINYIN_LIBPINYIN_DATA_DIR == "$2/lib/libpinyin/data" ]]
)

for dbm in bdb kc tkrzw; do fixture "$dbm"; done
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
	local dbm=$1 check=$2 message
	message=$(write_oracle_env 2>&1)
	[[ $(printf '%s\n' "$message" | grep -c "^$dbm:") == 1 && $message == *"$dbm: removed from environment file; $check check failed"* ]] || fail "$message"
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
