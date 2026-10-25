# tools/lib/bash-floor.sh — the one declaration of the repository's Bash floor.
#
# shellcheck shell=bash
#
# Sourced by every executed Bash script after its shebang and comment header,
# and by the workflow run-floor checks. POSIX syntax only: this must diagnose a
# non-Bash shell too, so it uses no arrays and no BASH_SOURCE.
#
# The caller's path is in $0. The error names that script:
#   <script>: needs Bash >= 5.0 (found <version or 'not bash'>)

OXPINYIN_BASH_FLOOR=5.0

_oxpinyin_bash_found=${BASH_VERSION-}
if [ -n "$_oxpinyin_bash_found" ]; then
    _oxpinyin_bash_major=${_oxpinyin_bash_found%%.*}
    case $_oxpinyin_bash_major in
        ''|*[!0-9]*) _oxpinyin_bash_major=0 ;;
    esac
else
    _oxpinyin_bash_found='not bash'
    _oxpinyin_bash_major=0
fi

if [ "$_oxpinyin_bash_major" -lt "${OXPINYIN_BASH_FLOOR%%.*}" ]; then
    printf '%s: needs Bash >= %s (found %s)\n' "$0" "$OXPINYIN_BASH_FLOOR" "$_oxpinyin_bash_found" >&2
    exit 1
fi

unset _oxpinyin_bash_found _oxpinyin_bash_major
