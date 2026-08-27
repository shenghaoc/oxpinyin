# Findings — W2-T5 divergence taxonomy

Date: 2026-08-09 · Source tier: Architect derivation.
Status: **frozen** (maintainer decisions 2026-08-09 on budget meaning and
when the budget gates). Class set and classification rules stand as proposed;
the auto-accept budget section records the decisions rather than open asks.

Every class below has a worked example, and every example is either measured on
the pin or explicitly marked as constructed.

## Purpose

The differential runner produces a `reason` per divergence — a mechanical
statement of *which check failed*. The taxonomy answers a different question:
*whose problem is it, and does a human need to look?*

Reasons are fixed and implementation-level. Classes are judgements. Keeping them
separate means a reason can never silently change what a class means, and the
class set can be corrected without touching the log schema.

## The classes

Eight classes. `class` is a `pinyin-diff-v1` and `pinyin-divergence-v1` field
whose value space this finding defines.

| Class | Whose problem | Auto-accepted | Gates S1b |
|---|---|---|---|
| `output-identical` | nobody | n/a — agreement, not divergence | n/a — this is success |
| `tie-swap` | nobody | yes (report only until W4) | no until decoder exists |
| `path-set` | ours, by policy | no | yes |
| `flag-semantics` | neither — different questions | no | no |
| `ours-bug` | ours | no | yes |
| `theirs-bug` | the pin | no | no |
| `data-version` | the tables | no | no |
| `distro-delta` | not the oracle | no | never |

### `output-identical`

The compared output is the same on both sides: the oracle's selected
segmentation is our **first** path, consumed length agrees, remainder agrees.
Nothing differs. This class is **agreement**, not divergence. It is not inside
the auto-accept budget and is not a triage population.

*Worked example (measured).* Input `nihao`. Oracle selects
`ni@0:2:complete,hao@2:5:complete`; that is our first path; both consume 5 bytes
with an empty remainder. Rank 0.

*Corpus count:* 9,506 of 10,465.

### `tie-swap`

Agreement on the path set, but at a positive rank: the oracle selected a
segmentation we do enumerate, just not the one our frozen order puts first.
Both sides admit the same path set; they differ only in which member is chosen.

*Worked example (measured).* Input `fangan`. Our frozen order is
`[fang, an]`, `[fan, gan]`, `[fa, ng, an]`. The oracle selects `[fan, gan]`, our
second path. Rank 1. Also pinned in `fixtures/foundation/f-a.txt` as
`ambiguous-fangan`.

*Corpus count:* 468 of 10,465, ranks 1 to 128.

Choice is not a parser concern. Our parser deliberately enumerates rather than
ranks, so at this stage `tie-swap` measures the gap a future ranking policy must
close. It is not a defect. The 468 is the **W4 baseline**: report it on every
run; do **not** gate S1b or the W2 runner on it until a decoder exists and
`tie-swap` means both sides chose differently among paths they both admit.

### `path-set`

The two sides admit **different sets of paths** for a feature both implement.
This is the class `.kiro/steering/structure.md` has in mind with "neither more
paths nor fewer".

*Worked example (measured).* Input `yingchon` under the parity profile `0x18a`.

```text
theirs  ying@0:4:complete, ch@4:6:partial, o@6:7:complete, n@7:8:partial
ours    ying@0:4:complete, chon@4:8:partial
        yi@0:2:complete, ng@2:4:complete, chon@4:8:partial
```

Both consume all 8 bytes, so the disagreement is purely which segmentations
exist. The pin admits an initial-only key at any position and repeatedly.
`docs/findings/parser-spec.md` now matches that policy (oracle-driven SPEC
correction); the portable parser and `parser-path-set.md` still implement the
pre-correction path set until a separate branch lands.

*Corpus count:* 483 of 10,465 (re-measured 2026-08-27 after #178's
parser-termination classes moved two boundary inputs to agreement). This
is currently the whole substantive parity
gap, and it has **one** root cause, measured in
`docs/findings/parser-spec-contradiction-incomplete-keys.md` (483 of 491
divergences). `path-set` is attributed to us because our path set still
disagrees with the pin — the SPEC field invariant is corrected; the
implementation is not yet.

### `flag-semantics`

The two sides were asked different questions, so the comparison is not about
correctness.

Our parser takes no option word. It implements exactly the parity profile
`0x18a`. Any comparison run under a different flag word is therefore comparing
two different questions by construction, and its divergences are
`flag-semantics`.

*Worked example (measured).* F-C case `pinyin-incomplete-off`, input `nih` under
`IS_PINYIN` alone (`0x02`). The oracle consumes 2 bytes and selects
`ni@0:2:complete`, refusing the partial tail. Our parser always models partials,
consumes 3, and returns `[ni, h(partial)]`. Reason `consumed-length`, class
`flag-semantics`: with `PINYIN_INCOMPLETE` clear the oracle was asked a question
our parser cannot be asked.

*F-C count:* 10 of 46 records — the `PINYIN_INCOMPLETE`, `USE_TONE` and
correction-bit cases.

This class never gates S1b. It would only become a correctness question if the
engine grew a configurable parser, which is a W4-or-later concern.

### `ours-bug`

Our side failed to produce a comparable answer, or produced one that violates
our own documented invariants.

*Worked example (constructed).* Thirteen apostrophe-separated `xian` groups.
Each group has two complete segmentations, so the exhaustive path set is
`2^13 = 8,192`, above `MAX_PARSE_RESULTS` (4,096), and the parser returns
`ParseError::TooManyAlternatives`. Reason `ours-error`, class `ours-bug`.

Marked constructed because the parity corpus deliberately excludes
result-bound inputs — see `docs/findings/parity-corpus.md`. The example is
reachable and is exercised by a test; it simply does not occur in the corpus.

*Corpus count:* 0.

Recording a zero here is the point. `ours-bug` is the class that must stay at
zero, and stating the measurement makes that checkable rather than assumed.

### `theirs-bug`

The pin violated its own invariants, could not describe its own output, or would
have crashed.

*Worked examples (both measured).*

1. Input `'`. The pin reports `parsed_input_length = 1` with no key-matrix
   column, and querying a key trips an `assert()` that reaches `abort()`. The
   harness records `<no-key-columns>@1`. Registered as **F-E-14**; see
   `docs/findings/oracle-apostrophe-abort.md`.
2. Input `ni'`. The pin yields a key column with no usable pinyin string, and
   the harness records `ni@0:2:complete,<missing-pinyin>@2`. This is catalogue
   row F-E-01 (issue #566) observed live for the first time.

*Corpus count:* 6 of 10,465 — three abort-guard and three missing-string.

`theirs-bug` does not gate S1b: we cannot fix the pin, and the pin is the
subject. It must stay visible, because a `theirs-bug` record is a defect we
should not reproduce and is the strongest candidate for an upstream report.

### `data-version`

The observation came from the pinned code with a different **data** payload: the
model component of the pin ref differs while the libpinyin component matches.
Table content, not code, explains the divergence.

*Worked example (constructed).* Take a frozen F-A record and replace the
`model20-…` digest. The reader still decodes it, the comparison reports
`off-pin`, and the classifier separates it from a code difference by seeing the
libpinyin component intact.

Constructed deliberately: manufacturing a real one would mean building a second
oracle with different tables, which `oracle-environment.md` reserves for a
human-reviewed pin-change PR.

### `distro-delta`

The observation did not come from our pin-built oracle at all — a different
libpinyin build, a different DBM backend, or a pin ref that is not in our
format.

*Worked example (measured, tampered input).* Rewriting `dbm-tkrzw` to
`dbm-berkeleydb` in a frozen F-A line yields reason `off-pin`, class
`distro-delta`. Pinned by a test.

Per `docs/findings/oracle-environment.md` this class is advisory and **never**
gates S1b. A distribution build can be compared for interest; it cannot be the
oracle. The classifier's job here is to make sure such a record can never be
mistaken for a parity result.

## Classification rules

Applied in this order, on top of the reason precedence frozen in
`docs/findings/differential-log.md`:

```text
agreement, rank 0        -> output-identical
agreement, rank > 0      -> tie-swap
reason off-pin           -> data-version   if only the model component differs
                         -> distro-delta   otherwise
reason oracle-sentinel
     | oracle-inconsistent
     | position-overflow  -> theirs-bug
reason ours-error         -> ours-bug
reason consumed-length
     | remainder
     | path-absent        -> flag-semantics  if flags != the parity profile
                          -> path-set        otherwise
```

The rules are total: every comparison receives exactly one class. The
classifier is pure and takes the parity profile as data, so a future profile
change does not require touching it.

## Auto-accept budget

### Frozen meaning (maintainer decision 2026-08-09)

The **0.5% auto-accept budget applies to auto-accepted divergence classes
only.** Today that is `tie-swap` alone.

`output-identical` is **agreement, not divergence.** It is outside the budget.
It must not be summed with `tie-swap` to form a "literal" reading of the card;
that reading inverted the goal (it would have required at most 52 of 10,465
inputs to agree) and is rejected.

For 10,465 inputs the budget limit is **52** auto-accepted divergence records.

Measured:

| Class | Count | Share | In budget? |
|---|---:|---:|---|
| `output-identical` | 9,506 | 90.84% | no — agreement |
| `tie-swap` | 468 | 4.47% | yes — sole auto-accepted divergence class |

`tie-swap` is 468 against a limit of 52 (about 9× over). That number is the
**W4 baseline**, not a W2 gate failure.

### Report, do not gate, until the decoder exists

`tie-swap` compares the oracle's *choice* against our *first enumerated path*.
Our parser does not choose; `parser-path-set.md` has it enumerate every valid
segmentation in a frozen greedy order, and selection is explicitly deferred to
the decoder. At parse level the 4.47% measures "how often does greedy order
disagree with the oracle's scoring", which no parser change can legitimately
reduce — only a decoder can.

The runner continues to compute and report the budget verdict. It must **not**
fail a run on `tie-swap` until W4, when our engine makes a choice and
`tie-swap` acquires its real sense: both sides chose differently among paths
they both admit. Enforcing it now would mean either a permanently red gate or
gaming the corpus, and `docs/findings/parity-corpus.md` already refuses to
trim the stratum that exposes ambiguity.

## Corpus roll-up

Live run, pin
`libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw`,
flags `0x18a`, 10,465 inputs:

| Class | Count |
|---|---:|
| `output-identical` | 9,506 |
| `tie-swap` | 468 |
| `path-set` | 483 |
| `theirs-bug` | 6 |
| `ours-bug` | 0 |
| `flag-semantics` | 0 |
| `data-version` | 0 |
| `distro-delta` | 0 |
| **total** | **10,465** |

`flag-semantics` is zero here because the corpus runs under the parity profile;
it is 10 on the F-C family, which is what that family exists to vary.

## Maintainer decisions (2026-08-09)

1. **Budget meaning — auto-accepted divergences only.** The 0.5% budget
   constrains auto-accepted *divergence* classes (`tie-swap` today).
   `output-identical` is agreement, not divergence, and is excluded.
2. **Budget enforcement — report until W4.** Parse-level `tie-swap` (468) is
   the W4 baseline. Report it; do not gate on it until a decoder exists.
3. **`path-set` attribution.** Confirmed: the 483 sit there because our path
   set disagrees with the pin. The SPEC field invariant is corrected
   (oracle-driven); path-set enumeration and the parser follow-up are a
   separate branch. Post-implementation `path-set` count is the real S1b
   parity gate.
