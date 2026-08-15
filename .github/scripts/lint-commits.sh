#!/bin/sh
# lint-commits.sh — lint commit messages against the settled trailer rules.
# Single source of truth for all rule logic; both the CI workflow and the
# commit-msg hook delegate to this script so they can never disagree.
#
# Usage:
#   lint-commits.sh <base-sha> <head-sha>     CI mode: lint the non-merge
#                                             commits in the range (R1–R4).
#   lint-commits.sh --hook <message-file>     Hook mode: lint a single commit
#                                             message for R1–R3 only.
#
# CI mode: the caller is responsible for computing the merge-base; this script
# lints the non-merge commits in `<base-sha>..<head-sha>`.
#
# House principles the rules mechanize: the human contributor is the author of
# and accountable for every commit; AI assistance is attributed through the
# `Assisted-by:` trailer and nowhere else; AI agents never appear as authors,
# committers, or co-authors.
#
# Requires git >= 2.32 (for %(trailers:only,unfold)). POSIX sh + git only.

set -eu

# ---------------------------------------------------------------------------
# Tunables — single shell variables, for easy extension.
# ---------------------------------------------------------------------------

# AGENT_IDENTITIES — one case-insensitive POSIX ERE alternation matching the
# machine email identities that AI agents actually emit. Seeded from the
# owner's 2026-08-15 identity inventory; extend as new agents are observed.
# Identity matching, never name matching: model names collide with the human
# namespace (Claude, Mistral, Kiro are human names), so this must stay email
# based. Verified non-matches: shenghaoc@outlook.com,
# shenghaoc@users.noreply.github.com, 34920365+shenghaoc@users.noreply.github.com,
# dependabot[bot], github-actions[bot], GitHub <noreply@github.com>.
AGENT_IDENTITIES='noreply@anthropic\.com|[0-9]+\+copilot@users\.noreply\.github\.com|copilot@github\.com|cursoragent@cursor\.com|codex@openai\.com|[0-9]+\+kiro-agent@users\.noreply\.github\.com|[0-9]+\+(claude|copilot-swe-agent|cursor[a-z-]*|devin[a-z-]*|codex|google-labs-jules|jules[a-z-]*|kiro[a-z-]*|gemini-code-assist|sweep[a-z-]*)\[bot\]@users\.noreply\.github\.com'

# R2 — Assisted-by house form when present. Four conditions:
#   1. shape (POSIX ERE; NOTHING after the model),
#   2. MODEL token (after the colon) contains >=1 ASCII letter,
#   3. no placeholder text,
#   4. set semantics (no duplicate identical lines).
# Conditions 1, 3, 4 are lifted verbatim from the repo's former
# `.githooks/commit-msg`. See check_message() for provenance notes.
ASSISTED_SHAPE_RE='^Assisted-by: [[:alnum:]][[:alnum:]._-]*:[[:alnum:]][[:alnum:].+_-]*$'
ASSISTED_PLACEHOLDER_RE='^Assisted-by: (AGENT|AGENT_NAME):|:(MODEL|MODEL_VERSION)([[:space:]]|$)|\[|\]'

# ---------------------------------------------------------------------------

# Exit codes: 0 = pass, 1 = lint failure, 2 = usage/environment error.
usage() {
    printf '%s\n' 'usage: lint-commits.sh <base-sha> <head-sha>' \
        '       lint-commits.sh --hook <message-file>' >&2
    exit 2
}

fail() {
    # $1 = rule id, $2 = short sha (may be empty in hook mode),
    # $3 = subject, $4 = detail.
    fails=$((fails + 1))
    if [ -n "$2" ]; then
        printf '::error::%s %s: R%s — %s\n' "$2" "$3" "$1" "$4"
    else
        printf '::error::%s: R%s — %s\n' "$3" "$1" "$4"
    fi
}

# check_message — the message-level rules (R1–R3) shared by CI mode and the
# commit-msg hook. Operates on the globals $short, $subject, $trailers;
# sets r1..r3 and increments $fails, emitting ::error:: lines per violation.
#
# R4 (author/committer identity) is an identity rule and is deliberately NOT
# here — it is CI-only (the hook runs before the commit exists, so there is no
# SHA to inspect, and the author identity is not a property of the message).
check_message() {
    r1=pass
    r2=pass
    r3=pass

    # R1 — no AI agent identity in Co-authored-by:. Key match is
    # case-insensitive (Claude Code emits Co-Authored-By:, GitHub emits
    # Co-authored-by:). Lines with no <email> portion are skipped. Matching is
    # by email, never by name — co-authorship is an authorship credit, and
    # authorship belongs to humans.
    coauth=$(printf '%s\n' "$trailers" | grep -iE '^co-authored-by:' || true)
    if [ -n "$coauth" ]; then
        oldIFS=$IFS
        IFS='
'
        set -f
        # shellcheck disable=SC2086  # intentional word-splitting into lines
        for line in $coauth; do
            [ -n "$line" ] || continue
            email=$(printf '%s\n' "$line" | sed -n 's/^[^<]*<\([^>]*\)>.*$/\1/p')
            [ -n "$email" ] || continue
            if printf '%s\n' "$email" | grep -Eiq "^($AGENT_IDENTITIES)$"; then
                r1=fail
                fail 1 "$short" "$subject" \
                    "AI agent identity in Co-authored-by trailer (authorship belongs to humans; AI attribution goes in Assisted-by only): $line"
                break
            fi
        done
        set +f
        IFS=$oldIFS
    fi

    # R2 — Assisted-by house form when present. Trailer key is matched
    # case-insensitively (as R1 does), so `assisted-by:` and `ASSISTED-BY:`
    # are treated identically.
    #
    # Condition 2 is a semantic heuristic: the MODEL token names the specific
    # model used, and a bare version number names no model (`Grok:4.6` fails;
    # `Grok:grok-4.6` passes). Semantics beyond shape remain human attestation.
    assisted_lines=$(printf '%s\n' "$trailers" | grep -iE '^assisted-by:' || true)
    if [ -n "$assisted_lines" ]; then
        # Condition 4 — set semantics: identical lines must not repeat.
        dup=$(printf '%s\n' "$assisted_lines" | sort | uniq -d)
        if [ -n "$dup" ]; then
            r2=fail
            fail 2 "$short" "$subject" "duplicate Assisted-by trailer (set semantics): $(printf '%s' "$dup" | tr '\n' ' ')"
        fi

        # Conditions 1–3 — per line.
        oldIFS=$IFS
        IFS='
'
        set -f
        # shellcheck disable=SC2086  # intentional word-splitting into lines
        for line in $assisted_lines; do
            [ -n "$line" ] || continue
            if ! printf '%s\n' "$line" | grep -Eiq "$ASSISTED_SHAPE_RE"; then
                r2=fail
                fail 2 "$short" "$subject" "Assisted-by violates house form (condition 1, nothing after the model): $line"
            elif printf '%s\n' "$line" | grep -Eiq "$ASSISTED_PLACEHOLDER_RE"; then
                r2=fail
                fail 2 "$short" "$subject" "Assisted-by placeholder text (condition 3): $line"
            else
                # Condition 2 — MODEL token must contain >=1 ASCII letter.
                # Key matched case-insensitively above, so strip through the
                # last colon (shape guarantees exactly two colons).
                model=${line##*:}
                if ! printf '%s\n' "$model" | grep -Eq '[A-Za-z]'; then
                    r2=fail
                    fail 2 "$short" "$subject" "Assisted-by model has no ASCII letter (condition 2): $line"
                fi
            fi
        done
        set +f
        IFS=$oldIFS
    fi

    # R3 — AI-session promotion. "AI-session: true" (exact value) requires at
    # least one Assisted-by trailer (which R2 then validates). Any other value
    # is a hard fail (typo guard). Inert until agents are configured to emit
    # it; session provenance is otherwise invisible to a linter.
    ai_session=$(printf '%s\n' "$trailers" | grep -iE '^ai-session:' || true)
    if [ -n "$ai_session" ]; then
        # Key is case-insensitive; the value must be exactly "true" (typo guard).
        bad_ai=$(printf '%s\n' "$ai_session" | grep -vE '^[^:]*:[[:space:]]*true[[:space:]]*$' || true)
        if [ -n "$bad_ai" ]; then
            r3=fail
            fail 3 "$short" "$subject" "AI-session must have exact value 'true' (got: $bad_ai)"
        elif ! printf '%s\n' "$trailers" | grep -Eiq '^assisted-by:'; then
            r3=fail
            fail 3 "$short" "$subject" "AI-session: true requires an Assisted-by trailer"
        fi
    fi
}

# Verify git is present and new enough for %(trailers:only,unfold).
git_version=$(git --version | awk '{ print $3 }')
if ! awk -v v="$git_version" 'BEGIN {
    n = split(v, p, ".");
    ok = (p[1] > 2) || (p[1] == 2 && p[2] >= 32);
    exit(ok ? 0 : 1);
}'; then
    printf 'error: git >= 2.32 required (found %s); %%(trailers:only,unfold) unavailable\n' "$git_version" >&2
    exit 2
fi

# ---------------------------------------------------------------------------
# Hook mode — lint a single commit message file for R1–R3.
# ---------------------------------------------------------------------------
if [ "$1" = "--hook" ]; then
    [ $# -eq 2 ] || usage
    msgfile=$2
    if [ ! -f "$msgfile" ]; then
        printf 'error: hook message file "%s" not found\n' "$msgfile" >&2
        exit 2
    fi
    short=''
    subject=$(sed -n '1p' "$msgfile")
    trailers=$(git interpret-trailers --parse <"$msgfile")
    fails=0
    check_message
    if [ "$fails" -gt 0 ]; then
        printf 'commit-msg: %d trailer violation(s)\n' "$fails" >&2
        exit 1
    fi
    exit 0
fi

# ---------------------------------------------------------------------------
# CI mode — lint the non-merge commits in <base-sha>..<head-sha>.
# ---------------------------------------------------------------------------
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
    author_email=$(git log -1 --format='%ae' "$sha")
    committer_email=$(git log -1 --format='%ce' "$sha")

    # R4 — no AI agent as git author or committer. Agent harnesses author
    # commits under their own identity; trailer checks alone miss that. Only
    # machine identities fail — a human author/committer whose name happens to
    # be Claude, Mistral, or Kiro must pass.
    r4=pass
    if printf '%s\n%s\n' "$author_email" "$committer_email" | grep -Eiq "^($AGENT_IDENTITIES)$"; then
        r4=fail
        fail 4 "$short" "$subject" \
            'git author/committer is an AI agent identity — before merge, take authorship (git commit --amend --reset-author or an interactive rebase) and retain the AI attribution via an Assisted-by trailer'
    fi

    # R1–R3 — shared message-level rules.
    check_message

    linted=$((linted + 1))
    subject_safe=$(printf '%s' "$subject" | sed 's/|/\\|/g')
    rows="$rows| $short | $subject_safe | $r1 | $r2 | $r3 | $r4 |$nl"
done

# Per-commit × per-rule summary table.
summary="### commit-trailers${nl}${nl}| commit | subject | R1 (no AI co-author) | R2 (Assisted-by) | R3 (AI-session) | R4 (no AI author) |${nl}| --- | --- | --- | --- | --- | --- |${nl}$rows"

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
