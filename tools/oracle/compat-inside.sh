#!/usr/bin/env bash
# Invoked only by compat.sh, inside the controlled image and Linux volume.
set -euo pipefail
[[ ${OXPINYIN_COMPAT_CONTAINER:-} == 1 && $(uname -sm) == 'Linux x86_64' ]] || {
	echo 'FAIL: compatibility requires the controlled Linux amd64 image' >&2; exit 2;
}
cell=${1:?cell}; identity=${2:?image identity}; run_id=${3:?run id}; reference_id=${4:?reference identity}; shift 4
mkdir -p /state/home /state/cargo /state/work /state/results/"$run_id"
# A volume is private to one checkout, and one invocation owns it at a time.
exec 9>/state/run.lock
flock 9
result=/state/results/$run_id
printf 'Execution environment: '; uname -sm
printf 'Reference/image identity: %s; cell: %s\n' "$identity" "$cell"
cat /input/source.txt
# Preserve Linux outputs while replacing the current source snapshot completely.
find /state/work -mindepth 1 -maxdepth 1 ! -name .git ! -name target -exec rm -rf -- {} +
tar -xf /input/source.tar -C /state/work
cd /state/work
# Existing runners need the Git root, not access to a host worktree's .git.
git init -q
printf 'Linux snapshot Git root: '; git rev-parse --show-toplevel
export OXPINYIN_ORACLE_ENV=/state/cache/$reference_id/oracle.env
export PINYIN_MODEL_CACHE=/state/model20
export PINYIN_ORACLE_DBM=$cell
export OXPINYIN_TARGET_ROOT=/state/targets/$identity
mkdir -p "$(dirname "$OXPINYIN_ORACLE_ENV")"
if [[ -s /input/model.tar.gz ]]; then
	bash tools/model/fetch-model.sh --cache-dir "$PINYIN_MODEL_CACHE" --archive /input/model.tar.gz
fi
bash tools/cloud/setup.sh --in-container --cells "$cell" --cache-dir "/state/cache/$reference_id" --jobs "${CARGO_BUILD_JOBS:-2}"
# Only this newly generated environment is sourced; host overrides never cross in.
# shellcheck disable=SC1090 # Generated and verified by setup.sh above.
source "$OXPINYIN_ORACLE_ENV"
printf 'Subject Rust toolchain: '; rustup show active-toolchain
set +e
bash tools/bisection/run-all.sh --cells "$cell" --oracle "$cell=$PINYIN_ORACLE_PREFIX" \
	--target-root "$OXPINYIN_TARGET_ROOT" --json "$result/results.json" --log-dir "$result/logs" "$@" 2>&1 | tee "$result/comparison.log"
status=${PIPESTATUS[0]}
set -e
printf 'Compatibility command exit: %s (missing/skipped inputs are not passes)\n' "$status"
exit "$status"
