#!/usr/bin/env bash
# Optional function-layout plumbing. Requires only the relinker's existing
# Bash, GNU nm, awk and sort. Python/uv are regeneration tools, not link inputs.
# Commands: inventory ARCHIVE; resolve ARCHIVE POLICY SONAME; policy POLICY SONAME
# Source this file to use the compiler capability check and resolver.

# shellcheck source=../lib/bash-floor.sh
# shellcheck disable=SC2128  # BASH_SOURCE is a Bash array; test direct execution.
[ "${BASH_SOURCE:-$0}" = "$0" ] && . "${0%/*}/../lib/bash-floor.sh"

function_order_inventory() {
  local archive="$1" here
  here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
  LC_ALL=C nm --format=sysv --defined-only --no-sort --demangle=rust "$archive" \
    | LC_ALL=C awk -v mode=inventory -f "$here/function-order.awk" \
    | LC_ALL=C sort -u
}

function_order_resolve() {
  local archive="$1" policy="$2" soname="$3" here
  here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
  LC_ALL=C nm --format=sysv --defined-only --no-sort --demangle=rust "$archive" \
    | LC_ALL=C awk -v mode=resolve -v policy="$policy" -v soname="$soname" \
        -f "$here/function-order.awk" "$policy" -
}

function_order_supported() {
  local compiler="$1" archive="$2" scratch="$3" version help
  # Query the actual driver, retaining its linker selection. Supplying the
  # archive also lets drivers that require an input invoke their linker.
  # GNU ld's --version/--help exit before linking. Any accidental output from
  # an incompatible driver stays in the caller's self-cleaning scratch.
  version="$(LC_ALL=C "$compiler" -shared -o "$scratch/probe" \
    -Xlinker --version "$archive" 2>/dev/null)" || return 1
  # gold has an option with this name and DIFFERENT semantics. Never infer
  # BFD compatibility from the option spelling alone.
  [[ "$version" =~ (^|$'\n')GNU\ ld\ \( ]] || return 1
  [[ "$version" != *gold* && "$version" != *LLD* ]] || return 1
  help="$(LC_ALL=C "$compiler" -shared -o "$scratch/probe" \
    -Xlinker --help "$archive" 2>/dev/null)" || return 1
  [[ "$help" =~ --section-ordering-file([=[:space:]]|$) ]]
}

# Measurement callers share the linked-object verifier. This is deliberately
# outside the package link path: Python/uv remain measurement prerequisites.
function_order_measurement() {
  local here
  here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
  uv run --no-project --managed-python --python "$(<"$here/../../.python-version")" \
    python -X lazy_imports=normal "$here/function_order.py" "$@"
}

# Legacy runners have no other command-line options. Keep their opt-outs
# explicit arguments, never an inherited environment variable.
# Callers expand "${FUNCTION_ORDER_ARGS[@]}" directly.
function_order_arguments() {
  FUNCTION_ORDER_ARGS=()
  while (( $# )); do
    case "$1" in
      --function-order|--unordered-arm)
        [[ $# -ge 2 && -n "${2:-}" ]] || return 2
        FUNCTION_ORDER_ARGS+=("$1" "$2"); shift 2 ;;
      *) echo "unknown function-order argument: $1" >&2; return 2 ;;
    esac
  done
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  set -euo pipefail
  case "${1:-}" in
    inventory)
      [[ $# -eq 2 ]] || exit 2
      function_order_inventory "$2"
      ;;
    resolve)
      [[ $# -eq 4 ]] || exit 2
      function_order_resolve "$2" "$3" "$4"
      ;;
    policy)
      [[ $# -eq 3 ]] || exit 2
      LC_ALL=C awk -v mode=policy -v policy="$2" -v soname="$3" \
        -f "$(dirname -- "${BASH_SOURCE[0]}")/function-order.awk" "$2"
      ;;
    *) echo "usage: $0 inventory ARCHIVE | resolve ARCHIVE POLICY SONAME | policy POLICY SONAME" >&2; exit 2 ;;
  esac
fi
