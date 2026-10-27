#!/usr/bin/env bash
# Shared oracle verification, host probes and environment writer for setup.sh.

# tkrzw_so FILE: where the platform loader, in the caller's environment,
# resolves FILE's libtkrzw (empty if it does not). Linux uses ldd; Darwin
# uses otool -L and the Homebrew dylib's install name. setup.sh provisions
# managed Python before these post-bootstrap probes.

tkrzw_so() {
    local runtime_root
    runtime_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
    DYLD_LIBRARY_PATH=${DYLD_LIBRARY_PATH:-} DYLD_FALLBACK_LIBRARY_PATH=${DYLD_FALLBACK_LIBRARY_PATH:-} \
    uv --quiet run --no-project --managed-python --python "$(<"$runtime_root/.python-version")" python -X lazy_imports=normal \
        "$runtime_root/tools/tkrzw/runtime.py" --resolve "$1"
}

# tkrzw_env_prefix [FILE]: the cache runtime named by an environment file.
# Reading its generated LD_LIBRARY_PATH assignment also recognizes files
# written before they recorded OXPINYIN_TKRZW_PREFIXES.
tkrzw_env_prefix() (
	local env_file=${1:-${OXPINYIN_ORACLE_ENV:-$HOME/.oxpinyin-oracle.env}} lib
	[[ -f $env_file ]] || return 0
	if grep -q '^export OXPINYIN_TKRZW_PREFIX=' "$env_file"; then
		local OXPINYIN_TKRZW_PREFIX=
		# shellcheck disable=SC1090  # generated assignment from the environment file, not a fixed path.
		source <(sed -n '/^export OXPINYIN_TKRZW_PREFIX=/p' "$env_file")
		printf '%s' "$OXPINYIN_TKRZW_PREFIX"
		return
	fi
	# An exact host identity is not a cache prefix, even when its directory
	# is prepended to LD_LIBRARY_PATH by the generated environment.
	grep -q '^export OXPINYIN_TKRZW_LIBRARY=' "$env_file" && return 0
	# Intentional: parse only the generated path, without caller inheritance.
	LD_LIBRARY_PATH=
	# shellcheck disable=SC1090  # generated assignment from the environment file, not a fixed path.
	source <(sed -n '/^export LD_LIBRARY_PATH=/p' "$env_file")
	lib=${LD_LIBRARY_PATH%%:*}
	[[ $lib == */lib ]] || return 0
	printf '%s' "${lib%/lib}"
)

# without_prefix LIST: remove this run's cache and every cache runtime
# added by sourced environment files. Cached builds are never the host.
without_prefix() {
	local entry prefix out=() excluded=() IFS=:
	read -r -a excluded <<<"${OXPINYIN_TKRZW_PREFIXES:-}"
	# shellcheck disable=SC2154  # set by setup.sh or the harness before this call.
	excluded+=("$tkrzw_prefix" "$(tkrzw_env_prefix)")
	for entry in $1; do
		for prefix in "${excluded[@]}"; do
			[[ -n $prefix ]] || continue
			if [[ $entry == "$prefix" || $entry == "$prefix"/* ]]; then continue 2; fi
		done
		out+=("$entry")
	done
	printf '%s' "${out[*]}"
}

# probe_host: the probe against the host's own libtkrzw, and a loader
# check that the probe's tkrzw_dbm_util resolved libtkrzw outside the cache.
probe_host() {
	local host_path host_ld util so libname=libtkrzw.so.1
	tkrzw_host_library=
	if [ "$(uname -s)" = Darwin ]; then libname=libtkrzw.dylib; fi
	host_path=$(without_prefix "$PATH")
	host_ld=$(without_prefix "${LD_LIBRARY_PATH:-}")
	# Intentional: the host probe must exclude every inherited cache runtime.
	# shellcheck disable=SC2154  # set by setup.sh or the harness before this call.
	env PATH="$host_path" LD_LIBRARY_PATH="$host_ld" sh "$probe" || return 1
	util=$(PATH=$host_path command -v tkrzw_dbm_util) || return 1
	if ! so=$(LD_LIBRARY_PATH=$host_ld tkrzw_so "$util"); then
		return 1
	fi
	printf 'host check: %s resolves %s to %s\n' "$util" "$libname" "${so:-nothing}"
	[[ -n $so && -f $so && $(without_prefix "$so") == "$so" ]] || return 1
	so=$(readlink -f -- "$so") || return 1
	[[ $(without_prefix "$so") == "$so" ]] || return 1
	tkrzw_host_library=$so
}

# libs_ok DBM [PREFIX]: both libraries hash to the recorded recipe values.
libs_ok() {
	local dbm=$1 prefix=${2:-$oracle_root/$1} lib want
	for lib in libpinyin libzhuyin; do
		want=$(pinval_platform "${lib}_sha256_$dbm")
		[[ -f $prefix/lib/$lib.so ]] || return 1
		[[ $(sha256sum "$prefix/lib/$lib.so" | cut -d' ' -f1) == "$want" ]] || return 1
	done
}

# data_ok DBM [PREFIX]: stable data matches the pin; generated data matches
# the unstable manifest recorded by this prefix's own build.
data_ok() {
	local dbm=$1 prefix=${2:-$oracle_root/$1} want unstable_manifest
	want=$(pinval_platform "data_manifest_sha256_$dbm")
	[[ -f $prefix/oracle-data.sha256 ]] || return 1
	[[ $(sha256sum "$prefix/oracle-data.sha256" | cut -d' ' -f1) == "$want" ]] || return 1
	(cd "$prefix" && sha256sum --check --quiet oracle-data.sha256 >/dev/null 2>&1) || return 1
	unstable_manifest=$prefix/oracle-data-unstable.sha256
	if [[ ! -f $unstable_manifest ]]; then
		printf 'data manifest missing: %s\n' "$unstable_manifest" >&2
		return 1
	fi
	if [[ ! -f $prefix/oracle-pin.txt ]]; then
		printf 'data manifest provenance missing: %s/oracle-pin.txt\n' "$prefix" >&2
		return 1
	fi
	want=$(sed -n 's/^data_unstable_manifest_sha256=//p' "$prefix/oracle-pin.txt")
	if [[ -z $want || $(sha256sum "$unstable_manifest" | cut -d' ' -f1) != "$want" ]]; then
		printf 'data manifest SHA-256 differs from %s/oracle-pin.txt: %s\n' "$prefix" "$unstable_manifest" >&2
		return 1
	fi
	(cd "$prefix" && sha256sum --check --quiet oracle-data-unstable.sha256 >&2)
}

# Run in a subshell so reading old assignments cannot affect provisioning.
write_oracle_env() (
	env_file=${OXPINYIN_ORACLE_ENV:-$HOME/.oxpinyin-oracle.env}
	declare -A prefixes=()
	# shellcheck disable=SC2154  # set by setup.sh or the harness before this call.
	env_cells=("${cells[@]}")
	kept_tkrzw_paths=
	kept_tkrzw_library=
	for dbm in "${cells[@]}"; do prefixes[$dbm]=$oracle_root/$dbm; done
	if [[ -f $env_file ]]; then
		# Read only the writer's membership and per-cell assignments. Do not run
		# the selector (the caller may name a cell that was never provisioned),
		# or import inherited per-cell variables from the invoking shell.
		_oxp_provisioned=
		for dbm in bdb kc tkrzw; do
			unset "OXPINYIN_ORACLE_PREFIX_${dbm^^}" "OXPINYIN_ORACLE_DATA_${dbm^^}"
		done
		# shellcheck disable=SC1090  # generated assignments from the environment file, not a fixed path.
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
					kept_tkrzw_library=$(
						OXPINYIN_TKRZW_LIBRARY=
						# shellcheck disable=SC1090  # generated assignment from the environment file, not a fixed path.
						source <(sed -n '/^export OXPINYIN_TKRZW_LIBRARY=/p' "$env_file")
						printf '%s' "$OXPINYIN_TKRZW_LIBRARY"
					)
				fi
			fi
		done
	fi
	default_cell=${env_cells[0]}
	for dbm in "${env_cells[@]}"; do [[ $dbm == bdb ]] && default_cell=bdb; done
	runtime_prefix=
	runtime_library=
	# shellcheck disable=SC2154  # set by setup.sh or the harness before this call.
	if ((tkrzw_local)); then
		runtime_prefix=$tkrzw_prefix
	elif [[ -n ${tkrzw_host_library:-} ]]; then
		runtime_library=$tkrzw_host_library
	elif [[ -n $kept_tkrzw_paths ]]; then
		runtime_prefix=$(tkrzw_env_prefix "$env_file")
	fi
	if [[ -n $runtime_prefix ]]; then
		runtime_library=$(readlink -f -- "$runtime_prefix/lib/libtkrzw.so.1" 2>/dev/null || true)
	elif [[ -z $runtime_library ]]; then
		runtime_library=$kept_tkrzw_library
	fi
	trap 'rm -f "$env_file.new"' EXIT
	{
		printf '# Written by tools/cloud/setup.sh; source it, do not edit it.\n'
		printf '_oxp_provisioned=%q\n' " ${env_cells[*]} "
		# shellcheck disable=SC2154  # set by setup.sh or the harness before this call.
		printf 'export PINYIN_MODEL_DIR=%q\n' "$model_cache/extracted"
		# shellcheck disable=SC2154  # set by setup.sh or the harness before this call.
		printf 'export PATH=%s\n' "${uv_dir@Q}:\$PATH"
		if [[ -n $runtime_library ]]; then
			printf 'export OXPINYIN_TKRZW_LIBRARY=%q\n' "$runtime_library"
		fi
		if [[ -n $runtime_prefix ]]; then
			printf 'export OXPINYIN_TKRZW_PREFIX=%q\n' "$runtime_prefix"
			# Accumulate at source time: a shell can load several environment
			# files before setup probes a different cache's host library.
			printf 'export OXPINYIN_TKRZW_PREFIXES=%s\n' "${runtime_prefix@Q}\${OXPINYIN_TKRZW_PREFIXES:+:\$OXPINYIN_TKRZW_PREFIXES}"
		fi
		if ((tkrzw_local)); then
			printf 'export PKG_CONFIG_PATH=%s\n' "${tkrzw_prefix@Q}/lib/pkgconfig\${PKG_CONFIG_PATH:+:\$PKG_CONFIG_PATH}"
			printf 'export LD_LIBRARY_PATH=%s\n' "${tkrzw_prefix@Q}/lib\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"
			printf 'export PATH=%s\n' "${tkrzw_prefix@Q}/bin:\$PATH"
		elif [[ -n $runtime_library && -z $runtime_prefix ]]; then
			runtime_dir=${runtime_library%/*}
			printf 'export LD_LIBRARY_PATH=%s\n' "${runtime_dir@Q}\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"
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
