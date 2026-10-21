#!/usr/bin/env bash
# Optional function-layout plumbing. Requires only the relinker's existing
# Bash, GNU nm, awk and sort. Python/uv are regeneration tools, not link inputs.
# Commands: inventory ARCHIVE; resolve ARCHIVE POLICY SONAME
# Source this file to use the compiler capability check and resolver.

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
    *) echo "usage: $0 inventory ARCHIVE | resolve ARCHIVE POLICY SONAME" >&2; exit 2 ;;
  esac
fi
