# Runbooks

Step-by-step procedures for the things this repository does by hand.
Each page says what it needs, what to run, and what "done" looks like;
the reasoning behind each procedure lives in the findings it links.

| Runbook | When |
| --- | --- |
| [`backends.md`](backends.md) | building and testing under a store backend other than the default |
| [`oracle.md`](oracle.md) | building the pin-built libpinyin and running the differentials against it |
| [`benches.md`](benches.md) | running a criterion bench, the perf baseline, or a profile |
| [`goldens-and-pins.md`](goldens-and-pins.md) | refreshing a committed golden, moving the oracle pin, re-freezing a pinned number |
| [`release.md`](release.md) | cutting a release and what the packaging pipeline gates |

Rules the runbooks assume: `AGENTS.md` (STOP conditions, the frozen
SPECs, the rebase discipline) and `docs/findings/compatibility-policy.md`
(what may diverge from the pin).
