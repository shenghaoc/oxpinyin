#!/usr/bin/env bash
# The single resolver implementation. Requires bash, awk and readlink;
# ldd on Linux and otool on Darwin. No Python/uv dependency in native
# runners or measurement images.
TKRZW_RUNTIME=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/runtime.sh
TKRZW_EXEC=(bash "$TKRZW_RUNTIME")

_tkrzw_refuse() {
    printf 'REFUSED: %s: %s: libtkrzw resolved to %s; expected %s (object %s, cwd %s)\n' \
        "$1" "$2" "$3" "$4" "$5" "$PWD" >&2
    return 1
}

_tkrzw_check() {
    local runner=$1 side=${2##*/} object=$3 output status=0 matches entry real
    local found= resolved= invalid=0 ambiguous=0 expected= prefix=
    case $side in
        L|A|pin*|oracle*|reference*|libpinyin*) side=pin ;;
        *) side=subject ;;
    esac
    # otool -L records the install name; a dyld override can bypass it.
    if [ "$(uname -s)" = Darwin ] &&
        { [ -n "${DYLD_LIBRARY_PATH:-}" ] || [ -n "${DYLD_FALLBACK_LIBRARY_PATH:-}" ]; }; then
        _tkrzw_refuse "$runner" "$side" \
            '<DYLD_LIBRARY_PATH or DYLD_FALLBACK_LIBRARY_PATH set>' \
            '<otool -L recorded install name; a dyld override can bypass it>' "$object"
        return 1
    fi
    case $(uname -s) in
    Darwin)
        output=$(otool -L "$object" 2>&1) || status=$?
        matches=$(printf '%s\n' "$output" | awk '
            /^[[:space:]]*\/.*libtkrzw[^[:space:]]*[.]dylib[[:space:]]+[(]compatibility version/ {
                sub(/^[[:space:]]*/, "")
                sub(/[[:space:]]+[(]compatibility version.*$/, "")
                print
            }') || { _tkrzw_refuse "$runner" "$side" '<resolver failed>' '<verified runtime>' "$object"; return 1; }
        ;;
    *)
        output=$(ldd "$object" 2>&1) || status=$?
        matches=$(printf '%s\n' "$output" | awk '
            /^[[:space:]]*libtkrzw[.]so[^[:space:]]*[[:space:]]+=>[[:space:]]*/ {
                sub(/^[[:space:]]*libtkrzw[.]so[^[:space:]]*[[:space:]]+=>[[:space:]]*/, "")
                sub(/[[:space:]]+[(]0x[[:xdigit:]]+[)][[:space:]]*$/, "")
                print; next
            }
            /^[[:space:]]*(\/|[.]\/|[.][.]\/).*\/libtkrzw[.]so.*[[:space:]]+[(]0x[[:xdigit:]]+[)]/ {
                sub(/^[[:space:]]*/, "")
                sub(/[[:space:]]+[(]0x[[:xdigit:]]+[)][[:space:]]*$/, "")
                print
            }') || { _tkrzw_refuse "$runner" "$side" '<resolver failed>' '<verified runtime>' "$object"; return 1; }
        ;;
    esac
    # Backend labels are advisory. Only the complete resolved set decides.
    if [[ -z $matches && $status == 0 ]]; then
        if printf '%s\n' "$output" | grep -q 'libtkrzw'; then
            _tkrzw_refuse "$runner" "$side" '<unresolved libtkrzw>' '<verified runtime>' "$object"
            return 1
        fi
        return 0
    fi
    while IFS= read -r entry; do
        [[ -n $entry ]] || continue
        if [[ -f $entry ]] && real=$(readlink -f -- "$entry"); then :
        else real=$entry; invalid=1; fi
        if [[ -z $found ]]; then found=$entry; resolved=$real
        elif [[ $real != "$resolved" ]]; then found="$found; $entry"; ambiguous=1; fi
    done <<< "$matches"
    [[ -n $found ]] || found='<missing; resolver failed>'
    expected=${OXPINYIN_TKRZW_LIBRARY:-}
    if [[ -z $expected ]]; then
        prefix=${OXPINYIN_TKRZW_PREFIX:-}
        if [[ -z $prefix ]]; then
            prefix=${OXPINYIN_TKRZW_PREFIXES:-}
            prefix=${prefix%%:*}
        fi
        if [ "$(uname -s)" = Darwin ]; then
            expected=${prefix:+$prefix/lib/libtkrzw.dylib}
        else
            expected=${prefix:+$prefix/lib/libtkrzw.so.1}
        fi
    fi
    # Older generated environments can select a cache prefix. Never infer
    # trust from LD_LIBRARY_PATH or accept an arbitrary system directory.
    if [[ -z $expected || ! -f $expected ]]; then
        _tkrzw_refuse "$runner" "$side" "$found" "${expected:-<unconfigured verified runtime>}" "$object"
        return 1
    fi
    expected=$(readlink -f -- "$expected") || return 1
    if [[ $status != 0 || $invalid != 0 || $ambiguous != 0 || $resolved != "$expected" ]]; then
        _tkrzw_refuse "$runner" "$side" "$found" "$expected" "$object"
        return 1
    fi
    printf '%s\n' "$resolved"
}

_tkrzw_is_elf() {
    local magic
    [[ -f $1 ]] && IFS= read -r -n 4 magic < "$1" || return 1
    [[ $magic == $'\177ELF' ]]
}

_tkrzw_is_macho() {
    local magic
    [[ -f $1 ]] && magic=$(od -An -tx1 -N4 "$1" 2>/dev/null | tr -d ' \n') || return 1
    case $magic in
    cffaedfe | cefaedfe | feedfacf | feedface) return 0 ;;
    esac
    return 1
}

_tkrzw_main() {
    local quiet=0 print_path=0 resolved cell runner side object executable
    while [[ ${1:-} == --quiet-stderr || ${1:-} == --print-path ]]; do
        case $1 in --quiet-stderr) quiet=1 ;; --print-path) print_path=1 ;; esac
        shift
    done
    if [[ $# -lt 4 ]]; then
        printf 'usage: runtime.sh [--quiet-stderr] [--print-path] CELL RUNNER SIDE OBJECT [-- COMMAND...]\n' >&2
        return 2
    fi
    cell=$1 runner=$2 side=$3 object=$4
    shift 4
    resolved=$(_tkrzw_check "$runner" "$side" "$object") || return 1
    [[ ${1:-} != -- ]] || shift
    if [[ $# -gt 0 ]]; then
        executable=$(command -v -- "$1") || return 1
        # A dlopen subject and a directly linked worker can each bring tkrzw.
        # Shell/profiler wrappers are not ELF objects; their subjects are checked
        # explicitly by the caller, outside the measured worker process.
        if [[ $executable != "$object" ]] && { _tkrzw_is_elf "$executable" || _tkrzw_is_macho "$executable"; }; then
            _tkrzw_check "$runner" "$side" "$executable" >/dev/null || return 1
        fi
    fi
    if [[ $print_path == 1 && -n $resolved ]]; then printf '%s\n' "$resolved"; fi
    if [[ $# -gt 0 ]]; then
        if [[ $quiet == 1 ]]; then exec "$@" 2>/dev/null
        else exec "$@"; fi
    fi
}

tkrzw_run() {
    local quiet=0
    if [[ ${1:-} == --quiet-stderr ]]; then quiet=1; shift; fi
    local cell=$1 side=$2 object=$3
    shift 3
    if [[ $quiet == 1 ]]; then
        "${TKRZW_EXEC[@]}" --quiet-stderr "$cell" "${0##*/}" "$side" "$object" -- "$@"
    else
        "${TKRZW_EXEC[@]}" "$cell" "${0##*/}" "$side" "$object" -- "$@"
    fi
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then _tkrzw_main "$@"; fi
