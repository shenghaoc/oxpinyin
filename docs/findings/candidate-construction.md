# Candidate-construction SPEC

Status: **reference**

§8 states the pin's phrase candidate list as the C APIs expose it: which candidates appear, their order, the dedup and the frequency key.
The oxpinyin function, test, constant and file names describe how the code produces the list.

This SPEC states how `Session` builds the phrase candidate list at a composition offset.
It covers the expanding-window scan, the three-key order, the frequency key and the dedup.
It also names the two tie classes that this construction closes.
Related documents: [matrix-split-tables.md](matrix-split-tables.md) (the scan's key set), [sentence-surface.md](sentence-surface.md) (the n-best sentence rows), [session-api.md](session-api.md) and [scoring-spec.md](scoring-spec.md).

## 8. The construction contract

The contract reproduces the pin's candidate collection, read from the pinned libpinyin sources (`tools/oracle/oracle-pin.txt`).
The pin pools phrases from every segmentation into one list.
For `xian` it lists `西安` (`xi` + `an`) before `县` (`xian`).
For `fangan` it lists `方案` (`fang` + `an`) before `反感` (`fan` + `gan`).

### 8.1 The contract

1. **Candidate construction is the expanding-window scan.**
   - `Session::refresh` fixes the start at the composition offset and walks the window end outward over every byte position.
     For each window, it enumerates every key-path through the scan matrix and searches the phrase table on each complete path.
   - The scan matrix is the selected parse plus the resplit and divided additions ([matrix-split-tables.md](matrix-split-tables.md)).
     The selected parse has the fewest keys, then the smaller total correction distance, then is the first found (`pinyin_parser2.cpp:294-309` at 074a2219).
   - The scan starts from the input as typed.
     `Session::replace_raw` keeps every character, and the parse stops at the first byte that no key matches, as the pin's parse does ([parser-spec.md](parser-spec.md)).
     So `b#ing` ranks `不` first, not `并`.
   - A path with an initial-only key goes to the dictionary as one projected-index lookup.
     The runtime dictionaries advertise `Dictionary::handles_partial_keys` and answer it in one probe.
     A dictionary that does not handle partial keys gets the key sequences from the table's wildcard expansion (`expand_keys`; [scoring](scoring-spec.md#incomplete-keys)).
   - Widening is prefix-driven: `SEARCH_CONTINUED` means that a stored phrase can extend the sequence.
     The scan skips consecutive apostrophe bytes.
     The key sequence is capped at 16 keys.
   - Cross-segmentation pooling is a result of the scan.
     There is no separate pooling step and no span policy.
2. **The ranking is the three-key order.**
   - The sort word selects the keys.
     `SORT_BY_PHRASE_LENGTH` (`0x4`), `SORT_BY_PINYIN_LENGTH` (`0x8`) and `SORT_BY_FREQUENCY` (`0x10`) each enable one key, and a cleared bit drops that key (`pinyin.cpp:1678-1709`).
   - With all three bits set, as in the W4 capture's `0x1e`, the order is:
     1. phrase length (Unicode scalar count), descending;
     2. pinyin span (window end − start), descending;
     3. the pin's amplified frequency, descending.
   - The amplified frequency is `trunc(((1−λ)·(unigram+1)/total)·2²⁴)` in `f32`, over the `interpolation2.text` counts and the index total 51,051,831 (§8.2; `pinyin.cpp:1855-1866`).
     It is the exact scale on which the oracle's comparator ties.
     A token that has no unigram in the model ranks at frequency zero.
   - Fully-tied candidates keep the scan's per-window token-ascending order (the collection loop).
     This is the array order that `_append_items` lays down and that the pin's stable sort keeps (Class A, §8.4).
   - The list is then deduplicated by phrase string, as the pin does.
     Of the rows that share a text, the row with the larger `m_freq` stays, and a tie keeps the earlier row ([addon-choose-promotion](addon-choose-promotion.md#5-the-next-candidate-list-the-string-dedup-2026-10-19-utc)).
   - The list is returned in full: upstream has no candidate cap.
     An empty scan gives the raw-input fallback row.
3. **Sentence and longer candidates come from the C facade, not the scan.**
   - `pinyin_guess_candidates` prepends the n-best sentence rows (`NBEST_MATCH_CANDIDATE`) unless `SORT_WITHOUT_SENTENCE_CANDIDATE` is set.
     It prepends a `LONGER_CANDIDATE` row unless `SORT_WITHOUT_LONGER_CANDIDATE` is set (`pinyin.cpp:2290-2300`; `crates/oxpinyin-capi/src/sentence.rs`; [sentence-surface.md](sentence-surface.md)).
   - The W4 capture (`fixtures/w4/oracle-candidate-structure.txt`, sort word `0x1e`, options `0x18a`) holds only `NORMAL_CANDIDATE` rows.
     That observation applies to the capture only.

### 8.2 The gate and the frequency key

The gate is the `candidate_surface` test, `crates/oxpinyin-runtime/tests/candidate_surface.rs`.

- It reads `fixtures/w4/oracle-candidate-structure.txt`: the pin's first ten candidates per input, with candidate type and n-best index.
- It runs a fresh session over every input that the capture holds, on the pinned libpinyin-data package.
- It requires every one of the 750 captured inputs to match the capture row for row at depth 10: text, candidate type and n-best index.
- It also requires the header totals and the fixed counts: 750 inputs and 4,572 rows.

The test needs `just fetch-contract-data` to have run first; a missing package fails it.
`docs/testing/README.md` describes the capture.

**The frequency key is the pin's amplified scale, not the raw count.**
RankKey-3 is `trunc(((1−λ)·(unigram+1)/total)·2²⁴)`, computed in `f32` with C evaluation order (λ = 0.312699).
In it, `unigram` is the interpolation2 count and `+1` is the model20 item-unigram identity.
`total` is the interpolation2 sum + 138,096 phrase-index items = 51,051,831 (`pinyin.cpp:1855-1866`).

The truncation collapses near-ties into equal keys.
The stable sort resolves them by the scan's per-window token-ascending flush (system facade before addon).
That is the array order that `_append_items` (`pinyin.cpp:1769-1791`) lays down and that GLib's stable merge sort keeps.

### 8.3 Invariants

These invariants hold in the current code:

1. **The key set is the pin's matrix.**
   The scan enumerates exactly the keys that the pinned matrix admits: the selected parse plus the split tables.
   A doubled (`ni''hao`) or leading (`'ni`) apostrophe run is not an exception: the parser and graph consume it as the pin does ([parser-spec.md correction log](parser-spec.md#correction-log)).
   Apostrophe-only input (`'`, `''`, `'''`) places no key.
   The capture holds no candidates for it, and a key query on it aborts as the pin's does (`a_key_query_on_an_apostrophe_only_parse_aborts` in `crates/oxpinyin-capi/src/parse.rs`).
2. **One `f32` frequency key.**
   The amplified key of §8.2 is the one `f32` computation in the construction.
   `amplified_frequency` evaluates the C `float` chain in C order, and the unit test `amplified_frequency_pins_the_class_a_probe_values` holds its values.
   The other ranking comparisons are integer.
   Real-table sentence scoring is outside this SPEC ([compatibility policy](compatibility-policy.md#s1-and-s2-one-ordered-adjusted-score)).
3. **Determinism.**
   The list is a pure function of (input, user state, config).
   The sort is stable, so fully-tied candidates keep the scan's collection order.
   No clock, locale or environment read enters.
4. **No candidate cap.**
   The list is returned in full.
   The scan bounds are `MAX_PHRASE_LENGTH` (16 keys per phrase) and `SCAN_EXPANSION_LIMIT` (4,096 expanded sequences per key-path), both in `crates/oxpinyin-engine/src/session/mod.rs`.

### 8.4 Tie classes

**Class A — comparator ties.**
Two candidates of equal phrase length and pinyin span can truncate to the same amplified frequency.
Equal keys then keep the pin's array order.
Per window, `_append_items` lays the rows down library by library (system facade before addon).
In each library, the rows follow the order in which the index chunk stores them: stored key first, then token (`chewing_large_table2.h:91-133`, `:208-230`).
The first row of each token is kept.
`flush_window_batch` in `crates/oxpinyin-engine/src/session/mod.rs` reproduces this order.
Recorded cases include `liangbi` (两笔 before 量比) and `goug` (沟谷 before 狗狗, which also tie on raw counts).

**Class B — doubled apostrophe.**
A run of consecutive apostrophes after a fully consumed group is one separator when the group after it consumes at least one byte (`pinyin_parser2.cpp:237-250`).
So `ni''hao` parses as `ni@0:2`, `hao@4:7` (`fixtures/w4/oracle-paths.txt:1781`) and reaches `你好` at rank 1.

### 8.5 What this contract does not cover

- **`NORMAL` ≠ "exactly one dictionary phrase".**
  The token decomposition is not reachable from the public API.
  The scan reproduces the emitted text and makes no claim about the token split.
- **Fuzzy and correction spellings** are outside this contract.
  The pin's fuzzy step under ambiguity options that `0x18a` does not set is pin behaviour in Stage 2 scope ([option bits](option-bits.md)).
- **Sentence rows** are the facade's prepend (item 3 of §8.1).
  Their contract is [sentence-surface.md](sentence-surface.md).

Evidence for a case outside this contract is never invented: the case is reported, and work on it needs the maintainer's approval ([approval conditions](../../CONTRIBUTING.md#approval-conditions)).
