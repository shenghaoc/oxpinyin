# libpinyin data structures and algorithms, and oxpinyin's counterparts

Date: 2026-10-16 UTC · Status: **register** (living) · Pin: libpinyin
`074a2219` (2.11.92), read from a clone at the pin · oxpinyin tree:
first surveyed at `origin/main` `3b2bfdf`; rows updated 2026-10-18 UTC
at `84b7e79` for #828 (merged as `928466f7`) and #832 (merged as
`1726f685`).

This is the record that `docs/source-policy.md` asks for:
what the pin stores and computes, subsystem by subsystem, and what
oxpinyin does in its place. Pin code is cited `file:line` under `src/`;
oxpinyin code by file and item.

Each row carries two marks, `structure / behaviour`, because whether
oxpinyin is built the same way and whether it behaves the same way are
separate questions:

- **Structure** — **same**: the pin's structure and algorithm,
  re-expressed in Rust; **different**: another structure or algorithm.
- **Behaviour**, against the pin, on the parity path:
  - **follows** — the structure is the pin's, so the behaviour follows
    from it;
  - **argued** — the structure differs and an equivalence argument is
    written, here or in the cited finding;
  - **unverified** — the structure differs and equivalence is believed
    but not yet argued (tracked in §11);
  - **class (a)**, **(b)** or **(c)** — a divergence registered under
    the compatibility policy;
  - **defect** — known to differ with no class to cover it;
  - **off path** — the code does not reach the parity path, and the row
    says why.

The **parity path** is code whose result reaches either C ABI —
libpinyin's 79 `pinyin_*` exports or libzhuyin's 52 — or the files
they write, and the offline tools whose output is promised identical
to the pin's (`oxpinyin-datagen`'s system files).

A row is a survey result, not a proof of equivalence. Rows whose
behaviour is unverified, and the open gaps, are listed at the end and
tracked by `docs/specs/oxidation-record/`.

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
`ContextCore::save_user` (`oxpinyin-facade`). Both C ABIs drive only
these. The same type also carries a key-driven layer from the first
design — `KeyInput`, `process_key`, `type_pinyin`, `Preedit`, `commit`
— which no C-ABI path calls; the oracle tools and tests use it. For the
bug-for-bug build the operations are the Rust surface that matters; the
key-driven layer belongs in an optional adapter on top of them, not in
the same type (`docs/specs/oxidation-record/` task 9).

### How oxpinyin divides it

oxpinyin keeps the pin's layers but makes each a crate with explicit
dependencies (`docs/architecture.md` has the full map):

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
| Phrase item | `PhraseItem` (`storage/phrase_index.h:64`): `{u8 len, u8 npron, u32 unigram, ucs4[len], {ChewingKey[len], u32 freq} × npron}` in a `MemoryChunk` | `PhraseItemView` and `PronunciationView` in `oxpinyin-data/src/phrase_library.rs`, borrowed views over the same bytes | same / follows |
| Sub phrase index | `SubPhraseIndex` (`storage/phrase_index.h:257`): header (total freq), offset array indexed by `token & PHRASE_MASK`, content area; file `mmap`ped `PROT_READ\|PROT_WRITE, MAP_PRIVATE` (`include/memory_chunk.h:505-506`); O(1) token → item | `PhraseLibrary` (`phrase_library.rs`), `item`, `slot_offset`; the same files mapped read-only; O(1) token → item | same / follows (reads) |
| Phrase index edits | in place on the private mapping: `PhraseItem::add_pronunciation` (`storage/phrase_index.cpp:56`), `SubPhraseIndex::add_unigram_frequency` (`:150`) | journal MODIFY writes into a separate `PhraseContent` private mapping; `SystemItems` retains completed replay views and caches `ChunkItem`s on first use; later imports/training use those decoded items and saves rebuild the pin's chunk format | different / argued (journal replay, below) |
| Facade phrase index | `FacadePhraseIndex` (`storage/phrase_index.h:438`): 16 `SubPhraseIndex*` and a running `m_total_freq` | `PhraseLibraries` (`oxpinyin-data/src/phrase_libraries.rs`): 16 lock-guarded slots; system total read from the chunk header, user total a separate row | different / unverified |
| Pinyin key → tokens | `ChewingLargeTable2` per backend (`storage/chewing_large_table2_bdb.h:34` and peers): DBM keyed by packed `ChewingKey[len]`, values `PinyinIndexItem2` arrays (`storage/pinyin_phrase3.h:181`), per-length `ChewingTableEntry` objects with a reused buffer (`storage/chewing_large_table2.h:74`); results compressed to token ranges by `convert` (`:90`) | `ChewingTable` (`oxpinyin-data/src/chewing_table.rs`) over the same file and keys; each lookup decodes rows into a fresh `Vec`; range compression ported as `PhraseIndexRanges::convert` (`oxpinyin-core/src/lib.rs`) | different / unverified; one heap allocation per record per lookup where the pin reuses a buffer |
| User pinyin table | the user DBM loaded into an in-memory DB (`storage/facade_chewing_table2.h:87`, `load_db`) and searched like the system table | `UserLookup` (`oxpinyin-user/src/lookup.rs`): text-keyed ordered maps plus an initials map, rebuilt in full when the phrase generation changes; the pin's file format is written back at save | different / unverified |
| Phrase string → tokens | `PhraseLargeTable3` per backend; system `attach`, user `load_db` (`storage/facade_phrase_table3.h:81`, `:86`) | system: `PhraseTable` (`oxpinyin-data/src/phrase_table.rs`); exact user lookup: `UserStore::phrase_table_tokens_for_text` (`oxpinyin-user/src/store.rs`), with one queried row cached by `RuntimeDict::tokens_for_text` | different / argued (exact lookup, below); suggestions retain the user snapshot |
| Bigram row | `SingleGram` (`storage/ngram.h:59`): `MemoryChunk` of `{u32 total}` then sorted `SingleGramItem {token, freq}` (`storage/ngram.cpp:31`) | free functions over the same bytes in `oxpinyin-core/src/single_gram.rs` (`lower_bound`, `insert_freq`, `mask_out`, `merge_single_gram`) | same / follows |
| System bigram | `Bigram::attach` read-only on the DBM (`storage/ngram_bdb.h:88`), one `load` per row | `BigramTable::load_raw_successors` (`oxpinyin-data/src/bigram_table.rs`) | same / follows |
| User bigram | `Bigram::load_db` into the backend's in-memory container (`storage/ngram_bdb.h:67`) | `UserBigramDb` (`oxpinyin-store/src/lib.rs`): the pin's container per backend through the real library | same / follows |
| Training-only n-grams | `FlexibleSingleGram` / `FlexibleBigram` on a DBM, used by `utils/training` only | in-memory ordered maps in `oxpinyin-kmm/src/model.rs` | different / off path — training tools only; their outputs are rows in §8 |

Journal replay follows `SubPhraseIndex::merge` at 074a2219
`storage/phrase_index.cpp:439-517`, called by `pinyin.cpp:250-276` and
`zhuyin.cpp:194-220`. A logger yields borrowed raw payloads; old-item equality
requires the same size and bytes (`storage/phrase_index.h:235-240`). REMOVE
publishes its removal before comparing. MODIFY compares first, overwrites the
private view when the payload fits, and otherwise removes/appends the whole
payload. A failed record leaves the applied prefix. Stale item views, cross-item
header reads and the registered short-ADD exception retain their existing laws.

`Merge` in `oxpinyin-user/src/replay.rs` owns a writable private view of the
original file, separate from the immutable save baseline. It copies content only
when an append needs owned storage, or as a fallback when a second mapping
cannot be made. Unchanged slot offsets are read directly from the baseline;
only relocated/removed offsets enter a sparse map. A vector of slot IDs marks
changed slots; it is sorted and deduplicated once at finalization (below).
The ordinary loader validates and walks one journal byte buffer without
building a vector of records or allocating item-write runs. Mask snapshots keep
the existing owned-record path. Session opens retain completed original-item
views after constant-time structural validation of each payload's extents,
without allocating a typed item. The checks match `decode_phrase_item`: both
header bytes, the unigram, text, and the first incomplete pronunciation's keys
or frequency. Extra trailing bytes and arbitrary scalar values stay accepted.
A failure records the eager diagnostic and leaves the baseline slot. Journal
preflight likewise checks record heads, tokens, lengths and payload extents
without constructing records; only the replay walk produces borrowed records.
Both validators keep static/numeric error values without allocation, formatting
the same diagnostics only when the caller reports an error.
`SystemItems` caches decoded items on use; added items still seed
the existing text/index rows, removals the removal set, and original items seed
only necessary unigram deltas. Replayed reading rows are never synthesized.

Replay never queries touched membership while applying records. It appends only
slot IDs to a vector, then sorts and deduplicates once before either finish.
Lazy validation filters rejected slots in one `retain` pass and trims capacity;
it never removes individual vector entries. Completed views use O(log J) binary
search for membership, as the tree did, and iterate ascending unique slots. No payloads
are staged and no array spans the token range.

Replay also records a separate vector of slots whose unigram bytes change.
ADD, REMOVE, relocation, partial unigram payloads and stale writes take the
conservative path; a stale write records the slot behind the saved item view.
During the existing validation pass, the already-read original and live views
also identify unigrams changed by overlapping cross-item writes. No extra
item walk or decoding is needed for this conservative guard.
The vector is sorted and deduplicated once, then filtered against the retained
slots after validation. Seeding visits only those candidates, compares their
final absolute unigram with the immutable baseline and writes nonzero deltas.
A later restoration can leave one candidate but writes no delta. Reading-only
changes require no second item visit. The existing per-library absolute total
and offset calculation still carry header changes independently of item counts.
For C candidate records, this adds O(C) transient IDs, O(C log C + C log J)
finalization and O(K) seeding visits for K distinct surviving candidates;
neither payloads nor a full-token-range array are staged. This replaces the
previous seeding walk over every retained slot without changing replay's
worst-case time or space class.

For B journal bytes, R touching records, J unique changed slots, M relocated
slots and K items subsequently used, the worst-case replay time remains
O(B + R log R + R log(M+1)), versus the pin's O(B + R) mapped walk. Collecting
IDs uses amortized O(1) per touch; ordering is paid at finish. Transient slot
metadata uses O(R + M), including duplicates, within the existing O(B) loader
space bound. After lazy filtering and capacity trimming it retains O(J + M),
plus typed payloads for the K used items. This trades transient duplicate IDs
for cheaper insertion; it does not restore decoded payload, write-run or record
staging. Validation checks one item's shape in O(1) time and space during init;
first use decodes and caches it. Bulk save/mask operations may materialize all changed items, as the
previous loader did at init. The extra private view shares clean file pages;
MODIFY dirties private pages as the pin does, without writing the system file.

For libpinyin sessions, `SessionValues::Memory`
(`oxpinyin-user/src/session_values.rs`) owns separate logical tables:
`Tables = BTreeMap<String, Rows>`, with
`Rows = BTreeMap<Vec<u8>, Vec<u8>>`. Table names select maps instead of
being prepended to every key. Keys and values are owned, and scans borrow
rows in bytewise key order. With T tables and N rows in the selected table,
a point read takes O(log T + log N) comparisons plus the returned value's
copy; a range scan takes O(log T + log N + R) for R visited rows. Retained
space is O(T + N_total) map entries plus their owned names, keys and values.

`SessionTxn::Memory` stages writes in
`BTreeMap<String, BTreeMap<Vec<u8>, Option<Vec<u8>>>>`; `None` marks a
deletion. Reads consult staged changes before the source maps. A transaction
scan streams the two ordered ranges together, replacing or omitting source
rows as staged, and stops when the visitor breaks. Its cost is
O(log T + log N + log W + R + C), where W is the number of staged keys in
that table and R and C are the source and staged rows examined. No scan or
transaction clones an entire table. Staging a put or delete takes
O(log T + log W) comparisons plus copied key/value bytes. A successful
callback applies the changes; an error drops them without changing the
source. For W_total distinct staged keys, commit takes at most
O(W_total (log T + log N_max)) comparisons, and staging uses O(W_total)
entries plus their owned bytes, rather than another copy of all source rows.

This keeps the session's logical values in owned memory, as the pin does
for phrase items, while using ordered maps and staged writes instead of
the pin's packed items and native in-memory DBs. Standalone stores use
`SessionValues::Native` and retain their backend transactions and
`table\0key` framing (`oxpinyin-store/src/common.rs`). The session's
`UserBigramDb` and file persistence still use the selected native backend.
At save, the pin's files are produced in the pin's formats; the logical
session maps themselves are not written to disk.

Exact-text lookup (#884, 2026-10-19 UTC) reads the system row, then the
live user row. `RuntimeDict::tokens_for_text` keeps stored multiplicity
and order within each library. Its stable library sort implements
`reduce_tokens` (`storage/phrase_large_table3.h:77-91`). The pin searches
both tables (`storage/facade_phrase_table3.h:120-126`) and appends enabled
tokens (`storage/phrase_large_table3.cpp:29-51`). These sources were read
with `git show 074a2219:<path>` in `/home/sheng/repos/libpinyin`.
The BDB search makes one keyed get (`storage/phrase_large_table3_bdb.cpp:196-205`).
The Rust user query caches one text and its matching tokens. A different
query or a phrase-table generation change triggers one keyed get. It scans
no other phrases or pronunciations. The cached space is O(query length +
matches). The pin also retains its last entry's buffer. New tokens, deletes,
masks and library replacement move the shared generation. Reading merges
keep it: the pin edits the item without calling `add_index`
(`pinyin.cpp:563-582`), while a new token calls it (`:596-599`).
`UserStore` advances the generation while it holds the database guard.
Another store handle therefore invalidates the row after a token edit.
Library visibility filters the cached raw tokens on every query.

## 2. Parsing

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Key | `ChewingKey` (`storage/chewing_key.h:41-48`): 16-bit bitfield 5/2/5/3/1 | `ChewingKey` in `oxpinyin-chewing/src/chewing_key.rs` with `to_packed` / `from_packed` for the identical layout; the engine itself keys on `SyllableKey` (`oxpinyin-core/src/vocab.rs`), a dense id, with tone carried beside it | same / follows at the ABI and on disk; different / unverified inside the engine |
| Content and key tables | `content_table`, `chewing_key_table` (`storage/pinyin_parser_table.h`) | copied in `oxpinyin-chewing/src/chewing_key_data.rs` | same / follows |
| Hanyu pinyin index | sorted `pinyin_index` (`storage/pinyin_parser_table.h:9`), 662 rows including correction rows, each with flags and a distance; `equal_range` per probe, O(log n) | `phf` maps generated by `oxpinyin-core/build.rs`; correction spellings recomputed by `option_alias_canonical` (`oxpinyin-core/src/syllables.rs`); O(1) per probe | different / argued — since #832 each edge carries the matched row's distance (`SyllableKey::from_option_text_with_distance`: 0 for an exact spelling, 1 for an admitted alias); verification in §11 |
| Full pinyin parse | forward DP, one `parse_value_t` per byte (`storage/pinyin_parser2.cpp:64`, `:217`); spans up to `max_full_pinyin_length = 7` (`:82`); a step is replaced by a longer parse, else fewer keys, else lower total distance (`:296-310`); `final_step` backtracks (`:346`); O(7n) time, O(n) space | `SegmentGraph::build_with_options` builds the whole lattice (`emit_edges`), then `SegmentGraph::fewest_keys` selects (`oxpinyin-core/src/graph.rs`); O(7n) time, O(7n) space | different / argued — since #832 `fewest_keys` ranks a step by (key count, total distance) and replaces it only on strictly less, the pin's three rules; verification in §11 |
| Double pinyin | greedy longest match, `DoublePinyinParser2::parse` (`storage/pinyin_parser2.cpp:532`), `parse_one_key` (`:405`) | `parse_with_options`, `parse_one_key` in `oxpinyin-core/src/scheme.rs` | same / follows |
| Zhuyin keyboards | `storage/zhuyin_parser2.cpp` (DaChen CP26 at `:573`) | `oxpinyin-core/src/zhuyin_map.rs` and the scheme parsers | same / follows |
| Pinyin direct parser | `PinyinDirectParser2` (`storage/pinyin_parser2.cpp:618`), used only to load `.table` text (`storage/chewing_large_table2.cpp:130`, `storage/phrase_index.cpp:564`), so on the data-generation path, not the runtime | `parse_pinyin_keys` (`oxpinyin-datagen/src/system.rs`), which resolves spellings through `ChewingKey::from_pinyin` where the pin looks them up in `pinyin_index` with option flags | different / unverified (§11) |

## 3. Phonetic key matrix

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Layout | `PhoneticKeyMatrix`: two parallel tables of columns (keys and key rests) | one `Vec<Vec<ScanKey>>` from `build_scan_matrix` (`oxpinyin-engine/src/session/mod.rs`); apostrophe zero keys as a side array in `ParsedMatrix` | different / unverified |
| Fill | `fill_matrix` (`storage/phonetic_key_matrix.cpp:30`) | `ParsedMatrix::from_scan` (`oxpinyin-engine/src/session/matrix.rs`) | different / unverified |
| Resplit and inner split | `resplit_step` (`:87`), `inner_split_step` (`:169`); linear scans of the `special_table.h` tables | `resplit_step`, `inner_split_step` (`session/mod.rs`), same tables (`matrix-split-tables.md`) | same / follows |
| Fuzzy | `fuzzy_syllable_step` (`:226`): an initials pass, then a finals pass over the grown column | `SyllableKey::fuzzy_alternatives` (`vocab.rs`) composes both swaps, keeping only keys that exist | different / argued — the pin's extra non-existent finals match nothing downstream |
| Column dedup | none — a column is a bag | `keep_first_in_column` (`session/mod.rs`), O(k²) per column | different / unverified (§11) |

## 4. Sentence decoding

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Instance | `PhoneticLookup<2, 3>` (`pinyin.cpp:55`, `:406`); zhuyin uses `<1, 1>` | `NbestShape` chooses the shape at run time (`oxpinyin-engine/src/session/state.rs`) | different / unverified |
| Trellis | `ForwardPhoneticTrellis` (`lookup/phonetic_lookup.h:181`): per step a token → node hash table and an array of `trellis_node<nstore>` | `Trellis` (`nbest.rs`): per step a token map plus an explicit insertion order, so iteration never depends on hashing | different / argued (`sentence-surface.md`, lane D) |
| Node store | `trellis_node::eval_item` (`lookup/phonetic_lookup_heap.h:56`): a fixed heap of `nstore` values whose front is replaced when a newcomer beats it; a separate `<1>` specialisation (`:89`, `:108`) | `Trellis::insert` reproduces the front replacement | same / follows; unverified for `nstore = 1` (§11) |
| Comparator | `trellis_value_less_than` (`lookup/phonetic_lookup.h:66`) with `LONG_SENTENCE_PENALTY = log(1.2f)` (`:39`) | `node_loses_to` (`nbest.rs`): pinyin orders `cost_add(cost, 263 * length)` with explicit special-cost ordering; zhuyin retains the pin's predicate | different / S1/S2 of the [2026-10-28 amendment](compatibility-policy.md#amendment--witnessed-algorithm-precondition-violations-2026-10-28-utc); node front eviction retained |
| Beam | `get_top_results` (`:143`), `nbeam = 32` (`:37`), libstdc++ `make_heap` / `pop_heap` | unchanged heap routines in `nbest.rs`; pinyin `selection_loses_to` adds preselection batch position after adjusted cost, independently of physical backtracking slots | different / S1/S2 comparator repair under the same amendment; selection algorithm unchanged |
| Expansion | `get_nbest_match` (`:736`): bigram over every beam value (`search_bigram2`, `:578`), unigram over the beam head only (`search_unigram2`, `:540`) | `nbest_sentences_with_seeds_generation`, `widen_free_span`, `expand_entries` | same / follows |
| Tails and extraction | heap top-n, `g_ptr_array_sort` with `trellis_value_compare` (`:174`), which truncates the cost difference to `gint`; `extract_result` backtracks (`:370`) | `Trellis::tails` selects as before, then `tail_merge` stably sorts raw Cost with explicit special-cost ordering; `Trellis::extract` backtracks as before | different / S3 of the [2026-10-28 amendment](compatibility-policy.md#amendment--witnessed-algorithm-precondition-violations-2026-10-28-utc); raw possibility descending, equal costs stable, NaN last |
| Scores | `gfloat` running sum of natural logs: unigram step `:663`, bigram step `:692-693` | integer fixed-point −log₂ × 1000 (`oxpinyin-core/src/cost.rs`); inputs from `nbest_step_costs_using`, `interpolate_ratio`, `raw_blend_cost` (`oxpinyin-data/src/lm/mod.rs`) | different / class (a), registered in `upstream-divergences.md`, but the arithmetic can be followed in Rust once verified (§11) |
| Constraints | `ForwardPhoneticConstraints` (`lookup/phonetic_lookup.cpp:61` onward); skip NOSEARCH, a single search for ONESTEP | `ConstraintStore` (`oxpinyin-engine/src/constraint.rs`) | same / argued — revalidation follows the pin's `validate_constraint` since #828 (`ConstraintSpelling::spells`, §11); its integer drop test is argued for model20 data only (§11) |
| Phrase segmentation | `PhraseLookup` (`lookup/phrase_lookup.cpp:119`), one value per (step, token), `save_next_step` (`:348`) replaces on strictly better, `final_step` (`:382`) | `phrase_segment`, `Step::relax`, `Step::best` (`oxpinyin-engine/src/phrase.rs`) | same / class (a) — fixed-point scores |
| Per-keystroke work | trellis rebuilt and bigram rows reloaded and merged for each predecessor and span | trellis rebuilt; merged rows cached per predecessor and step (`NbestStepRow`), parse graph cached | different / unverified — caching only |

### Heap-port optimisation assessment — 2026-10-28 UTC

**Not pursued (2026-10-28 UTC).** The estimated 0.2–0.5% keystroke
instruction saving is below what the timing and instruction controls can
establish, so no heap change was made. This records the measurement; it
does not rule out later work. This is a single-build attribution, not a
measured speedup or a neutrality-checked differential.

Measured tree: `1cb70696ecf8f80aa1fdcf3e2ba62ba7e0d27fd7`, after #1339
and #1342. Host: Ubuntu 26.10, x86_64 Intel i7-9750H, Rust 1.97.1,
Valgrind 3.27.1, healthy tkrzw 1.0.34. The health probe passed and `ldd`
resolved both subject and pin to that library. The complete model20 tkrzw
export and verified model20 cache supplied the data; user directories
were fresh and on ext4. The stage2 executable used the release profile
(opt-level 3, fat LTO, one codegen unit), with
`RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none"`, not `CARGO_PROFILE_RELEASE_DEBUG`.
The host's sole heavy slot was held through building and collection, then
released.

The profile predates the 2026-10-28 algorithm-precondition amendment
(#1347), so it measures the pin's comparator as ported (`loses_to`), not
the repaired S1/S2 ordering implemented in #1352.

All four cases use `nihaozhongguo`. Collection was restricted to the named
C API call; the averages include calls during Criterion warm-up.

| Stage2 case | Collected API | Calls | Ir/call |
| --- | --- | ---: | ---: |
| `parse_more_full_pinyins/medium` | `pinyin_parse_more_full_pinyins` | 12,216 | 49,862.19 |
| `guess_sentence_get_sentence_0/full_nbest_post_116` | `pinyin_guess_sentence` | 40 | 4,474,725.68 |
| `guess_candidates/offset_0` | `pinyin_guess_candidates` | 1,023 | 380,925.47 |
| `guess_candidates/mid_phrase` | `pinyin_guess_candidates` | 1,326 | 277,795.40 |

`nbest.rs`'s `heap_make`, `heap_adjust`, `heap_push`, `heap_pop` and
`loses_to` total **4,023,960 Ir / 178,989,027 sentence Ir = 2.248%**:
3,765,840 in separately named heap functions, plus 258,120 inlined into
callers. Source-line attribution includes generated copies and the
comparator's `cost.rs`/standard-library helpers, identified through DWARF
inline chains; inclusive callees are counted once. Adding `heap_top`'s
remaining wrapper/allocation work gives 2.349%; also adding
`Trellis::candidates` gives 2.988%.

One parse + sentence + candidate call totals 4,905,513.33 Ir at offset 0
or 4,802,383.26 mid-phrase. The heap/comparator shares are **2.051%** and
**2.095%** respectively; those other APIs execute no heap-selection work.
These are sums of independent stage2 averages, not captured consecutive
keystrokes.

| Constant-factor idea | Estimated keystroke Ir saving | Basis in the profile and code |
| --- | ---: | --- |
| Compact beam handles | 0–0.15% | Value-copy loads/stores cost about 0.30%; handles add dereferences. |
| Cache comparator inputs | 0–0.10% | Cost and length are already stored; penalty-add branches are not executed here. Length helpers cost about 0.30%, before preparation/storage costs. |
| Reuse selection scratch | 0.15–0.30% | Candidate-buffer growth and selection allocation callees cost about 0.32%. |
| Expose proven index bounds in safe Rust | 0–0.10% | Identified bounds-check pairs cost about 0.14%; some checks must remain. |
| Inline comparator | Approximately 0% | Already inlined into the sift loops. |

These planning estimates overlap; the combined estimate is 0.2–0.5%,
not their sum. No change was implemented or compared with controls.

To reproduce, use the [native setup](../runbooks/setup.md) and the
[benchmark inputs](../runbooks/benches.md#criterion-benches). Set
`PINYIN_MODEL_DIR`, `PINYIN_EXPORT_DIR` (complete model20 tkrzw export),
`TMPDIR` (ext4), and the healthy tkrzw prefix's `PATH`, `PKG_CONFIG_PATH`
and `LD_LIBRARY_PATH`. On the measurement host, run the following inside
the exclusive hold (`just heavy -- bash` held its sole slot). Set `BENCH`
to Cargo's printed executable and repeat the collection command for each
`CASE`/`API` pair above, using a distinct `PROFILE` file:

```bash
unset CARGO_PROFILE_RELEASE_DEBUG
export RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none"
cargo bench -v --locked -p oxpinyin-capi --no-default-features \
  --features tkrzw --bench stage2 --no-run
taskset -c 0 valgrind --tool=callgrind --collect-atstart=no \
  --toggle-collect="$API" --read-inline-info=yes --dump-instr=yes \
  --cache-sim=no --branch-sim=no --callgrind-out-file="$PROFILE" \
  "$BENCH" "$CASE" --bench --profile-time 3
callgrind_annotate --inclusive=yes --tree=caller --threshold=100 \
  --auto=no "$PROFILE" crates/oxpinyin-engine/src/nbest.rs
callgrind_annotate --inclusive=no --threshold=100 --auto=no \
  "$PROFILE" crates/oxpinyin-engine/src/nbest.rs crates/oxpinyin-core/src/cost.rs
exit # release the hold
```

For supporting inline sites, pass the profile's instruction addresses to
`addr2line -afiC -e "$BENCH"` on standard input. Use exclusive source
costs for inlined code and do not add a callee's inclusive cost again.

## 5. Candidate list

| Item | Pin (`pinyin.cpp`) | oxpinyin | Mark |
| --- | --- | --- | --- |
| Window scan | `:2224` onward | `collect_window_scan`, `scan_parsed_paths` (`oxpinyin-engine/src/session/lookup.rs`) | same / follows |
| Frequency | `_compute_frequency_of_items` (`:1810`), `BIGRAM_FREQUENCY_DISCOUNT 0.1f` (`:33`), f32 expression truncated to `guint32` (`:1862`) | `candidate_frequencies` (`session/lookup.rs`), `amplified_frequency_with_bigram` (`session/mod.rs`) | same / follows, bit for bit |
| Sort | `compare_item_with_sort_option` (`:1678`) under GLib's stable sort | `RankKey::pin_compare`, `glib_rank_sort` | same / follows |
| Longer candidate | `_prepend_longer_candidates` (`:1870`) | `longer_candidate` (`session/lookup.rs`) | same / follows |
| Dedup | `_remove_duplicated_items_by_phrase_string` (`:2058`, frequency rule `:2128-2138`); combined zhuyin slices (`zhuyin.cpp:1607-1630`) | `dedup_ranked_by_text` (`session/mod.rs`) includes LONGER and all before-cursor slices; `prepend_nbest_rows` keeps sentence precedence | same / follows for valid phrase text (#913); other text remains under #894/#927 |
| Prediction | `pinyin_guess_predicted_candidates` (`:2413`), filter 10 (`:2313`) | `oxpinyin-capi/src/predict.rs` | same / follows |

## 6. Training and user phrases

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Online training | `train_result3` (`lookup/phonetic_lookup.h:844`) | `train_spans` (`oxpinyin-engine/src/session/selection.rs`), `trained_gram` and `train_pronunciations` (`oxpinyin-user/src/store.rs`), constants in `oxpinyin-user/src/seed.rs` | same / follows; class (c) where the pin aborts |
| Predicted choice | `pinyin_choose_predicted_candidate` (`pinyin.cpp:2591`) | `observe_predicted`, `observe_predicted_prefix` | same / follows |
| Add phrase | `_add_phrase` (`pinyin.cpp:514`) | `add_phrase_in` (`store.rs`); next token from a cursor row instead of `get_range` | different / unverified |
| Remember input | `_remember_phrase_recur` (`pinyin.cpp:3578`), recursive matrix walk | `pinyin_remember_user_input` (`oxpinyin-capi/src/user_data.rs`), a walk over the selected syllables | different / unverified |
| Remove user phrase | `pinyin_remove_user_candidate` (`pinyin.cpp:3726`) | `remove_user_phrase` (`store.rs`), same order; refuses before mutating where the pin asserts | same / follows; class (c) at the asserts |
| Mask out | `pinyin_mask_out` (`pinyin.cpp:1224`): reload system chunks, `merge_with_mask` | `mask_out` (`oxpinyin-user/src/store_libpinyin.rs`) in one transaction, totals and allocation cursors recomputed | different / unverified |
| Import end | `pinyin_end_add_phrases` (`pinyin.cpp:655-660`): `compact()`, mark modified | `compact_libraries`, `mark_modified` (`oxpinyin-capi/src/iterators.rs`) | different / unverified (§11) |
| Export | export iterators (`pinyin.cpp:792` threshold) | `export_phrases_in`, `oxpinyin-facade/src/export_rows.rs` | different / unverified (§11) |

## 7. Object model and persistence

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Context and instance | `_pinyin_context_t` (`pinyin.cpp:39`) owns raw pointers to the tables; `_pinyin_instance_t` (`:73`) borrows them through a back pointer | `CapiContext` / `CapiInstance` (`oxpinyin-capi/src/state.rs`) over `ContextCore` and `UserStore` (shared handles, atomic dirty flag) | different / unverified — the field-by-field split is in §0, "The two objects"; an instance can outlive `pinyin_fini` where the pin's cannot, which no defined use observes |
| Edits between saves | direct mutation of the in-memory chunks and DBs | transactions on the in-memory session store | different / unverified |
| Save | `pinyin_save` (`pinyin.cpp:1132`), `_write_files` (`:922`), rename, `mark_version` | `save_with_bigram_reporting` (`oxpinyin-user/src/persistence.rs`) with the pin's file set and rename order | same / follows |
| Aborts | asserts and `abort()` on caller-reachable input | `Result`, then warning plus `false` at the C ABI | different / class (c) |

## 8. Offline tooling

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| K-mixture model | `utils/training` (`k_mixture_model.h` and tools) | `oxpinyin-kmm`, audited term by term in `kmm-arithmetic-audit.md` | same / argued (`kmm-arithmetic-audit.md`); output in token order, an iteration cap and `Err` for asserts (that audit's §7) |
| System data | `gen_binary_files`, `import_interpolation`, `gen_unigram` | `oxpinyin-datagen`, one compiler from `model20` (`datagen-model20.md`) | different / argued — byte-identical output (`datagen-model20.md`) |

## 9. Constants

Two inventories: the pin's coefficients and limits, which oxpinyin
takes at the pin's value (converted to oxpinyin's scale where it
appears there, never fitted), and oxpinyin's own limits and weights,
which have no pin counterpart and are classified by call path. The
second inventory was taken by searching the runtime crates for numeric
constants on 2026-10-16 UTC; it is not yet proven complete (§11).

### The pin's values

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
| parse-step distance | 1 on the 232 `PINYIN_CORRECT_*` rows and the two `PINYIN_AMB_S_SH` / `PINYIN_AMB_Z_ZH` rows (`sua`, `zua`); 0 on the other 428 | `storage/pinyin_parser_table.h:9-673` |
| `MAX_PHRASE_LENGTH` | 16 keys | `include/novel_types.h:119` (oxpinyin: `MAX_PHRASE_LENGTH` in `oxpinyin-engine` and `oxpinyin-user`, `MAX_INDEX_KEY_WORDS` in `oxpinyin-data`) |
| `PHRASE_INDEX_LIBRARY_COUNT` | 16 | `include/novel_types.h:43` |
| `OPEN_COUNTER_LIMIT` | 6 | `storage/table_info.cpp:32` (oxpinyin: `oxpinyin-data/src/user_files.rs`) |

The pin also reads λ from the system `table.conf`; that is data, not a
coefficient, and oxpinyin reads the same value. Ruling 2026-10-28 UTC:
`SystemTableInfo2::load` sets `LC_NUMERIC="C"` and scans `%f\n` into a
`gfloat` (`src/storage/table_info.cpp:197,220-224` at `074a2219`). Since
external-I/O stage 1 (2026-10-28 UTC) `TableConf::load` makes that very
call on the file's stream through `oxpinyin-input`, for both facades and the
tools; the decoder's rational routing value is the scanned `gfloat`'s
shortest decimal (`Lambda::from_f32`). See the
[source derivation and supersession](1297-table-floats.md).

### oxpinyin's own values, by call path

| Value | Where it acts | Parity path? | Status |
| --- | --- | --- | --- |
| `SCAN_EXPANSION_LIMIT = 4096` (`oxpinyin-engine/src/session/mod.rs`) — chosen by measuring the W2 corpus | (1) the window scan's expansion in `scan_parsed_paths` (`session/lookup.rs`), taken only when the dictionary does not handle partial keys; the runtime's does. (2) `lookup_finds_token` (`nbest.rs`), the constraint revalidation fallback for a dictionary that keeps no phrase items (the test doubles); the runtime's dictionaries read their items instead. (3) `oxpinyin-engine/src/char_offset.rs`, behind `pinyin_get_character_offset`, which expands one key at a time, so the product never approaches the cap. Until #828 it was also reached through `span_finds_token` in constraint revalidation (§11) | (1) no; (2) no; (3) yes, cap unreachable | off path or unreachable since #828 |
| `MAX_PHRASE_KEYS = 8` (`session/mod.rs`) | `Session::candidates_ending_at`, which libzhuyin's facade calls for its before-cursor candidates (`oxpinyin-zhuyin-capi/src/sentence.rs`) | yes (libzhuyin) | unverified — no pin counterpart found yet |
| `MAX_INPUT_BYTES = 32767` (`session/mod.rs`), `MAX_GRAPH_INPUT = 65535` (`oxpinyin-core/src/graph.rs`) | input clamps | yes, only past the bound | unverified — no pin counterpart found yet |
| `SEGMENTATION_K = 8` (`session/mod.rs`), `ScoringConfig.expansion_limit = 64` and the other `ScoringConfig` weights (`oxpinyin-core/src/scoring.rs`), `UNIGRAM_TIEBREAK_SCALE = 16` (`oxpinyin-data/src/lm/mod.rs`) | the k-best fallback and `LanguageModel::score`, read only when the model has no real unigram data | no — see §11 | fitted; removal is internal work (spec task 4a) |
| `COUNT_MEMO_MAX_ENTRIES = 8192` (`oxpinyin-user/src/store.rs`) | size of a count cache | no output effect | cache bound only |

## 10. Where oxpinyin keeps the pin's code shape

`docs/source-policy.md` prefers idiomatic Rust and keeps the
pin's code shape only where behaviour depends on it. The places found so
far where oxpinyin keeps C-shaped code:

| Place | Why | Needed for behaviour? |
| --- | --- | --- |
| Hand-written libstdc++ `push_heap` / `pop_heap` / `make_heap` (`heap_push`, `heap_adjust`, `heap_pop`, `heap_make` in `oxpinyin-engine/src/nbest.rs`) and GLib's merge sort (`glib_merge_sort`, shared with the candidate and predicted sorts) | the comparator is cyclic, so the pin's result is defined by the exact sequence of swaps | yes |
| `SingleGram` operations over raw bytes (`oxpinyin-core/src/single_gram.rs`), including libstdc++'s `lower_bound` on unsorted arrays | the byte layout is the on-disk format, and the unsorted-array result reaches output | yes |
| The pin's `SEARCH_*` / `ERROR_*` integer codes in the user phrase table (`oxpinyin-user/src/phrase_table.rs`) | carried over from the pin's interface | no — they are read only inside `oxpinyin-user` (`store.rs`) and never cross the C ABI; a candidate for an enum or `Result` |

## 11. Open gaps

**Full-pinyin distance rule — fixed in #832.** The pin's DP prefers,
among steps with equal parsed length and equal key count, the one with
the lower total distance (`storage/pinyin_parser2.cpp:306-310`); the
rows that carry distance 1 are listed in §9.

- *Found (2026-10-16 UTC, source reading).* `SegmentGraph::fewest_keys`
  had no distance and kept the first-found step. With
  `PINYIN_CORRECT_ON_ONG` set it returned `chon'ga`, `chon'gan` and
  `zhon'ge` for `chonga`, `chongan` and `zhonge`, where the pin's rule
  gives `chong'a`, `chong'an` and `zhong'e`. The survey also estimated
  "about 500" differing two-syllable strings, from a Python replay of
  the pin's DP over its `pinyin_index` with every correction bit set,
  over ordered pairs of the index's non-incomplete spellings. That
  replay was never committed and did not model option gating beyond
  the correction bits, so the figure is not reproducible from the
  repository and is superseded by the measurement below; the two
  enumerate different string sets and are not comparable.
- *Fixed (#832, 2026-10-17 UTC).* Each edge carries its row's distance
  and `fewest_keys` ranks by (key count, total distance).
- *Verified at the C ABI (#832, reference cell bdb).* The option-sweep
  differential logs every parsed key and key rest, and its parse-only
  `pairs` mode covers every ordered pair of the 662 pinned
  `pinyin_index` spellings with an admitted alias member
  (`tools/bisection/option-sweep.c`; the spellings are in
  `option-sweep-tables.h`, whose generator over the pin's table is in
  its header comment). Run as
  `PINYIN_ORACLE_PREFIX=<bdb oracle prefix> tools/bisection/run-option-sweep.sh`
  (exit 0 identical, 2 divergence). Before the fix, 291 of 253,344
  strings differed under every correction bit (`0x1fe00188`), all
  `PINYIN_CORRECT_ON_ONG` splits, and 223 of 22,219 under that bit
  alone; with it, every pair sweep and option case is identical.

**Sentence scoring: gfloat and libm, which Rust can follow.** The class (a)
entry treats the pin's `gfloat` log accumulation as platform arithmetic
oxpinyin cannot follow. It is two things, both of which Rust can follow:

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
  - comparisons run in the type the C gives them.
    `LONG_SENTENCE_PENALTY`, `log(1.2f)` (`lookup/phonetic_lookup.h:39`),
    is the float overload, folded to the f32 constant `0x3e3ab283`
    (0.18232159316539764) and added and compared in single precision:
    `addss`/`comiss` in `trellis_value_less_than<2>` of the amd64 BDB
    `libpinyin.so.15` (0x96060–0x96072, 0x96080–0x96097) and
    `fadd s`/`fcmpe s` on arm64 (0x79d60–0x79d78, 0x79d90–0x79da8).
    Corrected 2026-10-28 UTC from "in double"; evidence in
    [float-scoring-1378.md](float-scoring-1378.md), Appendix A, D-1.
- *libm's `log`.* The one transcendental. The pin calls the platform
  libm's `log`. Rust's `f64::ln` documents unspecified precision that
  may vary by platform and Rust version, so whether a given oxpinyin
  build resolves it to the same `log` is not a language guarantee; it
  has to be verified for the identified artifacts.

What the source does not settle has to be verified for the identified
artifacts, neither of which is verified yet: in the pin's
`libpinyin.so`, whether the compiler contracted any multiply-add (GCC
may on aarch64, where Rust does not; x86-64's baseline has no FMA); in
oxpinyin's shipping build, that `f64::ln` resolves to the same `log`
rather than being folded, inlined or replaced. Matching types,
conversion points and operation order is necessary, not sufficient: a
difference that remains after that is investigated and attributed, not
classified as a host difference by default. oxpinyin's fixed-point
scale (`oxpinyin-core/src/cost.rs`) was a design choice for
cross-platform determinism, not a language limit.

**Constraint revalidation and the expansion cap — fixed in #828.**

- *Found (2026-10-16 UTC).* `span_finds_token` answered "does this
  token still span this range" by enumerating key paths and expanding
  every incomplete key through `expand_keys` with
  `SCAN_EXPANSION_LIMIT`; past 4096 combinations it found nothing and
  the forcing was dropped. The pin revalidates without searching any
  table: `validate_constraint` (`lookup/phonetic_lookup.cpp:120-165`)
  reads the constrained phrase item and calls
  `compute_pronunciation_possibility`
  (`storage/phonetic_key_matrix.cpp:534` onward), which matches the
  matrix against the item's stored pronunciations, an incomplete key
  matching any final (`storage/pinyin_phrase3.h:80-86`), with no
  expansion and no cap. A first reproduction ran against a source build
  of the pin whose hash was not the pinned one, on the mini fixture,
  with user-imported phrases (#825).
- *Fixed (#828).* `ConstraintSpelling::spells` (`nbest.rs`) ports
  `validate_constraint` and `compute_pronunciation_possibility` through
  `Dictionary::phrase_item_pronunciations`, carrying the last item read
  across a pass as the pin's single `PhraseItem` does.
- *Verified (#828, reference cell bdb, hash-pinned oracle, full
  model20).* Cases in `tools/bisection/contract-diff.py`:
  `constraint-survives-{zhonghuarenmin,zhonghuarm,zhhrenmin,zhonghrm,zhhrm,zhhrmghg}`,
  `constraint-survives-reconcile-{juezhonghuarm,juezhhrm}` and
  `constraint-stale-item-{keeps-nini,drops-nihao}`, run as
  `tools/bisection/run-contract-diff.sh bdb <oracle prefix> <libpinyin_capi.so> -- --cases <names>`;
  with `--expect-parent` against the parent build, the over-cap and
  `keeps-nini` cases differ and the controls match. All match with the
  fix.
- *Scope of that verification.* The phrases are user imports on full
  model20, not phrases of the model. For the six original inputs the
  chosen phrase is taken from its NORMAL row, because on full data the
  n-best holds only the first phrase; the reconcile pair is chosen from
  its NBEST row. A forcing over a phrase of the pinned model whose
  abbreviation passes the cap has not been run (spec task 12b).

**Fitted coefficients still in the tree — off the parity path.**
`ScoringConfig::default` (`oxpinyin-core/src/scoring.rs`) carries `segmentation_penalty`,
`incomplete_penalty` and `phrase_key_bonus` values chosen by sweeping
against the oracle (`scoring-constant-sweep.md`, 2026-08-10), and the
engine `Session` still holds a `ScoringConfig`. These are not pin values.
The session reads them in two places, both on the branch taken when the
language model has no real unigram data (`has_real_unigrams` false): the
candidate scan in `session/lookup.rs` and the sentence guess in
`session/guess.rs`, each falling back to `k_best`. Through either C ABI
that branch is never taken: both facades build on `InstanceCore`, whose
session is `Session<RuntimeDict, RuntimeLm>` (`oxpinyin-runtime/src/lib.rs`,
`RuntimeSession`); `RuntimeLm` forwards `has_real_unigrams` to its
`BigramLanguageModel`, and that returns `true` unconditionally
(`oxpinyin-data/src/lm/mod.rs`). So the fitted weights reach no output
of either ABI; they act only for sessions built on models without
unigram data (test doubles and oracle tools). These weights do not affect either C ABI under the established runtime
configuration, so their removal is not required to close a C-ABI
compatibility gap. Removal remains internal-work scope (spec task 4a),
subject to checking in-tree Rust callers and test and tool consumers
(a Rust interface change needs no approval since 2026-10-18 UTC).

**The value inventory is not yet proven complete.** §9's second table
came from a search of the runtime crates for numeric constants; limits
expressed inline (a literal in an expression, a loop bound) are not yet
covered.

**Equivalence arguments not yet written.** The survey found these to
match, but the argument is not in this record:

- matrix column dedup (`keep_first_in_column`) against the pin's bag;
- candidate dedup for malformed phrase text remains under #894/#927;
  the valid-text frequency rule is argued in
  `addon-choose-promotion.md` section 5 (#913);
- the constraint drop test: oxpinyin's integer zero test against the
  pin's `< FLT_EPSILON` is argued for model20 data in the
  `validate_constraint` entry of `upstream-divergences.md` (no per-token
  total there reaches 2^23); the general case is not argued, and #828
  did not change it;
- the node store with `nstore = 1` against the `<1>` specialisation's
  empty check;
- `compact_libraries` against `FacadePhraseIndex::compact`, and the
  export iterators' per-pronunciation order;
- `parse_pinyin_keys` (`oxpinyin-datagen`) against `PinyinDirectParser2::parse` with `USE_TONE`;
- double pinyin's handling of a table whose first yunmu slot is empty
  and second is not (no shipped table has one).

### #853: exported system-item unigram increments

Source tree read: `/home/sheng/repos/libpinyin`, using `git show` at
`074a2219c90feaf962d0d24f034514033ece5f99` (the checkout itself is on a
later commit). `src/storage/phrase_index.h:628-636` rejects an absent
sub-index before adding to the facade's wrapping u32 total.
`src/storage/phrase_index.cpp:150-178` reads the item before checking
sub-index-total overflow; a refusal leaves the facade increment in place.
An accepted increment wraps the item's u32 frequency and raises the
sub-index total. Both exports merely return success and do not mark dirty
(`src/pinyin.cpp:2836-2843`, `src/zhuyin.cpp:1828-1835`).

Save requires a user directory and the dirty flag, then compacts
(`src/pinyin.cpp:1132-1145`, `src/zhuyin.cpp:547-554`). Compact rebuilds
sub-index totals from items without rebuilding the facade total
(`src/storage/phrase_index.cpp:648-675`). Diff writes the old/live header
and compares complete items (`:392-436`); reload/unload removes the old
sub-index total (`:237-269`). The #866 current-disk baseline remains the
old side of that comparison.

Design: `UserStore::add_system_unigram_frequency` keeps accepted live item
increments in a sparse in-memory map, with one cached running total per
touched library. It borrows the effective item's presence without cloning
its readings. `RuntimeDict::add_unigram_delta` delegates these increments
and does not also enter them in `FacadeBumps`. Count readers add these live
values while holding the live-count guard before the database/cache guards,
so a flush cannot split a read across old rows and a cleared overlay. Refused additions live
in the existing facade offset, outside the item sum rebuilt on replacement.
Before another count writer, replacement, compaction or the save diff, the
accepted values are folded into the existing UNIGRAM rows in one transaction;
this does not mark the context dirty. Thus training guards see all prior
increments, replacement removes accepted values with the old sub-index, and
save carries them without a store write for each exported call. Live reading
entries remain authoritative, including duplicate keys with different counts.

Compact and save use the wrapping sum of original item frequencies,
independent of an inconsistent stored header. Each immutable `PhraseLibrary`
shares a lazily initialized item sum across mapping clones; inline originals
are summed separately, replacing overlapping mapped slots. This scans at
first compaction, with no startup materialization. The #866 current-disk
baseline remains the old side of `diff_records_against`. Reopen seeds wrapping
differences, and unigram pricing wraps the merged u32 field.

The pin modifies a mapped item in O(1) time and space per call. The sparse
live count map uses expected O(1) count updates and O(k) space for k touched
items; checking presence in the live override tree costs O(log r) for r
replayed/imported items, followed by a constant-time mapped lookup if needed.
The first addition to a library computes its current total from stored rows;
subsequent additions reuse it until a count writer flushes the live values.
A flush makes O(k log n) ordered row updates once, rather than per call.
The immutable mapped sum costs O(n) once per mapping, then O(1) per compact
apart from inline fixture overrides. Save retains the existing sparse diff.
No startup scan, reading-row seeding, or candidate-path allocation is added.

### Compact Zhuyin experiment (2026-10-18 UTC; rejected)

The pinned global `zhuyin_index` uses spelling pointers, flags and canonical
content IDs (`src/storage/pinyin_parser_table.h:1492` at 074a2219).
`search_chewing_index` uses strcmp/equal_range, gates flags and reads the key
from content (`src/storage/zhuyin_parser2.cpp:44-101`). The Rust prototype
keeps the sorted 1493-row inventory and partition_point, but stores immutable
UTF-8 pool spans rather than three fat string slices; canonical rendering
keeps the original first-match scan. Both search complexities are O(log n)
with bounded-length comparisons; rendering remains O(n). No runtime table
allocation or initialization. See [the prototype findings](compact-zhuyin-prototype.md) for source
verification, alternatives and measured rejection: Simple lookup +21.7% and
CP26 +18.8% instructions despite 320 KiB paired stripped-size saving. The
prototype was reverted; the original representation remains in production.

Under #864, binary file savings cannot offset a runtime regression: file
size is governed separately by Constitution §2. Heap use and allocation
counts did not improve meaningfully, so the runtime time/space rule rejects
the increased parser instruction cost.

### Tool record splitting (#926, 2026-10-25 UTC)

The pin's segmented reader (`utils/utils_helper.h:51-69` at `074a2219`)
uses a two-field space/TAB split, an integer prefix and exact nonzero-token
phrase validation before `gen_ngram.cpp:82-117` updates model counts.
The classic frontend instead limits the split to three fields
(`PYLibPinyin.cc:254` at `2d2cdac`). GLib 2.89.4
`gstrfuncs.c:2489-2551` scans bytes using a delimiter table, copies each
field and reverses its temporary list into an array: O(n) time and O(n)
auxiliary bytes for either fixed field limit.

Rust uses `str::split_once` and `str::splitn(3, [' ', '\t'])` respectively,
borrowing fields and retaining empties/tails, with O(n) time and O(1)
auxiliary space. Its unsigned numeric-prefix helper now borrows a prefix
instead of constructing a `String`. No observable behavior is changed;
the existing numeric-domain and phrase-validation gaps are not excused by
this equivalent refactor. The [tool audit](926-tool-tokenizers.md) traces
callers, output contracts, classified partitions and remaining work.

## Scanner/tools native parsing (#1297; 2026-10-26 UTC)

Pin `table_info.cpp:208-287,338-359` uses five system/four marker directives,
then six-column library rows. Since external-I/O stage 1 (2026-10-28 UTC)
`oxpinyin-input` makes those `fscanf`, `feof` and `atoi` calls on the file's
stream, and `TableConf::load` and `UserTableInfo::load` keep the pin's
mappings, slots and refusals; the line-based Rust readers and their
[scanner records](1297-table-lines.md) are superseded. Both loads are O(n)
time in the file and O(1) space beyond the rows: the scanner keeps the pin's
fixed 256-byte buffers. Library slot vectors and persistent writers
retain their formats and ordering. Pin `utils_helper.h:23-35,57-69` feeds decimal
tokens/counts to the training tools; `numeric::decimal_field` is the one complete
field conversion for their domain types, O(field length) time and no successful
parse allocation. The [tool record](1297-tool-decimals.md) names the callers.
Punctuation remains O(words) time/space; chunk payload retries preserve O(bytes)
time and O(1) additional space, with the existing header/sync/rename contract.

## Point-search pronunciation records (#1031; 2026-10-27 UTC)

Read from the libpinyin tree at `074a2219`: candidate guess calls table search
in `src/pinyin.cpp:2233-2253` and `src/zhuyin.cpp:1499-1510`. The BDB adapter
borrows the returned row (`src/storage/chewing_large_table2_bdb.cpp:182-191`);
`src/storage/chewing_large_table2.h:145-152` finds an equal range, then
`:101-106` compares packed stored pronunciations and reads matching tokens.
This work happens during guess, not init. For N records of L keys and M
matches, the pin searches in O(L log N + ML), with output range storage.

Rust `ChewingTable::search` keeps its linear filter and the owned DBM row
buffer, but `pinyin_index::item_views` validates the row and borrows its keys.
Only matching tokens are collected; `resolve_items` consumes those tokens.
Worst-case time remains O(NL), while temporary space falls from O(B + NL)
to O(B + M), for B row bytes. No key vector is allocated per record, and
no work moves into init. Other consumers that need owned pronunciations
still use `decode_items`, over the same validated record views.

## Ordered bulk user-phrase reads (2026-10-29 UTC)

Pin sources read with `git show 074a2219:<path>` in
`/home/sheng/repos/libpinyin`: the phrase index stores token-indexed item
slots (`storage/phrase_index.cpp:624-645`), and each item's packed readings
retain insertion order (`:56-91`). The export iterator walks increasing tokens
and then each item's reading positions (`pinyin.cpp:662-687`, `:698-768`).
It needs no hash join between a phrase and its readings. Its traversal is
linear in the token range
and stored readings, with output storage proportional to the returned data.

oxpinyin's `UserStore::phrases` and `UserStore::export_phrases_in`
(`crates/oxpinyin-user/src/store.rs`) read separate PHRASE and PRONUNCIATION
tables. Both keys begin with the same four big-endian token bytes
(`codec::encode_token`, `codec::encode_token_bytes`). The store's `table\0key`
framing preserves that order. BDB's framed store uses a default-comparator
DB_BTREE, KC uses TreeDB's lexical comparator, and tkrzw uses TreeDBM's default
LexicalKeyComparator; each walk advances its ordered cursor. SessionValues'
BTreeMaps and the transaction scans preserve the same order. Extra key tails
can produce duplicate decoded phrase tokens, but those tokens remain adjacent.

The former join built a randomly seeded membership HashSet and a grouped
BTreeMap. It never iterated the HashSet for output: seeds changed probe cost,
while the ordered walks and sequence sort determined candidate and export
order. A fixed hasher would retain the extra index and collision behavior;
an ordered membership set would add logarithmic lookup cost and allocation.
The merge instead uses the ordering already guaranteed by every store. The
startup controller's RandomState allowlist is unchanged.

`collect_phrase_rows` materializes the phrase output first, preserving phrase
validation before pronunciation validation and library filtering before text
decoding. `collect_pronunciations_for_tokens` advances one phrase cursor while
walking the pronunciation table once. It fills each matching output row
directly, including every duplicate phrase row. No HashSet or grouped side map
is needed. A reading's sequence and original scan position travel in its output
buffer; `sort_unstable_by_key` on that pair preserves the former stable sequence
order, including duplicate decoded readings and ties, without sort scratch.
The join costs O(P + R + E), where E counts emitted readings including duplicate
phrase multiplicity. The existing per-phrase sequence ordering costs
O(sum(r_i log r_i)); no stronger bound is claimed for malformed unbounded groups.
Storage is O(P + E + returned text/key bytes), without an auxiliary join index.
The std owned-iterator conversions consume the output buffers.

The write-side `collect_pronunciation_keys_from_txn` likewise merges the
nondecreasing tokens selected by `mask_phrase_rows` with one pronunciation walk.
Repeated selected tokens do not duplicate the returned raw keys. Empty
selection skips the pronunciation scan; otherwise all pronunciation keys are
validated, even after the final selected token, and unselected values remain
undecoded. Unit tests cover these rules, legacy values, odd truncated key tails,
corrupt keys/text/values, duplicate phrase tokens and duplicate readings. This
changes the join's cost, not its candidate, export or persisted-file order.

### Measured result and accepted trade-off (2026-10-30 UTC)

The preserved native shipping baseline was built from `fcabbbf9`; later
documentation-only rebases do not relabel that baseline. The candidate's
shipping Rust sources and six generated function-order policies are unchanged
from the measured candidate. Both facades used release shipping builds; the
native measurements used BDB, Ubuntu 26.10 x86_64, CPU 0, eager binding and
ext4, with the same pin-provisioned model20 profiles and system data per arm.
The diagnostic controller and worker's measured C source did not change the
shipping objects. The pin comparisons were separate debug compatibility runs.

In the cold campaign's modest first-candidate scope,
`UserLookup::from_store` took 8,350 Ir in every candidate and same-bytes control
sample (five each), against 10,454–10,466 on base. All twelve instruction
checks continued under the existing BDB/RandomState diagnostic rules. The
first-candidate and steady attributions reported no functions outside the
unchanged RandomState allowlist; BDB init remained diagnostic.
The selected contract cases were 21 MATCH on each of BDB, KC and tkrzw.
Empty, modest, phrases and journals completed. Of forty phase timing results,
twelve were STABLE and twenty-eight INCONCLUSIVE, including drop/fini on the
populated profiles. No slowdown was detected, but the inconclusive controls do
not disprove one; this is not a no-regression result.

The separately authorized, single cold modest memory-only run retained full
per-mapping smaps at M0 (after load) and I1 (after init). Mean PSS in KiB:

| Mapping class | Base M0 to I1 | Candidate M0 to I1 | Candidate minus base at I1 |
| --- | ---: | ---: | ---: |
| Named `[heap]` | 88 to 244 | 88 to 244 | 0 |
| Other anonymous VMAs | 60 to 2,608 | 60 to 2,608 | 0 |
| Library executable mapping | 184 to 744 | 192 to 788 | +44 |
| Library read-only mappings | 120 to 268 | 120 to 288 | +20 |
| Library writable mapping | 4 to 4 | 4 to 4 | 0 |
| System-data mappings | 0 to 4,132 | 0 to 4,132 | 0 |
| User-file mappings | 0 to 0 | 0 to 0 | 0 |
| Other libraries, worker, stack/kernel | 294.33 to 1,580.67 | 294.50 to 1,580.83 | +0.17 |
| Sum of mappings | 750.33 to 9,580.67 | 758.50 to 9,644.83 | +64.17 |

Mapping sums reconcile exactly per sample before averaging; rounding the
displayed means can affect their sum. The separately read smaps_rollup PSS
exceeds the sum by 8–10 KiB, retained as a diagnostic rather than assigned to
a mapping. Anonymous/COW pages inside file-named VMAs remain in those mapping
classes, distinct from `[heap]` and other anonymous VMAs.

The library's I1 increase is exactly 64 KiB in every retained round: 44 KiB
in the executable mapping at file offset `0x11000` and 20 KiB in the read-only
data/unwind mapping (base `0x1fb000`, candidate `0x1f4000`). Both growing
mappings have Anonymous=0; headers, RELRO and writable pages are unchanged.
Eight KiB is already present at M0, so the excess M0-to-I1 growth is 56 KiB
(paired CI95 [56, 56]). Named heap and other anonymous PSS do not decrease at
init; these checkpoints precede the first-candidate join. The total paired
I1 median difference is 64 KiB (CI95 [63.5, 65]); the earlier rollup-only
67 KiB difference is not reproduced exactly and its remaining 3 KiB cannot
be assigned retrospectively. All thirty retained rounds per arm qualified
cold, with zero retained rejections; one initial warmup per arm was rejected
and excluded. Output was COMPARABLE and residency controls STABLE.

The measured trade is deterministic instruction cost and no first-candidate
hash join against increased library residency at init. The already regenerated
function-order policies are the policies measured above and did not recover
that increase. A separate policy-tuning experiment was not performed; whether
a different policy could recover it remains untested. The maintainer accepted
this trade on 2026-10-30 UTC. User-phrase representation work in #1418 will
replace this path. The native commands and the memory-only smaps capture are
documented in `docs/runbooks/benches.md`; policy regeneration and pin profile
provisioning are in `docs/packaging.md`.

## Stage 5 table generators (2026-10-30 UTC)

Read from `/workspace/eio-env/libpinyin-src` at 074a2219 before cases ran.
`utils/storage/gen_zhuyin_table.cpp:45-49,101-138` uses the same GLib
options as the pinyin utility, including its "pinyin" help description.
`:140-164` opens C++ ifstreams, reads getline, splits on single spaces
at most twice, calls atoi, then checks field count. `:168-195` counts
characters, rejects length >= 16, converts UCS-4, and parses direct zhuyin
under USE_TONE | FORCE_TONE. `src/storage/zhuyin_parser2.cpp:858-958`
defaults to tone 1, consumes apostrophe/space-separated keys, skips repeated
separators, and stops at the first unparseable key. Rendering omits first
tone and appends the other tone symbols (`chewing_key.cpp:74-89`).

The phrase tree compares length then native-endian UCS-4 bytes
(`gen_zhuyin_table.cpp:90-98`); readings retain insertion order, duplicates
add wrapping u32 frequency (`:197-237`). Stable per-length sorting uses
the first reading's exact key ordering (`:241-262`). Output opens after
all inputs and the node count, assigns one token per phrase, floors
frequencies at 3, and uses fprintf/fclose (`:267-334`).

`oxpinyin-utils::gen_pinyin_table::Table` shares that tree, stable sort
and output loop with `gen_zhuyin_table`. Each utility supplies its parser
and renderer. BTreeMap uses the same phrase order; lookup is O(L log N),
reading duplicate search O(R L), sorting O(N log N L), space O(N R L),
as for the pin (L < 16, R readings per phrase, N distinct phrases).
The boundary keeps native options, C++ streams, GLib text calls and C
formatted writes in their source order. Undefined reads follow the two
registered class (b) records; no crash or lost buffered output is emulated.
