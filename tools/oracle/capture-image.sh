# shellcheck shell=bash
# The capture image the pin runs in, for capture-contract.sh and capture-pin.sh. Sourced only.
#
#   run_in_capture_image OUT COMMAND [ARGUMENTS...]
#
# Run from the checkout root, on a native x86_64 or aarch64 host. The function pulls the
# published oracle image when it is absent (python -m tools.oracle.image image-tag), builds
# the capture image (Dockerfile.capture) on top of it, and replaces the shell with COMMAND,
# run by the image's managed Python. The checkout is mounted read-only at /work, OUT (a path
# relative to the checkout) writable at /work/OUT, and the pinned libpinyin-data
# (`just fetch-contract-data`) at /data. The container has no network. With
# OXPINYIN_CAPTURE_PROVISION set to a directory, it is mounted read-only at /provision too, and
# OXPINYIN_CAPTURE_MODEL, a model20 cache, at /model20.

run_in_capture_image() {
    local out=$1 platform suffix expected_arch multiarch tag reference recipe capture actual_arch data image python
    local runtime=${OXPINYIN_CAPTURE_RUNTIME:-docker}
    local -a build_pull run_user mounts=()
    shift
    command -v "$runtime" >/dev/null || { echo "FAIL: $runtime is not installed" >&2; return 1; }
    case $(uname -m) in
        x86_64) platform=linux/amd64; suffix=; expected_arch=amd64; multiarch=x86_64-linux-gnu ;;
        aarch64 | arm64) platform=linux/arm64; suffix=-arm64; expected_arch=arm64; multiarch=aarch64-linux-gnu ;;
        *) echo "FAIL: capture supports only native x86_64 and aarch64" >&2; return 2 ;;
    esac
    tag=$(uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image image-tag | tail -n 1)
    reference=ghcr.io/shenghaoc/oxpinyin-oracle:$tag$suffix
    recipe=$(sha256sum tools/oracle/Dockerfile.capture tools/oracle/versions.py tools/oracle/version-manifest.json .python-version | sha256sum | cut -c1-12)
    capture=oxpinyin-capture:$tag$suffix-$recipe
    if ! "$runtime" image inspect "$reference" >/dev/null 2>&1; then
        if ! "$runtime" pull --platform "$platform" "$reference"; then
            echo "FAIL: $reference is unavailable; build it with: bash tools/oracle/build-contract-image.sh" >&2
            return 1
        fi
    fi
    actual_arch=$("$runtime" image inspect --format '{{.Architecture}}' "$reference")
    [[ $actual_arch == "$expected_arch" ]] || { echo "FAIL: $reference is $actual_arch, expected native $expected_arch" >&2; return 1; }
    case $runtime in
        *podman) build_pull=(--pull=never); run_user=() ;;
        *) build_pull=(--pull=false); run_user=(--user "$(id -u):$(id -g)") ;;
    esac
    # OXPINYIN_CAPTURE_BUILD_FLAGS carries site flags for the image build, such as --network host
    # or --secret id=cacert,src=... behind a TLS-intercepting egress proxy (no quoting inside).
    # shellcheck disable=SC2086
    "$runtime" image inspect "$capture" >/dev/null 2>&1 || "$runtime" build --platform "$platform" "${build_pull[@]}" ${OXPINYIN_CAPTURE_BUILD_FLAGS:-} -f tools/oracle/Dockerfile.capture --build-arg "REFERENCE_IMAGE=$reference" -t "$capture" . || return 1
    actual_arch=$("$runtime" image inspect --format '{{.Architecture}}' "$capture")
    [[ $actual_arch == "$expected_arch" ]] || { echo "FAIL: $capture is $actual_arch, expected native $expected_arch" >&2; return 1; }
    data=$(uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev fetch-contract-data | tail -n 1)
    image=$("$runtime" image inspect --format '{{.Id}}' "$capture")
    python=python$(sed -n 's/^\([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' .python-version)
    mkdir -p "$out"
    [[ -z ${OXPINYIN_CAPTURE_PROVISION:-} ]] || mounts+=(-v "$OXPINYIN_CAPTURE_PROVISION:/provision:ro")
    [[ -z ${OXPINYIN_CAPTURE_MODEL:-} ]] || mounts+=(-v "$OXPINYIN_CAPTURE_MODEL:/model20:ro")
    exec "$runtime" run --rm --platform "$platform" --network none "${run_user[@]}" \
        -e HOME=/tmp -e TMPDIR=/tmp -e PYTHONPYCACHEPREFIX=/tmp/pyc \
        -e "OXPINYIN_CAPTURE_IMAGE=$image" -e "OXPINYIN_TKRZW_LIBRARY=/usr/lib/$multiarch/libtkrzw.so.1" \
        -v "$PWD:/work:ro" -v "$PWD/$out:/work/$out:rw" -v "$data:/data:ro" "${mounts[@]}" -w /work \
        "$image" "$python" -X lazy_imports=normal "$@"
}
