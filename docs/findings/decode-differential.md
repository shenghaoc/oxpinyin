# Decode-level differential SPEC

Date: 2026-08-09 · Status: **frozen for W4-T4**

`docs/findings/differential-log.md` froze the parse-level comparison and said
plainly what it was not doing:

> Candidate lists and their ranking are **not** compared … Candidate parity
> belongs to W4, against F-B.

This finding is that comparison. It is a new document rather than an edit:
`differential-log.md` is frozen, its schema is in use, and a decoder does not
change what a parser run measured.

## Two populations, because the pin answers two different questions

| Population | Source | Size |
|---|---:|---:|
| Segmentation | `fixtures/w4/oracle-paths.txt` — the pin's selected path for every corpus input | 10,459 |
| Candidates | F-A and F-C records carrying a candidate list | 56 |

The segmentation population is the whole W2 parity corpus and is where the
483 `path-set` divergences and the 468 `tie-swap` baseline live. The candidate
population is small because F-B was never captured
(`docs/findings/fixture-adapters.md`); it is what exists.

Both run in portable CI with no oracle build, which is the point of recovering
the paths in the first place.

## Metrics

Over segmentations, comparing the pin's selected path against our ranked
k-best (`k` = 8, matching the session):

- **top-1** — our best path *is* the pin's.
- **top-5** — the pin's path is in our first five.
- **present in the k best** — the pin's path is somewhere in our result. Its
  complement is the decode-level successor to `path-absent`.

Over candidates, driven through the session API so the numbers describe what
a shell would actually see:

- **top-1** — our first candidate is the pin's first candidate.
- **top-5-set** — the pin's first candidate appears in our first five.
- **prefix-10** — how many of the pin's first ten candidates appear in our
  first ten, as a share of the pin's ten. The capture protocol records ten, so
  ten is the depth the evidence supports.

A record no phrase of the mini vocabulary can reach is excluded from the
candidate denominator. Including it would measure the size of an authored
90-phrase fixture, not the decoder.

## Measured at this freeze

```text
decode differential — segmentation, W2 parity corpus
  compared                 10459
  top-1                    10119   96.75%
  top-5                    10457   99.98%
  present in the k best    10457   99.98%
  path absent                  2    0.02%
    absent  'ni
    absent  ni''hao

decode differential — candidates, F-A and F-C
  records with a candidate list   56
  reachable by the mini vocab     25
  top-1                           18   72.00%
  top-5-set                       23   92.00%
  prefix-10 overlap              186 of 250   74.40%
```

### What moved

| Measure | W2 baseline | W4 |
|---|---:|---:|
| `path-set` divergences | 485 | **0** |
| paths the pin chose that we cannot represent | 485 | **2** (both open by decision) |
| chose-differently-among-shared-paths (`tie-swap`) | 468 | **338** |
| the pin's path is our first | 9,506 (90.84%) | **10,119 (96.75%)** |

The 483 incomplete-key divergences are gone: every one of those paths is now a
graph path and the decoder reaches it. The two that remain are the leading-
and doubled-apostrophe cases that maintainer decision 3 in
`parser-spec-contradiction-incomplete-keys.md` leaves open, and the graph
deliberately does not settle.

`tie-swap` also acquires its real meaning here for the first time.
`divergence-taxonomy.md` said it could not be assessed until a decoder existed,
because at parse level it compared the pin's *choice* against our *first
enumerated path* and no parser change could legitimately move it. It is now
both sides choosing, and it fell from 468 to 338 — against an auto-accept
budget of 52. Still over, still reported rather than gating, and now for a
reason a decoder can actually address.

### How to read the candidate numbers

Do not read them as parity. They are evidence that the machinery works:

- the vocabulary is 90 authored phrases against the pin's tens of thousands,
  so 31 of 56 records are unreachable before scoring begins;
- the weights are derived from captured candidate *rank*, not frequency, so a
  phrase the pin ranked second for one input carries a weight it would never
  have in a real unigram table — `霓虹` outranks `你` here and never would
  there;
- the scoring constants are provisional (`scoring-spec.md`).

W3's real tables replace the fixture adapters by changing two type arguments.
These same three numbers, measured then, are what a parity claim would rest
on.

## Gating

The segmentation run **gates**: the set of unreachable paths must be exactly
the two named apostrophe cases, and top-1 must stay above 80%. A change that
loses a path the pin chose fails the build.

The candidate run **reports**, with a floor on top-5-set so a collapse is
visible. Its absolute values are not a parity gate and must not be quoted as
one until the real tables are behind it.

## Reproduction

```bash
cargo test -p pinyin-oracle --test decode_differential -- --nocapture
```

No oracle prefix, no `oracle-ffi`, no Linux requirement. The live tier is
unchanged and still available for producing a fresh
`fixtures/w4/oracle-paths.txt`; `segment-graph.md` records how.
