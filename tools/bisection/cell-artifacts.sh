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
REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)  # physical path: the identity names one worktree
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
# An empty selection builds nothing and claims nothing: the no-op stays a no-op.
if [[ ${#packages[@]} -eq 0 && $build_user_rt == 0 ]]; then
	exit 0
fi
# One target directory belongs to one worktree, one cell and one profile.
# Claim it before cargo runs: write and flush a private file, then publish
# it with a hard link that fails if the identity file exists. A later builder
# reads the complete claim and must match its owner, so two worktrees racing
# into one unclaimed directory cannot both enter cargo (#889: a target shared
# between a baseline and a candidate worktree linked units of both into the candidate's
# libraries, and the mixed objects were only noticed in DWARF). The full
# record, with the staged artifacts, is written after the build; a failed
# build leaves the claim or the previous record in place.
identity=$CARGO_TARGET_DIR/oxpinyin-cell-identity.json
mkdir -p "$CARGO_TARGET_DIR"
python3 - "$identity" "$REPO_ROOT" "$ORACLE_DBM" "$CAPI_FEATURE" "${CARGO_PROFILE_DEV_OPT_LEVEL}" <<'PY'
import datetime, json, os, sys, tempfile
path, worktree, cell, feature, opt_level = sys.argv[1:]
wanted = dict(worktree=worktree, cell=cell, feature=feature, profile='dev', opt_level=opt_level)
fd, staged = tempfile.mkstemp(prefix='.oxpinyin-cell-claim-', dir=os.path.dirname(path))
try:
    with os.fdopen(fd, 'w') as f:
        json.dump(dict(schema=1, **wanted, artifacts={},
                       claimed_utc=datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')),
                  f, indent=1, sort_keys=True)
        f.write('\n')
        f.flush()
    try:
        os.link(staged, path)
    except FileExistsError:
        try:
            with open(path, encoding='utf-8') as f:
                owner = json.load(f)
            if not isinstance(owner, dict):
                raise ValueError('expected a JSON object')
        except (OSError, ValueError, UnicodeError) as exc:
            sys.exit('fatal: unreadable identity record %s: %s' % (path, exc))
        claimed = {key: owner.get(key) for key in wanted}
        if claimed != wanted:
            print('fatal: target directory %s belongs to %s; this build is %s. One target directory per '
                  '(worktree, cell, profile): use another --target-root/CARGO_TARGET_DIR, or remove the '
                  'directory to start fresh.' % (path, json.dumps(claimed, sort_keys=True),
                                                 json.dumps(wanted, sort_keys=True)), file=sys.stderr)
            sys.exit(3)
finally:
    os.unlink(staged)
PY
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
# Record the identity of what this directory now holds (written last, so a
# failed build leaves the previous record, and atomically).
python3 - "$identity" "$REPO_ROOT" "$ORACLE_DBM" "$CAPI_FEATURE" "${CARGO_PROFILE_DEV_OPT_LEVEL}" \
	"$CARGO_TARGET_DIR" <<'PY'
import datetime, hashlib, json, os, subprocess, sys
path, worktree, cell, feature, opt_level, target = sys.argv[1:]
def run(*command):
    return subprocess.run(command, cwd=worktree, capture_output=True, text=True, check=True).stdout
rustc = dict(line.split(': ', 1) for line in run('rustc', '-vV').splitlines() if ': ' in line)
head = run('git', 'rev-parse', 'HEAD').strip()
dirty = hashlib.sha256(run('git', 'diff', 'HEAD', '--', '.').encode()).hexdigest()
artifacts = {}
for name, filename in (('pinyin-capi', 'libpinyin_capi.so'), ('zhuyin-capi', 'libzhuyin_capi.so'),
                       ('dictool', 'oxpinyin-dictool'), ('user-rt-test', 'user-dir-round-trip-test')):
    file = os.path.join(target, 'debug', filename)
    if os.path.exists(file):
        stat = os.stat(file)  # follows the user-rt-test symlink to the executable
        artifacts[name] = dict(path=file, size=stat.st_size, mtime_ns=stat.st_mtime_ns)
record = dict(schema=1, worktree=worktree, cell=cell, feature=feature, profile='dev', opt_level=opt_level,
              toolchain=dict(release=rustc.get('release'), commit=rustc.get('commit-hash'), host=rustc.get('host')),
              source=dict(head=head, tracked_diff_sha256=dirty),
              built_utc=datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
              artifacts=artifacts)
staged = path + '.%d' % os.getpid()
with open(staged, 'w') as f:
    json.dump(record, f, indent=1, sort_keys=True)
    f.write('\n')
os.replace(staged, path)
PY
