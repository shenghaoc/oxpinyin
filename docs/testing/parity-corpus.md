# Findings — W2-T2 parity corpus

Date: 2026-08-09 · Source tier: Architect derivation; human freeze pending.

This finding freezes the sampling method, the byte domain and the reproducibility
contract of the Stage 1 parity corpus. The corpus was the input set of the
`candidate_surface` test. Its input files under `tests/parity/corpus/inputs/`
are retired (#1764): the test now iterates the inputs that
`fixtures/w4/oracle-candidate-structure.txt` holds. The generator is not in the tree.
The generator, layout, strata and digests below are recorded evidence.

The corpus holds **inputs only**. The outputs of the pin are frozen captures
under `fixtures/w4/`, and they are not part of the corpus.

## Why generated rather than harvested

Parity is measured over *keystroke sequences*, not over Chinese text. A
generated corpus is better here than a harvested one, for three reasons:

- **Redistribution.** Harvested Chinese text has licensing obligations that the
  project does not need. Generated pinyin strings come from an inventory that the
  repository already holds.
- **Coverage control.** Adversarial shapes (partial tails, junk positions,
  apostrophe misuse) are rare in natural text, but a parser
  diverges on exactly these shapes. Stratified generation guarantees a minimum count of each.
- **Reproducibility.** A reviewer can audit a generator and a fixed seed. A
  harvest is a snapshot that no reviewer can derive again.

The trade-off is that the inter-syllable frequency does not match real typing. That
matters for *ranking* work (W4 onward), and not for the parse-level comparison that
W2-T3 performs. When ranking parity is measured, an additional
frequency-weighted family should be derived from F-B. That family is out of scope here.
This finding records it as a known limitation.

## Source inventory

Syllables come from `oxpinyin_core::FULL_PINYIN_SYLLABLES`: the 405 active
complete syllables that `docs/findings/parser-spec.md` freezes, in their source
numeric-ID order. The generator read that constant and did not copy it, so
the corpus could not drift from the frozen inventory.

## Byte domain

Every emitted line is printable ASCII in `0x20..=0x7e`, with no TAB, CR, LF or
NUL.

The exclusions are deliberate, and other tests cover each of them:

| Excluded | Why | Where it is covered |
|---|---|---|
| NUL (`0x00`) | Not representable across the oracle's `const char *` boundary | parser proptest and `cargo-fuzz` |
| CR, LF, TAB | Would break the one-record-per-line corpus and log formats | fuzz target |
| Non-ASCII and malformed UTF-8 | Not representable in a line-oriented ASCII corpus | parser proptest and `cargo-fuzz` (F-E-12) |

The corpus is therefore not the totality gate. The property and fuzz tiers of the parser
already prove totality over arbitrary bytes. The corpus exists to
measure *parity*, and it stays in the domain that both engines can represent.

## Result-bound exclusion

`docs/findings/parser-spec.md` limits the materialised path set to
`MAX_PARSE_RESULTS` (4,096), and returns `ParseError::TooManyAlternatives`
above it. The generator excludes inputs that would cross that limit, in two
layers:

1. structurally, by a limit of 8 apostrophe-separated groups per utterance;
2. by assertion, by rejecting any candidate input for which the frozen parser does
   not return `Ok`.

Resource-bound behaviour is therefore *not* a parity question in this corpus.
The parser's own
`cartesian_path_limit_is_exact` test already pins it exactly: it asserts the boundary at 12 and 13
two-way groups. The exclusion from the differential run loses nothing.
If the corpus included these inputs, they would show as divergence noise unrelated to
segmentation.

## Determinism

The generator uses SplitMix64 seeded with a constant, implemented inline. It adds no
dependency. It does not use system entropy, the clock, hash-map iteration order,
filesystem order or thread scheduling. The output is a pure function
of the frozen inventory and the seed.

```text
seed = 0x5061_7269_7479_3031   // "Parity01"
```

SplitMix64 was chosen because it is about twelve lines, has no state-initialisation
subtleties, and is portable. The corpus must regenerate identically
on every supported host, not only on Linux.

Indices are drawn with rejection sampling against the largest multiple of the
range below `2^64`, so no modulo bias changes the syllable distribution.

## Strata

The counts are fixed, not sampled, so the corpus size is stable across
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

- Stratum 01 is exhaustive, not sampled: every syllable must appear at
  least once, and 405 lines are cheap.
- Stratum 07 starts from the syllables
  that `parser-path-set.md` records as admitting more than one complete
  segmentation. Tie-swap divergences concentrate there.
- Stratum 09 mirrors
  the F-A edge cases, so that the corpus and the curated fixture agree on shape.

Duplicates are removed within each stratum; the first occurrence stays. Each
stratum is emitted in generation order. A line may repeat across strata.
This is intentional: a short utterance may also be a
syllable pair, and global de-duplication would make the counts depend on the draw
order.

## Layout and format

```text
tests/parity/corpus/inputs/01-single-syllable.txt
...
tests/parity/corpus/inputs/09-edge.txt
```

Each file has one input per LF-terminated line, no trailing blank line, no comments and no
escaping. The byte domain excludes CR, LF and NUL, so a line *is* its
input and no decoding step can add ambiguity.

The empty input in stratum 09 is the only case that a line-oriented file cannot
express. The single empty line at the start of
`09-edge.txt` represents it. Readers must therefore keep an empty first line and not
skip blank lines.

## Frozen output

The generator produced these files on 2026-08-09. Two runs produced
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

The corpus was a set of frozen input files. It has no generator in the tree, and the
digests above are the byte record.
`crates/oxpinyin-runtime/tests/candidate_surface.rs` reads no corpus file.
It reads the inputs from `fixtures/w4/oracle-candidate-structure.txt`, checks their count and compares the candidates of
each input with the frozen pin capture.

A change to the retained inputs is a deliberate, reviewed act. It moves the `candidate_surface`
counts and needs a new pin capture. It therefore belongs in its own commit, together with the edited capture and
the reason for the change.
