# Full-pinyin parser SPEC

Date: 2026-08-09 · Status: **frozen for Foundation Tasks 6–9**

What binds: the parser behaviour that this SPEC derives from the pin and that is observable through the C APIs — the syllable inventory, the input domain, the result fields and their invariants, the boundary and apostrophe rules, the result bound, and the totality and ownership rules ([frozen-SPEC ruling](compatibility-policy.md#frozen-specs)).
A change to that contract needs a maintainer ruling, recorded in the [correction log](#architect-correction-log). Implementation stops until the ruling is recorded.
The Rust type definitions are current reference for oxpinyin's internal surface.

This SPEC is the parser contract that the first parser implementation was written against.
It refines Foundation R3.
The `InputParser` trait of the first implementation no longer exists; `FullPinyinParser::parse_with_options` (`crates/oxpinyin-core/src/parser.rs`) is the entry point.
The path set that the parser returns, and its order, is in [parser-path-set.md](parser-path-set.md).

## Scope and inventory

The parser accepts untuned full pinyin only.
The complete-syllable inventory is the set of active keys in `PINYIN_DICT` in the libpinyin source file `scripts2/fullpinyin.py`.
The identity below is the pin at freeze, libpinyin `2.11.91`.
The file has the same SHA-256 at the current pin, `074a2219`:

- source tag: `2.11.91`;
- source commit: `0c5e80e1200f84fab185d1c5bde458b770a0636c`;
- source archive SHA-256:
  `eb25890dab0072eb0744c9ee1bc152051143b7bc23aea2a424792a9b1b84bdcb`;
- extracted `scripts2/fullpinyin.py` SHA-256:
  `031ea2909fd94fdea1cdcedbabcde66e696ed14c0c6d30138af5c0bb2f9a48b4`;
- active complete syllables: **405**;
- maximum complete-syllable length: **6 ASCII bytes**.

The inventory keeps those active keys in their source numeric-ID order.
It includes `ng`, `lv`, `lve`, `nv` and `nve`.
It excludes:

- commented entries (`den`, `kei`, `lue`, `nue`, `tei` and the commented “weird pinyins”);
- the 23 incomplete initial-only entries;
- correction spellings, fuzzy aliases, tones and Zhuyin.

The source file is GPL-2.0-or-later.
This GPL-3.0-or-later project records the source and the transformation.
It does not treat the inventory as model-archive data under the model-archive no-vendor rule ([model-provenance.md](model-provenance.md)).

A **partial syllable** is a non-empty proper byte prefix of at least one of the 405 complete syllables.
So initial-only strings such as `h`, `zh` and `w` are partials, not complete table entries.

A **valid segmentation** is a path of complete syllables that consumes its whole lowercase group.
R3.7 requires every such path.
A partial-tail result is the R3.3/R3.4 fallback when no valid segmentation consumes the group.
It is not an additional valid segmentation when complete paths exist.

## Input domain

`FullPinyinParser::parse_with_options` accepts the complete `&[u8]` domain.
Only lowercase ASCII `a`–`z` and ASCII apostrophe (`0x27`) have parser syntax.
All other bytes are unsupported: uppercase letters, digits, whitespace, punctuation other than apostrophe, non-ASCII UTF-8 and malformed UTF-8.
An unsupported byte starts the untouched remainder; it is not a syntax error.

The parser does no normalization, case folding, tone stripping, typo correction or fuzzy matching.
It does no resplitting beyond the enumeration of table-valid boundaries.
These are later policy layers.

## Output types

`oxpinyin-core` defines these public dependency-free types:

```rust
pub const MAX_PARSE_RESULTS: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Completeness {
    Complete,
    Partial,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ParsedSyllable {
    pub syllable: String,
    pub start: usize,
    pub end: usize,
    pub completeness: Completeness,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseResult {
    pub syllables: Vec<ParsedSyllable>,
    pub remainder: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FullPinyinParser;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ParseError {
    TooManyAlternatives { limit: usize },
}
```

`ParseError` implements `Display` and `std::error::Error`.
`FullPinyinParser` has this entry point:

```rust
impl FullPinyinParser {
    pub fn parse_with_options(&self, input: &[u8], options: OptionBits)
        -> Result<Vec<ParseResult>, ParseError>;
}
```

Every successful call returns at least one result.
Malformed, junk and partial bytes return `Ok`; they never cause `ParseError`.

## Result bound

A repeated ambiguous input has exponentially many complete paths: `n` apostrophe-separated `xian` groups have `2^n` Cartesian combinations.
The entry point returns an owned `Vec`, so it cannot hold an unbounded exhaustive result in finite `usize` and memory.
So the design phrase “never an error for well-formed UTF-8” has this meaning: syntax never gives an error, but representational exhaustion does.

Before it materializes results, the implementation counts paths, saturating at `MAX_PARSE_RESULTS + 1`.
If the complete or maximal-partial ordered path set has more than 4,096 results, the parser returns `ParseError::TooManyAlternatives { limit: MAX_PARSE_RESULTS }` and no subset.
At or below the limit, it returns every path.
This keeps R3.5 (no panic for any bytes) and R3.7 (never silently truncate valid paths).

## Field invariants

- `syllable` is lowercase ASCII.
  For a complete segment it is one of the 405 table entries; for a partial segment it is a proper prefix of one.
- `start..end` is a non-empty half-open byte range into the original input, and it is exactly equal to `syllable.as_bytes()`.
- Segment ranges are strictly increasing and do not overlap.
  Apostrophe bytes may form one-byte gaps; no segment spans an apostrophe.
- A `ParseResult` that `FullPinyinParser` returns contains at most one partial segment, and it is the last segment.
  This restriction belongs to the portable parser; it is not pin behaviour.
- The pin's selected split has no such restriction (correction of 2026-08-09; see the log).
  Under `PINYIN_INCOMPLETE`, an initial-only key may appear at any position and more than once.
- `remainder` is an exact owned copy of an input suffix.
  Its start offset is `input.len() - remainder.len()`.
- A segment or a consumed apostrophe separator accounts for every byte before the remainder start.
- An empty remainder represents complete consumption.

## Boundaries, partials, and junk

**Apostrophes.**
An apostrophe is a hard boundary: complete syllables never span it.
Alternatives on fully consumed sides combine as a Cartesian product.
In the portable parser, a trailing apostrophe is not a valid separator and starts the remainder.
Upstream's `FullPinyinParser2::parse` (`pinyin_parser2.cpp:237-250`) treats every apostrophe at a reachable position as a zero-width step-propagation.
From that:

- A **leading** run of consecutive apostrophes is consumed when the group after it consumes at least one byte.
  `'ni` consumes all three bytes, and `''ni` consumes all four.
- A **non-leading** run of consecutive apostrophes after a fully consumed left group is a single separator under the same rule.
  `ni''hao` consumes the whole input.
- In the portable parser, an apostrophe run with no productive right side (for example `'`, `ni''`, `ni''!`) starts the remainder at the first apostrophe of the run.
  For a leading run, that is position 0.
- An empty result with an unchanged remainder is not a right-side continuation.

The parsed length that the C APIs report follows the pin, and it includes a trailing apostrophe run (`pinyin_parser2.cpp:237-250`; `full_parsed_len` in `crates/oxpinyin-engine/src/session/offsets.rs`).

**Partials.**
Two layers handle partials, and they differ.

*The pin's selected split.*
This behaviour binds.
`FullPinyinParser2::parse` returns one key sequence, not a path set (`pinyin_parser2.cpp:217-344`).
When the option word has `PINYIN_INCOMPLETE`, each of the 23 initial-only keys is a key at any position (`pinyin_parser2.cpp:43-45`, `:266-284`).
A split can contain more than one such key, interleaved with complete keys.
When the option word does not have `PINYIN_INCOMPLETE`, the pin admits no initial-only key.
The pin selects the longest parsed length, then the fewest keys, then the smallest distance (`pinyin_parser2.cpp:296-310`).
`SegmentGraph::fewest_keys` implements this split for the C APIs ([segment-graph.md](segment-graph.md)).

*The portable path set.*
This description is current reference.
`FullPinyinParser::parse_with_options` returns the path set of [parser-path-set.md](parser-path-set.md).
A path there has at most one partial, and that partial is the last segment.
That partial is any proper prefix of a table syllable, not only an initial-only key.
The path set does not enumerate mid-path or repeated partials, because that enumeration is exponential.
No C API returns the path set.

**Junk.**
At the first unsupported or otherwise unconsumable byte, parsing stops.
The parser keeps the prefix that it already segmented.
It returns that byte and every byte after it unchanged as `remainder`.

## Totality and ownership

- The implementation is iterative or uses input-bounded control flow.
  It must not recurse according to untrusted input length.
- No indexing operation may assume UTF-8 or a character boundary.
- Every byte sequence returns `Ok` or the bounded alternative error, without a parser panic.
- Outputs own all variable data and do not borrow the input.
- Selection among returned paths belongs to the decoder and is outside this SPEC.

## Architect correction log

Each entry is an oracle-driven SPEC correction: differential evidence against the pin, not design preference, changed a frozen invariant.
The constitution permits an edit to a frozen SPEC only with an explicit ask; the ask for each entry is the maintainer decision recorded here.

### 2026-08-09 — oracle-driven SPEC correction: partial placement

**Previous invariant.** A result contains at most one partial segment, and it is the last segment.

**Corrected invariant.** Partial segments may appear at any position, more than once, interleaved with complete segments.

**Evidence.** W2-T3 live run over the 10,465-input parity corpus against the pin-built oracle at flags `0x18a`.
Of 491 divergences, **483 (98.4%)** share this single root cause.
Worked example, input `yingchon`:

```text
theirs  ying@0:4:complete, ch@4:6:partial, o@6:7:complete, n@7:8:partial
ours    ying@0:4:complete, chon@4:8:partial
        yi@0:2:complete, ng@2:4:complete, chon@4:8:partial
```

The pin's selected path has two non-final partials.

**Scope.** Mid-path and repeated partials multiply the path set.
This correction changes only the field invariant and the prose that states the observed upstream policy.
If the parser admits mid-path and repeated partials, `MAX_PARSE_RESULTS` (4,096) and its interaction with the corpus must be re-evaluated. That re-evaluation, the `parser-path-set.md` amendment and the parser change are separate work.

### 2026-08-21 — oracle-driven SPEC correction: doubled apostrophe

Maintainer decision 3 is whether the parser follows the pin on a leading apostrophe and on a doubled apostrophe.
This entry resolves the doubled-apostrophe half by alignment with the pin.

**Previous invariant.** A leading, repeated, or trailing apostrophe is not a valid separator and starts the remainder.

**Corrected invariant.** A leading or trailing apostrophe is not a valid separator and starts the remainder.
A run of consecutive apostrophes after a fully consumed left group is a single separator when the group after it consumes at least one byte.

**Evidence** (recorded evidence; the reduced `fixtures/w4/oracle-candidate-structure.txt` does not hold this input). `ni''hao` (line 33 of stratum `09-edge` of the retired W2 corpus) → oracle top-1 `你好`.
Every candidate in the oracle's top-10 requires the doubled apostrophe to act as a single separator.
Before this correction, `ni''hao` was the only `absent` input in the W2 residual ([candidate-construction.md](candidate-construction.md) §8.4, Class B).

**Freeze move** (recorded evidence; the test is deleted).
`real_tables_session_reports_parity` moved from 10,177 / 10,189 / 94,871 of 98,930 / absent 1 to **10,178 / 10,190 / 94,872 of 98,930 / absent 0**; tie-swaps stayed at 1,036.

### 2026-08-21 — oracle-driven SPEC correction: leading apostrophe

This entry resolves the leading-apostrophe half of maintainer decision 3 by alignment with the pin.

**Previous invariant.** A leading or trailing apostrophe is not a valid separator and starts the remainder.

**Corrected invariant.** A trailing apostrophe starts the remainder.
A leading run of consecutive apostrophes is consumed when the group after it consumes at least one byte.
A leading run with no productive right side starts the remainder at position 0.

**Evidence** (recorded evidence). For `'ni` the oracle consumes 3 and oxpinyin consumed 0 (`consumed-length`).
Upstream's `FullPinyinParser2::parse` (`pinyin_parser2.cpp:237-250`) treats every apostrophe at a reachable position as a zero-width step-propagation, including at position 0.
