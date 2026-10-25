#!/usr/bin/env bash
# Shared selection only; callers retain their runtime-specific execution paths.

oxpinyin_select_runtime() {
	local override=${1:-${OXPINYIN_CONTAINER:-}} host version candidate
	local -a candidates
	host=$(uname -s)
	case $host in
	Darwin) candidates=(container podman docker) ;;
	MINGW*|MSYS*|CYGWIN*) candidates=(wslc podman docker) ;;
	*)
		if [[ -n ${WSL_DISTRO_NAME:-} ]]; then candidates=(wslc podman docker)
		else candidates=(podman docker); fi ;;
	esac
	if [[ -n $override ]]; then
		ctr=$override
	else
		ctr=''
		for candidate in "${candidates[@]}"; do
			if command -v "$candidate" >/dev/null 2>&1; then ctr=$candidate; break; fi
		done
	fi
	if [[ -z $ctr ]] || ! command -v "$ctr" >/dev/null 2>&1; then
		printf 'FAIL: no runtime available; candidates in order: %s (override: %s)\n' "${candidates[*]}" "${override:-none}" >&2
		return 1
	fi
	if [[ $(basename "$ctr") == wslc ]]; then
		printf 'Runtime: %s; version unavailable (adapter not yet verified)\n' "$ctr" >&2
		echo 'FAIL: wslc adapter not yet verified; select an exercised runtime explicitly with OXPINYIN_CONTAINER or --runtime' >&2
		return 1
	fi
	version=$("$ctr" --version) || { echo "FAIL: cannot query runtime version: $ctr" >&2; return 1; }
	printf 'Runtime: %s; %s\n' "$ctr" "$version"
	if [[ $(basename "$ctr") == container ]]; then
		echo 'Apple container interoperability and native-platform execution are not yet verified by the compat smoke.' >&2
	fi
}
