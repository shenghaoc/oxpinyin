#!/bin/sh
# tests/lint-crate-cfg-placement.test.sh — self-contained test harness for
# `.github/scripts/lint-crate-cfg-placement.sh`.
#
# Writes fixture `.rs` files into a throwaway directory, runs the linter on
# them by path, and asserts the exit code and — for expected violations — that
# the reported line numbers point at the offending `//!`. Nothing here touches
# the working tree or needs a git repository: the linter takes explicit paths.
#
# Usage: tests/lint-crate-cfg-placement.test.sh
#
# POSIX sh only.

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
LINTER="$REPO_ROOT/.github/scripts/lint-crate-cfg-placement.sh"

failures=0
tests_run=0

tmp=$(mktemp -d)
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT INT TERM

# --- tiny TAP-ish harness ----------------------------------------------------

fail_test() {
    failures=$((failures + 1))
    printf 'not ok - %s: %s\n' "$1" "$2"
}

ok() {
    printf 'ok - %s\n' "$1"
}

out=''
rc=0
run_lint() {
    set +e
    out=$("$LINTER" "$@" 2>&1)
    rc=$?
    set -e
}

# fixture <name> — writes stdin to $tmp/<name>.rs and echoes the path.
fixture() {
    cat > "$tmp/$1.rs"
    printf '%s\n' "$tmp/$1.rs"
}

# expect_pass <name> <file>...
expect_pass() {
    name=$1
    shift
    tests_run=$((tests_run + 1))
    run_lint "$@"
    if [ "$rc" -eq 0 ]; then
        ok "$name"
    else
        fail_test "$name" "expected exit 0, got $rc; output: $(printf '%s' "$out" | head -n 3)"
    fi
}

# expect_fail <name> <expected-substring> <file>...
expect_fail() {
    name=$1
    want=$2
    shift 2
    tests_run=$((tests_run + 1))
    run_lint "$@"
    if [ "$rc" -eq 0 ]; then
        fail_test "$name" 'expected nonzero exit, got 0'
    elif ! printf '%s' "$out" | grep -q -- "$want"; then
        fail_test "$name" "exit $rc but output lacks '$want': $(printf '%s' "$out" | head -n 4)"
    else
        ok "$name"
    fi
}

# --- fixtures ----------------------------------------------------------------

# The shape this repo requires, and the one the fix installed.
docs_then_gate=$(fixture docs_then_gate <<'EOF'
//! Crate docs.
#![cfg(target_os = "linux")]

#[test]
fn t() {}
EOF
)

# The bug: the gate swallows the docs on every non-Linux host.
gate_then_docs=$(fixture gate_then_docs <<'EOF'
#![cfg(target_os = "linux")]
//! Crate docs.

#[test]
fn t() {}
EOF
)

# Same bug with distance between the two — placement, not adjacency, is the rule.
gate_then_docs_spaced=$(fixture gate_then_docs_spaced <<'EOF'
#![cfg(any(feature = "a", feature = "b"))]
#![allow(clippy::pedantic)]

// An ordinary comment.

//! Crate docs arriving far too late.
EOF
)

# A gate with no crate docs cannot lose any.
gate_only=$(fixture gate_only <<'EOF'
#![cfg(target_os = "linux")]

#[test]
fn t() {}
EOF
)

# Docs with no gate.
docs_only=$(fixture docs_only <<'EOF'
//! Crate docs.

#[test]
fn t() {}
EOF
)

# `cfg_attr` rewrites attributes, it never strips the crate — must not fire.
cfg_attr_then_docs=$(fixture cfg_attr_then_docs <<'EOF'
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]
//! Crate docs.

#[test]
fn t() {}
EOF
)

# An indented `//!` belongs to a nested module, not the crate — must not fire.
gate_then_nested_module_docs=$(fixture gate_then_nested_module_docs <<'EOF'
//! Crate docs.
#![cfg(target_os = "linux")]

mod inner {
    //! Module docs, not crate docs.
    #[test]
    fn t() {}
}
EOF
)

# --- assertions --------------------------------------------------------------

expect_pass 'docs above the gate pass' "$docs_then_gate"
expect_pass 'a gate with no docs passes' "$gate_only"
expect_pass 'docs with no gate pass' "$docs_only"
expect_pass 'cfg_attr is not a crate-stripping gate' "$cfg_attr_then_docs"
expect_pass 'an indented module doc is not a crate doc' "$gate_then_nested_module_docs"
expect_pass 'several clean files pass together' \
    "$docs_then_gate" "$gate_only" "$docs_only"

expect_fail 'docs below the gate are rejected' \
    'gate_then_docs.rs:2' "$gate_then_docs"
expect_fail 'the gate line number is reported' \
    'on line 1' "$gate_then_docs"
expect_fail 'distance from the gate does not excuse it' \
    'gate_then_docs_spaced.rs:6' "$gate_then_docs_spaced"
expect_fail 'one bad file among good ones fails the run' \
    'gate_then_docs.rs:2' "$docs_then_gate" "$gate_then_docs" "$docs_only"

# --help is a usage exit, distinct from a violation.
tests_run=$((tests_run + 1))
run_lint --help
if [ "$rc" -eq 2 ]; then
    ok '--help exits 2 (usage)'
else
    fail_test '--help exits 2 (usage)' "expected exit 2, got $rc"
fi

# --- summary -----------------------------------------------------------------

printf '\n1..%d\n' "$tests_run"
if [ "$failures" -ne 0 ]; then
    printf 'crate-cfg-placement harness: %d of %d assertion(s) failed\n' \
        "$failures" "$tests_run" >&2
    exit 1
fi
printf 'crate-cfg-placement harness: %d assertion(s) passed\n' "$tests_run"
