#!/usr/bin/env bash
# Shared oracle verification and environment-file writer for setup.sh.
# libs_ok DBM [PREFIX]: both libraries hash to the recorded recipe values.
libs_ok() {
	local dbm=$1 prefix=${2:-$oracle_root/$1} lib want
	for lib in libpinyin libzhuyin; do
		want=$(pinval "${lib}_sha256_$dbm")
		[[ -f $prefix/lib/$lib.so ]] || return 1
		[[ $(sha256sum "$prefix/lib/$lib.so" | cut -d' ' -f1) == "$want" ]] || return 1
	done
}

# data_ok DBM [PREFIX]: data is present, intact and the recorded manifest.
data_ok() {
	local dbm=$1 prefix=${2:-$oracle_root/$1} want
	want=$(pinval "data_manifest_sha256_$dbm")
	[[ -f $prefix/oracle-data.sha256 ]] || return 1
	[[ $(sha256sum "$prefix/oracle-data.sha256" | cut -d' ' -f1) == "$want" ]] || return 1
	(cd "$prefix" && sha256sum --check --quiet oracle-data.sha256 >/dev/null 2>&1)
}

# Run in a subshell so reading old assignments cannot affect provisioning.
write_oracle_env() (
	env_file=${OXPINYIN_ORACLE_ENV:-$HOME/.oxpinyin-oracle.env}
	declare -A prefixes=()
	env_cells=("${cells[@]}")
	kept_tkrzw_paths=
	for dbm in "${cells[@]}"; do prefixes[$dbm]=$oracle_root/$dbm; done
	if [[ -f $env_file ]]; then
		# Read only the writer's membership and per-cell assignments. Do not run
		# the selector (the caller may name a cell that was never provisioned),
		# or import inherited per-cell variables from the invoking shell.
		_oxp_provisioned=
		for dbm in bdb kc tkrzw; do
			unset "OXPINYIN_ORACLE_PREFIX_${dbm^^}" "OXPINYIN_ORACLE_DATA_${dbm^^}"
		done
		source <(sed -nE '/^(_oxp_provisioned=|export OXPINYIN_ORACLE_(PREFIX|DATA)_(BDB|KC|TKRZW)=)/p' "$env_file")
		for dbm in bdb kc tkrzw; do
			[[ -z ${prefixes[$dbm]:-} && $_oxp_provisioned == *" $dbm "* ]] || continue
			prefix_var=OXPINYIN_ORACLE_PREFIX_${dbm^^}
			prefix=${!prefix_var:-}
			if [[ -z $prefix ]] || ! libs_ok "$dbm" "$prefix"; then
				log "$dbm: removed from environment file; oracle-pin.txt library hash check failed"
			elif ! data_ok "$dbm" "$prefix"; then
				log "$dbm: removed from environment file; data manifest check failed"
			else
				prefixes[$dbm]=$prefix
				env_cells+=("$dbm")
				if [[ $dbm == tkrzw ]]; then
					# Preserve a retained cell's sound local tkrzw runtime, even
					# when this run uses another cache or requests only bdb.
					kept_tkrzw_paths=$(sed -n '/^export PKG_CONFIG_PATH=/,/^export PATH=/p' "$env_file")
				fi
			fi
		done
	fi
	default_cell=${env_cells[0]}
	for dbm in "${env_cells[@]}"; do [[ $dbm == bdb ]] && default_cell=bdb; done
	trap 'rm -f "$env_file.new"' EXIT
	{
		printf '# Written by tools/cloud/setup.sh; source it, do not edit it.\n'
		printf '_oxp_provisioned=%q\n' " ${env_cells[*]} "
		printf 'export PINYIN_MODEL_DIR=%q\n' "$model_cache/extracted"
		printf 'export PATH=%s\n' "$(printf '%q' "$uv_dir"):\$PATH"
		if ((tkrzw_local)); then
			printf 'export PKG_CONFIG_PATH=%s\n' "$(printf '%q' "$tkrzw_prefix/lib/pkgconfig")\${PKG_CONFIG_PATH:+:\$PKG_CONFIG_PATH}"
			printf 'export LD_LIBRARY_PATH=%s\n' "$(printf '%q' "$tkrzw_prefix/lib")\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"
			printf 'export PATH=%s\n' "$(printf '%q' "$tkrzw_prefix/bin"):\$PATH"
		elif [[ -n $kept_tkrzw_paths ]]; then
			printf '%s\n' "$kept_tkrzw_paths"
		fi
		for dbm in "${env_cells[@]}"; do
			up=${dbm^^}
			printf 'export OXPINYIN_ORACLE_PREFIX_%s=%q\n' "$up" "${prefixes[$dbm]}"
			printf 'export OXPINYIN_ORACLE_DATA_%s=%q\n' "$up" "${prefixes[$dbm]}/lib/libpinyin/data"
		done
		# The selected cell (PINYIN_ORACLE_DBM, default the reference cell)
		# decides the prefix and data directory every time the file is sourced.
		printf "export PINYIN_ORACLE_DBM=\${PINYIN_ORACLE_DBM:-%s}\n" "$default_cell"
		cat <<'SELECT'
case $_oxp_provisioned in
    *" $PINYIN_ORACLE_DBM "*) ;;
    *) echo "cell $PINYIN_ORACLE_DBM was not provisioned by this environment file" >&2; unset _oxp_provisioned; return 1 ;;
esac
_oxp_cell=${PINYIN_ORACLE_DBM^^}
_oxp_prefix=OXPINYIN_ORACLE_PREFIX_$_oxp_cell
_oxp_data=OXPINYIN_ORACLE_DATA_$_oxp_cell
export PINYIN_ORACLE_PREFIX=${!_oxp_prefix:?cell not provisioned by tools/cloud/setup.sh}
export OXPINYIN_LIBPINYIN_DATA_DIR=${!_oxp_data}
unset _oxp_cell _oxp_prefix _oxp_data _oxp_provisioned
SELECT
	} >"$env_file.new" || return
	mv "$env_file.new" "$env_file" || return

	log "wrote $env_file; source it before a runner"
)
