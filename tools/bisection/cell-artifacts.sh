#!/usr/bin/env bash
# Shared build stage; executions consume these paths without invoking Cargo.
#
# Usage: cell-artifacts.sh [--artifacts LIST]
#        cell-artifacts.sh --describe | --describe-oracle BASE_PREFIX VARIANT
#
# LIST names the artifacts to build, comma-separated, from the set every
# runners.json row declares under "artifacts": pinyin-capi, zhuyin-capi,
# dictool, user-rt-test. The packages are built in one cargo invocation and
# an empty LIST builds nothing. Without --artifacts the three libraries are
# built, plus user-rt-test when OXPINYIN_BUILD_USER_RT=1 (the pre-selection
# behaviour, kept for direct callers).
set -euo pipefail
REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
# shellcheck source=tools/bisection/oracle-cell.sh
source "$REPO_ROOT/tools/bisection/oracle-cell.sh"
export PINYIN_ORACLE_DBM=$ORACLE_DBM
if [[ ${1:-} == --describe-oracle && $# == 3 ]]; then
    oracle_cell_resolve_prefix "$2" "$3"
    set -- --describe
fi
if [[ ${1:-} == --describe ]]; then
	python3 - <<'PY'
import json, os
keys = ('PINYIN_ORACLE_DBM', 'ORACLE_DBM', 'CAPI_FEATURE',
        'OXPINYIN_CAPI_BACKEND_EXT', 'EXPECTED_PIN_REF',
        'CARGO_TARGET_DIR', 'CARGO_PROFILE_DEV_OPT_LEVEL')
keys += ('PINYIN_ORACLE_PREFIX', 'EXPECTED_ORACLE_PIN_REF', 'ORACLE_PATCH_FILE')
print(json.dumps({key: os.environ[key] for key in keys if key in os.environ}))
PY
	exit 0
fi
artifacts=pinyin-capi,zhuyin-capi,dictool
[[ ${OXPINYIN_BUILD_USER_RT:-0} == 1 ]] && artifacts+=,user-rt-test
if [[ ${1:-} == --artifacts && $# == 2 ]]; then
	artifacts=$2
	set --
fi
[[ $# == 0 ]] || {
	echo 'usage: cell-artifacts.sh [--artifacts LIST | --describe | --describe-oracle BASE_PREFIX VARIANT]' >&2
	exit 2
}
packages=()
build_user_rt=0
IFS=, read -ra wanted <<<"$artifacts"
for artifact in "${wanted[@]}"; do
	case $artifact in
	'') ;;
	pinyin-capi) packages+=(-p oxpinyin-capi) ;;
	zhuyin-capi) packages+=(-p oxpinyin-zhuyin-capi) ;;
	dictool) packages+=(-p oxpinyin-dictool) ;;
	user-rt-test) build_user_rt=1 ;;
	*)
		echo "fatal: unknown artifact '$artifact' (pinyin-capi, zhuyin-capi, dictool, user-rt-test)" >&2
		exit 2
		;;
	esac
done
if [[ ${#packages[@]} -gt 0 ]]; then
	cargo build --locked --manifest-path "$REPO_ROOT/Cargo.toml" "${packages[@]}" \
		--no-default-features --features "$CAPI_FEATURE"
fi
if [[ $build_user_rt == 1 ]]; then
	manifest=$(mktemp)
	trap 'rm -f "$manifest"' EXIT
	cargo test --locked --profile dev --no-run --message-format=json \
		--manifest-path "$REPO_ROOT/Cargo.toml" -p oxpinyin-runtime \
		--no-default-features --features "$CAPI_FEATURE" \
		--test user_dir_round_trip >"$manifest"
	python3 - "$manifest" "$CARGO_TARGET_DIR/debug/user-dir-round-trip-test" <<'PY'
import json, os, pathlib, sys
executables = []
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
    row = json.loads(line)
    if (row.get('reason') == 'compiler-artifact' and
            row.get('target', {}).get('name') == 'user_dir_round_trip' and
            row.get('executable')):
        executables.append(row['executable'])
if len(executables) != 1:
    raise SystemExit('FAIL: expected exactly one user_dir_round_trip executable')
link = pathlib.Path(sys.argv[2])
# Concurrent builds of the same cell share this path: rename a private link
# over it, so no invocation sees it missing or fails on a competing create.
staged = link.with_name(f'.{link.name}.{os.getpid()}')
staged.unlink(missing_ok=True)
staged.symlink_to(executables[0])
os.replace(staged, link)
PY
fi
