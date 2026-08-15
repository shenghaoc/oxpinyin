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
HOOK="$REPO_ROOT/.githooks/commit-msg"

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

# human — commit with the human identity, reading the message from stdin.
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
msg 'baseline' '' | human >/dev/null

# --- case 1: valid -------------------------------------------------------------
# Valid: `Assisted-by: Claude:claude-opus-5` (verbatim from inventory) passes;
# a plain human commit with no trailers also passes.
c1a=$(msg 'valid assisted-by' \
    '' \
    'Assisted-by: Claude:claude-opus-5' | human)
c1b=$(msg 'plain human commit' | human)

# --- case 2: regression pinyin-rs PR#41, 7b504d0 ------------------------------
c2=$(msg 'copilot co-authored-by' \
    '' \
    'Co-authored-by: Copilot App <223556219+Copilot@users.noreply.github.com>' | human)

# --- case 3: observed-identity coverage (all verbatim from inventory) ----------
# Fail R1:
c3a=$(msg 'claude co-authored-by' \
    '' \
    'Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>' | human)
c3b=$(msg 'cursor co-authored-by' \
    '' \
    'Co-authored-by: Cursor <cursoragent@cursor.com>' | human)
c3c=$(msg 'jules bot co-authored-by' \
    '' \
    'Co-authored-by: google-labs-jules[bot] <161369871+google-labs-jules[bot]@users.noreply.github.com>' | human)
c3d=$(msg 'copilot github co-authored-by' \
    '' \
    'Co-authored-by: Copilot <copilot@github.com>' | human)
# Pass R1 (human / non-agent bot identities must NOT match):
c3e=$(msg 'human co-authored-by' \
    '' \
    'Co-authored-by: shenghaoc <34920365+shenghaoc@users.noreply.github.com>' | human)
c3f=$(msg 'dependabot co-authored-by' \
    '' \
    'Co-authored-by: dependabot[bot] <49699333+dependabot[bot]@users.noreply.github.com>' | human)

# --- case 4: human-namespace pass guards --------------------------------------
# Claude, Mistral, and Kiro are human names; matching is by machine identity,
# never by name.
c4a=$(msg 'claude martin co-authored-by' \
    '' \
    'Co-authored-by: Claude Martin <claude.martin@example.fr>' | human)
c4b=$(msg 'claude martin author' '' \
    | commit 'Claude Martin' 'claude.martin@example.fr' \
             'Claude Martin' 'claude.martin@example.fr')

# --- case 5: regression pinyin-rs PR#41, ba25ff7 ------------------------------
# `Grok:4.6` fails the letter heuristic (31 such trailers observed); the
# model-string companion passes.
c5a=$(msg 'grok bare version' \
    '' \
    'Assisted-by: Grok:4.6' | human)
c5b=$(msg 'grok model string' \
    '' \
    'Assisted-by: Grok:grok-4.6' | human)
c5c=$(msg 'lowercase assisted-by key' \
    '' \
    'assisted-by: Claude:claude-opus-5' | human)

# --- case 6: malformed Assisted-by --------------------------------------------
c6a=$(msg 'generic llm chatbot' \
    '' \
    'Assisted-by: generic LLM chatbot' | human)
c6b=$(msg 'chatgptv5 no colon' \
    '' \
    'Assisted-by: ChatGPTv5' | human)
c6c=$(msg 'placeholder assisted-by' \
    '' \
    'Assisted-by: AGENT_NAME:MODEL_VERSION' | human)
c6d=$(msg 'duplicate assisted-by' \
    '' \
    'Assisted-by: Claude:claude-opus-4' \
    'Assisted-by: Claude:claude-opus-4' | human)
c6e=$(msg 'trailing token assisted-by' \
    '' \
    'Assisted-by: Claude:claude-opus-4 coccinelle' | human)
c6f=$(msg 'lowercase assisted-by key bad model' \
    '' \
    'assisted-by: Grok:4.6' | human)

# --- case 7: AI-session promotion ---------------------------------------------
c7a=$(msg 'ai session no assisted-by' \
    '' \
    'AI-session: true' | human)
c7b=$(msg 'ai session typo' \
    '' \
    'AI-session: yes' | human)
c7c=$(msg 'ai session with assisted-by' \
    '' \
    'AI-session: true' \
    'Assisted-by: Claude:claude-opus-5' | human)
c7d=$(msg 'ai session lowercase key' \
    '' \
    'ai-session: true' | human)

# --- case 8: no AI agent as git author/committer ------------------------------
c8a=$(msg 'kiro agent author' '' \
    | commit 'Kiro Agent' '244629292+kiro-agent@users.noreply.github.com' \
             'Kiro Agent' '244629292+kiro-agent@users.noreply.github.com')
c8b=$(msg 'claude bot author' '' \
    | commit 'claude[bot]' '12345+claude[bot]@users.noreply.github.com' \
             'claude[bot]' '12345+claude[bot]@users.noreply.github.com')
c8c=$(msg 'dependabot author' '' \
    | commit 'dependabot[bot]' "$BOT_EMAIL" \
             'dependabot[bot]' "$BOT_EMAIL")
c8d=$(msg 'web merge committer' '' \
    | commit "$HUMAN_NAME" "$HUMAN_EMAIL" \
             'GitHub' 'noreply@github.com')

# --- run the single-commit cases ----------------------------------------------

expect_pass 'valid Assisted-by passes' "$c1a~1" "$c1a"
expect_pass 'plain human commit passes' "$c1b~1" "$c1b"
expect_fail 1 'Copilot App co-author fails R1 (regression 7b504d0)' "$c2~1" "$c2"
expect_fail 1 'Claude Opus co-author fails R1' "$c3a~1" "$c3a"
expect_fail 1 'Cursor co-author fails R1' "$c3b~1" "$c3b"
expect_fail 1 'google-labs-jules[bot] co-author fails R1' "$c3c~1" "$c3c"
expect_fail 1 'Copilot <copilot@github.com> co-author fails R1' "$c3d~1" "$c3d"
expect_pass 'shenghaoc co-author passes R1' "$c3e~1" "$c3e"
expect_pass 'dependabot[bot] co-author passes R1' "$c3f~1" "$c3f"
expect_pass 'Claude Martin co-author passes R1 (human name)' "$c4a~1" "$c4a"
expect_pass 'Claude Martin author passes R4 (human name)' "$c4b~1" "$c4b"
expect_fail 2 'Assisted-by Grok:4.6 fails R2 (regression ba25ff7)' "$c5a~1" "$c5a"
expect_pass 'Assisted-by Grok:grok-4.6 passes R2' "$c5b~1" "$c5b"
expect_pass 'lowercase assisted-by key passes R2' "$c5c~1" "$c5c"
expect_fail 2 'generic LLM chatbot fails R2' "$c6a~1" "$c6a"
expect_fail 2 'ChatGPTv5 (no colon) fails R2' "$c6b~1" "$c6b"
expect_fail 2 'placeholder Assisted-by fails R2 (condition 3)' "$c6c~1" "$c6c"
expect_fail 2 'duplicate Assisted-by fails R2 (condition 4)' "$c6d~1" "$c6d"
expect_fail 2 'trailing token Assisted-by fails R2 (condition 1)' "$c6e~1" "$c6e"
expect_fail 2 'lowercase assisted-by key fails R2' "$c6f~1" "$c6f"
expect_fail 3 'AI-session: true without Assisted-by fails R3' "$c7a~1" "$c7a"
expect_fail 3 'AI-session: yes fails R3 (typo guard)' "$c7b~1" "$c7b"
expect_pass 'AI-session: true with Assisted-by passes R3' "$c7c~1" "$c7c"
expect_fail 3 'lowercase ai-session key fails R3' "$c7d~1" "$c7d"
expect_fail 4 'Kiro Agent author fails R4' "$c8a~1" "$c8a"
expect_fail 4 'claude[bot] author fails R4' "$c8b~1" "$c8b"
expect_pass 'dependabot[bot] author passes R4' "$c8c~1" "$c8c"
expect_pass 'GitHub <noreply@github.com> committer passes R4' "$c8d~1" "$c8d"

# --- case 10: multi-commit range, one bad commit ------------------------------

pre10=$(git rev-parse HEAD)
good_a=$(msg 'good a' '' | human)
bad=$(msg 'bad middle' '' \
    'Co-authored-by: Copilot <copilot@github.com>' | human)
good_b=$(msg 'good b' '' | human)

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

# --- case 9: merge commit with agent co-author trailer is skipped -------------

git checkout -q -b merge-side
msg 'side commit' '' | human >/dev/null
git checkout -q main
main_before=$(git rev-parse HEAD)
msg 'main commit' '' | human >/dev/null
merge_msg="$TMPDIR/merge-msg.txt"
printf 'merge side branch\n\nCo-authored-by: Copilot App <223556219+Copilot@users.noreply.github.com>\n' >"$merge_msg"
git merge -q --no-ff -F "$merge_msg" merge-side >/dev/null 2>&1
merge_sha=$(git rev-parse HEAD)

expect_pass 'merge commit with agent co-author is skipped' "$main_before" "$merge_sha"

# --- case 11: hook parity (delegator = single source of truth) -----------------

# run_hook <message-file> — run the commit-msg delegator from the real repo
# root (where it can resolve the linter via `git rev-parse --show-toplevel`).
run_hook() {
    set +e
    out=$( (cd "$REPO_ROOT" && "$HOOK" "$1") 2>&1 )
    rc=$?
    set -e
}

# parity <name> <message-args...> — write the message to a file, run BOTH the
# hook (on the file) and the CI script (on a commit carrying the identical
# message), and assert the two verdicts agree (same exit code, and when
# failing, the same rule ID). This is the "hook and CI can never disagree"
# guarantee from Deliverable 6.
parity() {
    name=$1
    shift
    tests_run=$((tests_run + 1))
    f="$TMPDIR/parity.txt"
    msg "$@" >"$f"

    run_hook "$f"
    hook_rc=$rc
    hook_out=$out

    sha=$(commit "$HUMAN_NAME" "$HUMAN_EMAIL" "$HUMAN_NAME" "$HUMAN_EMAIL" <"$f")
    run_lint "$sha~1" "$sha"
    ci_rc=$rc
    ci_out=$out

    if [ "$hook_rc" -ne "$ci_rc" ]; then
        fail_test "$name" "hook exit $hook_rc vs CI exit $ci_rc; hook: $(printf '%s' "$hook_out" | head -n 2); ci: $(printf '%s' "$ci_out" | head -n 2)"
        return
    fi
    if [ "$hook_rc" -eq 0 ]; then
        ok "$name"
        return
    fi
    hook_rule=$(printf '%s' "$hook_out" | grep -oE 'R[0-9]' | head -n 1)
    ci_rule=$(printf '%s' "$ci_out" | grep -oE 'R[0-9]' | head -n 1)
    if [ "$hook_rule" = "$ci_rule" ] && [ -n "$hook_rule" ]; then
        ok "$name (both report $hook_rule)"
    else
        fail_test "$name" "rule mismatch: hook=$hook_rule ci=$ci_rule"
    fi
}

parity 'hook/CI agree: agent co-author (R1)' 'parity r1' '' \
    'Co-authored-by: Copilot App <223556219+Copilot@users.noreply.github.com>'
parity 'hook/CI agree: Grok:4.6 (R2)' 'parity r2' '' \
    'Assisted-by: Grok:4.6'
parity 'hook/CI agree: AI-session no Assisted-by (R3)' 'parity r3' '' \
    'AI-session: true'
parity 'hook/CI agree: valid Assisted-by' 'parity ok' '' \
    'Assisted-by: Claude:claude-opus-5'
parity 'hook/CI agree: [human] subject is inert' '[human] fix typo'

# --- summary -------------------------------------------------------------------

printf '\n%d test(s) run, %d failure(s)\n' "$tests_run" "$failures"
[ "$failures" -eq 0 ]
