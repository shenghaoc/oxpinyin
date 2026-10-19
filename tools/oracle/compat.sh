#!/usr/bin/env bash
# Native development stays native. This launcher runs selected registry gates
# wholly inside the pinned Linux amd64 environment, without a nested runtime.
set -euo pipefail
umask 077
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo"
usage() {
	cat <<'HELP'
Usage: bash tools/oracle/compat.sh [OPTIONS] --runners NAME[,NAME...]
       bash tools/oracle/compat.sh [OPTIONS] --issue NUMBER
       bash tools/oracle/compat.sh --list
Options:
  --runtime docker|podman|container|wslc  Override platform-native, Podman, Docker order
  --cell bdb|kc|tkrzw               Default: bdb; one backend on both sides
  --archive PATH                    Optional local model20 archive (checksum verified)
  --jobs N                          Build/runner concurrency (default 2)
  --timeout N                       Existing registry runner timeout override
  --runner-timeout NAME=SECONDS     Existing per-runner override
  --list                            Existing registry listing; needs Python 3, no build
  --help                            No runtime or build required
Needs Bash, Git, tar, SHA-256 utility, and a working runtime. macOS/WSL/Linux
hosts need no Rust, native DBM or libpinyin for compatibility execution.
Results remain local in target/compat/results; inputs/targets live in a
checkout-specific Linux volume. See docs/runbooks/compatibility-container.md.
HELP
}
die() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
hash() { if command -v sha256sum >/dev/null 2>&1; then sha256sum; else shasum -a 256; fi; }
runtime=${OXPINYIN_CONTAINER:-} cell=bdb archive='' jobs=2
selection=() selected=0
while (($#)); do
	case $1 in
	--help|-h) usage; exit 0 ;;
	--list) exec python3 tools/bisection/run-all.py --list ;;
	--runners=*|--issue=*) set -- "${1%%=*}" "${1#*=}" "${@:2}"; continue ;;
	--runtime|--cell|--archive|--jobs|--runners|--issue|--timeout|--runner-timeout)
		(($# >= 2)) || { usage >&2; exit 2; }
		case $1 in
		--runtime) runtime=$2 ;;
		--cell) cell=$2 ;;
		--archive) archive=$2 ;;
		--jobs) jobs=$2 ;;
		--runners|--issue)
			[[ -n ${2//[[:space:],]/} ]] || { printf 'Give a value for %s.\n' "$1" >&2; exit 2; }
			selection+=("$1" "$2"); selected=1 ;;
		*) selection+=("$1" "$2") ;;
		esac
		shift 2 ;;
	*) printf 'unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
	esac
done
case $cell in bdb|kc|tkrzw) ;; *) echo 'cell must be bdb, kc or tkrzw' >&2; exit 2 ;; esac
[[ $jobs =~ ^[1-9][0-9]*$ ]] || { echo '--jobs must be positive' >&2; exit 2; }
((selected)) || { echo 'select --runners or --issue (use --list to discover gates)' >&2; exit 2; }
for command in git tar; do command -v "$command" >/dev/null || die "missing $command"; done
# shellcheck source=../container/runtime.sh
source "$repo/tools/container/runtime.sh"
oxpinyin_select_runtime "$runtime"
runtime=$ctr
case $(uname -s) in MINGW*|MSYS*) die 'Run this compatibility launcher from WSL Bash, not Git Bash. Native Windows development needs neither WSL nor containers.' ;; esac
case $(basename "$runtime") in docker|podman|container) ;; *) die 'runtime must be docker, podman, container or unverified wslc' ;; esac
if [[ $(basename "$runtime") == container ]]; then
	"$runtime" image list --quiet >/dev/null || die 'Apple container service unavailable; see container system start'
else
	"$runtime" info >/dev/null || die "$runtime capability unavailable; check its installation/access (no daemon or host packages are installed by this launcher)"
fi
mkdir -p target/compat/results
run_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
results=$repo/target/compat/results/$run_id
mkdir -p "$results"
input=$(mktemp -d "${TMPDIR:-/tmp}/oxpinyin-compat.XXXXXX")
name=oxpinyin-compat-$(date -u +%s)-$$
created=0
# shellcheck disable=SC2329 # Invoked by the EXIT trap.
cleanup() {
	if ((created)); then "$runtime" rm -f "$name" >/dev/null 2>&1 || true; fi
	rm -rf "$input"
}
trap cleanup EXIT
if [[ -n $archive ]]; then
	[[ -f $archive ]] || die "local model archive missing: $archive"
	cp "$archive" "$input/model.tar.gz"
else
	: > "$input/model.tar.gz"
fi
# Snapshot visible source, including uncommitted edits. No host .git pointer,
# ignored model/cache bytes, native target output or other worktrees cross in.
git ls-files --cached --others --exclude-standard -z | while IFS= read -r -d '' path; do
	if [[ -e $path || -L $path ]]; then printf '%s\0' "$path"; fi
done > "$input/files"
tar --null -T "$input/files" -cf "$input/source.tar"
{
	printf 'Host source commit: '; git rev-parse HEAD
	printf 'Host source status:\n'; git status --short
} > "$input/source.txt"
printf 'Source archive SHA-256: %s\n' "$(hash < "$input/source.tar" | cut -d' ' -f1)" >> "$input/source.txt"
cp "$input/source.tar" "$results/source.tar"
cp "$input/source.txt" "$results/source.txt"
cp tools/oracle/compat-inside.sh "$input/inside.sh"
reference_tag=$(bash tools/oracle/image-tag.sh)
reference=ghcr.io/shenghaoc/oxpinyin-oracle:$reference_tag
deps=$reference-deps
ca_identity=no-custom-ca
if [[ -n ${OXPINYIN_PROXY_CA:-} ]]; then
	[[ -s $OXPINYIN_PROXY_CA ]] || die "custom CA file missing: $OXPINYIN_PROXY_CA"
	ca_identity=$(hash < "$OXPINYIN_PROXY_CA" | cut -d' ' -f1)
fi
key=$( { cat tools/oracle/Dockerfile.compat rust-toolchain.toml; printf '%s:%s' "$reference_tag" "$ca_identity"; } | hash | cut -c1-16)
image=oxpinyin-compat:$key
volume=oxpinyin-compat-$(printf '%s' "$repo" | hash | cut -c1-16)
platform=(--platform linux/amd64)
relabel=
[[ $(basename "$runtime") == podman ]] && relabel=,z
# Only explicit proxy settings cross the boundary; ordinary proxies need no
# Claude files. Registry/daemon trust remains the runtime owner's configuration.
build_flags=() run_env=(--cpus "$jobs" -e "CARGO_BUILD_JOBS=$jobs" -e "RUST_TEST_THREADS=$jobs")
if [[ $(basename "$runtime") == podman ]]; then
	build_flags+=(--cpu-period 100000 --cpu-quota "$((jobs * 100000))" --jobs 1)
elif [[ $(basename "$runtime") == container ]]; then
	build_flags+=(--cpus "$jobs")
fi
for variable in HTTP_PROXY HTTPS_PROXY NO_PROXY http_proxy https_proxy no_proxy; do
	value=${!variable:-}
	if [[ -n $value ]]; then build_flags+=(--build-arg "$variable=$value"); run_env+=(-e "$variable=$value"); fi
done
if [[ -n ${OXPINYIN_COMPAT_NETWORK:-} ]]; then
	[[ $(basename "$runtime") != container ]] || die 'Apple builder does not expose Docker/Podman --network; use a proxy reachable on its VM network'
	build_flags+=(--network "$OXPINYIN_COMPAT_NETWORK")
	run_env+=(--network "$OXPINYIN_COMPAT_NETWORK")
fi
if [[ -n ${OXPINYIN_PROXY_CA:-} ]]; then
	[[ -s $OXPINYIN_PROXY_CA ]] || die "custom CA file missing: $OXPINYIN_PROXY_CA"
	build_flags+=(--secret "id=cacert,src=$OXPINYIN_PROXY_CA")
fi
image_present() {
	if [[ $(basename "$runtime") == container ]]; then
		"$runtime" image list --quiet | grep -Fxq "$1"
	else
		"$runtime" image inspect "$1" >/dev/null 2>&1
	fi
}
build_image() {
	"$runtime" build "$@" 2>&1 | tee -a "$results/build.log"
}
pull_or_build() {
	local wanted=$1 target=$2 error=$results/pull-$2.log
	image_present "$wanted" && return 0
	if [[ $(basename "$runtime") == container ]]; then
		if "$runtime" image pull "${platform[@]}" "$wanted" 2>"$error"; then return 0; fi
	else
		if "$runtime" pull "${platform[@]}" "$wanted" 2>"$error"; then return 0; fi
	fi
	cat "$error" >&2
	if ! grep -Eiq 'manifest unknown|name unknown|manifest.*not found|404[[:space:]]+not found' "$error"; then
		die "reference image access/network/proxy failure: $wanted (retained $error); no silent fallback"
	fi
	printf 'Image tag absent; building %s from the pinned source recipe\n' "$wanted" >&2
	build_image "${platform[@]}" "${build_flags[@]}" --target "$target" \
		-f tools/oracle/Dockerfile.oracle -t "$wanted" tools/oracle
}
# The unchanged oracle final stage has no name; use a private build stage name
# only for deps and build the artifact's default final target separately.
if ! image_present "$image"; then
	pull_or_build "$deps" deps
	if ! image_present "$reference"; then
		if [[ $(basename "$runtime") == container ]]; then pull_command=("$runtime" image pull); else pull_command=("$runtime" pull); fi
		if ! "${pull_command[@]}" "${platform[@]}" "$reference" 2>"$results/pull-reference.log"; then
			cat "$results/pull-reference.log" >&2
			grep -Eiq 'manifest unknown|name unknown|manifest.*not found|404[[:space:]]+not found' "$results/pull-reference.log" || die 'reference access/network/proxy failure; see retained pull-reference.log'
			build_image "${platform[@]}" "${build_flags[@]}" -f tools/oracle/Dockerfile.oracle -t "$reference" tools/oracle
		fi
	fi
	build_image "${platform[@]}" "${build_flags[@]}" \
		--build-arg "DEPS_IMAGE=$deps" --build-arg "REFERENCE_IMAGE=$reference" \
		--build-arg "CA_IDENTITY=$ca_identity" \
		-f tools/oracle/Dockerfile.compat -t "$image" .
fi
if [[ $(basename "$runtime") == container ]]; then
	"$runtime" volume inspect "$volume" >/dev/null 2>&1 || "$runtime" volume create -s 20G "$volume"
else
	"$runtime" volume inspect "$volume" >/dev/null 2>&1 || "$runtime" volume create "$volume"
fi
printf 'Compatibility: Linux amd64; non-amd64 hosts require emulation. No native performance claims.\nVolume: %s\nResults: %s\n' "$volume" "$results"
created=1
set +e
"$runtime" run --name "$name" "${platform[@]}" "${run_env[@]}" \
	-v "$volume:/state" -v "$input:/input:ro$relabel" \
	-w / "$image" bash /input/inside.sh "$cell" "$key" "$run_id" "$reference_tag" \
	"${selection[@]}" --jobs "$jobs" 2>&1 | tee "$results/console.log"
status=${PIPESTATUS[0]}
set -e
if [[ $(basename "$runtime") == container ]]; then
	# Apple copy requires a running container; read the persisted volume instead.
	"$runtime" run --rm "${platform[@]}" --cpus "$jobs" -v "$volume:/state:ro" -w / "$image" \
		tar -C "/state/results/$run_id" -cf - . | tar -C "$results" -xf -
else
	"$runtime" cp "$name:/state/results/$run_id/." "$results/"
fi || echo "Additional result files unavailable; console retained at $results/console.log" >&2
exit "$status"
