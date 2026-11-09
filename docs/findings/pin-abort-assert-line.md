# Reproduced library aborts write no assertion line

Status: accepted, 2026-10-31 UTC (maintainer ruling that retires class (c)).
Disposition: POLICY-ACCEPTED for the stderr line only; the SIGABRT status
is MATCH. Rule: [compatibility policy, class (c) retired](compatibility-policy.md#failure-behaviour).
Pin: libpinyin 074a2219, the asserts-live reference build (`-g -O2`, no
`-DNDEBUG`). In that build, `check_result` is `assert`
(`src/include/pinyin_utils.h:27-31`).

At a pin `assert` site, glibc's `__assert_fail` writes
`<progname>: <file>:<line>: <function>: Assertion '<expr>' failed.` and a
newline to stderr. It then raises SIGABRT. The file, line, function and
expression are text that the compiler generates for the pin's build.
oxpinyin does the pin's preceding work, then reproduces the site with
`std::process::abort()`. It writes no assertion line. A pin `abort()` site
writes nothing, so it is an ordinary MATCH on both status and stderr.

**Contract preserved.** These are unchanged:

- the exit status (SIGABRT);
- every byte that the pin wrote to stderr before the assertion line;
- the files that the pin left (`userdir=True` cases);
- the ABI.

Only the trailing assertion line is missing.

**Differential.** These rules apply to [archived contract diff source](https://github.com/shenghaoc/oxpinyin/blob/1b91c898795b6edd8b92dd1fecf65ec6f4d1a434/tools/bisection/contract-diff.py) cases registered
`sigabrt=True` (the recorded abort-verdict rule):

- Both sides must die of SIGABRT.
- The subject's stderr must equal the pin's stderr less exactly one trailing
  glibc assertion line (`stderr_less_assert_line`). The line may follow
  earlier text that did not end in a newline, such as `open %s failed.`.
- The profiles must agree when the case compares them.

A case whose only stderr difference is that line reports POLICY-ACCEPTED.
A case with identical stderr reports MATCH. Any other difference is DIFFER.
Return-value and state cases are never folded into this disposition. Each
retirement member's record lists its cases. [The divergence register](upstream-divergences.md)
links those records.
