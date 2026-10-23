#!/usr/bin/env bash
# Invoked only by compat.sh, inside the controlled image and Linux volume.
set -euo pipefail
[[ ${OXPINYIN_COMPAT_CONTAINER:-} == 1 && $(uname -sm) == 'Linux x86_64' ]] || {
	echo 'FAIL: compatibility requires the controlled Linux amd64 image' >&2; exit 2;
}
cell=${1:?cell}; identity=${2:?image identity}; run_id=${3:?run id}; reference_id=${4:?reference identity}
input_identity=${5:?input content identity}; shift 5
[[ $cell =~ ^(bdb|kc|tkrzw)$ && $identity =~ ^[a-f0-9]{16}$ && $run_id =~ ^[0-9]{8}T[0-9]{6}Z-[0-9]+$ ]] || {
	echo 'FAIL: invalid container namespace input' >&2; exit 2;
}
# Set every state path explicitly. The interpreter installation belongs to the
# immutable image; writable caches, targets and inner admission belong to volume.
unset OXPINYIN_HEAVY_SLOT_HELD CARGO_TARGET_DIR PYTHONHOME PYTHONPATH
export HOME=/state/container/$identity/home
export CARGO_HOME=/state/container/$identity/cargo RUSTUP_HOME=/opt/rustup
export UV_PYTHON_INSTALL_DIR=/opt/uv/python UV_CACHE_DIR=/state/container/$identity/uv/cache
export UV_INSTALL_DIR=/opt/uv/bin
export OXPINYIN_LOCK_DIR=/state/container/$identity/locks
export XDG_CACHE_HOME=/state/container/$identity/cache
export PATH=/opt/uv/bin:/opt/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export OXPINYIN_COMPAT_INPUT_SHA256=$input_identity
export OXPINYIN_COMPAT_IMAGE_ID=$identity OXPINYIN_COMPAT_REFERENCE_ID=$reference_id
source_identity=$(uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal \
	/input/identity.py verify-inputs /input "$input_identity" "$identity" "$reference_id")
uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal \
	/input/identity.py verify-state "$identity" "$source_identity" "$cell" "$run_id"
result=/state/results/container/$source_identity/$run_id
mkdir -p "$HOME" "$CARGO_HOME" "$UV_CACHE_DIR" "$OXPINYIN_LOCK_DIR" /state/work "$result"
# A volume is private to one checkout, and one invocation owns it at a time.
exec 9>/state/run.lock
flock 9
printf 'Execution environment: '; uname -sm
printf 'Reference/image identity: %s; cell: %s\n' "$identity" "$cell"
cat /input/source.txt
# Preserve Linux outputs while replacing the current source snapshot completely.
find /state/work -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
tar --same-permissions -xf /input/source.tar -C /state/work
cd /state/work
# Existing runners need the Git root, not access to a host worktree's .git.
git init -q
printf 'Linux snapshot Git root: '; git rev-parse --show-toplevel
export OXPINYIN_ORACLE_ENV=/state/cache/$reference_id/oracle.env
export PINYIN_MODEL_CACHE=/state/model20
export PINYIN_ORACLE_DBM=$cell
export OXPINYIN_TARGET_ROOT=/state/container/$identity/targets/$source_identity
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
uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal \
	-m tools.dev.compat.identity seal-results "$result" "$OXPINYIN_TARGET_ROOT" "$cell"
printf 'Compatibility command exit: %s (missing/skipped inputs are not passes)\n' "$status"
exit "$status"
