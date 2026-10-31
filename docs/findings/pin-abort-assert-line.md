# Reproduced library aborts write no assertion line

Status: accepted, 2026-10-31 UTC (maintainer ruling retiring class (c)).
Disposition: POLICY-ACCEPTED for the stderr line only; the SIGABRT status
is MATCH. Rule: [compatibility policy, class (c) retired](compatibility-policy.md#amendment--class-c-retired-pin-aborts-reproduced-2026-10-31-utc).
Pin: libpinyin 074a2219, the asserts-live reference build (`-g -O2`, no
`-DNDEBUG`); `check_result` is `assert` there
(`src/include/pinyin_utils.h:27-31`).

At a pin `assert` site glibc's `__assert_fail` writes
`<progname>: <file>:<line>: <function>: Assertion '<expr>' failed.` and a
newline to stderr, then raises SIGABRT. The file, line, function and
expression are compiler-generated text of the pin's build. oxpinyin
reproduces the site with `std::process::abort()` after the pin's
preceding work and writes no such line. A pin `abort()` site writes
nothing, so it is an ordinary MATCH on both status and stderr.

**Contract preserved.** The exit status (SIGABRT), every byte the pin
wrote to stderr before the assertion line, the files the pin left
(`userdir=True` cases) and the ABI are unchanged. Only the trailing
assertion line is missing.

**Differential.** `contract-diff.py` cases registered `sigabrt=True`
(`contract_verdict.sigabrt_reproduced`): both sides must die of SIGABRT,
the subject's stderr must equal the pin's less exactly one trailing
glibc assertion line (`stderr_less_assert_line`; the line may follow
earlier text that did not end in a newline, such as `open %s failed.`),
and the profiles must agree
when the case compares them. A case whose only stderr difference is that
line reports POLICY-ACCEPTED; one with identical stderr reports MATCH.
Any other difference is DIFFER. Return-value and state cases are never
folded into this disposition. The cases are listed in each former
class (c) row's record in [the divergence register](upstream-divergences.md).
