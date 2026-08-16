# Findings — W2-T2 parity corpus

Date: 2026-08-09 · Source tier: Architect derivation; human freeze pending.

This finding freezes the sampling methodology, byte domain and reproducibility
contract for the Stage 1 parity corpus. It is the implementation contract for
W2-T2 and the input set for W2-T3.

The corpus holds **inputs only**. Expected outputs are not committed here:
W2-T3 produces the oracle side by observation, so a committed expectation would
be a second, unverified oracle.

## Why generated rather than harvested

Parity is measured over *keystroke sequences*, not over Chinese text. A
generated corpus is preferable to a harvested one here for three reasons:

- **Redistribution.** Harvested Chinese text carries licensing weight the
  project does not need. Generated pinyin strings derive from an inventory the
  repository already holds.
- **Coverage control.** Adversarial shapes (partial tails, junk positions,
  apostrophe misuse) are rare in natural text but are exactly where a parser
  diverges. Stratified generation guarantees a floor on each.
- **Reproducibility.** A committed generator plus a fixed seed is auditable. A
  harvest is a snapshot no reviewer can re-derive.

The trade-off is that inter-syllable frequency does not match real typing. That
matters for *ranking* work (W4 onward) and not for the parse-level comparison
W2-T3 performs. When ranking parity is measured, an additional
frequency-weighted family should be derived from F-B; that is out of scope here
and is recorded as a known limitation rather than silently ignored.

## Source inventory

Syllables come from `oxpinyin_core::FULL_PINYIN_SYLLABLES`: the 405 active
complete syllables frozen by `docs/findings/parser-spec.md`, in their source
numeric-ID order. The generator reads that constant rather than restating it, so
the corpus cannot drift from the frozen inventory.

## Byte domain

Every emitted line is printable ASCII in `0x20..=0x7e`, with no TAB, CR, LF or
NUL.

The exclusions are deliberate and each has coverage elsewhere:

| Excluded | Why | Where it is covered |
|---|---|---|
| NUL (`0x00`) | Not representable across the oracle's `const char *` boundary; `Session::observe` rejects it | `interior_nul_is_rejected_without_reaching_the_oracle` |
| CR, LF, TAB | Would break the one-record-per-line corpus and log formats | fuzz target |
| Non-ASCII and malformed UTF-8 | Not representable in a line-oriented ASCII corpus | parser proptest and `cargo-fuzz` (F-E-12) |

So the corpus is not the totality gate. Totality over arbitrary bytes is
already proven by the parser's property and fuzz tiers; the corpus exists to
measure *parity*, and it stays in the domain both engines can represent.

## Result-bound exclusion

`docs/findings/parser-spec.md` bounds the materialised path set at
`MAX_PARSE_RESULTS` (4,096) and returns `ParseError::TooManyAlternatives`
beyond it. The generator excludes inputs that would cross that bound, in two
layers:

1. structurally, by capping apostrophe-separated groups per utterance at 8;
2. by assertion, rejecting any candidate input for which the frozen parser does
   not return `Ok`.

Resource-bound behaviour is therefore *not* a parity question in this corpus.
It is already pinned exactly by the parser's own
`cartesian_path_limit_is_exact` test, which asserts the boundary at 12 and 13
two-way groups. Nothing is lost by keeping it out of the differential run,
and including it would otherwise show up as divergence noise unrelated to
segmentation.

## Determinism

The generator uses SplitMix64 seeded with a constant, implemented inline. No
dependency is added, and no system entropy, clock, hash-map iteration order,
filesystem order or thread scheduling is consulted. Output is a pure function
of the frozen inventory and the seed.

```text
seed = 0x5061_7269_7479_3031   // "Parity01"
```

SplitMix64 was chosen because it is a dozen lines, has no state-initialisation
subtleties, and is trivially portable — the corpus must regenerate identically
on every supported host, not merely on Linux.

Indices are drawn with rejection sampling against the largest multiple of the
range below `2^64`, so no modulo bias skews the syllable distribution.

## Strata

Counts are fixed, not sampled, so the corpus size is stable across
regenerations.

| File | Stratum | Count | Shape |
|---|---|---:|---|
| `01-single-syllable.txt` | Every table entry | 405 | each of the 405 syllables alone, in frozen order |
| `02-syllable-pairs.txt` | Two syllables | 2,000 | concatenated, no separator |
| `03-short-utterances.txt` | Three to four syllables | 2,500 | concatenated |
| `04-long-utterances.txt` | Five to twelve syllables | 1,500 | concatenated |
| `05-apostrophe.txt` | Explicit boundaries | 1,200 | 2–8 groups joined by `'` |
| `06-partial-tails.txt` | Incomplete tail | 1,200 | complete prefix plus a proper prefix of a syllable |
| `07-ambiguity.txt` | Segmentation-ambiguous | 800 | built from syllables that admit multiple splits |
| `08-junk.txt` | Unsupported bytes | 800 | printable non-lowercase inserted at prefix, middle, suffix |
| `09-edge.txt` | Boundary shapes | 60 | empty, single letters, apostrophe misuse, long junk |
| **Total** | | **10,465** | |

Stratum 01 is exhaustive rather than sampled: every syllable must appear at
least once, and 405 lines is cheap. Stratum 07 is seeded from the syllables
that `parser-path-set.md` records as admitting more than one complete
segmentation, which is where tie-swap divergences concentrate. Stratum 09 mirrors
the F-A edge cases so the corpus and the curated fixture agree on shape.

Duplicates are removed within each stratum, preserving first occurrence, and
each stratum is emitted in generation order. Across strata a line may repeat;
that is intentional, because a short utterance may legitimately also be a
syllable pair and de-duplicating globally would make counts depend on draw
order.

## Layout and format

```text
tests/parity/corpus/inputs/01-single-syllable.txt
...
tests/parity/corpus/inputs/09-edge.txt
```

One input per LF-terminated line, no trailing blank line, no comments, no
escaping. Because the byte domain excludes CR, LF and NUL, a line *is* its
input and no decoding step can introduce ambiguity.

The empty input in stratum 09 is the sole case a line-oriented file cannot
express, and it is represented by the single empty line at the head of
`09-edge.txt`. Readers must therefore preserve an empty first line rather than
skipping blanks.

## Frozen output

Generated 2026-08-09 with the committed generator. Running it twice produced
byte-identical files, which is the W2-T2 acceptance criterion.

| File | Inputs | SHA-256 |
|---|---:|---|
| `01-single-syllable.txt` | 405 | `30284a9f08317d99047430694dccc2d03c985b8cc0b8c3055cbb569542e14933` |
| `02-syllable-pairs.txt` | 2,000 | `0231beff32cd75e7aa30efb18d4dad828af4568f63b835d9823038b929217486` |
| `03-short-utterances.txt` | 2,500 | `1cce781f600113863262a7671de93219e47755daea495b2289a2819b155bbd69` |
| `04-long-utterances.txt` | 1,500 | `aace505f39bc0c4fef48edaf76aeadf226459e4cf1ea9dfcac467f176319a186` |
| `05-apostrophe.txt` | 1,200 | `92b9793e6f1b1abaad4b4c1f9585cae6fcde8c756657162abf011aa4dc2de414` |
| `06-partial-tails.txt` | 1,200 | `fc33e25d2429b528bb838be53dee650d9a3d2d72ac431af4e2c38c3f4bfd79dc` |
| `07-ambiguity.txt` | 800 | `8bae35d41a69fdadc23384f35ab8344b7ff3f575ad22249ddd045481fe280812` |
| `08-junk.txt` | 800 | `5442ca37c553d4cfa92f1cf9ab743c2fd79f59542a98a63cd6ed87952fe82abb` |
| `09-edge.txt` | 60 | `87e230fa2ba89f2b4b58f832214fd2ffa6d15db68b610eb0d1938783cf5a20ce` |
| **Total** | **10,465** | |

## Reproducibility contract

The acceptance criterion is that generating twice yields identical output. That
is enforced as a test rather than a manual step: `tests/parity_corpus.rs`
regenerates every stratum in memory and asserts byte equality against the
committed files, so drift fails CI on the portable tier with no oracle present.
It also checks that the directory holds exactly the generated strata, that the
per-stratum counts match the table above, and that no line carries edge
whitespace or a control byte.

Regenerate with:

```bash
cargo run -p pinyin-oracle --bin parity-corpus -- tests/parity/corpus/inputs
git diff --exit-code tests/parity/corpus/inputs
```

A corpus change is a deliberate, reviewed act: it moves the W2-T3 divergence
baseline, so it belongs in its own commit alongside the regenerated files and
the reason for the change.
