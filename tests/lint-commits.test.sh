#!/bin/sh
# tests/lint-commits.test.sh — self-contained test harness for
# `.github/scripts/lint-commits.sh`.
#
# Builds a throwaway git repo (temp dir, configured user), creates fixture
# commits, runs the linter on constructed ranges, and asserts both the exit
# code and the presence of the expected rule ID in the output.
#
# Usage: tests/lint-commits.test.sh
#
# POSIX sh + git only.

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
LINTER="$REPO_ROOT/.github/scripts/lint-commits.sh"

# The linter writes its step summary to $GITHUB_STEP_SUMMARY when set; in the
# harness we want it on stdout instead.
unset GITHUB_STEP_SUMMARY

HUMAN_NAME='Test Human'
HUMAN_EMAIL='human@example.com'
BOT_EMAIL='49699333+dependabot[bot]@users.noreply.github.com'

failures=0
tests_run=0

# --- tiny TAP-ish harness ----------------------------------------------------

fail_test() {
    failures=$((failures + 1))
    printf 'not ok - %s: %s\n' "$1" "$2"
}

ok() {
    printf 'ok - %s\n' "$1"
}

# run_lint <base> <head> — run the linter, capture merged output into $out
# and the exit code into $rc.
out=''
rc=0
run_lint() {
    set +e
    out=$("$LINTER" "$1" "$2" 2>&1)
    rc=$?
    set -e
}

# expect_pass <name> <base> <head>
expect_pass() {
    tests_run=$((tests_run + 1))
    run_lint "$2" "$3"
    if [ "$rc" -eq 0 ]; then
        ok "$1"
    else
        fail_test "$1" "expected exit 0, got $rc; output: $(printf '%s' "$out" | head -n 3)"
    fi
}

# expect_fail <rule> <name> <base> <head>
expect_fail() {
    tests_run=$((tests_run + 1))
    run_lint "$3" "$4"
    if [ "$rc" -eq 0 ]; then
        fail_test "$2" "expected nonzero exit (R$1), got 0"
        return
    fi
    if printf '%s' "$out" | grep -q "R$1"; then
        ok "$2"
    else
        fail_test "$2" "expected R$1 in output, got: $(printf '%s' "$out" | head -n 5)"
    fi
}

# --- fixture helpers ----------------------------------------------------------

# msg <line>... — print each argument on its own line.
msg() { printf '%s\n' "$@"; }

# commit <author-name> <author-email> <committer-name> <committer-email>
# Reads the full commit message from stdin and prints the new commit SHA.
commit() {
    GIT_AUTHOR_NAME=$1 GIT_AUTHOR_EMAIL=$2 \
    GIT_COMMITTER_NAME=$3 GIT_COMMITTER_EMAIL=$4 \
    git commit --allow-empty -q -F -
    git rev-parse HEAD
}

# human_commit <message-args...> — commit with the human identity, reading the
# message from the remaining args printed one per line.
human() {
    commit "$HUMAN_NAME" "$HUMAN_EMAIL" "$HUMAN_NAME" "$HUMAN_EMAIL"
}

# --- build the fixture repo ----------------------------------------------------

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

cd "$TMPDIR"
git init -q -b main
git config user.name "$HUMAN_NAME"
git config user.email "$HUMAN_EMAIL"
git config commit.gpgsign false

# Root baseline commit: a parent for the fixtures, never linted directly.
msg 'baseline' '' "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" | human >/dev/null

# 1. Valid: SOB + kernel-form Assisted-by + valid Fixes.
c1=$(msg 'valid commit' \
    '' \
    "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" \
    'Assisted-by: Claude:claude-opus-4 coccinelle sparse' \
    'Fixes: deadbeefdeadbeef ("fix the bug")' | human)

# 2. Missing SOB.
c2=$(msg 'missing sob' | human)

# 3. SOB carrying an AI assistant name.
c3=$(msg 'ai signed-off-by' \
    '' \
    'Signed-off-by: Claude <noreply@anthropic.com>' | human)

# 4. Assisted-by in Fedora lax form — MUST fail (intersection semantics).
c4=$(msg 'fedora generic assisted-by' \
    '' \
    "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" \
    'Assisted-by: generic LLM chatbot' | human)

# 5. Assisted-by with no colon-separated version.
c5=$(msg 'assisted-by no version' \
    '' \
    "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" \
    'Assisted-by: ChatGPTv5' | human)

# 6. Malformed Fixes (short hash / unquoted subject).
c6=$(msg 'bad fixes' \
    '' \
    "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" \
    'Fixes: deadbeef (bad)' | human)

# 7. AI-session: true without Assisted-by.
c7=$(msg 'ai session no assisted-by' \
    '' \
    "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" \
    'AI-session: true' | human)

# 9. Bot-author commit without SOB (R1 exempted when toggle on).
c9=$(msg 'bot bump' | commit 'dependabot[bot]' "$BOT_EMAIL" 'dependabot[bot]' "$BOT_EMAIL")

# 11. Regression (pinyin-rs PR#41, 7b504d0): human SOB + Co-authored-by by an
#     AI agent. Certification trailers beyond Signed-off-by must be checked;
#     disclosure via Co-authored-by is the wrong mechanism (authorship vs
#     assistance).
c11=$(msg 'copilot co-authored-by' \
    '' \
    "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" \
    'Co-authored-by: Copilot App <223556219+Copilot@users.noreply.github.com>' | human)

# 12a. Regression (pinyin-rs PR#41, ba25ff7): shape-valid but model-name-empty
#      Assisted-by. Shape-valid but model-name-empty strings must fail; do not
#      "fix" the letter heuristic away.
c12a=$(msg 'grok bare version' \
    '' \
    "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" \
    'Assisted-by: Grok:4.6' | human)

# 12b. Companion: identical commit with a model string — passes.
c12b=$(msg 'grok model string' \
    '' \
    "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" \
    'Assisted-by: Grok:grok-4.6' | human)

# 13. Git author is an AI agent; clean (human-SOB) message.
c13=$(msg 'clean message ai author' \
    '' \
    "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" \
    | commit 'Copilot' '223556219+Copilot@users.noreply.github.com' \
             'Copilot' '223556219+Copilot@users.noreply.github.com')

# 10. Multi-commit range with one bad commit among good ones.
pre10=$(git rev-parse HEAD)
good_a=$(msg 'good a' '' "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" | human)
bad=$(msg 'bad middle' | human)
good_b=$(msg 'good b' '' "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" | human)

# --- run the single-commit cases ----------------------------------------------

expect_pass 'valid commit passes' "$c1~1" "$c1"
expect_fail 1 'missing SOB fails R1' "$c2~1" "$c2"
expect_fail 2 'AI name in Signed-off-by fails R2' "$c3~1" "$c3"
# Intentional intersection semantics, not a bug: Fedora's own lax examples
# (`Assisted-by: generic LLM chatbot`, `Assisted-by: ChatGPTv5`) fail here.
expect_fail 3 'generic LLM chatbot fails R3' "$c4~1" "$c4"
expect_fail 3 'ChatGPTv5 (no version) fails R3' "$c5~1" "$c5"
expect_fail 4 'bad Fixes fails R4' "$c6~1" "$c6"
expect_fail 5 'AI-session without Assisted-by fails R5' "$c7~1" "$c7"
expect_pass 'bot author exempts R1' "$c9~1" "$c9"
expect_fail 2 'Co-authored-by Copilot App fails R2 (regression 7b504d0)' "$c11~1" "$c11"
expect_fail 3 'Assisted-by Grok:4.6 fails R3 (regression ba25ff7)' "$c12a~1" "$c12a"
expect_pass 'Assisted-by Grok:grok-4.6 passes R3' "$c12b~1" "$c12b"
expect_fail 6 'AI git author fails R6' "$c13~1" "$c13"

# --- case 10: multi-commit range, one bad commit ------------------------------

tests_run=$((tests_run + 1))
run_lint "$pre10" "$good_b"
bad_short=$(git rev-parse --short "$bad")
if [ "$rc" -eq 0 ]; then
    fail_test 'multi-commit range with bad commit' 'expected nonzero exit, got 0'
elif printf '%s' "$out" | grep -q 'R1'; then
    if printf '%s' "$out" | grep -q "$bad_short"; then
        ok 'multi-commit range with bad commit'
    else
        fail_test 'multi-commit range with bad commit' "annotations do not identify offending SHA $bad_short"
    fi
else
    fail_test 'multi-commit range with bad commit' "expected R1 in output, got: $(printf '%s' "$out" | head -n 5)"
fi

# --- case 8: merge commit without SOB is skipped ------------------------------

# Fork a side branch from the current HEAD, commit with SOB, then merge with
# --no-ff so the merge commit itself has no Signed-off-by.
git checkout -q -b merge-side
msg 'side commit' '' "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" | human >/dev/null
git checkout -q main
main_before=$(git rev-parse HEAD)
msg 'main commit' '' "Signed-off-by: $HUMAN_NAME <$HUMAN_EMAIL>" | human >/dev/null
git merge -q --no-ff -m 'merge side branch' merge-side >/dev/null 2>&1
merge_sha=$(git rev-parse HEAD)

expect_pass 'merge commit without SOB is skipped' "$main_before" "$merge_sha"

# --- summary -------------------------------------------------------------------

printf '\n%d test(s) run, %d failure(s)\n' "$tests_run" "$failures"
[ "$failures" -eq 0 ]
