# Full-pinyin parser SPEC

Date: 2026-08-09 · Status: frozen for Foundation Tasks 6–9
(amended 2026-08-09: oracle-driven SPEC correction on partial placement)

This finding is the human-frozen parser contract required before the first
parser implementation commit. It refines Foundation R3 and the signature in
`docs/findings/core-trait-seam.md`; a behavior change requires an Architect
correction before implementation resumes. One such correction has landed: see
the Architect correction log at the end of this file.

## Scope and inventory

Foundation parses untuned full pinyin only. The complete-syllable inventory is
the set of active keys in `PINYIN_DICT` from the pinned libpinyin `2.11.91`
source file `scripts2/fullpinyin.py`:

- source tag: `2.11.91`;
- source commit: `0c5e80e1200f84fab185d1c5bde458b770a0636c`;
- source archive SHA-256:
  `eb25890dab0072eb0744c9ee1bc152051143b7bc23aea2a424792a9b1b84bdcb`;
- extracted `scripts2/fullpinyin.py` SHA-256:
  `031ea2909fd94fdea1cdcedbabcde66e696ed14c0c6d30138af5c0bb2f9a48b4`;
- active complete syllables: **405**;
- maximum complete-syllable length: **6 ASCII bytes**.

Task 6 preserves those active keys in their source numeric-ID order. The
inventory includes `ng`, `lv`, `lve`, `nv`, and `nve`. It excludes commented
entries (`den`, `kei`, `lue`, `nue`, `tei` and the commented “weird pinyins”),
the 23 incomplete initial-only entries, correction spellings, fuzzy aliases,
tones, and Zhuyin. The source file is GPL-2.0-or-later; this GPL-3.0-or-later
project records the source and transformation rather than treating the
inventory as model-archive data covered by the model-archive no-vendor rule
(see `model-provenance.md`).

A **partial syllable** is a non-empty proper byte prefix of at least one of the
405 complete syllables. Initial-only strings such as `h`, `zh`, and `w` are
therefore partials, not complete table entries.

A **valid segmentation** is a path of complete syllables that consumes its
whole lowercase group. R3.7 requires every such path. A partial-tail result is
the R3.3/R3.4 fallback when no valid segmentation consumes the group; it is
not an additional valid segmentation when complete paths exist.

## Input domain

`FullPinyinParser` implements `InputParser` over the complete `&[u8]` domain.
Only lowercase ASCII `a`–`z` and ASCII apostrophe (`0x27`) have parser syntax.
Uppercase letters, digits, whitespace, punctuation other than apostrophe,
non-ASCII UTF-8 and malformed UTF-8 are unsupported bytes. An unsupported byte
starts the untouched remainder; it is not a syntax error.

There is no normalization, case folding, tone stripping, typo correction,
fuzzy matching, or resplitting beyond enumeration of table-valid boundaries.
These are later policy layers.

## Frozen output types

Task 7 adds these public dependency-free types to `oxpinyin-core`:

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
`FullPinyinParser` implements:

```rust
impl InputParser for FullPinyinParser {
    type Parse = ParseResult;
    type Error = ParseError;

    fn parse(&self, input: &[u8]) -> Result<Vec<ParseResult>, ParseError>;
}
```

Every successful call returns at least one result. Malformed, junk and partial
bytes return `Ok`; they never cause `ParseError`.

## Resource-bound Architect clarification

A repeated ambiguous input has exponentially many complete paths: `n`
apostrophe-separated `xian` groups have `2^n` Cartesian combinations. The
frozen `InputParser` seam returns an owned `Vec`, so unbounded exhaustive
materialization is not representable for finite `usize` and memory. This
finding therefore clarifies the design phrase “never an error for well-formed
UTF-8”: syntax never errors, but representational exhaustion does.

Before materializing results, the implementation counts paths with saturation
at `MAX_PARSE_RESULTS + 1`. If the complete or maximal-partial ordered path set
would exceed 4,096 results, it returns
`ParseError::TooManyAlternatives { limit: MAX_PARSE_RESULTS }` and returns no
subset. At or below the limit, it returns every path. This preserves R3.5
(no panic for any bytes) and R3.7 (never silently truncate valid paths).

## Field invariants

- `syllable` is lowercase ASCII. For a complete segment it is one of the 405
  table entries; for a partial segment it is a proper prefix of one.
- `start..end` is a non-empty half-open byte range into the original input and
  exactly equals `syllable.as_bytes()`.
- Segment ranges are strictly increasing and non-overlapping. Apostrophe bytes
  may form one-byte gaps; no segment spans an apostrophe.
- Partial segments may appear at any position in a result, and a result may
  contain multiple partial segments. There is no "at most one, and it is
  last" restriction. (oracle-driven SPEC correction; see log below.)
- `remainder` is an exact owned copy of an input suffix. Its start offset is
  `input.len() - remainder.len()`.
- Every byte before the remainder start is accounted for by a segment or a
  consumed apostrophe separator.
- Complete consumption is represented by an empty remainder.

## Boundaries, partials, and junk

Apostrophe is a hard boundary: complete syllables never span it. Alternatives
on fully consumed sides are combined as a Cartesian product. A trailing
apostrophe is not a valid separator and starts the remainder.
Upstream's `FullPinyinParser2::parse` (`pinyin_parser2.cpp:237-250`) treats
every apostrophe at a reachable position as a zero-width step-propagation.
A **leading** run of consecutive apostrophes is consumed when the group
after it consumes at least one byte: `'ni` consumes all three bytes and
`''ni` consumes all four. A **non-leading** run of consecutive apostrophes
after a fully consumed left group is a single separator under the same
rule: `ni''hao` consumes the whole input. An apostrophe run with no
productive right side (e.g. `'`, `ni''`, `ni''!`) starts the remainder at
the first apostrophe of the run (position 0 for a leading run). An empty
result with unchanged remainder is not a right-side continuation.

Under the parity profile (`PINYIN_INCOMPLETE` set), an initial-only key is a
first-class partial usable at any cursor, any number of times, freely
interleaved with complete keys. This matches the pinned oracle. Complete
segmentations of a group remain first-class paths; partial-bearing paths are
not confined to a terminal fallback after every complete path has been
rejected.

Path-set enumeration for mid-path and repeated partials — which paths are
retained, in what order, and how they interact with complete-path precedence
— is normative in `docs/findings/parser-path-set.md` and is **not** restated
here. That SPEC, and the parser that implements it, are updated on a separate
branch. Until that branch lands, the portable parser still implements the
pre-correction path set (at most one trailing partial).

At the first unsupported or otherwise unconsumable byte, parsing stops. The
already segmented prefix is retained and that byte plus every following byte
is returned unchanged as `remainder`.

## Totality and ownership

The implementation is iterative or uses input-bounded control flow; it must
not recurse according to untrusted input length. No indexing operation may
assume UTF-8 or a character boundary. Every byte sequence returns `Ok` or the
bounded alternative error without a parser panic. Outputs own all variable
data and do not borrow the input. Selection among returned paths belongs to
the decoder and is outside Foundation.

## Architect correction log

### 2026-08-09 — oracle-driven SPEC correction: partial placement

**Kind:** oracle-driven SPEC correction. A frozen field invariant was changed
by differential evidence against the pin, not by design preference. Future
readers should treat this as a legitimate freeze edit under the constitution's
"edit frozen SPECs only with an explicit ask" rule: the ask is the maintainer
decision recorded here.

**Previous invariant.** A result contains at most one partial segment, and it
is the last segment.

**Corrected invariant.** Partial segments may appear at any position, multiple
times, interleaved with complete segments.

**Evidence.** W2-T3 live run over the 10,465-input parity corpus against the
pin-built oracle at flags `0x18a`. Of 491 divergences, **483 (98.4%)** share
this single root cause. Worked example, input `yingchon`:

```text
theirs  ying@0:4:complete, ch@4:6:partial, o@6:7:complete, n@7:8:partial
ours    ying@0:4:complete, chon@4:8:partial
        yi@0:2:complete, ng@2:4:complete, chon@4:8:partial
```

The pin's selected path has two non-final partials. Full accounting is in
`docs/findings/parser-spec-contradiction-incomplete-keys.md`.

**Deferred.** Admitting mid-path and repeated partials multiplies the path
set. `MAX_PARSE_RESULTS` (4,096) and its interaction with the corpus must be
re-evaluated once the parser admits these paths. That re-evaluation, the
`parser-path-set.md` amendment, and the parser implementation change all
belong on a **separate branch**. This commit changes only the frozen field
invariant and the prose that states the observed upstream policy.

### 2026-08-21 — oracle-driven SPEC correction: doubled apostrophe

**Kind:** oracle-driven SPEC correction. Half of maintainer decision 3 in
`parser-spec-contradiction-incomplete-keys.md` — the doubled-apostrophe
half — is resolved by aligning with the pin. The leading half stays open.

**Previous invariant.** A leading, repeated, or trailing apostrophe is not
a valid separator and starts the remainder.

**Corrected invariant.** A leading or trailing apostrophe is not a valid
separator and starts the remainder. A run of consecutive apostrophes after
a fully consumed left group is a single separator when the group after it
consumes at least one byte.

**Evidence.** `fixtures/w4/oracle-candidates.txt` `ni''hao` (stratum
`09-edge.txt` line 33) → oracle top-1 `你好`; every candidate in the
oracle's top-10 requires the doubled apostrophe to act as a single
separator. Before this correction, `ni''hao` was the sole `absent` input in
the W2 residual (`docs/findings/corpus-tail.md`, W12 Class B).

**Freeze move.** `real_tables_session_reports_parity` re-freezes from
10,177 / 10,189 / 94,871 of 98,930 / absent 1 to
**10,178 / 10,190 / 94,872 of 98,930 / absent 0**; tie-swaps stay at
1,036. Recorded in `docs/findings/pin-refreeze-2026-08.md`
(the doubled-apostrophe amendment).

### 2026-08-21 — oracle-driven SPEC correction: leading apostrophe

**Kind:** oracle-driven SPEC correction. The remaining half of maintainer
decision 3 in `parser-spec-contradiction-incomplete-keys.md` — the
leading-apostrophe half — is resolved by aligning with the pin.

**Previous invariant.** A leading or trailing apostrophe is not a valid
separator and starts the remainder.

**Corrected invariant.** A trailing apostrophe starts the remainder. A
leading run of consecutive apostrophes is consumed when the group after it
consumes at least one byte. A leading run with no productive right side
starts the remainder at position 0.

**Evidence.** `parser-spec-contradiction-incomplete-keys.md` remaining
divergences: `'ni` oracle consumes 3 vs our 0 (`consumed-length`).
Upstream's `FullPinyinParser2::parse` (`pinyin_parser2.cpp:237-250`)
treats every apostrophe at a reachable position as a zero-width
step-propagation, including at position 0.
