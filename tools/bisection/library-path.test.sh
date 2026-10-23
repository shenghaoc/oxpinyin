#!/usr/bin/env bash
# Exercise the real runner's launch environment with an isolated driver fixture.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
caller=$work/'caller root'
mkdir -p "$caller/lib" "$work/bin" "$work/pin" "$work/subject" "$work/data"
touch "$work/pin/libpinyin.so" "$work/subject/libpinyin.so" "$work/data/gb_char.bin"

# The runner compiles its driver through this fixture. It checks the complete
# launch value, including subject-first ordering, then emits identical output.
cat >"$work/bin/gcc" <<'COMPILER'
#!/usr/bin/env bash
set -euo pipefail
while [[ $# -gt 0 && $1 != -o ]]; do shift; done
[[ $# -ge 2 ]]
cat >"$2" <<'DRIVER'
#!/usr/bin/env bash
set -euo pipefail
expected=$(dirname "$1")
expected+=${FIXTURE_INHERITED:+:$FIXTURE_INHERITED}
if [[ ${LD_LIBRARY_PATH-} != "$expected" ]]; then
    printf 'FAIL: expected <%s>, got <%s>\n' "$expected" "${LD_LIBRARY_PATH-}" >&2
    exit 1
fi
printf 'fixture: identical\n'
DRIVER
chmod +x "$2"
COMPILER
chmod +x "$work/bin/gcc"

selection=${1:-all}
passed=0
check() {
    local name=$1 input=$2 expected=$3 output status=0
    [[ $selection == all || $selection == "$name" ]] || return 0
    output=$(
        cd "$caller"
        if [[ $name == unset ]]; then unset LD_LIBRARY_PATH
        else export LD_LIBRARY_PATH=$input; fi
        export FIXTURE_INHERITED=$expected
        export PATH="$work/bin:$PATH"
        bash "$root/tools/bisection/run-same-data-dir-diff.sh" \
            "$work/pin/libpinyin.so" "$work/subject/libpinyin.so" "$work/data" key-surface-diff
    ) 2>&1 || status=$?
    if [[ $status != 0 || $output != *'IDENTICAL'* ]]; then
        printf 'FAIL: %s (runner exit %s)\n%s\n' "$name" "$status" "$output" >&2
        exit 1
    fi
    passed=$((passed + 1))
    printf 'PASS: %s\n' "$name"
}

check relative './lib' "$caller/./lib"
check leading ':/absolute/lib' "$caller:/absolute/lib"
check trailing '/absolute/lib:' "/absolute/lib:$caller"
check doubled '/absolute/lib::lib' "/absolute/lib:$caller:$caller/lib"
check empty_entries ':' "$caller:$caller"
check absolute '/first/lib:/second/lib' '/first/lib:/second/lib'
check order 'last:/absolute/lib:./first' "$caller/last:/absolute/lib:$caller/./first"
check spaces 'relative lib:/absolute path/lib' "$caller/relative lib:/absolute path/lib"
check origin 'prefix/$ORIGIN/lib' 'prefix/$ORIGIN/lib'
check origin_braces 'prefix/${ORIGIN}/lib' 'prefix/${ORIGIN}/lib'
check lib 'prefix/$LIB/suffix' 'prefix/$LIB/suffix'
check lib_braces 'prefix/${LIB}/suffix' 'prefix/${LIB}/suffix'
check platform 'prefix/$PLATFORM/suffix' 'prefix/$PLATFORM/suffix'
check platform_braces 'prefix/${PLATFORM}/suffix' 'prefix/${PLATFORM}/suffix'
check empty '' ''
check unset '' ''
[[ $passed -gt 0 ]] || { printf 'unknown case: %s\n' "$selection" >&2; exit 1; }
printf '%s library-path cases passed\n' "$passed"
