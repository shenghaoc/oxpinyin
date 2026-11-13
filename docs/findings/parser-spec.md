# Full-pinyin parser SPEC

Status: **reference**

This SPEC states the full-pinyin behaviour at pin `074a2219`.
The single-key parser is `FullPinyinParser::parse_one_key` in `crates/oxpinyin-core/src/parser.rs`.
The selected split is implemented by `SegmentGraph::fewest_keys` ([segment-graph.md](segment-graph.md)).

## Scope and inventory

The base inventory contains untuned full-pinyin spellings.
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

## Single-key parsing

`FullPinyinParser2::parse_one_key` probes the whole spelling (`src/storage/pinyin_parser2.cpp:164-214`).
It refuses a spelling that does not match one key; it does not return a prefix match for a multi-syllable string.
Under `USE_TONE`, a trailing `1` through `5` is stripped for the spelling probe and becomes the key's tone.
`FORCE_TONE` is tested inside `USE_TONE` and rejects a toneless spelling (`:176-190`).
Initial-only keys require `PINYIN_INCOMPLETE` (`:38-58`).
The C adapters reproduce the pin's apostrophe assertion at the boundary.

## Selected split

`FullPinyinParser2::parse` returns one key sequence (`src/storage/pinyin_parser2.cpp:217-344`).
It selects the longest parsed length, then the fewest keys, then the smallest distance (`:296-310`).
`final_step` backtracks the selected predecessors into aligned key and key-rest arrays (`:346-380`).
There is no exhaustive Cartesian result set or alternative-count limit.

### Partial placement

When the option word has `PINYIN_INCOMPLETE`, the 23 initial-only keys are admitted at any position (`:43-45`, `:266-284`).
A split can contain more than one such key, interleaved with complete keys.
Without `PINYIN_INCOMPLETE`, the pin admits no initial-only key.

### Apostrophes

Each apostrophe propagates a parse step without adding a key (`:237-250`).
This includes leading, repeated and trailing apostrophes.
The C APIs' parsed length includes a trailing apostrophe run; `full_parsed_len` in `crates/oxpinyin-engine/src/session/offsets.rs` reproduces that rule.

## Verification

`crates/oxpinyin-core/src/parser.rs` tests the single-key law.
`crates/oxpinyin-core/tests/graph_paths.rs` checks that the pin's selected paths and consumed lengths are admissible in the graph.
The F-A fixture records selected splits, not every possible segmentation ([capture-fixtures.md](../testing/capture-fixtures.md)).
