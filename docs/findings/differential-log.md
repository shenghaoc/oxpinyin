# Findings — W2-T3 differential runner and log schema

Date: 2026-08-09 · Source tier: Architect derivation; human freeze pending.

This finding freezes what the differential runner compares, how it decides
agreement, and the two machine-readable schemas it emits. It is the
implementation contract for W2-T3.

## Scope at this stage

The decoder does not exist yet. W2-T3 therefore compares **parse-level output
only**: the segmentation, the length of the consumed prefix, and the remainder.

Candidate lists and their ranking are **not** compared. `candidate_total` is
recorded in the comparison log because it is free and useful as a later signal,
but no assertion is made about it. Candidate parity belongs to W4, against F-B.

Stating this narrowly matters: a runner that silently compared candidates would
report thousands of divergences that mean nothing more than "we have no
decoder", drowning the real parse-level signal.

## The asymmetry, and why agreement is membership

The two engines answer different questions.

- The oracle reports **one selected segmentation** per input. That is what
  `pinyin_get_pinyin_key` and friends expose, and it is what
  `fixtures/foundation/f-a.txt` records.
- `pinyin_core::FullPinyinParser` returns **every valid segmentation**, in the
  frozen order of `docs/findings/parser-path-set.md`.

`parser-path-set.md` already settles the relationship in its acceptance section:
oracle ranking and its selected segmentation "do not reorder or remove portable
parser paths". So equality of the two outputs is the wrong predicate — it would
fail on `xian` and `fangan` by construction, for inputs where nothing is wrong.

The frozen predicate is therefore:

> **Agreement** holds when the oracle's selected segmentation is a member of our
> ordered path set, and the consumed prefix length agrees, and the remainder
> agrees.

Path membership compares syllable text, `start`, `end` and completeness
exactly. It is not a fuzzy or subset match.

The **rank** of the oracle's path within our ordered set is recorded on every
record, not just divergences. Rank 0 means the oracle chose what our greedy
order puts first. A positive rank is the signal that a ranking policy will have
to reproduce later, and it is exactly the population W2-T5 classifies as
`tie-swap`. Recording it now, before a decoder exists, means W4 starts with a
measured baseline instead of a guess.

### Consumed length is per input, not per path

Every `ParseResult` for one input shares one remainder: the parser starts the
remainder at the first unconsumed byte and attaches the same suffix to each
path. So `ours_consumed` is `input.len() - remainder.len()`, a single value per
input, comparable directly against `pinyin_get_parsed_input_length`.

### Position width

Oracle positions are `guint16`. Inputs longer than 65,535 bytes cannot be
represented on the oracle side, so the runner records a divergence rather than
truncating. The parity corpus caps at 4,096 bytes, so this is a guard, not a
routine path.

## Two producers, one comparison

The comparison is a pure function of an `OracleObservation` and our parse. That
lets the same code run in two modes:

| Mode | Oracle side | Runs where |
|---|---|---|
| `live` | `pinyin-oracle`'s FFI against the pin-built prefix | Linux, `--features oracle-ffi` |
| `replay` | a `pinyin-capture-v1` record parsed from a fixture | any host, portable CI |

Replay is not a mock. It reads real output the pinned oracle already produced,
frozen in `fixtures/foundation/f-a.txt` under a recorded pin ref. So the
comparison logic, the classifier and the log format are all exercised in
portable CI with no oracle installed, and the only thing the Linux tier adds is
fresh observations.

A replayed record whose `pin_ref` is not the frozen pin is a divergence, not an
input error: an off-pin fixture must be visible in the log rather than rejected
at the door.

## Schema: comparison log

`pinyin-diff-v1`. One LF-terminated line per corpus input, TAB-separated
`key=value` fields. Escaping is exactly `pinyin-capture-v1`'s, from
`docs/findings/capture-fixtures.md`: `\\`, `\t`, `\r`, `\n` and `\xNN` for other
control bytes. Reusing that convention rather than inventing one means the
capture fixtures and the differential logs can be read by the same tooling.

Fields, in order:

| Field | Meaning |
|---|---|
| `schema` | `pinyin-diff-v1` |
| `pin_ref` | pin the oracle side was produced under |
| `source` | `live` or `replay` |
| `corpus` | stratum file name, or fixture family |
| `case` | stable identifier: fixture case name, or `stratum:index` |
| `input` | escaped input bytes |
| `flags` | oracle option word, `0x%08x` |
| `outcome` | `agreement` or `divergence` |
| `class` | divergence class, or `-` on agreement |
| `rank` | index of the oracle path in our ordered set, or `-` if absent |
| `ours_paths` | number of paths our parser returned |
| `ours_consumed` | bytes our parser consumed |
| `theirs_consumed` | `parsed_input_length` from the oracle |
| `candidate_total` | oracle's uncapped candidate count, recorded not compared |

The `class` value space is opened by this finding and populated by
`docs/findings/divergence-taxonomy.md` (W2-T5). Until that taxonomy is frozen,
a divergence carries `unclassified`. The field exists from `v1` so adding the
taxonomy does not bump the schema.

## Schema: divergence log

`pinyin-divergence-v1`. One line per divergence only, carrying enough detail to
triage without re-running. W2-T4 is the human triage pass over this file.

Fields, in order:

| Field | Meaning |
|---|---|
| `schema` | `pinyin-divergence-v1` |
| `pin_ref`, `source`, `corpus`, `case`, `input`, `flags` | as above |
| `class` | divergence class |
| `reason` | machine-readable token, see below |
| `theirs_path` | oracle segmentation in capture notation, or `-` |
| `theirs_consumed`, `theirs_remainder` | oracle side |
| `ours_paths` | total number of paths our parser returned |
| `ours_shown` | how many are spelled out in `ours_path_set` |
| `ours_path_set` | first [`MAX_LOGGED_PATHS`] of our paths, `;`-separated |
| `ours_consumed`, `ours_remainder` | our side |

Segment notation matches `capture-fixtures.md`: `text@begin:end:complete` or
`:partial`, comma-separated within a path. Our path set caps at
`MAX_LOGGED_PATHS` = 8 spelled-out paths, with `ours_paths` retaining the
uncapped count, so a highly ambiguous input cannot produce an unbounded line.

### Reason tokens

Fixed, machine-readable, and independent of the taxonomy. A record carries the
first applicable token in **this precedence order**, so triage buckets are
stable rather than depending on the order checks happen to run:

| # | Token | Meaning |
|---:|---|---|
| 1 | `off-pin` | the observation's pin ref is not the frozen pin |
| 2 | `oracle-inconsistent` | the oracle's own invariants failed |
| 3 | `position-overflow` | a position does not fit `u16` |
| 4 | `oracle-sentinel` | the oracle emitted a sentinel segment (F-E-01 shape) |
| 5 | `ours-error` | our parser returned `Err` |
| 6 | `consumed-length` | consumed prefix lengths disagree |
| 7 | `remainder` | remainders disagree |
| 8 | `path-absent` | the oracle's segmentation is not in our path set |

The order is not arbitrary. Validity of the subject and of the observation comes
first, because a comparison against an off-pin or self-inconsistent oracle is
meaningless. Our own failure comes next, since path membership cannot be
evaluated without a path set. Only then do the three substantive comparisons
run, narrowing from the coarsest disagreement (how much was consumed) to the
finest (which segmentation was chosen).

## Summary report

The runner also emits a human-readable summary: totals, agreement count,
per-class counts, the rank histogram, and the auto-accept budget verdict from
W2-T5. The summary is derived from the logs and is not itself a schema.

## Determinism

Given the same corpus, flags and oracle prefix, the runner emits byte-identical
logs. Records appear in corpus order, strata in file-name order. Nothing is
parallelised across records, and no timestamp, hostname or path outside the
prefix enters a log line. A differential log is therefore diffable between runs,
which is what makes "did this change alter parity?" answerable by `git diff`.
