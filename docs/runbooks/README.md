# Runbooks

Step-by-step procedures for the things this repository does by hand.
Each page says what it needs, what to run, and what "done" looks like;
the reasoning behind each procedure lives in the findings it links.

| Runbook | When |
| --- | --- |
| [`backends.md`](backends.md) | building and testing under a store backend other than the default |
| [`oracle.md`](oracle.md) | building the pin-built libpinyin and running the differentials against it |
| [`cloud.md`](cloud.md) | provisioning a Claude Code cloud session (dockerd, oracle, data) so every gate runs there |
| [`benches.md`](benches.md) | running a criterion bench, the perf baseline, or a profile |
| [`initbench.md`](initbench.md) | measuring library load, init and time to first candidate against the pin |
| [`goldens-and-pins.md`](goldens-and-pins.md) | refreshing a committed golden, moving the oracle pin, re-freezing a pinned number |
| [`axis-j-ledgers.md`](axis-j-ledgers.md) | what may be expected of an axis-J ledger at a later `main`, and when to regenerate one |
| [`release.md`](release.md) | cutting a release and what the packaging pipeline gates |

Rules the runbooks assume: `AGENTS.md` (STOP conditions, the frozen
SPECs, the rebase discipline) and `docs/findings/compatibility-policy.md`
(what may diverge from the pin).
