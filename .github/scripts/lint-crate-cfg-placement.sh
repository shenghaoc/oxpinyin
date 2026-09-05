#!/bin/sh
# lint-crate-cfg-placement.sh — reject a crate-level `#![cfg(...)]` written
# ABOVE its crate's `//!` documentation.
#
# Why this is a gate and not a style note. When a crate-level `#![cfg(...)]`
# evaluates false, rustc keeps the crate attributes written BEFORE it and
# discards the ones written AFTER it. A gate on line 1 therefore takes the
# `//!` block with it on every excluded platform, and the workspace's
# `missing_docs` fires on a crate that is now undocumented as well as empty:
#
#     error: missing documentation for the crate
#      --> crates/oxpinyin-capi/tests/abi.rs:1:1
#       = note: `-D missing-docs` implied by `-D warnings`
#
# No existing job can observe that. `lint` and `test` run in debian:testing
# containers where every `target_os = "linux"` gate is true, and
# test-macos/test-windows build only the portable crates. The breakage lands
# on a contributor's macOS or Windows checkout instead, on exactly the two
# workspace-wide commands the project documents. This check reads placement
# rather than cfg outcomes, so it is platform-independent and the Linux lint
# job can enforce it on every host's behalf.
#
# Fix a hit by moving the `#![cfg(...)]` below the `//!` block: the gate keeps
# its predicate and its whole-crate scope, and the crate keeps its docs. The
# same ordering rule governs any other crate attribute a false gate would
# swallow (`#![allow]`, `#![feature]`); only the `//!` case is mechanized here,
# because that is the one the constitution's `missing_docs` turns into an error.
#
# `#![cfg_attr(...)]` is deliberately NOT matched: it rewrites attributes, it
# never strips the crate.
#
# Usage:
#   lint-crate-cfg-placement.sh              lint every tracked *.rs file
#   lint-crate-cfg-placement.sh <file>...    lint the named files
#
# Exit codes: 0 = pass, 1 = violation, 2 = usage/environment error.
# POSIX sh + awk + git only.

set -eu

usage() {
    printf '%s\n' 'usage: lint-crate-cfg-placement.sh [file...]' \
        '       with no arguments, lints every tracked *.rs file' >&2
    exit 2
}

case "${1:-}" in
    -h | --help) usage ;;
esac

if [ "$#" -gt 0 ]; then
    list=$(printf '%s\n' "$@")
    # A path we were asked for by name must exist and be a regular file.
    # Skipping a typo silently would report "no violations" for a file nobody
    # read, which is the one answer a linter must never give. The tracked-file
    # listing below is deliberately NOT held to this: a path git still lists
    # but the working tree lacks (mid-rebase, sparse checkout) stays a skip.
    unreadable=''
    for file in "$@"; do
        [ -f "$file" ] || unreadable="${unreadable}  ${file}
"
    done
    if [ -n "$unreadable" ]; then
        printf 'crate-cfg-placement: not a readable regular file:\n%s' \
            "$unreadable" >&2
        exit 2
    fi
elif list=$(git ls-files -- '*.rs' 2>/dev/null); then
    :
else
    printf 'crate-cfg-placement: not a git repository and no files given\n' >&2
    exit 2
fi

# Emits "<gate-line> <doc-line>" for the first column-0 `//!` that follows a
# column-0 crate-level `#![cfg(`, and nothing otherwise. Column 0 keeps the
# scan on crate-root attributes: an inner attribute or doc comment belonging
# to a nested module is indented by rustfmt, which this repo enforces.
scan='
    gate == 0 && /^#!\[cfg\(/ { gate = NR; next }
    gate  > 0 && /^\/\/!/     { printf "%d %d\n", gate, NR; exit }
'

violations=''
checked=0

while IFS= read -r file; do
    [ -n "$file" ] || continue
    # Only reachable in tracked-listing mode; named paths were checked above.
    [ -f "$file" ] || continue
    checked=$((checked + 1))
    hit=$(awk "$scan" "$file")
    [ -n "$hit" ] || continue
    gate_line=${hit% *}
    doc_line=${hit#* }
    violations="${violations}  ${file}:${doc_line}: \`//!\` doc comment below the crate-level \`#![cfg(...)]\` on line ${gate_line}
"
done <<LIST
$list
LIST

if [ -n "$violations" ]; then
    # shellcheck disable=SC2016  # literal backticks in prose, not expansions
    printf 'crate-cfg-placement: a crate-level `#![cfg(...)]` must sit BELOW its `//!` docs.\n\n' >&2
    printf '%s\n' "$violations" >&2
    # shellcheck disable=SC2016  # literal backticks in prose, not expansions
    printf '%s\n' \
        'A false crate-level `#![cfg(...)]` discards the crate attributes that follow' \
        'it, so these docs vanish on the excluded platform and `missing_docs` fires' \
        'there. Move the `#![cfg(...)]` below the `//!` block; the gate keeps its' \
        'predicate and its whole-crate scope.' >&2
    exit 1
fi

printf 'crate-cfg-placement: %d file(s) checked, no violations\n' "$checked"
