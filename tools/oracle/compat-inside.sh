#!/usr/bin/env bash
# Bootstrap: establish image-owned interpreter paths before managed Python runs.
# Invoked only by compat.sh, inside the controlled image and Linux volume.

# compat.sh runs this as /input/inside.sh and stages the floor file beside it.
# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/bash-floor.sh"
# Bash only warns when `.` cannot read the file; stop rather than skip the check.
[[ -n ${OXPINYIN_BASH_FLOOR-} ]] || { echo "FAIL: $0: ${0%/*}/bash-floor.sh not sourced; Bash floor unchecked" >&2; exit 2; }

set -euo pipefail
[[ ${OXPINYIN_COMPAT_CONTAINER:-} == 1 && $(uname -s) == Linux &&
	($(uname -m) == x86_64 || $(uname -m) == aarch64 || $(uname -m) == arm64) ]] || {
	echo 'FAIL: compatibility requires a controlled Linux amd64 or arm64 image' >&2; exit 2;
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
python_request=$(</opt/python-pin/.python-version)  # the image's copy: no checkout is the working directory yet
exec uv run --no-project --managed-python --python "$python_request" python -X lazy_imports=normal \
    /input/guest.py "$cell" "$identity" "$run_id" "$reference_id" "$input_identity" "$@"
