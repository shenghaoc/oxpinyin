# W9-T1 segmenter port — `ngseg` → Rust

Date: 2026-08-15 · Status: **SHOWN-verified against libpinyin 2.11.91 `ngseg`
and `PhraseLookup::get_best_match`** · Crate: `pinyin-segment` (never ships).

This finding records the mapping from libpinyin's training segmenter to the
Rust port, the scoring-model choice that breaks the training circularity, and
the differential-parity result. Algorithmic claims are tagged **SHOWN**
(cited source) or **INFERRED**.

## Decision: reproduce `ngseg`, not `spseg`

`docs/findings/training-algorithm.md` (W9-T0) characterises both tools.
They share a driver and differ only in how a *segmentable* run is scored:

| Tool | Scoring | Source |
|---|---|---|
| `ngseg` | bigram-scored Viterbi (`PhraseLookup::get_best_match`) | `utils/segment/ngseg.cpp:67-91, 168-171` |
| `spseg` | fewest-words shortest path (`m_nword`) | `utils/segment/spseg.cpp:48-138` |

T0 did **not** establish that the shipped pipeline used `spseg`. T0's
differential plan named `spseg` only as the simpler tokenisation pin for
later counting, not as the quality path. This task reproduces `ngseg`
(the full-quality path). `spseg` remains out of scope.

`spseg` also skips nothing extra at the driver level: its main loop
(`spseg.cpp:263-335`) is the same three-state machine. The "skips certain
sentences" reading is the *scoring* difference (a 1-char miss becomes
`null_token` inside the fewest-words DP, `spseg.cpp:116-120`), not a
second input filter.

## Crate placement

New crate `crates/pinyin-segment`, not a module in `pinyin-engine` or
`pinyin-data`.

- The segmenter is a training preprocessor. Putting it on the supported
  session surface would grow the engine ABI for a never-ship tool.
- There is no training crate yet. W9-T2 (the counter) will consume this
  crate's stdout format; a dedicated crate keeps that seam small.
- `unsafe`: deny. Portable. `publish = false`.

## `ngseg` → Rust mapping

### Driver (`utils/segment/ngseg.cpp`)

| Behaviour | Source | Rust |
|---|---|---|
| `getline`; strip one trailing `\n` only (not `\r`) | `:183-186` | `driver::getline_lines` |
| Invalid UTF-8 / `g_utf8_to_ucs4` length mismatch → `0 \n` | `:189-196` | `segment_bytes` invalid-UTF-8 branch |
| Empty line → `0 \n` | `:199-202` | `segment_line` on `""` |
| `phrase_table.search(1, char)` → segmentable / unknown | `:204-210` | `PhraseLexicon::is_char_segmentable` |
| Three-state run splitter | `:212-245` | `segment_line` |
| `deal_with_segmentable` → `get_best_match` + `convert_to_utf8` | `:67-91` | `trellis::get_best_match` |
| Failure prints `"Un-segmentable sentence"` to stderr; caller **ignores** the return | `:79-86`, `:226`, `:240` | omit the run on `None` (stdout-identical) |
| `deal_with_unknown` → `0 {raw}\n` | `:93-100` | `Emitted::Unknown` |
| `--generate-extra-enter` → extra `0 \n` per line | `:248-249` | `segment_bytes(..., extra_enter)` |
| File-tail `0 \n` | `:256` | always appended |

`convert_to_utf8(result, "\n", true)` (`phrase_lookup.h:131-136`,
`lookup.cpp:27-69`) prints `{token} {phrase}` joined by `\n`, skipping
`null_token` slots. `fprintf("%s\n")` adds the last newline. Rust emits
one `{token} {phrase}\n` per surviving phrase.

### Trellis (`src/lookup/phrase_lookup.cpp`)

`get_best_match` (`:119-157`) is a position-indexed Viterbi:

1. `nstep = length + 1` columns. Column 0 holds `sentence_start` with
   `log(1) = 0` (`populate_prefixes`, `:36-53`).
2. For each start `i` and end `m > i`, `phrase_table.search(m-i, …)`.
   `SEARCH_OK` expands; `!SEARCH_CONTINUED` breaks (`:141-151`).
3. `search_bigram2` (`:255-305`) expands from **every** node at `i` that
   has a system-bigram row. `get_freq` miss → no bigram edge.
4. `search_unigram2` (`:217-253`) expands from the **max-score** node
   only. Ties keep the first-inserted node (`>` not `>=`).
5. Unigram step (`:307-325`):
   `poss += log(P_uni × (1-λ))`, skip if `P_uni < DBL_EPSILON`.
6. Bigram step (`:327-346`):
   `poss += log(λ P_bi + (1-λ) P_uni)`, skip if
   `P_bi < FLT_EPSILON && P_uni < DBL_EPSILON`.
   `P_bi = freq / (gfloat) total` is an `f32` division.
7. `save_next_step` (`:348-379`) merges by arriving token. Equal scores
   keep the original (`orig.m_poss < next.m_poss`).
8. `final_step` (`:382-434`) picks the max at the last column (first
   wins ties), backtraces `handles[0]` through `steps_index[last_step]`,
   writes the token at each phrase start, leaves covered slots as
   `null_token`.

`user_bigram` is constructed empty (`ngseg.cpp:164`). Only
`SYSTEM_BIGRAM` participates.

λ is `system_table_info.get_lambda()` (`ngseg.cpp:166-171`), scanned from
`table.conf` as `lambda parameter:%f` (`table_info.cpp:220`). The pin's
file is `0.312699` (`data/table.conf.in:3`; installed
`lib/libpinyin/data/table.conf`). Rust's [`PINNED_LAMBDA`] is that
decimal as `f32`.

### Phrase table

`ngseg` searches `FacadePhraseTable3` loaded from `phrase_index.bin`
(`ngseg.cpp:149-150`) and the four `SYSTEM_FILE` libraries
(`load_phrase_index` in `utils_helper.h:73-80`: `gb_char`, `gbk_char`,
`opengram`, `merged`). Addons are not loaded.

The Rust export (`docs/findings/data-layer-export.md`) is those same four
libraries. `phrase_index.redb` is `token → UTF-8`. The port inverts it:
exact span → tokens, plus every proper prefix for `SEARCH_CONTINUED`.
Token order is sorted by `phrase_token_t`, which is library-then-index
and matches `search_unigram2`'s `n = 0 .. PHRASE_INDEX_LIBRARY_COUNT`
walk.

## What was reused vs newly written

**Reused (existing loaders, no second model format):**

- `pinyin_data::LookupTable` / `phrase_index.redb` — token → text.
- `pinyin_data::BigramLanguageModel::load_successors` — `bigram.redb`,
  the verbatim `SYSTEM_BIGRAM` export. One new public method; the parse
  was already private.
- `pinyin_data::parse_interpolation2` — real unigram counts from the
  fetched `interpolation2.text` cache (`locate_model_dir` / `PINYIN_MODEL_DIR`).

**Newly written (character-domain handling + `ngseg` formatting):**

- `PhraseLexicon` — inverted phrase table and the segmentable test.
- `driver` — the three-state run splitter and the `null_token` text
  format `gen_ngram` / W9-T2 will parse
  (`TAGLIB_PARSE_SEGMENTED_LINE`, `utils_helper.h:51-74`).
- `trellis` — the `get_best_match` column machine above.

### Why `session.rs::collect_sentence` is not called

The task's thesis is that segmentation and decoding share a trellis.
`collect_sentence` (`crates/pinyin-engine/src/session.rs`, post-#46)
*is* a position-indexed expand-merge-backtrace — but it cannot be
invoked as `get_best_match`:

1. It walks **pinyin keys** and looks up phrases through
   `Scorer::rank_phrases` / `SystemDictionary` (pinyin-keyed). The
   segmenter walks **characters**.
2. It keeps **one** cheapest history per position. `get_best_match`
   keeps one node per `(position, arriving token)` because the next
   bigram depends on which token arrived (`steps_index`,
   `phrase_lookup.cpp:348-379`).
3. It uses the decoder's integer surprisal, provisional λ = 1/2,
   `UNIGRAM_TIEBREAK_SCALE`, and an `UNKNOWN_COST` floor. `get_best_match`
   uses `log(λ P_bi + (1-λ) P_uni)` with `table.conf` λ. Feeding
   `LanguageModel::score` into the segmenter would change the path.
4. After #46 it is only the **pre-frequency fallback**. The live
   candidate path is the window scan, which is not a Viterbi at all.

Reading `pinyin_lookup2.cpp` (decode internals) was not required and was
not done. The segmenter-specific variant is therefore the authorised
`phrase_lookup.cpp` machine, scored from the existing loaders. This is
documented rather than papered over: a second *shape* of the same
trellis, not a second scoring model.

The `f64::ln` here is the training tool matching libm `log` on the pin
host. It does not enter the engine cost path (constitution item 6 /
`scoring-spec.md` stay intact).

## Scoring-model choice

The segmenter scores with the **pinned, fetched system model**:

- Bigrams: `bigram.redb` via `BigramLanguageModel` (same bytes `ngseg`
  reads from `SYSTEM_BIGRAM`).
- Unigrams: `interpolation2.text` via `parse_interpolation2`, then the
  `gen_unigram` freq-1 floor (`gen_unigram.cpp:45-68`) over every token
  in the loaded phrase index. The pin's data recipe runs that floor
  after `import_interpolation` (`data/Makefile.am:58-62`), so
  `get_phrase_item().get_unigram_frequency()` is `interp + 1` and
  `get_phrase_index_total_freq()` is the sum of those values over the
  four `SYSTEM_FILE` libraries.
- λ: `0.312699` from the pin `table.conf`.

This breaks the training circularity (the segmenter needs a model; the
model is what W9 trains) and makes differential parity possible: both
sides score the same counts.

`LanguageModel::score` is **not** used. Its λ, scale, and unseen-transition
floor are decoder constants, not `get_best_match`.

## Differential parity

Fixture: `fixtures/w9/segmenter-han.txt` — **111 lines** of synthetic,
hand-written Han (plus empty / ASCII / mixed / punctuation). Not
Wikipedia; no corpus-licence question. Golden:
`fixtures/w9/segmenter-ngseg.txt` (pin-built `ngseg` stdout).

Command:

```text
ngseg  = <oracle-pin-build>/utils/segment/ngseg
cwd    = <oracle-prefix>/lib/libpinyin/data
input  = fixtures/w9/segmenter-han.txt
```

**Result: 111 input lines → 233 output records (plus the file-tail
`null_token`); token sequences bit-identical to `ngseg`.** Asserted by
`crates/pinyin-segment/tests/differential.rs` against the golden and,
when `PINYIN_NGSEG` + the pin data dir are present, against a live
`ngseg` run. No class of input on this fixture required a documented
divergence.

## What W9-T2 consumes

The counter (`gen_ngram`) reads this crate's stdout with
`TAGLIB_PARSE_SEGMENTED_LINE` (`utils_helper.h:51-74`):

- one record per line: `{token} {phrase}` or `0 ` or `0 {raw}`;
- empty / `null_token` lines are sentence boundaries;
- `null_token` after a token becomes `sentence_start` for the next
  bigram (unless `--skip-pi-gram-training`).

W9-T2 should treat `pinyin-segment`'s `Emitted::to_ngseg_line` / the CLI
stdout as the tokenised input, not invent a second format.
