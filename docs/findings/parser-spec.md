# Full-pinyin parser SPEC

Status: **reference**

This SPEC states the parser behaviour that the pin defines and the C APIs expose: the syllable inventory, the input domain, the result fields and their invariants, the boundary and apostrophe rules, the result bound, and the totality and ownership rules.
The Rust type definitions describe oxpinyin's internal surface.

This SPEC refines Foundation R3.
The entry point is `FullPinyinParser::parse_with_options` (`crates/oxpinyin-core/src/parser.rs`).
The path set that the parser returns, and its order, is in [parser-path-set.md](parser-path-set.md).

## Scope and inventory

The parser accepts untuned full pinyin only.
The complete-syllable inventory is the set of active keys in `PINYIN_DICT` in the libpinyin source file `scripts2/fullpinyin.py`.
The identity below is libpinyin `2.11.91`.
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
- The pin's selected split has no such restriction ([partial placement](#partial-placement)).
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
`FullPinyinParser2::parse` returns one key sequence, not a path set (`pinyin_parser2.cpp:217-344`).
When the option word has `PINYIN_INCOMPLETE`, each of the 23 initial-only keys is a key at any position (`pinyin_parser2.cpp:43-45`, `:266-284`).
A split can contain more than one such key, interleaved with complete keys.
When the option word does not have `PINYIN_INCOMPLETE`, the pin admits no initial-only key.
The pin selects the longest parsed length, then the fewest keys, then the smallest distance (`pinyin_parser2.cpp:296-310`).
`SegmentGraph::fewest_keys` implements this split for the C APIs ([segment-graph.md](segment-graph.md)).

*The portable path set.*
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

## Correction log

Each entry states a parser rule where the pin's behaviour replaced an earlier rule.

### Partial placement

Partial segments in the pin's selected split may appear at any position, more than once, interleaved with complete segments (`pinyin_parser2.cpp:266-284`).
For `yingchon` the pin selects `ying@0:4:complete, ch@4:6:partial, o@6:7:complete, n@7:8:partial`, a path with two non-final partials.
The portable path set keeps at most one partial, as the last segment ([field invariants](#field-invariants)).

### Doubled apostrophe

The parser follows the pin on a doubled apostrophe.
A run of consecutive apostrophes after a fully consumed left group is a single separator when the group after it consumes at least one byte (`pinyin_parser2.cpp:237-250`).
For `ni''hao` the pin's top-1 candidate is `你好`.

### Leading apostrophe

The parser follows the pin on a leading apostrophe.
A leading run of consecutive apostrophes is consumed when the group after it consumes at least one byte.
A leading run with no productive right side starts the remainder at position 0.
`FullPinyinParser2::parse` (`pinyin_parser2.cpp:237-250`) treats every apostrophe at a reachable position as a zero-width step-propagation, including at position 0, so `'ni` consumes 3 bytes.

## Open

- If the parser admits mid-path and repeated partials, `MAX_PARSE_RESULTS` (4,096) and its interaction with the corpus must be re-evaluated, with the matching change to [parser-path-set.md](parser-path-set.md) ([partial placement](#partial-placement)).
