#!/bin/sh
# lint-commits.sh — lint commit messages against the settled trailer rules.
# Single source of truth for all rule logic; both the CI workflow and the
# commit-msg hook delegate to this script so they can never disagree.
#
# Usage:
#   lint-commits.sh <base-sha> <head-sha>     CI mode: lint the non-merge
#                                             commits in the range (R2).
#   lint-commits.sh --hook <message-file>     Hook mode: lint a single commit
#                                             message for R2.
#
# CI mode: the caller is responsible for computing the merge-base; this script
# lints the non-merge commits in `<base-sha>..<head-sha>`.
#
# House principle the rule mechanizes: Assisted-by is linted for house shape
# when present.
#
# Requires git >= 2.32 (for %(trailers:only,unfold)). POSIX sh + git only.

set -eu

# ---------------------------------------------------------------------------
# Tunables — single shell variables, for easy extension.
# ---------------------------------------------------------------------------

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

# check_message — the message-level rule (R2) shared by CI mode and the
# commit-msg hook. Operates on the globals $short, $subject, $trailers;
# sets r2 and increments $fails, emitting ::error:: lines per violation.
check_message() {
    r2=pass

    # R2 — Assisted-by house form when present. Trailer key is matched
    # case-insensitively, so `assisted-by:` and `ASSISTED-BY:`
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
# Hook mode — lint a single commit message file for R2.
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

    check_message

    linted=$((linted + 1))
    subject_safe=$(printf '%s' "$subject" | sed 's/|/\\|/g')
    rows="$rows| $short | $subject_safe | $r2 |$nl"
done

# Per-commit × per-rule summary table.
summary="### commit-trailers${nl}${nl}| commit | subject | R2 (Assisted-by) |${nl}| --- | --- | --- |${nl}$rows"

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
