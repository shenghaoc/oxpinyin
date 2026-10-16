# libpinyin data structures and algorithms, and oxpinyin's counterparts

Date: 2026-10-16 UTC · Status: **register** (living) · Pin: libpinyin
`074a2219` (2.11.92), read from a clone at the pin · oxpinyin tree:
`origin/main` at `3b2bfdf`.

This is the record that `.kiro/steering/oxidation-method.md` asks for:
what the pin stores and computes, subsystem by subsystem, and what
oxpinyin does in its place. Pin code is cited `file:line` under `src/`;
oxpinyin code by file and item.

Each row carries one of three marks:

- **same** — the same structure and algorithm, re-expressed in Rust.
- **same behaviour** — the same observable behaviour from a different
  structure or algorithm; the row says what differs.
- **different** — a different design. Inside the parity path this needs a
  compatibility-policy class or is a defect; outside it, the row says
  why it does not reach output.

A row is a survey result, not a proof of equivalence. Rows whose
equivalence argument is not yet written down, and the open gaps, are
listed at the end and tracked by `.kiro/specs/oxidation-record/`.

## 0. Components and modularisation

### How libpinyin is put together

libpinyin is one shared library whose layers are directories and static
convenience archives, not separately built components:

| Layer | Pin | Role |
| --- | --- | --- |
| Base | `src/include/` — `memory_chunk.h`, `novel_types.h`, `stl_lite.h`, `unaligned_memory.h`, `pinyin_utils.h` | byte buffer, token and range types, STL shims |
| Storage | `src/storage/`, built as `libstorage.a` (`src/storage/Makefile.am:77`) | keys and key tables, the pinyin/zhuyin parsers, the phonetic key matrix, the phrase index, the phrase and pinyin tables, n-grams, punctuation, table info, and the facades over system and user tables |
| Lookup | `src/lookup/`, built as `liblookup.a` (`src/lookup/Makefile.am:31`) | sentence and phrase decoding (`PhoneticLookup`, `PhraseLookup`; `PinyinLookup2` is legacy) |
| API | `src/pinyin.cpp` (3,774 lines), `src/zhuyin.cpp` (2,250 lines) | the C API: context and instance state, candidate construction, training entry points, user phrases, import and export, persistence |
| Tools | `utils/storage`, `utils/training`, `utils/segment` | data generation, k-mixture-model training, segmentation; they link `libstorage.a` |

Three things about that shape matter for a re-expression:

- **The shipped libraries are monoliths.** `libpinyin.so` is
  `$(pinyin_SOURCES) pinyin.cpp` and `libzhuyin.so` is the same sources
  plus `zhuyin.cpp` (`src/Makefile.am:89`, `:110`): each carries its own
  copy of storage and lookup, and the two API files repeat much of the
  same state handling.
- **The backend is a preprocessor switch.** Each DBM-backed class has
  one header and one source per backend, chosen by `HAVE_BERKELEY_DB`,
  `HAVE_KYOTO_CABINET` or `HAVE_TKRZW` (`src/storage/chewing_large_table2.h:29-39`
  and its peers) and by `if BERKELEYDB` / `KYOTOCABINET` / `TKRZW` in
  `src/Makefile.am`. There is no backend interface; callers include the
  backend's concrete class.
- **The API file is also the application.** `pinyin.cpp` holds the
  context and instance structs (`:39`, `:73`) and also candidate
  construction, dedup, prediction, training orchestration and save — the
  logic that decides output lives beside the ABI glue.

### The two objects

Everything the API does hangs off two objects (`pinyin.cpp:39-95`):

- **`pinyin_context_t`** (`:39-71`) — one per process or engine, the
  long-lived state: the option word; the three parsers (full pinyin,
  double pinyin, zhuyin); the dictionaries (pinyin table, phrase table,
  phrase index, and the addon set of each); the system and user bigrams;
  the punctuation table; the system and user table info; the system and
  user directories; the `m_modified` flag that gates save; and the two
  decoders, `PhoneticLookup<2, 3>` and `PhraseLookup`.
- **`pinyin_instance_t`** (`:73-95`) — one per input session, the state
  of one composition: a back pointer to its context; the prefix text
  and its tokens; the parsed `PhoneticKeyMatrix` and parsed lengths; the
  constraints; the n-best results; the phrase result; the candidate
  list; and the sort option. Selection is not a separate structure:
  choosing a candidate writes a constraint (`:2582`), and the next guess
  reads it back.

The split is not quite clean. The decoders sit in the context but keep
per-call working state there — the trellis, cached keys and phrase item,
and pointers to the calling instance's matrix and constraints
(`lookup/phonetic_lookup.h:522-532`) — so every instance on one context
shares one decoder and its scratch, and two instances on one context
cannot decode from two threads at once.

oxpinyin keeps the same two objects at the ABI and moves the decoder
state to the instance side:

| Pin | oxpinyin |
| --- | --- |
| `pinyin_context_t`: options, parsers, dictionaries, bigrams, user state, save flag | `ContextCore` (`oxpinyin-facade/src/context.rs`): `Config`, the assembled `Runtime` (tables and model), the `UserStore` (whose shared dirty flag is `m_modified`), `LiveOptions`; no parser objects — parsing runs per call in `oxpinyin-core` |
| `pinyin_context_t`: the two decoders and their scratch | per instance: the `Session`'s lookup state and `Scratch` (`oxpinyin-engine/src/session/mod.rs`) |
| `pinyin_instance_t`: matrix, constraints, n-best, phrase result, candidates, sort option | `InstanceCore` (`oxpinyin-facade/src/instance.rs`) holding a `RuntimeSession`, whose `Session` owns the input and parsed matrix, the `SelectionRecord`, the `ConstraintStore`, the sentence (n-best) state, the `CandidateList` and the sort word; the phrase result sits on `InstanceCore` |
| back pointer from instance to context | cloned handles to the context's dictionary, model and user store; the raw context pointer is kept only for `pinyin_get_context` |

### The operations

libpinyin is a library, not a frontend: it never sees a key press. The
frontend (ibus-libpinyin, fcitx5) owns keys, preedit and commit, and
drives the instance through a small set of operations (`pinyin.h`):
parse (`pinyin_parse_more_full_pinyins`, `:465`, and its double-pinyin
and zhuyin peers), guess (`pinyin_guess_sentence`, `:376`;
`pinyin_guess_candidates`, `:554`), select (`pinyin_choose_candidate`,
`:568`, and the constraint calls), train (`pinyin_train`, `:618`),
segment (`pinyin_phrase_segment`, `:423`) and save (`pinyin_save`,
`:264`).

oxpinyin's `Session` (`oxpinyin-engine`) carries the same operations as
methods — `replace_raw` and its variants for parse, `guess_sentence`
and `candidates_at` for guess, `select` and `clear_constraint` for
select, `train` and `train_nbest`, `phrase_segment` — with save on
`ContextCore::save_user` (`oxpinyin-facade`). The C ABI drives only
these. The same type also carries a key-driven layer from the first
design — `KeyInput`, `process_key`, `type_pinyin`, `Preedit`, `commit`
— which no C-ABI path calls; the oracle tools and tests use it. For the
first release the operations are the Rust surface that matters; the
key-driven layer belongs in an optional adapter on top of them, not in
the same type (`.kiro/specs/oxidation-record/` task 9).

### How oxpinyin divides it

oxpinyin keeps the pin's layers but makes each a crate with explicit
dependencies (`.kiro/steering/structure.md` has the full map):

| Pin layer | oxpinyin crates |
| --- | --- |
| Base (`src/include/`) | `oxpinyin-chewing` (keys and content tables, a dependency-free leaf), `oxpinyin-data` (`chunk_format`, the `MemoryChunk` reader) |
| Storage: keys, parsers, matrix helpers, `SingleGram` | `oxpinyin-core` |
| Storage: system tables and their file formats | `oxpinyin-data` |
| Storage: the DBM backends | `oxpinyin-store`, one backend per build by cargo feature behind one ordered byte-KV seam |
| Storage: user tables and user files | `oxpinyin-user` |
| Lookup, and the output logic in `pinyin.cpp` | `oxpinyin-engine` |
| Wiring done in `pinyin_init` | `oxpinyin-runtime` |
| Context and instance state shared by both APIs | `oxpinyin-facade` |
| `pinyin.cpp` / `zhuyin.cpp` as ABI | `oxpinyin-capi`, `oxpinyin-zhuyin-capi`, sharing `oxpinyin-capi-marshal` |
| `utils/storage` | `oxpinyin-datagen` |
| `utils/training`, `utils/segment` | `oxpinyin-kmm`, `oxpinyin-training` |

The differences are deliberate: the backend is a seam rather than an
`#ifdef`, the two ABIs share one facade rather than two API files, and
the logic that decides output sits in `oxpinyin-engine`, away from the
ABI glue. The crate split dates from the first, clean-room design and is
kept (crate-count ruling, October 2026): its purpose is to undo the
pin's coupling while preserving its behaviour.

## 1. Storage

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Phrase item | `PhraseItem` (`storage/phrase_index.h:64`): `{u8 len, u8 npron, u32 unigram, ucs4[len], {ChewingKey[len], u32 freq} × npron}` in a `MemoryChunk` | `PhraseItemView` and `PronunciationView` in `oxpinyin-data/src/phrase_library.rs`, borrowed views over the same bytes | same |
| Sub phrase index | `SubPhraseIndex` (`storage/phrase_index.h:257`): header (total freq), offset array indexed by `token & PHRASE_MASK`, content area; file `mmap`ped `PROT_READ\|PROT_WRITE, MAP_PRIVATE` (`include/memory_chunk.h:505-506`); O(1) token → item | `PhraseLibrary` (`phrase_library.rs`), `item`, `slot_offset`; the same files mapped read-only; O(1) token → item | same for reads |
| Phrase index edits | in place on the private mapping: `PhraseItem::add_pronunciation` (`storage/phrase_index.cpp:56`), `SubPhraseIndex::add_unigram_frequency` (`:150`) | mapping never written; edits go to overlays — `StoreInner` system-item map (`oxpinyin-user/src/registry.rs`), user value tables in `oxpinyin-user/src/store.rs`, `ChunkItem` (`oxpinyin-data/src/chunk_write.rs`) — and the chunk is rebuilt by `build_chunk` at save | different (internal only; files and results match) |
| Facade phrase index | `FacadePhraseIndex` (`storage/phrase_index.h:438`): 16 `SubPhraseIndex*` and a running `m_total_freq` | `PhraseLibraries` (`oxpinyin-data/src/phrase_libraries.rs`): 16 lock-guarded slots; system total read from the chunk header, user total a separate row | same behaviour |
| Pinyin key → tokens | `ChewingLargeTable2` per backend (`storage/chewing_large_table2_bdb.h:34` and peers): DBM keyed by packed `ChewingKey[len]`, values `PinyinIndexItem2` arrays (`storage/pinyin_phrase3.h:181`), per-length `ChewingTableEntry` objects with a reused buffer (`storage/chewing_large_table2.h:74`); results compressed to token ranges by `convert` (`:90`) | `ChewingTable` (`oxpinyin-data/src/chewing_table.rs`) over the same file and keys; each lookup decodes rows into a fresh `Vec`; range compression ported as `PhraseIndexRanges::convert` (`oxpinyin-core/src/lib.rs`) | same behaviour; one heap allocation per record per lookup where the pin reuses a buffer |
| User pinyin table | the user DBM loaded into an in-memory DB (`storage/facade_chewing_table2.h:87`, `load_db`) and searched like the system table | `UserLookup` (`oxpinyin-user/src/lookup.rs`): text-keyed ordered maps plus an initials map, rebuilt in full when the phrase generation changes; the pin's file format is written back at save | different (internal only) |
| Phrase string → tokens | `PhraseLargeTable3` per backend; system `attach`, user `load_db` (`storage/facade_phrase_table3.h:81`, `:86`) | system: `PhraseTable` (`oxpinyin-data/src/phrase_table.rs`), point gets and a cursor walk for suggestions; user: rows in the session store (`oxpinyin-user/src/phrase_table.rs`) | same (system), same behaviour (user) |
| Bigram row | `SingleGram` (`storage/ngram.h:59`): `MemoryChunk` of `{u32 total}` then sorted `SingleGramItem {token, freq}` (`storage/ngram.cpp:31`) | free functions over the same bytes in `oxpinyin-core/src/single_gram.rs` (`lower_bound`, `insert_freq`, `mask_out`, `merge_single_gram`) | same |
| System bigram | `Bigram::attach` read-only on the DBM (`storage/ngram_bdb.h:88`), one `load` per row | `BigramTable::load_raw_successors` (`oxpinyin-data/src/bigram_table.rs`) | same |
| User bigram | `Bigram::load_db` into the backend's in-memory container (`storage/ngram_bdb.h:67`) | `UserBigramDb` (`oxpinyin-store/src/lib.rs`): the pin's container per backend through the real library | same |
| Training-only n-grams | `FlexibleSingleGram` / `FlexibleBigram` on a DBM, used by `utils/training` only | in-memory ordered maps in `oxpinyin-kmm/src/model.rs` | different (training tools only; outputs listed in §8) |

oxpinyin's own internal tables live in one in-memory session keyspace
with a `table\0key` prefix (`oxpinyin-store/src/common.rs`). Nothing in
that keyspace is written to disk: at save the pin's files are produced
in the pin's formats.

## 2. Parsing

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Key | `ChewingKey` (`storage/chewing_key.h:41-48`): 16-bit bitfield 5/2/5/3/1 | `ChewingKey` in `oxpinyin-chewing/src/chewing_key.rs` with `to_packed` / `from_packed` for the identical layout; the engine itself keys on `SyllableKey` (`oxpinyin-core/src/vocab.rs`), a dense id, with tone carried beside it | same at the ABI and on disk; different inside the engine |
| Content and key tables | `content_table`, `chewing_key_table` (`storage/pinyin_parser_table.h`) | copied in `oxpinyin-chewing/src/chewing_key_data.rs` | same |
| Hanyu pinyin index | sorted `pinyin_index` (`storage/pinyin_parser_table.h:9`), 662 rows including correction rows, each with flags and a distance; `equal_range` per probe, O(log n) | `phf` maps generated by `oxpinyin-core/build.rs`; correction spellings recomputed by `option_alias_canonical` (`oxpinyin-core/src/syllables.rs`); O(1) per probe | same behaviour for key lookup; the distance column is not carried — see §11 |
| Full pinyin parse | forward DP, one `parse_value_t` per byte (`storage/pinyin_parser2.cpp:64`, `:217`); spans up to `max_full_pinyin_length = 7` (`:82`); a step is replaced by a longer parse, else fewer keys, else lower total distance (`:296-310`); `final_step` backtracks (`:346`); O(7n) time, O(n) space | `SegmentGraph::build_with_options` builds the whole lattice (`emit_edges`), then `SegmentGraph::fewest_keys` selects (`oxpinyin-core/src/graph.rs`); O(7n) time, O(7n) space | same behaviour except the distance rule — see §11 |
| Double pinyin | greedy longest match, `DoublePinyinParser2::parse` (`storage/pinyin_parser2.cpp:532`), `parse_one_key` (`:405`) | `parse_with_options`, `parse_one_key` in `oxpinyin-core/src/scheme.rs` | same |
| Zhuyin keyboards | `storage/zhuyin_parser2.cpp` (DaChen CP26 at `:573`) | `oxpinyin-core/src/zhuyin_map.rs` and the scheme parsers | same |
| Pinyin direct parser | `PinyinDirectParser2` (`storage/pinyin_parser2.cpp:618`) | not a parser type in oxpinyin | not yet recorded — see §11 |

## 3. Phonetic key matrix

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Layout | `PhoneticKeyMatrix`: two parallel tables of columns (keys and key rests) | one `Vec<Vec<ScanKey>>` from `build_scan_matrix` (`oxpinyin-engine/src/session/mod.rs`); apostrophe zero keys as a side array in `ParsedMatrix` | same behaviour |
| Fill | `fill_matrix` (`storage/phonetic_key_matrix.cpp:30`) | `ParsedMatrix::from_scan` (`oxpinyin-engine/src/session/matrix.rs`) | same behaviour |
| Resplit and inner split | `resplit_step` (`:87`), `inner_split_step` (`:169`); linear scans of the `special_table.h` tables | `resplit_step`, `inner_split_step` (`session/mod.rs`), same tables (`matrix-split-tables.md`) | same |
| Fuzzy | `fuzzy_syllable_step` (`:226`): an initials pass, then a finals pass over the grown column | `SyllableKey::fuzzy_alternatives` (`vocab.rs`) composes both swaps, keeping only keys that exist | same behaviour (the pin's extra non-existent finals match nothing downstream) |
| Column dedup | none — a column is a bag | `keep_first_in_column` (`session/mod.rs`), O(k²) per column | different (argued harmless in the survey; argument not yet written — §11) |

## 4. Sentence decoding

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Instance | `PhoneticLookup<2, 3>` (`pinyin.cpp:55`, `:406`); zhuyin uses `<1, 1>` | `NbestShape` chooses the shape at run time (`oxpinyin-engine/src/session/state.rs`) | same behaviour |
| Trellis | `ForwardPhoneticTrellis` (`lookup/phonetic_lookup.h:181`): per step a token → node hash table and an array of `trellis_node<nstore>` | `Trellis` (`nbest.rs`): per step a token map plus an explicit insertion order, so iteration never depends on hashing | same behaviour |
| Node store | `trellis_node::eval_item` (`lookup/phonetic_lookup_heap.h:56`): a fixed heap of `nstore` values whose front is replaced when a newcomer beats it; a separate `<1>` specialisation (`:89`, `:108`) | `Trellis::insert` reproduces the front replacement | same; the `<1>` specialisation's empty check is not modelled separately (§11) |
| Comparator | `trellis_value_less_than` (`lookup/phonetic_lookup.h:66`) with `LONG_SENTENCE_PENALTY = log(1.2f)` (`:39`) | `loses_to` (`nbest.rs`), penalty 263 on oxpinyin's scale | same |
| Beam | `get_top_results` (`:143`), `nbeam = 32` (`:37`), libstdc++ `make_heap` / `pop_heap` | hand ports of the heap routines in `nbest.rs`, needed because the comparator is cyclic | same |
| Expansion | `get_nbest_match` (`:736`): bigram over every beam value (`search_bigram2`, `:578`), unigram over the beam head only (`search_unigram2`, `:540`) | `nbest_sentences_with_seeds_generation`, `widen_free_span`, `expand_entries` | same |
| Tails and extraction | heap top-n, `g_ptr_array_sort` with `trellis_value_compare` (`:174`), which truncates the cost difference to `gint`; `extract_result` backtracks (`:370`) | `Trellis::tails` with `glib_merge_sort` and `tail_compare` (same truncation, NaN included), `Trellis::extract` | same |
| Scores | `gfloat` running sum of natural logs: unigram step `:663`, bigram step `:692-693` | integer fixed-point −log₂ × 1000 (`oxpinyin-core/src/cost.rs`); inputs from `nbest_step_costs_using`, `interpolate_ratio`, `raw_blend_cost` (`oxpinyin-data/src/lm/mod.rs`) | different — registered as class (a) in `upstream-divergences.md`, but reproducible: see §11 |
| Constraints | `ForwardPhoneticConstraints` (`lookup/phonetic_lookup.cpp:61` onward); skip NOSEARCH, a single search for ONESTEP | `ConstraintStore` (`oxpinyin-engine/src/constraint.rs`) | same; `validate` drops a stale constraint by "span search still finds the token" rather than the pin's `FLT_EPSILON` test (§11) |
| Phrase segmentation | `PhraseLookup` (`lookup/phrase_lookup.cpp:119`), one value per (step, token), `save_next_step` (`:348`) replaces on strictly better, `final_step` (`:382`) | `phrase_segment`, `Step::relax`, `Step::best` (`oxpinyin-engine/src/phrase.rs`) | same, with fixed-point scores |
| Per-keystroke work | trellis rebuilt and bigram rows reloaded and merged for each predecessor and span | trellis rebuilt; merged rows cached per predecessor and step (`NbestStepRow`), parse graph cached | same behaviour; fewer row loads |

## 5. Candidate list

| Item | Pin (`pinyin.cpp`) | oxpinyin | Mark |
| --- | --- | --- | --- |
| Window scan | `:2224` onward | `collect_window_scan`, `scan_parsed_paths` (`oxpinyin-engine/src/session/lookup.rs`) | same |
| Frequency | `_compute_frequency_of_items` (`:1810`), `BIGRAM_FREQUENCY_DISCOUNT 0.1f` (`:33`), f32 expression truncated to `guint32` (`:1862`) | `candidate_frequencies` (`session/lookup.rs`), `amplified_frequency_with_bigram` (`session/mod.rs`) | same, bit for bit |
| Sort | `compare_item_with_sort_option` (`:1678`) under GLib's stable sort | `RankKey::pin_compare`, `glib_rank_sort` | same |
| Longer candidate | `_prepend_longer_candidates` (`:1870`) | `longer_candidate` (`session/lookup.rs`) | same |
| Dedup | `_remove_duplicated_items_by_phrase_string` (`:2058`) | `prepend_nbest_rows` (`session/lookup.rs`), `dedup_by_text_keep_first` (`session/mod.rs`) | same behaviour (argument in the survey; not yet written — §11) |
| Prediction | `pinyin_guess_predicted_candidates` (`:2413`), filter 10 (`:2313`) | `oxpinyin-capi/src/predict.rs` | same |

## 6. Training and user phrases

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Online training | `train_result3` (`lookup/phonetic_lookup.h:844`) | `train_spans` (`oxpinyin-engine/src/session/selection.rs`), `trained_gram` and `train_pronunciations` (`oxpinyin-user/src/store.rs`), constants in `oxpinyin-user/src/seed.rs` | same arithmetic; a preflight refuses before any change where the pin would abort (class (c)) |
| Predicted choice | `pinyin_choose_predicted_candidate` (`pinyin.cpp:2591`) | `observe_predicted`, `observe_predicted_prefix` | same |
| Add phrase | `_add_phrase` (`pinyin.cpp:514`) | `add_phrase_in` (`store.rs`); next token from a cursor row instead of `get_range` | same behaviour |
| Remember input | `_remember_phrase_recur` (`pinyin.cpp:3578`), recursive matrix walk | `pinyin_remember_user_input` (`oxpinyin-capi/src/user_data.rs`), a walk over the selected syllables | same behaviour |
| Remove user phrase | `pinyin_remove_user_candidate` (`pinyin.cpp:3726`) | `remove_user_phrase` (`store.rs`), same order; refuses before mutating where the pin asserts | same, class (c) at the asserts |
| Mask out | `pinyin_mask_out` (`pinyin.cpp:1224`): reload system chunks, `merge_with_mask` | `mask_out` (`oxpinyin-user/src/store_libpinyin.rs`) in one transaction, totals and allocation cursors recomputed | same behaviour |
| Import end | `pinyin_end_add_phrases` (`pinyin.cpp:655-660`): `compact()`, mark modified | `compact_libraries`, `mark_modified` (`oxpinyin-capi/src/iterators.rs`) | same behaviour (not yet checked line by line — §11) |
| Export | export iterators (`pinyin.cpp:792` threshold) | `export_phrases_in`, `oxpinyin-facade/src/export_rows.rs` | same behaviour (per-pronunciation order not yet checked — §11) |

## 7. Object model and persistence

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Context and instance | `_pinyin_context_t` (`pinyin.cpp:39`) owns raw pointers to the tables; `_pinyin_instance_t` (`:73`) borrows them through a back pointer | `CapiContext` / `CapiInstance` (`oxpinyin-capi/src/state.rs`) over `ContextCore` and `UserStore` (shared handles, atomic dirty flag) | different (internal; the field-by-field split is in §0, "The two objects"; an instance can outlive `pinyin_fini` where the pin's cannot, which no defined use observes) |
| Edits between saves | direct mutation of the in-memory chunks and DBs | transactions on the in-memory session store | different (internal) |
| Save | `pinyin_save` (`pinyin.cpp:1132`), `_write_files` (`:922`), rename, `mark_version` | `save_with_bigram_reporting` (`oxpinyin-user/src/persistence.rs`) with the pin's file set and rename order | same |
| Aborts | asserts and `abort()` on caller-reachable input | `Result`, then warning plus `false` at the C ABI | different — class (c) |

## 8. Offline tooling

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| K-mixture model | `utils/training` (`k_mixture_model.h` and tools) | `oxpinyin-kmm`, audited term by term in `kmm-arithmetic-audit.md` | same arithmetic; output in token order, an iteration cap and `Err` for asserts (that audit's §7) |
| System data | `gen_binary_files`, `import_interpolation`, `gen_unigram` | `oxpinyin-datagen`, one compiler from `model20` (`datagen-model20.md`) | different pipeline, byte-identical output |

## 9. Constants

Every coefficient on the parity path, at the pin's value. oxpinyin uses
the same values; a constant that appears on oxpinyin's scale is the
pin's value converted, not a fitted one.

| Constant | Value | Pin |
| --- | --- | --- |
| `nbeam` | 32 | `lookup/phonetic_lookup.h:37` |
| `LONG_SENTENCE_PENALTY` | `log(1.2f)` | `lookup/phonetic_lookup.h:39` |
| node store × n-best (pinyin) | 2 × 3 | `pinyin.cpp:55` |
| training `initial_seed` | 23 × 3 = 69 | `lookup/phonetic_lookup.h:847` |
| training `expand_factor` | 2 | `:848` |
| training `unigram_factor` | 7 | `:849` |
| training `pinyin_factor` | 1 | `:850` |
| training `ceiling_seed` | 23 × 15 × 64 = 22080 | `:851` |
| predicted choice seed × unigram factor | 69 × 7 | `pinyin.cpp:2597-2598` |
| `_add_phrase` default count, unigram factor | 5, 3 | `pinyin.cpp:521-522` |
| export threshold | `initial_seed - 1` = 68 | `pinyin.cpp:792` |
| `BIGRAM_FREQUENCY_DISCOUNT` | 0.1f | `pinyin.cpp:33` |
| predicted bigram candidate filter | 10 | `pinyin.cpp:2313` (`_compute_predicted_bigram_candidates`) |
| `max_full_pinyin_length` | 7 (tone included) | `storage/pinyin_parser2.cpp:82` |
| correction-row distance | 1 (plain rows 0) | `storage/pinyin_parser_table.h:9` onward |

The pin also reads λ from the system `table.conf`; that is data, not a
coefficient, and oxpinyin reads the same value.

## 10. Where oxpinyin keeps the pin's code shape

`.kiro/steering/oxidation-method.md` prefers idiomatic Rust and keeps the
pin's code shape only where behaviour depends on it. The places found so
far where oxpinyin keeps C-shaped code:

| Place | Why | Needed for behaviour? |
| --- | --- | --- |
| Hand-written libstdc++ `push_heap` / `pop_heap` / `make_heap` (`heap_push`, `heap_adjust`, `heap_pop`, `heap_make` in `oxpinyin-engine/src/nbest.rs`) and GLib's merge sort (`glib_merge_sort`) | the comparator is cyclic, so the pin's result is defined by the exact sequence of swaps | yes |
| `SingleGram` operations over raw bytes (`oxpinyin-core/src/single_gram.rs`), including libstdc++'s `lower_bound` on unsorted arrays | the byte layout is the on-disk format, and the unsorted-array result reaches output | yes |
| The pin's `SEARCH_*` / `ERROR_*` integer codes in the user phrase table (`oxpinyin-user/src/phrase_table.rs`) | carried over from the pin's interface | no — they are read only inside `oxpinyin-user` (`store.rs`) and never cross the C ABI; a candidate for an enum or `Result` |

## 11. Open gaps

**Full-pinyin distance rule — defect.** The pin's DP prefers, among steps
with equal parsed length and equal key count, the one with the lower
total distance (`storage/pinyin_parser2.cpp:306-310`); correction rows
carry distance 1. `SegmentGraph::fewest_keys` has no distance and keeps
the first-found step, so under a correction option a path through a
correction spelling can win where the pin picks the plain one. With
`PINYIN_CORRECT_ON_ONG` set, `fewest_keys` on `origin/main` returns
`chon'ga` for `chonga`, `chon'gan` for `chongan` and `zhon'ge` for
`zhonge`; the pin's rule gives `chong'a`, `chong'an` and `zhong'e`. A
replay of the pin's DP over its own index with every correction bit set
finds about 500 two-syllable strings that differ. Not a divergence class:
it is selection logic, so it is a defect to fix.

**Sentence scoring is reproducible — gfloat and libm.** The class (a)
entry treats the pin's `gfloat` log accumulation as platform arithmetic
oxpinyin cannot follow. It is two things, both reproducible in Rust:

- *The float types and conversion points.* `gfloat` and `gdouble` are
  IEEE binary32 and binary64, as Rust's `f32` and `f64` are, and on
  x86-64 and aarch64 C evaluates each operation in its own type. The
  pin's steps are fixed by its source:
  - `unigram_lambda` is `(gfloat)(1. - lambda)`, computed in double
    and rounded to float (`lookup/phonetic_lookup.h:713`);
  - unigram step (`:650-663`): `elem_poss` is a double quotient;
    `elem_poss * pinyin_poss * unigram_lambda` runs in double; `log`
    is taken in double; the float `m_poss` is widened, added in
    double, and the sum rounded back to float;
  - bigram step (`:679-693`): `bigram_lambda * bigram_poss` is a float
    product; `unigram_lambda * unigram_poss` a double product; their
    sum and the product with `pinyin_poss` run in double; then `log`
    and the same widen, add and round;
  - `pinyin_poss` is a float sum over the matrix walk, in its order
    (`storage/phonetic_key_matrix.cpp:534` onward);
  - comparisons widen to double where the C does, for example
    `LONG_SENTENCE_PENALTY`, `log(1.2f)` in double
    (`lookup/phonetic_lookup.h:39`).
- *libm's `log`.* The one transcendental. Calling the same platform
  libm the pin calls gives the same result on that host; on Linux
  targets Rust's `f64::ln` lowers to a call to the system `log`.

What the source does not settle is read from the pin's binary: whether
the compiler fused any multiply-add (GCC may contract on aarch64, where
Rust never does; x86-64's baseline has no FMA) and that oxpinyin's
build calls `log` rather than folding or inlining it. With those
confirmed, the sentence decoder can follow the pin bit for bit on the
same host, and the class (a) entry reduces to "a host whose libm
differs from the pin's" — a property of the host, not of oxpinyin.
oxpinyin's fixed-point scale (`oxpinyin-core/src/cost.rs`) was a design
choice for cross-platform determinism, not a language limit.

**Fitted coefficients still in the tree — off the parity path.** `ScoringConfig::default`
(`oxpinyin-core/src/scoring.rs`) carries `segmentation_penalty`,
`incomplete_penalty` and `phrase_key_bonus` values chosen by sweeping
against the oracle (`scoring-constant-sweep.md`, 2026-08-10), and the
engine `Session` still holds a `ScoringConfig`. These are not pin values.
The session reads them in two places, both on the branch taken when the
language model has no real unigram data (`has_real_unigrams` false): the
candidate scan in `session/lookup.rs` and the sentence guess in
`session/guess.rs`, each falling back to `k_best`. Through the C ABI
that branch is never taken: the ABI's session is
`Session<RuntimeDict, RuntimeLm>` (`oxpinyin-runtime/src/lib.rs`,
`RuntimeSession`), `RuntimeLm` forwards `has_real_unigrams` to its
`BigramLanguageModel`, and that returns `true` unconditionally
(`oxpinyin-data/src/lm/mod.rs`). So the fitted weights reach no output
of the 79 exports; they act only for sessions built on models without
unigram data (test doubles and oracle tools). The fallback path and its
weights are a candidate for removal after the first release.

**Equivalence arguments not yet written.** The survey found these to
match, but the argument is not in this record:

- matrix column dedup (`keep_first_in_column`) against the pin's bag;
- candidate dedup (`dedup_by_text_keep_first`) against
  `_remove_duplicated_items_by_phrase_string`;
- the constraint `validate` drop test against `FLT_EPSILON`;
- the node store with `nstore = 1` against the `<1>` specialisation's
  empty check;
- `compact_libraries` against `FacadePhraseIndex::compact`, and the
  export iterators' per-pronunciation order;
- `PinyinDirectParser2`, which has no parser type in oxpinyin;
- double pinyin's handling of a table whose first yunmu slot is empty
  and second is not (no shipped table has one).
