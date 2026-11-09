# Candidate-construction SPEC (Discrepancy 2)

Status: **frozen (§8).**

This SPEC states how `Session` builds the phrase candidate list at a
composition offset: the expanding-window scan, the three-key order, the
frequency key and the dedup. It also names the two tie classes that this
construction closes. Neighbours: `docs/findings/matrix-split-tables.md` (the
scan's key set), `docs/findings/sentence-surface.md` (the n-best sentence rows),
`docs/findings/session-api.md` and `docs/findings/scoring-spec.md`.

## 8. The frozen construction contract

The contract reproduces the pin's candidate collection, read from the pinned
libpinyin sources (`tools/oracle/oracle-pin.txt`). The pin pools phrases from
every segmentation into one list: for `xian` it lists `西安` (`xi` + `an`)
before `县` (`xian`), and for `fangan` it lists `方案` (`fang` + `an`) before
`反感` (`fan` + `gan`).

### 8.1 The contract

1. **Candidate construction is the expanding-window scan.** `Session::refresh`
   fixes the start at the composition offset, walks the window end outward over
   every byte position, enumerates every key-path through the scan matrix, and
   searches the phrase table on each complete path. The scan matrix is the
   selected parse (fewest keys, then the smaller total correction distance,
   then first found — `pinyin_parser2.cpp:294-309` at 074a2219) plus the
   frozen resplit and divided additions
   (`docs/findings/matrix-split-tables.md`). The scan starts from the input as
   typed: `Session::replace_raw` keeps every character, and the parse stops at
   the first byte no key matches, as the pin's does (`parser-spec.md`); `b#ing`
   ranks `不` first, not `并`. A path with an initial-only key goes to the
   dictionary as one projected-index lookup: the runtime dictionaries advertise
   `Dictionary::handles_partial_keys` and answer it in one probe. A dictionary
   that does not handle partial keys gets the key sequences from the table's
   wildcard expansion (`expand_keys`; [scoring](scoring-spec.md#incomplete-keys)).
   Widening is prefix-driven (`SEARCH_CONTINUED` = a stored phrase can extend
   the sequence); consecutive apostrophe bytes are skipped; the key sequence
   is capped at 16 keys. Cross-segmentation pooling falls out of the scan —
   there is no separate pooling step and no span policy.
2. **The ranking is the three-key order.** The sort word selects the keys:
   `SORT_BY_PHRASE_LENGTH` (`0x4`), `SORT_BY_PINYIN_LENGTH` (`0x8`) and
   `SORT_BY_FREQUENCY` (`0x10`) each enable one key, and a cleared bit drops
   that key (`pinyin.cpp:1678-1709`). With all three set, as in the W4
   capture's `0x1e`, the order is: phrase length (Unicode scalar
   count) descending, pinyin span (window end − start) descending, the
   pin's amplified frequency descending — `trunc(((1−λ)·(unigram+1)/
   total)·2²⁴)` in `f32` over the `interpolation2.text` counts and the
   index total 51,051,831 (§8.2; `pinyin.cpp:1855-1866`), the exact scale
   the oracle's comparator ties on. A token the model has no unigram for
   ranks at frequency zero. Fully-tied candidates keep the scan's per-window
   token-ascending order (the collection loop) — the array order
   `_append_items` lays down and the pin's stable sort keeps (Class A, §8.4).
   The list is then deduplicated by phrase string as the pin does: of the
   rows that share a text, the row with the larger `m_freq` survives, and a
   tie keeps the earlier row
   ([addon-choose-promotion](addon-choose-promotion.md#5-the-next-candidate-list-the-string-dedup-2026-10-19-utc)).
   The list is returned in full — upstream has no candidate cap. An empty
   scan gives the raw-input fallback row.
3. **Sentence and longer candidates come from the C facade, not the scan.**
   `pinyin_guess_candidates` prepends the n-best sentence rows
   (`NBEST_MATCH_CANDIDATE`) unless `SORT_WITHOUT_SENTENCE_CANDIDATE` is set,
   and a `LONGER_CANDIDATE` row unless `SORT_WITHOUT_LONGER_CANDIDATE` is set
   (`pinyin.cpp:2290-2300`; `crates/oxpinyin-capi/src/sentence.rs`;
   [sentence-surface.md](sentence-surface.md)). The W4 capture
   (`fixtures/w4/oracle-candidate-structure.txt`, sort word `0x1e`, options
   `0x18a`) holds only `NORMAL_CANDIDATE` rows; that observation is scoped to
   the capture.

### 8.2 The gate and the frequency key

The gate is the `candidate_surface` test,
`crates/oxpinyin-runtime/tests/candidate_surface.rs`. It reads
`fixtures/w4/oracle-candidate-structure.txt` (the pin's first ten candidates
per input, with candidate type and n-best index) and runs a fresh session over
every distinct input of `tests/parity/corpus/inputs`, on the pinned
libpinyin-data package. It requires every one of the 10,037 captured inputs to
match the capture row for row at depth 10: text, candidate type and n-best
index. It also checks 10,312 distinct corpus inputs and 97,442 captured rows.
Run `just fetch-contract-data` first; a missing package fails the test. The
capture is described in `docs/testing/README.md`.

**The frequency key is the pin's amplified scale, not the raw count.**
RankKey-3 is `trunc(((1−λ)·(unigram+1)/total)·2²⁴)` computed in `f32`
with C evaluation order (λ = 0.312699), where `unigram` is the
interpolation2 count, `+1` the model20 item-unigram identity, and
`total` = interpolation2 sum + 138,096 phrase-index items = 51,051,831
(`pinyin.cpp:1855-1866`; probe-verified over the whole index). The
truncation collapses near-ties into equal keys — 1,030,385 adjacent
fully-tied pairs across 10,027 inputs, counted on the amplified scale
the comparator actually compares — which the stable sort resolves by
the scan's per-window token-ascending flush (system facade before
addon), the array order `_append_items` (`pinyin.cpp:1769-1791`) lays
down and GLib's stable merge sort keeps.

### 8.3 Invariants

These hold in the current code:

1. **The key set is the pin's matrix.** The scan enumerates exactly the keys
   the pinned matrix admits: the selected parse plus the frozen split tables.
   A doubled (`ni''hao`) or leading (`'ni`) apostrophe run is no exception; the
   parser and graph consume it as the pin does (`parser-spec.md`, correction
   log 2026-08-21). Apostrophe-only input (`'`, `''`, `'''`) places no key: the
   capture holds no candidates for it, and a key query on it aborts as the
   pin's does
   ([F-E-14](robustness-evidence.md#f-e-14--pinned-oracle-aborts-on-apostrophe-only-input)).
2. **One `f32` frequency key.** The amplified key of §8.2 is the one `f32`
   computation in the construction. `amplified_frequency` evaluates the C
   `float` chain in C order, and the unit test
   `amplified_frequency_pins_the_class_a_probe_values` holds its values. The
   other ranking comparisons are integer. Real-table sentence scoring is
   outside this SPEC ([compatibility policy](compatibility-policy.md#s1-and-s2-one-ordered-adjusted-score)).
3. **Determinism.** The list is a pure function of (input, user state,
   config). The sort is stable, so fully-tied candidates keep the scan's
   collection order. No clock, locale or environment read enters.
4. **No candidate cap.** The list is returned in full. The scan bounds are
   `MAX_PHRASE_LENGTH` (16 keys per phrase) and `SCAN_EXPANSION_LIMIT` (4,096
   expanded sequences per key-path), both in
   `crates/oxpinyin-engine/src/session/mod.rs`.

### 8.4 Tie classes

**Class A — comparator ties.** Two candidates of equal phrase length and
pinyin span can truncate to the same amplified frequency. Equal keys then
keep the pin's array order. Per window, `_append_items` lays the rows down
library by library (system facade before addon), and each library's rows in
the order the index chunk stores them: stored key first, then token
(`chewing_large_table2.h:91-133`, `:208-230`). The first row of each token is
kept. `flush_window_batch` in `crates/oxpinyin-engine/src/session/mod.rs`
reproduces this order. Recorded cases include `liangbi` (两笔 before 量比) and
`goug` (沟谷 before 狗狗, which also tie on raw counts).

**Class B — doubled apostrophe.** A run of consecutive apostrophes after a
fully consumed group is one separator when the group after it consumes at
least one byte (`pinyin_parser2.cpp:237-250`). So `ni''hao` reaches `你好` at
rank 1, as the capture records.

### 8.5 What this contract does not cover

- **`NORMAL` ≠ "exactly one dictionary phrase".** The token
  decomposition is unreachable from the public API; the scan reproduces the
  emitted text without claiming the token split.
- **Fuzzy/correction spellings** (the pin's fuzzy step under ambiguity
  options not set by `0x18a`) are pinned behaviour — Stage 2 scope, the
  option bits of W10 — outside this contract.
- **Sentence rows** are the facade's prepend (item 3 of §8.1); their
  contract is [sentence-surface.md](sentence-surface.md).

STOP and report rather than inventing evidence.
