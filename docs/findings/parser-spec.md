# Full-pinyin parser SPEC

Date: 2026-08-09 · Status: frozen for Foundation Tasks 6–9

This finding is the human-frozen parser contract required before the first
parser implementation commit. It refines Foundation R3 and the signature in
`docs/findings/core-trait-seam.md`; a behavior change requires an Architect
correction before implementation resumes.

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
inventory as model-archive data covered by the Branch B restriction.

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

Task 7 adds these public dependency-free types to `pinyin-core`:

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
- A result contains at most one partial segment, and it is the last segment.
- `remainder` is an exact owned copy of an input suffix. Its start offset is
  `input.len() - remainder.len()`.
- Every byte before the remainder start is accounted for by a segment or a
  consumed apostrophe separator.
- Complete consumption is represented by an empty remainder.

## Boundaries, partials, and junk

Apostrophe is a hard boundary: complete syllables never span it. Alternatives
on fully consumed sides are combined as a Cartesian product. A leading,
repeated, or trailing apostrophe is not a valid separator and starts the
remainder. An apostrophe is consumed only when its left group is fully
segmented and its right group consumes at least one byte as a complete or
terminal partial segment. An empty result with unchanged remainder is not a
right-side continuation.

Complete segmentations take precedence. A terminal lowercase group emits
partial-tail paths only when no complete segmentation consumes that whole
group. Among partial paths, only those consuming the greatest byte prefix are
returned. If a non-final group before an apostrophe cannot be completed, its
maximal partial prefix is retained and parsing stops before the apostrophe;
the apostrophe and following bytes are the remainder.

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
