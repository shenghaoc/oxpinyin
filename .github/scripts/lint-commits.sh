#!/bin/sh
# lint-commits.sh — lint every commit in a range against commit-message
# trailer rules. Single source of truth for the rule logic; the workflow
# (`.github/workflows/commit-trailers.yml`) only invokes this script.
#
# Usage: lint-commits.sh <base-sha> <head-sha>
#
# The caller is responsible for computing the merge-base; this script lints
# the non-merge commits in `<base-sha>..<head-sha>`.
#
# The rule set below is the settled interpretation of the Linux kernel
# "AI Coding Assistants" policy and the Fedora "AI-Assisted Contributions
# Policy" v1.0. Do not re-derive rules from the source texts.
#
# Requires git >= 2.32 (for %(trailers:only,unfold)). POSIX sh + git only.

set -eu

# ---------------------------------------------------------------------------
# Tunables — single shell variables, for easy extension.
# ---------------------------------------------------------------------------

# Certification/authorship trailers. Both source policies reserve these to
# humans: they certify origin, authorship, review, approval, or testing.
# Reported-by / Suggested-by are deliberately NOT checked — kernel practice
# already accepts automated reporters there (syzbot precedent), and the
# policies prohibit AI certification, not AI credit for finding.
CERT_TRAILERS='Signed-off-by: Co-authored-by: Co-developed-by: Reviewed-by: Acked-by: Tested-by:'

# AI-assistant denylist, matched case-insensitively as a substring against
# the whole line (so emails like <223556219+Copilot@users.noreply.github.com>
# are caught). "muse spark" is a single entry. This is a tripwire for honest
# labeling, not an adversarial control — it cannot detect undisclosed AI
# authorship — and substring matching can false-positive on human names; the
# list is tunable.
AI_DENYLIST='claude|gpt|chatgpt|codex|copilot|gemini|grok|deepseek|llama|mistral|qwen|cursor|kiro|musecode|muse spark|openai|anthropic|xai'

# Bot exemption (default ON) — single toggle variable. Commits whose author
# email matches *[bot]@users.noreply.github.com (e.g. dependabot) skip R1
# (DCO) only; R2–R6 still apply. Non-AI automation (e.g. dependabot) passes
# R6 because it is not on the denylist; AI agents are never exempted. Set
# BOT_EXEMPT=0 to disable the exemption.
BOT_EXEMPT=1

# R1 — DCO. Every non-merge, non-exempt commit must carry at least one
# Signed-off-by with an email address.
SOB_RE='^Signed-off-by: .+ <.+@.+>$'

# R3 — Assisted-by, kernel form. AGENT_NAME:MODEL_VERSION [tool1] [tool2],
# where MODEL_VERSION must contain at least one ASCII letter.
#
# House style — both source policies make the tag itself a
# SHOULD/recommendation; hard-failing the format is a deliberate local
# escalation ("intersection semantics": kernel-form strings are the subset
# valid under both policies). Fedora's own examples
# (`Assisted-by: generic LLM chatbot`, `Assisted-by: ChatGPTv5`) intentionally
# FAIL this check. To relax to Fedora laxity, replace the regex with a
# non-empty check: `^Assisted-by:[[:space:]]*[^[:space:]].*$`.
ASSISTED_RE='^Assisted-by:[[:space:]]*[^:[:space:]]+:[^[:space:]]*[A-Za-z][^[:space:]]*([[:space:]]+[^[:space:]]+)*[[:space:]]*$'

# R4 — Fixes format. 12–40 hex chars plus a quoted subject.
FIXES_RE='^Fixes:[[:space:]]+[0-9a-fA-F]{12,40} \(".+"\)[[:space:]]*$'

# ---------------------------------------------------------------------------

# Exit codes: 0 = pass, 1 = lint failure, 2 = usage/environment error.
usage() {
    printf '%s\n' 'usage: lint-commits.sh <base-sha> <head-sha>' >&2
    exit 2
}

# R1 comment — the kernel permits Signed-off-by-less *intermediate* commits
# during an AI session (procedure step 6 says the agent must not sign off);
# the DCO requirement applies at PR/submission state, which is what this
# lints. This linter is therefore only valid on PR ranges, not on arbitrary
# working-branch pushes.

fail() {
    # $1 = rule id, $2 = short sha, $3 = subject, $4 = detail
    fails=$((fails + 1))
    printf '::error::%s %s: R%s — %s\n' "$2" "$3" "$1" "$4"
}

# Verify git is present and new enough for %(trailers:only,unfold).
git_version=$(git --version | awk '{ print $3 }')
if ! awk -v v="$git_version" 'BEGIN {
    n = split(v, p, ".");
    ok = (p[1] > 2) || (p[1] == 2 && p[2] >= 32);
    exit(ok ? 0 : 1);
}'; then
    printf 'error: git >= 2.32 required (found %s); %%('trailers:only,unfold') unavailable\n' "$git_version" >&2
    exit 2
fi

[ $# -eq 2 ] || usage
BASE=$1
HEAD=$2

if ! git rev-parse --verify --quiet "$BASE^{commit}" >/dev/null 2>&1; then
    printf 'error: base-sha "%s" is not a valid commit\n' "$BASE" >&2
    exit 2
fi
if ! git rev-parse --verify --quiet "$HEAD^{commit}" >/dev/null 2>&1; then
    printf 'error: head-sha "%s" is not a valid commit\n' "$HEAD" >&2
    exit 2
fi

# Literal newline for building the summary table without %b (which would
# mangle backslashes in subjects).
nl='
'

fails=0
linted=0
rows=''

# Empty range → pass with a notice.
if [ -z "$(git rev-list --no-merges "$BASE..$HEAD")" ]; then
    printf '::notice::commit-trailers: no commits to lint in %s..%s\n' "$BASE" "$HEAD"
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
        printf '%s\n' '### commit-trailers' >>"$GITHUB_STEP_SUMMARY"
        printf '%s\n' '_No commits in range — nothing to lint._' >>"$GITHUB_STEP_SUMMARY"
    fi
    printf 'commit-trailers: no commits in range, passing\n'
    exit 0
fi

# shellcheck disable=SC2046  # word-splitting git SHAs is intentional
for sha in $(git rev-list --no-merges "$BASE..$HEAD"); do
    short=$(git log -1 --format='%h' "$sha")
    subject=$(git log -1 --format='%s' "$sha")
    trailers=$(git log -1 --format='%(trailers:only,unfold)' "$sha")
    author_name=$(git log -1 --format='%an' "$sha")
    author_email=$(git log -1 --format='%ae' "$sha")
    committer_name=$(git log -1 --format='%cn' "$sha")
    committer_email=$(git log -1 --format='%ce' "$sha")

    r1=pass
    r2=pass
    r3=pass
    r4=pass
    r5=pass
    r6=pass

    # R1 — DCO present (bot exemption skips this rule only).
    if [ "$BOT_EXEMPT" -eq 1 ]; then
        case $author_email in
            *'[bot]@users.noreply.github.com')
                r1=skip
                printf '::notice::%s %s: R1 skipped (bot author <%s>)\n' "$short" "$subject" "$author_email"
                ;;
        esac
    fi
    if [ "$r1" = pass ] && ! printf '%s\n' "$trailers" | grep -Eq "$SOB_RE"; then
        r1=fail
        fail 1 "$short" "$subject" 'missing Signed-off-by trailer'
    fi

    # R2 — no AI agent in certification/authorship trailers.
    # shellcheck disable=SC2086  # word-splitting the trailer list is intentional
    for t in $CERT_TRAILERS; do
        if printf '%s\n' "$trailers" | grep -E "^$t" | grep -Eiq "$AI_DENYLIST"; then
            r2=fail
            fail 2 "$short" "$subject" "AI-assistant token in $t trailer"
            break
        fi
    done

    # R3 — Assisted-by format (kernel form) when present.
    bad_assisted=$(printf '%s\n' "$trailers" | grep '^Assisted-by:' | grep -Ev "$ASSISTED_RE" | head -n 1 || true)
    if [ -n "$bad_assisted" ]; then
        r3=fail
        fail 3 "$short" "$subject" "malformed Assisted-by: $bad_assisted"
    fi

    # R4 — Fixes format when present.
    bad_fixes=$(printf '%s\n' "$trailers" | grep '^Fixes:' | grep -Ev "$FIXES_RE" | head -n 1 || true)
    if [ -n "$bad_fixes" ]; then
        r4=fail
        fail 4 "$short" "$subject" "malformed Fixes: $bad_fixes"
    fi

    # R5 — AI-session promotion. This implements kernel bug-fix procedure
    # step 6 — Assisted-by is mandatory for AI find-and-fix sessions — using
    # an explicit trailer as the observable signal, since session provenance
    # is otherwise invisible to a linter. Configure your agents to emit it.
    # "AI-session" with any value other than exactly "true" is a hard fail
    # (typo guard).
    ai_session=$(printf '%s\n' "$trailers" | grep '^AI-session:' || true)
    if [ -n "$ai_session" ]; then
        bad_ai=$(printf '%s\n' "$ai_session" | sed -n '/^AI-session:[[:space:]]*true[[:space:]]*$/!p' || true)
        if [ -n "$bad_ai" ]; then
            r5=fail
            fail 5 "$short" "$subject" "AI-session must have exact value 'true' (got: $bad_ai)"
        elif ! printf '%s\n' "$trailers" | grep -Eq '^Assisted-by:'; then
            r5=fail
            fail 5 "$short" "$subject" "AI-session: true requires an Assisted-by trailer"
        fi
    fi

    # R6 — no AI agent as git author or committer. Agent harnesses author
    # commits under their own identity; trailer checks alone miss that.
    if printf '%s\n%s\n' "$author_name <$author_email>" "$committer_name <$committer_email>" | grep -Eiq "$AI_DENYLIST"; then
        r6=fail
        fail 6 "$short" "$subject" \
            'git author/committer matches AI-assistant denylist — before merge, take authorship (git commit --amend --reset-author or interactive rebase), add your own Signed-off-by, and retain the AI attribution via a kernel-form Assisted-by trailer'
    fi

    linted=$((linted + 1))
    subject_safe=$(printf '%s' "$subject" | sed 's/|/\\|/g')
    rows="$rows| $short | $subject_safe | $r1 | $r2 | $r3 | $r4 | $r5 | $r6 |$nl"
done

# Per-commit × per-rule summary table.
summary="### commit-trailers${nl}${nl}| commit | subject | R1 (DCO) | R2 (no AI cert) | R3 (Assisted-by) | R4 (Fixes) | R5 (AI-session) | R6 (no AI author) |${nl}| --- | --- | --- | --- | --- | --- | --- | --- |${nl}$rows"

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    printf '%s\n' "$summary" >>"$GITHUB_STEP_SUMMARY"
else
    printf '%s\n' "$summary"
fi

if [ "$fails" -gt 0 ]; then
    printf 'commit-trailers: %d commit(s) linted, %d rule violation(s)\n' "$linted" "$fails" >&2
    exit 1
fi
printf 'commit-trailers: %d commit(s) linted, no violations\n' "$linted"
exit 0
