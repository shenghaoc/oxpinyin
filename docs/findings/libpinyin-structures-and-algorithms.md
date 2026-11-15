# libpinyin data structures and algorithms, and oxpinyin's counterparts

Status: **register** · Pin: libpinyin `074a2219` (2.11.92)

This [source-policy record](../source-policy.md) compares the pin and oxpinyin
by subsystem. Pin citations are `file:line` under `src/` at `074a2219`; oxpinyin citations name files and items.

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
    but not yet argued (listed under [Open](#open));
  - **class (a)** or **(b)**, **S1**–**S3**, or a named record — a difference the
    [compatibility policy](compatibility-policy.md#the-default-is-the-pin) permits;
  - **defect** — known to differ with no permission to cover it;
  - **off path** — the code does not reach the parity path, and the row
    says why.

The **parity path** is code whose result reaches either C ABI —
libpinyin's 79 `pinyin_*` exports or libzhuyin's 52 — or the files
they write, and the offline tools whose output is promised identical
to the pin's (`oxpinyin-utils`' pin-named utilities and the system files
they write).

A row is a survey result, not a proof of equivalence. Rows whose
behaviour is unverified, and the open gaps, are listed under [Open](#open);
the [roadmap](../../ROADMAP.md) tracks the work that closes them.

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

The API keeps all of its state in two objects (`pinyin.cpp:39-95`):

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

The split is not complete. The decoders are in the context, but they
keep per-call working state there: the trellis, the cached keys and
phrase item, and pointers to the matrix and constraints of the calling
instance (`lookup/phonetic_lookup.h:522-532`). Every instance on one
context therefore shares one decoder and its scratch. Two instances on
one context cannot decode from two threads at the same time.

oxpinyin keeps the same two objects at the ABI and moves the decoder
state to the instance side:

| Pin | oxpinyin |
| --- | --- |
| `pinyin_context_t`: options, parsers, dictionaries, bigrams, user state, save flag | `ContextCore` (`oxpinyin-facade/src/context.rs`): the `SessionSettings` instances open with, the assembled `Runtime` (tables and model), the `UserStore` (whose shared dirty flag is `m_modified`), `LiveOptions`; no parser objects — parsing runs per call in `oxpinyin-core` |
| `pinyin_context_t`: the two decoders and their scratch | `Session` lookup state and composition scratch per instance, with the sentence trellis built per guess (`oxpinyin-engine/src/session/mod.rs`, `oxpinyin-engine/src/nbest.rs:849-871`) |
| `pinyin_instance_t`: matrix, constraints, n-best, phrase result, candidates, sort option | `InstanceCore` (`oxpinyin-facade/src/instance.rs`) holding a `RuntimeSession`, whose `Session` owns the input and parsed matrix, the `SelectionRecord`, the `ConstraintStore`, the sentence (n-best) state, the `CandidateList` and the sort word; the phrase result sits on `InstanceCore` |
| back pointer from instance to context | one retained context owner. Each operation borrows its runtime. The Pinyin instance also keeps the public context pointer for `pinyin_get_context` (`oxpinyin-capi/src/state.rs:259-271`). |

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
select, `prepare_training` and its `observe` plan, `phrase_segment` — with save on
`ContextCore::save_user` (`oxpinyin-facade`). Both C ABIs drive only
these.

### How oxpinyin divides it

oxpinyin keeps the pin's layers but makes each a crate with explicit
dependencies (`docs/architecture.md` has the full map):

| Pin layer | oxpinyin crates |
| --- | --- |
| Base (`src/include/`) | `oxpinyin-core` (keys and content tables), `oxpinyin-data` (`chunk_format`, the `MemoryChunk` reader) |
| Storage: keys, parsers, matrix helpers, `SingleGram` | `oxpinyin-core` |
| Storage: system tables and their file formats | `oxpinyin-data` |
| Storage: the DBM backends | `oxpinyin-store`, one backend per build by cargo feature behind one ordered byte-KV seam |
| Storage: user tables and user files | `oxpinyin-user` |
| Lookup, and the output logic in `pinyin.cpp` | `oxpinyin-engine` |
| Wiring done in `pinyin_init` | `oxpinyin-runtime` |
| Context and instance state shared by both APIs | `oxpinyin-facade` |
| `pinyin.cpp` / `zhuyin.cpp` as ABI | `oxpinyin-capi`, `oxpinyin-zhuyin-capi`, sharing `oxpinyin-capi-marshal` |
| `utils/storage`, `utils/training`, `utils/segment` | `oxpinyin-utils` (the pin-named binaries and the `k_mixture_model` operations) |

The differences are deliberate:

- the backend is a seam, not an `#ifdef`;
- the two ABIs share one facade, not two API files;
- the logic that decides output is in `oxpinyin-engine`, apart from the
  ABI glue.

## 1. Storage

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Phrase item | `PhraseItem` (`storage/phrase_index.h:64`): `{u8 len, u8 npron, u32 unigram, ucs4[len], {ChewingKey[len], u32 freq} × npron}` in a `MemoryChunk` | `PhraseItemView` and `PronunciationView` in `oxpinyin-data/src/phrase_library.rs`, borrowed views over the same bytes | same / follows |
| Sub phrase index | `SubPhraseIndex` (`storage/phrase_index.h:257`): header (total freq), offset array indexed by `token & PHRASE_MASK`, content area; file `mmap`ped `PROT_READ\|PROT_WRITE, MAP_PRIVATE` (`include/memory_chunk.h:505-506`); O(1) token → item | `PhraseLibrary` (`phrase_library.rs`), `item`, `slot_offset`; the same files mapped read-only; O(1) token → item | same / follows (reads) |
| Phrase index edits | in place on the private mapping: `PhraseItem::add_pronunciation` (`storage/phrase_index.cpp:56`), `SubPhraseIndex::add_unigram_frequency` (`:150`) | journal MODIFY writes into a separate `PhraseContent` private mapping; `SystemItems` retains completed replay views and caches `ChunkItem`s on first use; later imports/training use those decoded items and saves rebuild the pin's chunk format | different / argued (journal replay, below) |
| Facade phrase index | `FacadePhraseIndex` (`storage/phrase_index.h:438-451`): 16 sub-index pointers and a running `guint32` total | `PhraseLibraries` (`oxpinyin-data/src/phrase_libraries.rs:54-69`): 16 slots and a baseline total, with live totals and a facade adjustment in `oxpinyin-user/src/system_items.rs:33-43` | different / [defect](#configured-user-file-dispatch) |
| Pinyin key → tokens | `ChewingLargeTable2`: DBM keyed by projected packed `ChewingKey[len]`, values of `PinyinIndexItem2` arrays (`storage/pinyin_phrase3.h:146-195`). The entry locates the projected range, filters the entries and compresses tokens (`storage/chewing_large_table2.h:91-152`). | `ChewingTable` (`oxpinyin-data/src/chewing_table.rs:334-370,486-503`) probes the same key and filters in stored order. The sentence path reuses the DBM buffer and decodes owned records before range compression. | different / defect ([tkrzw read](#chewing-table-tkrzw-read), [Kyoto Cabinet checked read](#chewing-table-kyoto-checked-read)) |
| User pinyin table | the user DBM loaded into an in-memory DB (`storage/facade_chewing_table2.h:87`, `load_db`) and searched like the system table | `UserLookup` (`oxpinyin-user/src/lookup.rs`) borrows the session's canonical `UserIndex`; packed records and independent prefix markers live in the backend’s ordered in-memory tree, written back at save | different / argued (lookup ownership, below) |
| Phrase string → tokens | `PhraseLargeTable3` per backend; system `attach`, user `load_db` (`storage/facade_phrase_table3.h:81`, `:86`) | system: `PhraseTable` (`oxpinyin-data/src/phrase_table.rs`); exact user lookup: `UserStore::phrase_table_tokens_for_text` (`oxpinyin-user/src/store.rs`), with one queried row cached by `RuntimeDict::tokens_for_text` | different / argued (exact lookup and lookup ownership, below) |
| Bigram row | `SingleGram` (`storage/ngram.h:59`): `MemoryChunk` of `{u32 total}` then sorted `SingleGramItem {token, freq}` (`storage/ngram.cpp:31`) | free functions over the same bytes in `oxpinyin-core/src/single_gram.rs` (`lower_bound`, `insert_freq`, `mask_out`, `merge_single_gram`) | same / follows |
| System bigram | `Bigram::attach` read-only on the DBM (`storage/ngram_bdb.h:88`), one `load` per row | `BigramTable::load_raw_successors` (`oxpinyin-data/src/bigram_table.rs`) | same / follows |
| User bigram | `Bigram::load_db` into the backend's in-memory container (`storage/ngram_bdb.h:67`) | `UserBigramDb` (`oxpinyin-store/src/lib.rs`): the pin's container per backend through the real library | same / follows |
| Training-only n-grams | `FlexibleSingleGram` / `FlexibleBigram` on a DBM, used by `utils/training` and `tests/storage/test_flexible_ngram.cpp` | `SingleGram` / `FlexibleBigram` in `oxpinyin-store/src/flexible_ngram.rs`, native DBM records consumed by nonshipping `oxpinyin-utils` | same container layout; attach, magic-header reads and tools-only KC mutation follow backend twins ([record](upstream-divergences.md#utility-check-macros-write-no-message)) |

For a correctly projected row, each retrieved record falls in the pin's
`equal_range`. Both filters keep the same entries, including duplicates, in
record order before library grouping and range compression. A present empty
value permits continuation in both.

Flexible attach follows `src/storage/flexible_ngram_tkrzwdb.h:128-187`,
`flexible_ngram_kyotodb.h:129-179` and `flexible_ngram_bdb.h:94-152`. Both
implementations make at most two native open calls and one signature read or
write. Checking the signature copies O(v) bytes for a value of length v on
KC/tkrzw, as the pin does, and at most four bytes through BDB's partial read.
Retaining a refused handle adds no second live DBM. The KC magic-header read
uses the CXX `accept` visitor with a bounded sink, copying at most 4 + H bytes
for a header of size H and checking the native returned size
(`flexible_ngram_kyotodb.h:282-308`). BDB reads H bytes at offset four (`flexible_ngram_bdb.h:302-328`);
tkrzw reads the whole value before checking its size
(`flexible_ngram_tkrzwdb.h:292-323`). Tools-only KC writes call native
`set` without transactions; reset synchronizes at
`flexible_ngram_kyotodb.h:88`. Shipping store transactions are unchanged.

`SingleGram::search` starts at the existing lower bound and appends in
stored order until the range end (`flexible_single_gram.h:118-139`). For
n stored items and k emitted items its time is O(log n + k), with O(k)
output storage, matching the pin. Unsorted records use that same lower
bound under [the existing record](upstream-divergences.md#flexiblebigram-unsorted-record-search).

### User lookup ownership

`UserStore::lookup` borrows the session's phrase tables. Each query uses the existing `UserIndex` or `UserPhraseTable` search over that borrow. Matched text comes from the library arena
through `PhraseItemView::phrase_text`; it is not cached for every token.
`RuntimeDict` holds no second set of tables. Both facade prediction
paths acquire this borrowed view.

The pin likewise searches its live user tables
(`storage/facade_chewing_table2.h:109-129`,
`storage/facade_phrase_table3.h:120-126,141-153`).
`pinyin.cpp:1389-1422` searches each prefix suffix; `:2373-2405` walks
suggestions and resolves their live items. The existing search methods keep
record order, duplicates, empty markers and malformed-row refusals. Search
side effects read the same canonical record. Full token identity is preserved when resolving
text, including rejection of high-bit aliases. The in-tree callers do not
consume an old snapshot's results after a write.

Acquisition takes O(1) time and space. For N live items, resolving M matched
texts costs O(M log N + output words) in the worst case: at most 16
libraries, then a dense slot read or sparse O(log N) slot lookup, and text
decoding. Prediction's suffix search runs at most 16 times.

### Journal replay

Journal replay follows `SubPhraseIndex::merge`
(`storage/phrase_index.cpp:439-517`), called by `pinyin.cpp:250-276` and
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
only relocated/removed offsets enter a sparse map. The loader validates and
walks one journal byte buffer without building a vector of records; mask
snapshots keep the owned-record path. Session opens retain completed
original-item views after constant-time structural validation of each
payload's extents. The checks match `decode_phrase_item`: both header bytes,
the unigram, text, and the first incomplete pronunciation's keys or frequency.
Extra trailing bytes and arbitrary scalar values stay accepted. A failure
records the eager diagnostic and leaves the baseline slot. Journal preflight
checks record heads, tokens, lengths and payload extents without constructing
records. `SystemItems` caches decoded items on first use; added items seed the
text/index rows, removals the removal set, and original items seed only
necessary unigram deltas. Replayed reading rows are never synthesized.

Replay appends changed slot IDs to a vector and sorts and deduplicates it once
at finish; lazy validation filters rejected slots in one `retain` pass.
Completed views use O(log J) binary search for membership. A second vector
holds slots whose unigram bytes may change (ADD, REMOVE, relocation, partial
unigram payloads, stale writes, and overlapping cross-item writes found during
validation). Seeding visits only those candidates, compares their final
unigram with the immutable baseline and writes nonzero deltas. The per-library
absolute total and offset calculation carry header changes independently.

For B journal bytes, R touching records, J unique changed slots, M relocated
slots and K items later used, worst-case replay time is
O(B + R log R + R log(M+1)), against the pin's O(B + R) mapped walk. Transient
slot metadata is O(R + M), within the O(B) loader space bound; after filtering
it retains O(J + M), plus typed payloads for the K used items. Bulk save and
mask operations may materialize all changed items. The private view shares
clean file pages; MODIFY dirties private pages as the pin does, without
writing the system file.

### Exact-text lookup

Exact-text lookup reads the system row, then the
live user row. `RuntimeDict::tokens_for_text` keeps stored multiplicity
and order within each library. Its stable library sort implements
`reduce_tokens` (`storage/phrase_large_table3.h:77-91`). The pin searches
both tables (`storage/facade_phrase_table3.h:120-126`) and appends enabled
tokens (`storage/phrase_large_table3.cpp:29-51`).
The BDB search makes one keyed get (`storage/phrase_large_table3_bdb.cpp:196-205`).
Each Rust user query makes one keyed get. New tokens, deletes and masks
change the native records. A reading merge edits the item without calling
`add_index` (`pinyin.cpp:563-582`), while a new token calls it (`:596-599`).
The two user indexes use the pin's BabyDBM, ProtoTreeDB or in-memory DB_BTREE.
Mask visits and rewrites records in native walk order using the library's
visitor or cursor. It retains only the current replacement and the
backend-specific entry scratch state, including the carried tail (#785).

## 2. Parsing

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Key | `ChewingKey` (`storage/chewing_key.h:41-48`): 16-bit bitfield 5/2/5/3/1 | `ChewingKey` packs the same fields at the ABI and on disk. The engine uses `SyllableKey`, a dense ID or a tagged packed key, with a separate tone (`oxpinyin-core/src/vocab.rs:48-58,105-129,212-220`). | same / follows at the ABI and on disk. Different / [defect](#remember-packed-alternate) on the engine-to-store path. |
| Content and key tables | `content_table`, `chewing_key_table` (`storage/pinyin_parser_table.h`) | copied in `oxpinyin-core/src/chewing_key_data.rs` | same / follows |
| Hanyu pinyin index | sorted `pinyin_index` (`storage/pinyin_parser_table.h:9`), 662 rows including correction rows, each with flags and a distance; `equal_range` per probe, O(log n) | `phf` maps generated by `oxpinyin-core/build.rs`; correction spellings recomputed by `option_alias_canonical` (`oxpinyin-core/src/syllables.rs`); O(1) per probe | different / argued — each edge carries the matched row's distance (`SyllableKey::from_option_text_with_distance`: 0 for an exact spelling, 1 for an admitted alias); see [distance rule](#full-pinyin-distance-rule) |
| Full pinyin parse | forward DP, one `parse_value_t` per byte (`storage/pinyin_parser2.cpp:64`, `:217`); spans up to `max_full_pinyin_length = 7` (`:82`); a step is replaced by a longer parse, else fewer keys, else lower total distance (`:296-310`); `final_step` backtracks (`:346`); O(7n) time, O(n) space | `SegmentGraph::build_with_options` builds the whole lattice (`emit_edges`), then `SegmentGraph::fewest_keys` selects (`oxpinyin-core/src/graph.rs`); O(7n) time, O(7n) space | different / argued — `fewest_keys` ranks a step by (key count, total distance) and replaces it only on strictly less, the pin's three rules; see [distance rule](#full-pinyin-distance-rule) |
| Double pinyin | greedy longest match, `DoublePinyinParser2::parse` (`storage/pinyin_parser2.cpp:532`), `parse_one_key` (`:405`) | `parse_with_options`, `parse_one_key` in `oxpinyin-core/src/scheme.rs` | same / follows |
| Zhuyin keyboards | `storage/zhuyin_parser2.cpp` (DaChen CP26 at `:573`) | `oxpinyin-core/src/zhuyin_map.rs` and the scheme parsers | same / follows |
| Direct parsers | `PinyinDirectParser2::parse` (`storage/pinyin_parser2.cpp:623-715`) and `ZhuyinDirectParser2::parse` (`storage/zhuyin_parser2.cpp:858-960`) probe a whole segment, append its key/raw begin/raw end, skip consecutive spaces/apostrophes and return the last reached byte; first failure stops | `oxpinyin-core::direct::parse` shares that walk with the shipping `parse_zhuyin_direct`; tools expose its consumed count and spans and reuse `FullPinyinParser::parse_one_key` / `parse_zhuyin_direct_key` | same / follows; the nonshipping `test_parser2` cases exercise both direct parsers and their incomplete option |

The nonshipping parser driver (`tests/storage/test_parser2.cpp:37-147`) uses the same `SegmentGraph` selection and `apostrophe_extended`
helper as the C API's `Session::full_parsed_len`. It takes keys before the
scan-matrix transformations: the pin disables parser `post_process2` at
`pinyin_parser2.cpp:377-379`. Double-pinyin and keyboard zhuyin use the
existing `parse_with_options` results directly. Tool-only accessors are
behind `tools`; shipping parsing has no additional result allocation.

For n input bytes and k returned keys, the shared direct walk visits each
segment and separator once: O(n) scanning and O(k) output, with the pin's
single-key parser work. The pin's bounded inventories add
O(m log T) work across m segments (T is the fixed table size); Rust uses
the existing frozen lookup maps. Full-pinyin selection keeps the O(7n)
time/space bound above and copies k selected keys/spans; keyboard parser
bounds are unchanged. The driver's 1000 parse calls and the `gettimeofday`
and `printf` calls are the pin's protocol (`tests/timer.h:30-44`).

### Full-pinyin distance rule

Among steps with equal parsed length and equal key count, the pin's DP
prefers the step with the lower total distance
(`storage/pinyin_parser2.cpp:306-310`). [§9](#9-constants) lists the rows
that carry distance 1. Each oxpinyin edge carries the distance of its row,
and `SegmentGraph::fewest_keys` ranks by (key count, total distance). Without
the distance, `PINYIN_CORRECT_ON_ONG` gives `chon'ga`, `chon'gan` and
`zhon'ge` for `chonga`, `chongan` and `zhonge`, where the pin's rule gives
`chong'a`, `chong'an` and `zhong'e`.

## 3. Phonetic key matrix

Plain full-pinyin auxiliary text uses the option word from the last parse.
The pin stores a live context word separately from the matrix already filled
by a parse (`pinyin.cpp:1299-1306,1497-1524,3381-3438`). Rust reconstructs the
selected key path for this display and retains one option word to reproduce
the same generation after a context write or candidate refresh. The same
path supplies the empty-matrix and leading-separator checks. The existing
bounded O(7n) graph reconstruction and O(n) rendering bounds remain for n
consumed bytes.

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Layout | `PhoneticKeyMatrix` pairs the key and rest arrays at each column (`storage/phonetic_key_matrix.h:65-168`). | `ScanMatrix` is an ordered key buffer with column offsets. `ParsedMatrix` retains placed columns and zero flags (`oxpinyin-engine/src/session/matrix.rs:10-73`). | different / argued ([matrix representation](#matrix-representation)) |
| Fill | `fill_matrix` places the parser's raw spans and inserts zero keys after real keys and at the terminal column (`storage/phonetic_key_matrix.cpp:30-84`). | `ParsedMatrix::from_scan` places real keys and marks zero columns (`oxpinyin-engine/src/session/matrix.rs:76-119`). Index-parsed full pinyin also builds a decoder matrix over canonical spelling (`oxpinyin-facade/src/parse.rs:288-323`). | different / [open](#index-parse-separator-fill), shared cause with policy row 122 |
| Resplit and inner split | `resplit_step` (`:87`), `inner_split_step` (`:169`); linear scans of the `special_table.h` tables | `resplit_step`, `inner_split_step` (`session/mod.rs`), same tables (`matrix-split-tables.md`) | same / follows |
| Fuzzy | `fuzzy_syllable_step` (`:226-310`): an initials pass, then a finals pass over the grown column | `extend_matrix` and `parse_time_keys` run the two column passes; packed final alternates survive without a syllable-table row | same / follows |
| Column multiplicity | `PhoneticTable::append` (`phonetic_key_matrix.h:92-100`): a column is a bag | both matrix builders append every entry in pass order, without collapse | same / follows |

### Matrix representation

For a given filled matrix, each real entry keeps its key, rest and position
within the column. The flat buffer records empty columns with equal adjacent
offsets. The placed view represents each zero-only column as a hop to the
next byte. Traversal keeps the pin's order and multiplicity, and the ABI
reads the first key and rest as a pair. The empty, terminal and out-of-range
checks are at the ABI consumers (`oxpinyin-facade/src/cursor.rs:382-395,513-527`,
policy rows 63 and 66). The [index-parse fill](#index-parse-separator-fill)
changes the columns supplied to the decoder.

## 4. Sentence decoding

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Instance | `PhoneticLookup<2, 3>` (`pinyin.cpp:406-409`) and `<1, 1>` (`zhuyin.cpp:348-351`) | `NbestShape` selects the same pair per instance (`oxpinyin-engine/src/nbest.rs:56-100`, `oxpinyin-zhuyin-capi/src/state.rs:59-74`). | different / argued ([decoder shape](#decoder-shape)) |
| Trellis | `ForwardPhoneticTrellis` (`lookup/phonetic_lookup.h:181`): per step a token → node hash table and an array of `trellis_node<nstore>` | `Trellis` (`nbest.rs`): per step a token map plus an explicit insertion order, so iteration never depends on hashing | different / argued (`sentence-surface.md`) |
| Node store | `trellis_node::eval_item` (`lookup/phonetic_lookup_heap.h:56-81`) replaces the front when a newcomer beats it. The `<1>` specialisation checks the empty sentinel and then uses the raw predicate (`:89-122`). | `Node::keep` preserves front replacement and uses an explicit occupancy count (`oxpinyin-engine/src/nbest.rs:254-299`). | same / follows ([decoder shape](#decoder-shape)) |
| Comparator | `trellis_value_less_than` (`lookup/phonetic_lookup.h:66`) with `LONG_SENTENCE_PENALTY = log(1.2f)` (`:39`) | `node_loses_to` (`nbest.rs`): pinyin orders the adjusted `f32` possibility `poss − length × ln(1.2f)` (`adjusted_poss`), best first with NaN last (`ordered_poss_compare`); zhuyin retains the pin's predicate | different / S1/S2 of the [algorithm-precondition ruling](compatibility-policy.md#algorithm-precondition-violations); node front eviction retained |
| Beam | `get_top_results` (`:143`), `nbeam = 32` (`:37`), libstdc++ `make_heap` / `pop_heap` | `select_top` (`nbest.rs`): `select_nth_unstable_by`, then a sort of the kept prefix, on `selection_compare`; pinyin orders the adjusted `f32` possibility (`adjusted_poss`), then the earlier preselection batch position, independently of physical backtracking slots | different / S1/S2 comparator repair under the same ruling; std selection replaces the heap transcription ([inventory](glib-libc-inventory.md), item B) |
| Expansion | `get_nbest_match` (`:736`): bigram over every beam value (`search_bigram2`, `:578`), unigram over the beam head only (`search_unigram2`, `:540`) | `nbest_sentences_with_seeds_generation`, `widen_free_span`, `expand_entries` | same / follows |
| Tails and extraction | heap top-n, `g_ptr_array_sort` with `trellis_value_compare` (`:174`), which truncates the cost difference to `gint`; `extract_result` backtracks (`:370`) | `Trellis::tails` selects with `select_top`, then `tail_merge` stably sorts by raw `f32` possibility (`ordered_poss_compare`); `Trellis::extract` backtracks | different / S3 of the [algorithm-precondition ruling](compatibility-policy.md#algorithm-precondition-violations); raw possibility descending, equal possibilities stable, NaN last |
| Scores | `gfloat` running sum of natural logs: unigram step `:663`, bigram step `:692-693` | `f32` running sum of natural logs: `(f64::from(poss) + probability.ln()) as f32` (`expand_one`, `nbest.rs`); pre-log pieces (`NbestStepCosts`) from `nbest_step_costs_using` (`oxpinyin-data/src/lm/mod.rs`) | same / class (a) — Rust std `ln` (`compatibility-policy.md`, row 11); see [sentence scoring types](#sentence-scoring-types) |
| Constraints | `ForwardPhoneticConstraints` (`lookup/phonetic_lookup.cpp:61` onward); skip NOSEARCH, a single search for ONESTEP | `ConstraintStore` (`oxpinyin-engine/src/constraint.rs`) | same / argued — revalidation follows the pin's `validate_constraint` ([constraint revalidation](#constraint-revalidation)); its recursive f32 path sum clears only below f32::EPSILON (`phonetic_lookup.cpp:159-163`) |
| Phrase segmentation | `PhraseLookup` (`lookup/phrase_lookup.cpp:119`), one value per (step, token), `save_next_step` (`:348`) replaces on strictly better, `final_step` (`:382`) | `phrase_segment`, `Step::relax`, `Step::best` (`oxpinyin-engine/src/phrase.rs`) | same / class (a) — `f32` scores with Rust std `ln` |
| Per-keystroke work | Trellis rebuilt. The decoder loads and merges both bigram rows for each beam value and successful span (`lookup/phonetic_lookup.h:588-599,750-753,785-810`). | Trellis rebuilt. `NbestStepRow` reuses a row or miss per predecessor, step and user generation (`oxpinyin-engine/src/nbest.rs:1382-1389`, `oxpinyin-runtime/src/lib.rs:1464-1475`). | different / [defect](#predecessor-row-reuse) under the system bigram native-read ruling |

### Decoder shape

Both pairs satisfy `nstore <= nbest` (`lookup/phonetic_lookup.h:715`).
The runtime pair controls the same node and tail limits and adds no new
input or failure condition. Beam selection and tail order follow the
existing Pinyin and Zhuyin comparator rulings.

For the one-slot store, the zero-length seed occupies step 0
(`lookup/phonetic_lookup.h:244-265`). Expansion inserts into a later
step through a nonempty pronunciation path, adding the phrase length
(`storage/phonetic_key_matrix.cpp:350-373,534-551`,
`lookup/phonetic_lookup.h:649-667,677-697`). The pin's zero-length
sentinel therefore does not replace an occupied node during expansion.
Both stores accept the first value and then replace it only when the
same Zhuyin length and raw score predicate says it loses. Equal-length
ties and unordered raw score comparisons keep the incumbent.

### Guess-local ordinary-item facts

`SentenceFacts` (`oxpinyin-core/src/sentence_facts.rs`) shares one ordinary
live-overlay item read between runtime sentence pricing and scoring. The pin
reads length, unigram and pronunciation through one `PhraseItem`
(`lookup/phonetic_lookup.h:643–697`). The memo uses the trellis's
multiplicative u32 hasher, for probes only; range traversal and tie order do
not depend on it. User/imported, hidden, unreadable and non-overlay items
retain the existing resolution. A changed write generation clears the memo
and disables it for the rest of the guess; existing prices and trellis nodes
are not rebuilt.

For U eligible distinct tokens, item resolution is O(U) with expected O(1)
probes; pronunciation matching stays per path and scoring per occurrence.
Each priced range occurrence now sums all P paths of its span before the
epsilon check (`phonetic_key_matrix.cpp:534-600`,
`phonetic_lookup.h:643-697`). For R priced occurrences, the sum requires
O(R(V + PC)) work, instead of O(RC), for V visited matrix prefixes
and path-price cost C (including dictionary pronunciation matching).
The pin also walks the span for each transition. The existing path memos
remain span-local; more of them can now be populated. Recursive summation
adds O(d) stack space for matrix-walk depth d, including separator hops,
with d bounded by the span byte length and at most 16 nonzero keys.
No additional path collection is retained.
Actual time and peak-memory effects are unknown. The
memo adds O(T + B) temporary space for T distinct tokens entered (including
`None` fallbacks) and B copied item-body bytes, and survives no sentence
invocation. The facade total adjustment is computed lazily once per stable
guess.

## 5. Candidate list

| Item | Pin (`pinyin.cpp`) | oxpinyin | Mark |
| --- | --- | --- | --- |
| Window scan | `:2224` onward | `collect_window_scan`, `scan_parsed_paths` (`oxpinyin-engine/src/session/lookup.rs`) | same / follows |
| Frequency | `_compute_frequency_of_items` (`:1810`), `BIGRAM_FREQUENCY_DISCOUNT 0.1f` (`:33`), f32 expression truncated to `guint32` (`:1862`) | `candidate_frequencies` (`session/lookup.rs`), `amplified_frequency_with_bigram` (`session/mod.rs`) | same / follows, bit for bit |
| Sort | `compare_item_with_sort_option` (`:1678`) under GLib's stable sort | `rank_candidates` (`session/mod.rs`) over `rank_by_frequency`, under the [candidate-frequency comparator ruling](compatibility-policy.md#no-ported-external-library-internals) | same / follows |
| Longer candidate | `_prepend_longer_candidates` (`pinyin.cpp:1870-1928`), library-first reduction (`phrase_large_table3.h:77-97`) | `longer_candidate` (`session/lookup.rs`) keeps the maximum unigram; ties keep the earlier library, then the earlier path/token within that library | same / follows |
| Dedup | `_remove_duplicated_items_by_phrase_string` (`:2058`, frequency rule `:2128-2138`); combined zhuyin slices (`zhuyin.cpp:1607-1630`) | `dedup_ranked_by_text` (`session/mod.rs`) includes LONGER and all before-cursor slices; `prepend_nbest_rows` keeps sentence precedence | same / follows for valid phrase text ([argument](addon-choose-promotion.md), section 5); malformed text is [open](#open) |
| Prediction | `pinyin_guess_predicted_candidates` (`:2413`), filter 10 (`:2313`) | `oxpinyin-capi/src/predict.rs` | same / follows |

Offset-zero dynamic ranking retains the prefix tokens independently of
parsing. The pin's `_get_previous_token` (`pinyin.cpp:1711-1740`,
`zhuyin.cpp:1146-1175`) reads each current phrase item's stored length,
skips missing/unloaded items and chooses the first longest prefix, falling
back to sentence_start. `Session::previous_token` follows that scan;
`RuntimeDict::phrase_length_for_token` reads live item headers, not text
or a fallback to the original system item. Prefix sentence guesses and
Pinyin prediction replace the tokens before any lookup refusal; ordinary
sentence guesses and full reset clear them. Parsing preserves them.

For P retained tokens, replacement copies O(P) words and the scan performs
P item reads with O(1) auxiliary space, as the pin's scan of P item headers
does. The C producers bound P by 64: 16 suffix lengths, at most four tokens
per successful `reduce_tokens` (`pinyin.cpp:1404-1420`,
`phrase_large_table3.h:95`). Internal Rust callers have no such bound.
Nonzero offsets keep the decoded-result walk.

`order_window_tail` (`oxpinyin-engine/src/session/mod.rs`) skips its
stable library sort and token-membership set only when every row has a
token and the tokens are strictly increasing. This implies ascending
library fields and no duplicate tokens, so both operations are identities.
Otherwise it retains the existing stable sort and first-occurrence dedup.
The pin groups ranges by library while searching
(`chewing_large_table2.h:91-132`) and appends libraries in
ascending order, retaining each library's range encounter order
(`pinyin.cpp:1769-1791`). The guard preserves that order, including ties
and interleaved-library windows handled by the fallback.

For a window of `n` rows, the guard takes O(n) time and O(1) space; on
success it avoids the stable sort's O(n log n) time and the membership set's
O(n) space. The pin's append traverses 16 library slots and their ranges in
O(n + r + 16) time for `r` ranges.

## 6. Training and user phrases

### Live bigram export items

Each qualifying bigram pair reads both current phrase items (`pinyin.cpp:809-815`). The walk retains the active pair and its pronunciation rows. `get_next`
copies the current row before advancing the walk (`:904-910`), so an import between
calls affects the next pair load and leaves the buffered pair intact.

For a pair with P and Q readings and maximum reading length L, the walk renders
both items in O((P + Q) × L) time and forms the cross product in O(P × Q × L) time
and space, with two item reads per qualifying pair, as at the pin.

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Online training | `train_result3` (`lookup/phonetic_lookup.h:844`) | `TrainingPlan::observe` over `Session::prepare_training` (`oxpinyin-engine/src/session/selection.rs`), `trained_gram` and `train_pronunciations` (`oxpinyin-user/src/store.rs`), constants in `oxpinyin-user/src/seed.rs` | same / follows; the C adapters abort where the pin aborts, except on an unsorted user gram, where the pin's `set_freq` misses and aborts and oxpinyin trains and answers `true` (accepted; row 85, [record](upstream-divergences.md#stored-gram-partitions-training)) |
| Predicted choice | `pinyin_choose_predicted_candidate` (`pinyin.cpp:2591`) | `observe_predicted`, `observe_predicted_prefix` | same / follows |
| Add phrase | `_add_phrase` searches the phrase table, then merges a reading or allocates at the end of the sub-index range (`pinyin.cpp:514-611`). | `plan_add` preflights the lookup. `arena_add_phrase` uses `UserLibrary::token_end` or the live system range (`oxpinyin-facade/src/import.rs:35-91`, `oxpinyin-user/src/store_arena.rs:317-387,506-508`). | different / [defect](#configured-user-file-dispatch) |
| Remember input | `_remember_phrase_recur` (`pinyin.cpp:3578`), recursive matrix walk | `pinyin_remember_user_input` (`oxpinyin-capi/src/user_data.rs`) over `remember_walk` (`oxpinyin-engine/src/char_offset.rs`), the same walk over the parse-time matrix in every parse mode, with the pin's fuzzy alternatives (and, for the index-parsed full pinyin, its resplit and divided additions) built at the parse (register row 93) | same walk. The store conversion drops a [packed alternate](#remember-packed-alternate). |
| Remove user phrase | `pinyin_remove_user_candidate` (`pinyin.cpp:3726`) | `remove_user_phrase` (`store.rs`), same order; the store returns `Ok(false)` where the pin asserts and the C adapter aborts (row 81) | same / follows |
| Mask out | `pinyin_mask_out` masks user tables, reloads the system chunks and merges filtered journals (`pinyin.cpp:1224-1295`). | `ContextCore::mask_out` preflights faults and remaps libraries around the user-store mask (`oxpinyin-facade/src/context.rs:627-707`, `oxpinyin-user/src/store.rs:2379-2445`). | different / [defect](#mask-out-slot-bits) |
| Import end | `pinyin_end_add_phrases` (`pinyin.cpp:655-660`) compacts and marks modified. | `ContextCore::end_import` calls `compact_libraries` and marks modified (`oxpinyin-facade/src/context.rs:905-910`, `oxpinyin-user/src/store.rs:1358-1407`). | different / defect ([slot zero](#compact-slot-zero), [system slot aliases](#compact-system-slot-aliases)) |
| Export | Live token and pronunciation cursor and the bigram iterator with count greater than 68 (`pinyin.cpp:662-768,775-910`). | `PhraseExportCursor` and `BigramExportWalk` (`oxpinyin-facade/src/export_rows.rs:19-51,401-436,747-775`). | different / [defect](#export-packed-tone-assertion) |

## 7. Object model and persistence

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| Context and instance | The context owns the tables and lookup scratch, which instances borrow (`pinyin.cpp:39-95`). | Instances and iterators retain the context allocation through `Rc`. The instance owns composition state (`oxpinyin-capi/src/state.rs:259-283,333`, §0 above). | different / [boundary defect](#fini-native-handles) |
| Edits between saves | Mutations of in-memory chunks and DBs. Updating an imported reading removes and reinserts its item (`pinyin.cpp:563-582`). | Packed user arenas, live system items and native bigram storage. An exact reading update can write its count in place (`oxpinyin-user/src/user_library.rs:323-375`). | different / [defect](#import-aliased-reading-edit) |
| Init's library loop | Each default row in index order: judge it (`pinyin.cpp:388`), map the chunk (`:254-256`), merge the log (`:268-276`) or read the `USER_FILE` chunk (`:303-318`), then the next row; the bigram attaches after the loop (`:394-397`). `zhuyin.cpp:318-334`, `:198-220`, `:336-339` likewise. O(rows) steps, each linear in its chunk and log | `library_loop::run` over `TableConf::default_row_steps`, with the first fault in `LibraryLoopStop`. Same order and complexity; on a fault nothing past that row is touched | same / follows |
| Save | `pinyin_save` (`pinyin.cpp:1132`), `_write_files` (`:922`), rename, `mark_version` | `save_reporting` (`oxpinyin-user/src/persistence.rs`) with the pin's file set and rename order | same / follows |
| Aborts | asserts and `abort()` on caller-reachable input | `Result`, then the C adapters abort, except the accepted unsorted-gram training case (row 85, [record](upstream-divergences.md#stored-gram-partitions-training)) | same / follows |

### Exported unigram reads and adds

The default facade selects the library and slot with the token's masks
(`novel_types.h:41-44`, `storage/phrase_index.h:628-652`, and
`storage/phrase_index.cpp:153-155,185-187`). The getter
reads the item's `guint32` frequency. The adder rejects a missing sub-index
before moving the facade total. A loaded sub-index takes the facade bump
before checking its item, then checks the item before guarding its own
total. An accepted item increment wraps at `guint32`
(`storage/phrase_index.cpp:150-177`).

OxPinyin writes system and user frequencies in their live items. The same
fields reach getters, scoring, removal, and an armed save. The exported add
leaves the dirty flag alone (`pinyin.cpp:2836-2843`,
`zhuyin.cpp:1828-1835`). A later armed save stores the item
(`pinyin.cpp:978-981,1132-1147`). The store's existing facade accounting
preserves bumps that failed after finding a loaded sub-index. Each add uses
a bounded number of library, slot, and item-header accesses,
using the existing sparse or dense arena lookup. It retains no second
per-token map and requires no new per-item storage.

Save requires a user directory and the dirty flag, then compacts
(`src/pinyin.cpp:1132-1145`, `src/zhuyin.cpp:547-554`). Compact rebuilds
sub-index totals from items without rebuilding the facade total
(`src/storage/phrase_index.cpp:648-675`). Diff writes the old/live header
and compares complete items (`:392-436`); reload/unload removes the old
sub-index total (`:237-269`). Compact and save use the wrapping sum of item
frequencies, independent of an inconsistent stored header; unigram pricing
wraps the merged u32 field.

## 8. Offline tooling

| Item | Pin | oxpinyin | Mark |
| --- | --- | --- | --- |
| K-mixture model | `utils/training` (`k_mixture_model.h` and tools) | `oxpinyin-utils` `k_mixture_model` | same / argued ([training algorithm](training-algorithm.md) §7); abort sites reproduced ([record](upstream-divergences.md#kmm-tool-asserts-write-no-macro-text)) |
| System data | `gen_binary_files`, `import_interpolation`, `gen_unigram` | `oxpinyin-utils`' utilities of the same names, run in the pin's `data/Makefile.am` sequence | different / argued — byte-identical output |

## 9. Constants

Product coefficients take the pin's values. The tables record those values,
the pin's limits and their Rust counterparts, and limits introduced by
oxpinyin. Pin paths are relative to `src/` unless stated. Rust paths are
relative to `crates/`.

### The pin's values

| Coefficient or limit | Pin value | Pin site | oxpinyin site |
| --- | --- | --- | --- |
| `nbeam` | 32 | `lookup/phonetic_lookup.h:37` | `oxpinyin-engine/src/nbest.rs:102`, `oxpinyin-utils/src/training/eval/decode.rs:44` |
| Node store × n-best, pinyin | 2 × 3 | `pinyin.cpp:55,406` | `oxpinyin-engine/src/nbest.rs:52-54,72-75` |
| Node store × n-best, zhuyin and correction-rate evaluation | 1 × 1 | `zhuyin.cpp:50,348`, `utils/training/eval_correction_rate.cpp:161` from the repository root | `oxpinyin-engine/src/nbest.rs:78-80`, `oxpinyin-utils/src/training/eval/decode.rs:14-15` |
| `LONG_SENTENCE_PENALTY` | `log(1.2f)` | `lookup/phonetic_lookup.h:39,71-76` | `oxpinyin-engine/src/nbest.rs:185` |
| Sentence length window for the penalty | Difference of one character, for `nstore > 1` | `lookup/phonetic_lookup.h:66-88` | The adjusted score applies across lengths under [S1 and S2](compatibility-policy.md#s1-and-s2-one-ordered-adjusted-score). `oxpinyin-engine/src/nbest.rs:185,202-226` |
| Empty trellis score | `FLT_MAX` by default, `-FLT_MAX` for the single-slot node, whose empty check uses length 0 | `lookup/lookup.h:46-51`, `lookup/phonetic_lookup.h:54-61`, `lookup/phonetic_lookup_heap.h:95-116` | Explicit slot occupancy replaces the score sentinel. `oxpinyin-engine/src/nbest.rs:265-295`. See the [single-slot argument](#decoder-shape). |
| Pronunciation cutoff | `FLT_EPSILON` = 2⁻²³ | `lookup/phonetic_lookup.h:657,686`, `lookup/phonetic_lookup.cpp:162`, `pinyin.cpp:3172,3652`, `zhuyin.cpp:2127` | `oxpinyin-engine/src/nbest.rs:1317,1369`, `oxpinyin-engine/src/char_offset.rs:125`, `oxpinyin-utils/src/training/eval/decode.rs:386,422` |
| Unigram and combined bigram cutoffs | Unigram below `DBL_EPSILON` = 2⁻⁵². The bigram branch rejects if both the bigram is below 2⁻²³ and the unigram is below 2⁻⁵² | `lookup/phonetic_lookup.h:652,680`, `lookup/phrase_lookup.cpp:316,336` | `oxpinyin-data/src/lm/mod.rs:568-576`, `oxpinyin-utils/src/segment/trellis.rs:156,183`, `oxpinyin-utils/src/training/eval/decode.rs:382,418` |
| `BIGRAM_FREQUENCY_DISCOUNT` | 0.1f in pinyin, unit weight in zhuyin | `pinyin.cpp:33,1862`, `zhuyin.cpp:1264` | `oxpinyin-engine/src/session/mod.rs:455,499`, `oxpinyin-engine/src/session/lookup.rs:493-500` |
| Candidate frequency scale | 256 × 256 × 256 = 2²⁴ | `pinyin.cpp:1823,1840,1865`, `zhuyin.cpp:1267` | `oxpinyin-engine/src/session/mod.rs:461,501`, `oxpinyin-engine/src/session/lookup.rs:498` |
| Longer-candidate phonetic lookup | At most `2 * prefix_len` keys, capped at 16 | `storage/phonetic_key_matrix.cpp:452-458,512` | `oxpinyin-engine/src/session/lookup.rs:119-126` |
| Longer and predicted-prefix phrase length | At most `2 * prefix_len + 1` characters | `pinyin.cpp:1897,2395` | `oxpinyin-engine/src/session/lookup.rs:191`, `oxpinyin-capi/src/predict.rs:450,480` |
| Predicted bigram phrase lengths | 2, then 1 | `pinyin.cpp:2312,2345-2359` | `oxpinyin-capi/src/predict.rs:347-360` |
| Predicted bigram minimum count | 10 | `pinyin.cpp:2313,2351` | `oxpinyin-capi/src/predict.rs:26,390` |
| Training `initial_seed` | 23 × 3 = 69 | `lookup/phonetic_lookup.h:847` | `oxpinyin-user/src/seed.rs:14` |
| Training `expand_factor` | 2 | `lookup/phonetic_lookup.h:848` | `oxpinyin-user/src/seed.rs:17,42-47` |
| Training `unigram_factor` | 7 | `lookup/phonetic_lookup.h:849,928-929` | `oxpinyin-user/src/seed.rs:23,64`, `oxpinyin-user/src/store.rs:1545-1558` |
| Training `pinyin_factor` | 1 | `lookup/phonetic_lookup.h:850,925-927` | `oxpinyin-user/src/store.rs:1461-1467` |
| Training `ceiling_seed` | 23 × 15 × 64 = 22080 | `lookup/phonetic_lookup.h:851` | `oxpinyin-user/src/seed.rs:20,47` |
| Predicted choice seed × unigram factor | 69 × 7 = 483 | `pinyin.cpp:2597-2598,2609-2639` | `oxpinyin-user/src/seed.rs:53-64`, `oxpinyin-user/src/store.rs:1494-1495`, `oxpinyin-capi/src/candidates.rs:725` |
| Longer and without-sentence choice unigram increment | 69 × 7 = 483 | `pinyin.cpp:2504-2505,2523-2527,2565-2571` | `oxpinyin-capi/src/candidates.rs:433,465` |
| `_add_phrase` default count and unigram factor | 5 and 3 | `pinyin.cpp:521-523`, `zhuyin.cpp:407-409` | `oxpinyin-user/src/phrase.rs:41-46`, `oxpinyin-user/src/store_arena.rs:383` |
| Bigram export threshold | Count above `initial_seed - 1` = 68 | `pinyin.cpp:791-792,809` | `oxpinyin-facade/src/export_rows.rs:749,841` |
| Bigram export count multiplier | 2 | `pinyin.cpp:901-906` | `oxpinyin-facade/src/export_rows.rs:894` |
| `max_full_pinyin_length` | 7 bytes including tone | `storage/pinyin_parser2.cpp:82,269` | `oxpinyin-core/src/graph.rs:534-548`, `oxpinyin-core/src/full_pinyin_index.rs:53,247` |
| Parse-step distance | 1 on the 232 `PINYIN_CORRECT_*` rows and the two ambiguity rows `sua` and `zua`. 0 on the other 428 rows and all Luoma and secondary zhuyin rows | `storage/pinyin_parser_table.h:9-1491`, `storage/pinyin_parser2.cpp:289-309` | `oxpinyin-core/src/vocab.rs:141-151`, `oxpinyin-core/src/graph.rs:493`, `oxpinyin-core/src/full_pinyin_index.rs:175-183` |
| Double-pinyin probe window | 3 bytes. A key takes 1 byte if incomplete, 2 if complete without tone, or 3 with tone | `storage/pinyin_parser2.cpp:84,412-452,550-562` | `oxpinyin-core/src/scheme.rs:345,448-465` |
| Double-pinyin final and fallback probes | Two finals per key. The first successful final wins. Fallback compares two bytes | `storage/pinyin_parser2.h:76-78`, `storage/pinyin_parser2.cpp:473-517` | `oxpinyin-core/src/scheme.rs:149-152,489-511` |
| Double-pinyin auxiliary-text interior cut | 1 or 2 bytes into a key. Other interior cuts abort | `pinyin.cpp:3479-3488` | `oxpinyin-capi/src/aux_matrix.rs:298-302`, `oxpinyin-capi/src/text.rs:313` |
| Zhuyin keyboard windows | 4 keystrokes for Simple and Discrete, 12 for DaChen CP26, including tone | `storage/zhuyin_parser2.cpp:37-39,243,435,744` | `oxpinyin-core/src/scheme.rs:1188-1204` |
| DaChen CP26 repeat cycles | `(count - 1) % 2` for dual initials, dual finals and `m`. `(count - 1) % 3` for `u` | `storage/zhuyin_parser2.cpp:623,642,654,687` | `oxpinyin-core/src/scheme.rs:1538,1552,1562,1583` |
| Keyboard symbol counts | At most 2 per slot and at most 3 display entries before the terminator for Discrete and CP26 | `storage/zhuyin_parser2.cpp:158,534,841` | Fixed shipped tables bound the counts in place of the asserts. `oxpinyin-core/src/scheme.rs:1035-1068,1607-1616` |
| Tone digits and direct Zhuyin default | Digits 1–5. Direct Zhuyin defaults to tone 1 | `storage/pinyin_parser2.cpp:179,443,542,638`, `storage/zhuyin_parser2.cpp:865,882-889` | `oxpinyin-core/src/full_pinyin_index.rs:151`, `oxpinyin-core/src/scheme.rs:338,821-828` |
| Chewing-key table bounds | 24 initials, 4 middles, 18 finals, 6 tones | `storage/chewing_enum.h:38-40,53-55,81-83,97-99`, `storage/chewing_key.cpp:28-48,75,92,105` | `oxpinyin-core/src/chewing_key_tables.rs:3-7`, `oxpinyin-core/src/chewing_key.rs:100-111` |
| Full-pinyin consumed length | At most 32767 from the signed 16-bit parse step | `storage/pinyin_parser2.cpp:67-70,289-292,353-359` | Wider internal positions under the [16-bit parse-step ruling](upstream-divergences.md#the-full-pinyin-parses-16-bit-steps). `oxpinyin-core/src/graph.rs:18-26` |
| Key rests and candidate positions | Wrap modulo 65536 | `storage/chewing_key.h:102-103`, `pinyin.cpp:103-104`, `zhuyin.cpp:83-84` | ABI values wrap. Internal placement follows the [key-position ruling](upstream-divergences.md#key-positions-past-byte-65535). `oxpinyin-capi/src/cursor.rs:102-103`, `oxpinyin-zhuyin-capi/src/cursor.rs:122-123` |
| `MAX_PHRASE_LENGTH` | 16 keys. Import and remember accept lengths 1–15 | `include/novel_types.h:119`, `pinyin.cpp:643,1406,3602,3680`, `zhuyin.cpp:528,924`, `storage/phonetic_key_matrix.cpp:362,452,512` | `oxpinyin-engine/src/session/mod.rs:24`, `oxpinyin-user/src/phrase.rs:38`, `oxpinyin-data/src/chewing_table.rs:438`, `oxpinyin-capi/src/iterators.rs:273`, `oxpinyin-zhuyin-capi/src/iterators.rs:192` |
| Phrase-token reduction and import | At most 4 tokens where reduction validates. At most 1 match in the requested sub-index at import | `storage/phrase_large_table3.h:77-106`, `pinyin.cpp:536-555`, `zhuyin.cpp:422-445` | `oxpinyin-facade/src/import.rs:46-62`, `oxpinyin-facade/src/predict.rs:47`, `oxpinyin-engine/src/char_offset.rs:370-374`, `oxpinyin-capi/src/dict.rs:73`, `oxpinyin-zhuyin-capi/src/dict.rs:42` |
| `PHRASE_INDEX_LIBRARY_COUNT` | 16 | `include/novel_types.h:43` | `oxpinyin-data/src/table_info.rs:41`, `oxpinyin-runtime/src/lib.rs:44` |
| Token slot mask and allocation wrap | `0x00ffffff`. After the last slot, import uses slot 1 and addon promotion uses slot 0 | `include/novel_types.h:41-46`, `storage/phrase_index.cpp:620`, `pinyin.cpp:589-594,2541-2545` | `oxpinyin-user/src/phrase.rs:29,110-116`, `oxpinyin-user/src/store_arena.rs:378-382` |
| `OPEN_COUNTER_LIMIT` | 6 | `storage/table_info.cpp:32,409,423` | `oxpinyin-data/src/user_files.rs:291,300,500` |
| Configuration scanner | 6 conversions per library row, at most 255 bytes per `%s` field | `storage/table_info.cpp:228,232,255-259,352` | `oxpinyin-input/src/native.rs:394,406,425-432`, `oxpinyin-input/src/config.rs:158` |
| Table-text scanner | First 4 fields. At most 4095 bytes per read and 255 bytes per copied string. Decimal tokens fit `u32` and counts fit `size_t` | `storage/table_text_parser.h:34-49,54-90,99-114` | `oxpinyin-utils/src/storage/table_text.rs:38-41,69-85`, `oxpinyin-input/src/tool.rs:1260,1312-1334` |
| MemoryChunk header and checksum | 8-byte minimum header. Four-byte little-endian checksum words and eight-bit shifts for remaining bytes | `include/memory_chunk.h:69,136-160,427-438,486-496` | `oxpinyin-input/src/chunk.rs:109-118`, `oxpinyin-data/src/chunk_format.rs:33,70-80` |
| SingleGram header and record stride | 4-byte minimum header, 8-byte item stride | `storage/ngram.cpp:37-38,48-70`, `include/novel_types.h:105-108` | `oxpinyin-core/src/single_gram.rs:20-23`, `oxpinyin-user/src/store.rs:1526-1539` |
| Journal ADD item header and mask-out header count | ADD needs at least 6 bytes. Mask-out needs one journal header and asserts on a second | `storage/phrase_index.h:56,124-126`, `storage/phrase_index.cpp:457,721-747` | `oxpinyin-user/src/replay.rs:918-924`, `oxpinyin-user/src/persistence.rs:1844-1855` |
| Phrase-item length and pronunciation count | Both fields are 8-bit unsigned. The 256th distinct pronunciation wraps the count to zero | `storage/phrase_index.h:100-114`, `storage/phrase_index.cpp:56-91,195-204` | `oxpinyin-user/src/user_library.rs:364,411`, `oxpinyin-user/src/store.rs:405-408,443` |
| Phrase-content offset and signed bulk growth | Offsets wrap modulo 2³². A fresh bulk copy reaches the unsafe growth boundary at 2³¹ bytes | `include/novel_types.h:128`, `storage/phrase_index.cpp:208-217,365-390`, `storage/phrase_index_logger.h:115-117`, `include/memory_chunk.h:95-98,290-296` | `oxpinyin-user/src/user_library.rs:19-24,73,557`, `oxpinyin-user/src/persistence.rs:2307`. Compatibility policy rows 111–112 cover these boundaries |
| Journal record payload length | The 16-bit field wraps modulo 65536 | `storage/phrase_index_logger.h:248-293` | The encoder refuses a payload above 65535 bytes. `oxpinyin-data/src/user_files.rs:879-919`. [Accepted language artefact](upstream-divergences.md#oversized-journal-record-payloads-policy-row-123) |
| Interpolation and K-mixture estimation | Initial λ 0.6, convergence delta 0.001 | `utils/training/estimate_interpolation.cpp:34-35` and `utils/training/estimate_k_mixture_model.cpp:40-41` from the repository root | `oxpinyin-utils/src/training/estimate_interpolation.rs:31-34`, `oxpinyin-utils/src/k_mixture_model/estimate.rs:28-30` |
| K-mixture generation defaults | Maximum occurrences 20, maximum increase rate 3.0 | `utils/training/gen_k_mixture_model.cpp:50-51` from the repository root | `oxpinyin-utils/src/k_mixture_model/generate.rs:33-35` |
| K-mixture pruning | Default `k = 3` and CDF 0.99. Remaining probability test uses `DBL_EPSILON` = 2⁻⁵² | `utils/training/prune_k_mixture_model.cpp:34-35,68` from the repository root | `oxpinyin-utils/src/k_mixture_model/prune.rs:24-26,119` |

The system `table.conf` supplies the interpolation coefficient λ.
The stock value is 0.312699 (`data/table.conf.in:3` from the repository root).
`SystemTableInfo2::load` sets
`LC_NUMERIC="C"` and scans `%f\n` into a `gfloat`
(`storage/table_info.cpp:197,220-224`). Both Rust facades and the tools
call the same scanner through `oxpinyin-input/src/native.rs:374-385`.
The decoder uses the scanned value and derives the unigram coefficient as
`1 - λ` (`lookup/phonetic_lookup.h:712-713`).

### oxpinyin's own values, by call path

| Call path | Value | oxpinyin site |
| --- | --- | --- |
| Graph construction and decoding | The maximum graph position is `u32::MAX - 1` (4294967294). The graph rejects longer input and the decoder reserves `u32::MAX` for the seed step | `oxpinyin-core/src/graph.rs:18-26,251-255,330-334`, `oxpinyin-engine/src/nbest.rs:130` |
| Candidate frequency sorting | A consistent wrapping-frequency group fits in a circular interval narrower than 2³¹. A cyclic group uses stable descending numeric order under the [candidate-frequency ruling](compatibility-policy.md#no-ported-external-library-internals) | `oxpinyin-engine/src/candidate.rs:33-40,58-70` |
| Backend attachment | Berkeley DB 5.3 and Kyoto Cabinet 1.2 runtime version gates | `oxpinyin-store/src/bdb/ffi.rs:101-102,124-137`, `oxpinyin-store/src/kyotocabinet/ffi.rs:67,80-94` |
| Kyoto Cabinet key and value transfer | At most `0x3fffffff` bytes per region, from Kyoto Cabinet's `MEMMAXSIZ` | `oxpinyin-store/src/kyotocabinet/ffi.rs:96-112` |

## 10. Where oxpinyin keeps the pin's code shape

`docs/source-policy.md` prefers idiomatic Rust and keeps the
pin's code shape only where behaviour depends on it. The places where
oxpinyin keeps C-shaped code:

| Place | Why | Needed for behaviour? |
| --- | --- | --- |
| `SingleGram` operations over raw bytes (`oxpinyin-core/src/single_gram.rs`), including libstdc++'s `lower_bound` on unsorted arrays | the byte layout is the on-disk format, and the unsorted-array result reaches output | yes |

## 11. Further designs

### Sentence scoring types

The pin's `gfloat` log accumulation is two things, both of which Rust can follow:

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
- *libm's `log`.* The one transcendental. The pin calls the platform
  libm's `log`; oxpinyin uses `f64::ln` under the
  [Maths rule](compatibility-policy.md#a-maths-platform-dependent-floating-point-accumulation).
  Standard-maths precision differences stay class (a). Compiler
  contraction and library implementation details are not reproduction
  targets under the
  [internal-language-artefact rule](compatibility-policy.md#internal-language-artefacts),
  so no binary-level check is needed.

Matching types, conversion points and operation order is necessary, not
sufficient: a difference that remains after that is investigated and
attributed, not classified as a host difference by default. The decoder
follows these types and conversion points in `f32` and `f64` (`expand_one`,
`nbest.rs`; `nbest_step_costs_using`, `oxpinyin-data/src/lm/mod.rs`).
Register row 11 classes std-maths precision differences as (a).
Candidate and sentence rows carry no separate integer score.

### Constraint revalidation

The pin revalidates a constraint without a table search.
`validate_constraint` (`lookup/phonetic_lookup.cpp:120-165`) reads the
constrained phrase item and calls `compute_pronunciation_possibility`
(`storage/phonetic_key_matrix.cpp:534` onward). That call matches the
matrix against the stored pronunciations of the item, with no expansion and
no cap. An incomplete key matches any final
(`storage/pinyin_phrase3.h:80-86`).

`ConstraintSpelling::spells` (`nbest.rs`) ports `validate_constraint` and
`compute_pronunciation_possibility` through
`Dictionary::phrase_item_pronunciations`. It carries the last item read
across a pass, as the single `PhraseItem` of the pin does. It borrows the
parse's placed matrix and shares `GenerationView::sum_paths` with sentence
pricing. Each leaf sums matching stored frequencies and the total with u32
wrap, divides in f32 (`phrase_index.h:136-163`), and each recursion frame adds
its children in matrix order (`phonetic_key_matrix.cpp:534-600`). The final
sum is kept at or above f32::EPSILON, including equality. No integer-zero
substitute or model20-only boundary assumption remains.

For P complete paths, V visited prefixes, and C pronunciation-match work per
leaf, revalidation costs O(V + PC), like the pin's walk. It uses O(d) walk
stack and one key scratch for maximum depth d, rather than short-circuiting
on a positive match. No second placed matrix is allocated. Duplicated entries
increase path multiplicity and therefore work and pronunciation writes
(`phonetic_key_matrix.cpp:603-668`; `pinyin.cpp:3594-3606`). Fuzzy passes take
O(k) work for k original column entries and a constant number of swaps; the
removed collapse took O(k²). The split passes and dictionary-call costs are
unchanged. Net wall time, peak RSS and allocation effects are unknown.

### Persisted empty items through arena reconstruction

`src/storage/phrase_index.cpp:180-217` uses offset zero for absence and keeps
an empty phrase as a six-byte header plus four bytes per pronunciation.
ADD appends the bytes and adds the unigram after the journal's header update
(`:453-459`, `:503-514`). Compact copies each present item in slot order,
rebuilds sub-index totals, and retains the facade total (`:648-675`).
Store copies the packed bodies (`:365-390`). Truncated bodies whose declared
extent reaches outside the content buffer retain policy row 107's safety
refusal; the pin checks the two header reads but then constructs the declared
extent without a body bound check (`:195-205`).

`oxpinyin-user`'s `persistence::encode_persisted_item` shares the journal's
typed-item shape validation with arena reconstruction. `UserLibrary::store`
copies each validated `view.as_bytes()` directly, including empty items.
Public phrase-import validation stays separate. Within the current arena
limits, empty items retain slot identity, ordered pronunciation frequencies,
and the separate item sum, sub-index total and facade total through reload
and save. `load_system_libraries` and `reload_system_libraries` retain their
`JournalFault`-only results and existing journal handling.

Both C nth-pronunciation getters append zero words and return true for an
empty-key pronunciation, like `src/pinyin.cpp:2815-2818` and
`src/zhuyin.cpp:1807-1810`.
`reload-persisted-empty-user-file-pinyin` is a refused-reload control for
library 5: pinyin unloads only GBK (`src/pinyin.cpp:464-475`), so it checks
compaction, save and reopen without a successful unload. The zhuyin case
does unload and reload USER_FILE: only TSI is refused (`src/zhuyin.cpp:378-390`).

For n live items occupying B bytes, encoding visits each word/key once,
O(B) time and output space, with at most one item's temporary encoded body;
an offset lookup is O(1). For S offset words, the dense array occupies O(S)
space and a scan takes O(S) time. Saving validated views copies B bytes
directly, as the pin does, with no additional per-item allocation.

### Zhuyin index representation

The pinned global `zhuyin_index` uses spelling pointers, flags and canonical
content IDs (`src/storage/pinyin_parser_table.h:1492`).
`search_chewing_index` uses strcmp/equal_range, gates flags and reads the key
from content (`src/storage/zhuyin_parser2.cpp:44-101`). oxpinyin keeps its
original representation of that sorted 1493-row inventory.

Rejected alternative: immutable UTF-8 pool spans instead of three string
slices per row, with the same `partition_point` search and first-match
rendering. It reduces file size but makes the Simple and CP26 lookups slower.
File size cannot offset a runtime regression; it is governed separately by
Constitution §2.

### Table-file scanning

Pin `table_info.cpp:208-287,338-359` uses five system/four marker directives,
then six-column library rows. `oxpinyin-input` makes those `fscanf`, `feof`
and `atoi` calls on the file's stream, and `TableConf::load` and
`UserTableInfo::load` keep the pin's mappings, slots and refusals. Both loads
are O(n) time in the file and O(1) space beyond the rows: the scanner keeps
the pin's fixed 256-byte buffers. Library slot vectors and persistent writers
retain their formats and ordering. Punctuation remains O(words) time/space;
chunk payload retries preserve O(bytes) time and O(1) additional space, with
the existing header/sync/rename contract.

### Point-search pronunciation records

Candidate guess calls table search
in `src/pinyin.cpp:2233-2253` and `src/zhuyin.cpp:1499-1510`. The BDB adapter
borrows the returned row (`src/storage/chewing_large_table2_bdb.cpp:182-191`);
`src/storage/chewing_large_table2.h:145-152` finds an equal range, then
`:101-106` compares packed stored pronunciations and reads matching tokens.
This work happens during guess, not init. For N records of L keys and M
matches, the pin searches in O(L log N + ML), with output range storage.

Rust `ChewingTable::search` keeps its linear filter and the owned DBM row
buffer, but `pinyin_index::item_views` validates the row and borrows its keys.
Only matching tokens are collected; `resolve_items` consumes those tokens.
Worst-case time is O(NL) and temporary space O(B + M), for B row bytes. No
key vector is allocated per record, and no work moves into init. Other consumers that need owned pronunciations
still use `decode_items`, over the same validated record views.

### Narrow token getters

Pin sources: `src/pinyin.cpp:1627-1645,2786-2833`,
`src/zhuyin.cpp:1096-1113,1778-1825`,
`src/storage/phrase_index.h:100-125,646-652`, and
`src/storage/phrase_index.cpp:33-43,56-92,180-205`.
The facade selects the loaded sub-index and the token's offset. A missing
library or slot fails before phrase outputs are written. Count and frequency
outputs are zeroed first; the nth getter clears its GArray first. Text copies
the stored words and uses GLib-owned output. The nth getter copies the selected
insertion-ordered reading, ignoring its failure result; the existing class-(b)
zero-key treatment for out-of-range readings remains. Appending a new reading
keeps earlier positions; accumulating an existing reading does not reorder it.
The pin's field reads take O(1) for metadata and O(L) for text or one reading,
where L is the stored phrase length.

`RuntimeDict::token_text`, `token_pronunciation_count`, `token_exists` and
`token_nth_pronunciation` keep those fields separate for mapped system items,
live system overrides and USER_FILE items. Both C facades and prediction text
assembly use these narrow reads. C allocation, array operations and
structured-key packing remain at the existing boundaries.

For session USER_FILE text, `RuntimeDict::token_text` borrows the item's words
through `with_user_item` and `PhraseItemView::phrase_text`, with no
intermediate word copy, and keeps the stored length, first-zero cut and
non-scalar conversion. `UserStore::user_unigram_frequency` returns existence and
frequency in one guarded arena read, including pending increments; both C
facades still apply visibility and their overlays. `Some(0)` remains distinct
from an absent item. Text is O(L) time and O(L) result space. The nth
getter on a session item reads that one reading's packed words
(`UserStore::nth_pronunciation_words`), O(L), as the pin's
`get_nth_pronunciation` does.

### Table generators

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

### Memory-chunk test program

`tests/include/test_memory_chunk.cpp:5-69` exercises the
pin's own logical capacity algorithm (`src/include/memory_chunk.h:95-133`):
borrowed storage copies to exactly size + growth; owned storage keeps its
extent when sufficient, otherwise grows to max(2 * capacity, size + growth).
New storage from the current size onward is zeroed. The tool-side Buffer
uses safe bytes with independent logical size; Vec allocator capacity is
never observed. Its fixed program prints capacities 4, 6, 12 and the
source's integer rows. Save/load and optional map use the existing input
boundary. The tool-diff runner compares all /tmp/test.bin bytes.

### Phrase-index and logger test programs

`tests/storage/test_phrase_index.cpp:9-122` checks the synthetic
item, shared pronunciation mutation, text imports, compact, and store/load.
`test_phrase_index_logger.cpp:26-65` increments the whole local range,
stores without compacting, diffs, reloads and merges. The shared importer
keeps its existing generator behavior and exposes the test programs' phonetic
choice and approved invalid-UTF-8 guard. The user crate's tools feature
exposes the existing ReplayedLibrary arena, add/train/compact operations,
diff_live_records and Merge, with raw offset/content serialization.

FacadePhraseIndex's cache takes a delta for every attempted range slot
(`phrase_index.h:628-634`), even a hole or overflow refusal. Load subtracts
the old local total unconditionally, adding the new total only if its layout
is accepted (`phrase_index.cpp:237-248`);
merge does the same around its record walk (`:291-296`). That cache is kept
across both operations. The logger's ordinary sparse/overflow fixtures print
the same cached total twice, while their stored totals and fields can differ.

The fixed 100000-call loop and the pin's basic f32 pronunciation ratios stay.

### In-memory phrase and chewing table test programs

`tests/storage/test_phrase_table.cpp:36-125` and
`test_chewing_table.cpp:36-126` load text through
`tests/tests_helper.h:51-83`, with both unconditional rewinds. The programs
use the existing importers, a native in-memory DBM and the production row
decoder. A present empty row is SEARCH_CONTINUED. Searches preserve stored
item order, then emit libraries in slot order; contiguous matching tokens
form ranges; a cursor starting at null_token is empty both before extending
or flushing and at the final flush (`chewing_large_table2.h:90-132`). Range dumps use the native
item pronunciations, including the literal backspace (`tests_helper.h:89-133`).
The final mask retains keys and empties values, as the pin does.

The source's 1000-call loops remain. Direct
queries first check the first incomplete key's tone, then those longer than
16 keys exit EINVAL before the pin's out-of-bounds
index write; invalid UTF-8 exits EINVAL before the invalid conversion read.
Both approved class (b) outcomes retain preceding observable work.

### Matrix and direct lookup test programs

`tests/storage/test_matrix.cpp:93-179` and
`tests/lookup/test_pinyin_lookup.cpp:41-153` enable neither
resplit nor divided-table options. The physical dump uses fill's original
keys, explicit gap/terminal zero keys, and the existing fuzzy-alternative
order (`phonetic_key_matrix.cpp:29-84,226-347`). Matrix search recursively
follows each span, returns continued for empty end columns, and retains
longest-reach information (`:350-437`); its range/pronunciation dump is
compiled out. The source's fixed 1000/100 timed loops remain. The pinyin lookup parses
and builds its scan matrix once before timing, then passes the same prepared
matrix into all 100 n-best calls (`test_pinyin_lookup.cpp:85-137`).

The runtime tools assembly attaches only the requested index, heap-loads
each configured SYSTEM_FILE library in order, then attaches the system
bigram when needed. The pin's NULL user-bigram attach returns before native
I/O. The existing native dictionary/model readers feed the existing
PhoneticLookup<2,3> trellis with sentence_start and no constraints, and the
existing PhraseLookup span DP. N-best output keeps phrase-start positions;
phrase lookup joins token-prefixed entries with newlines
(`phrase_lookup.h:131-135`, `lookup.cpp:31-66`). Empty results write the
source diagnostic and still exit zero.

`FullPinyinParser2::parse` duplicates its explicit-length bytes with
`g_strndup` (`pinyin_parser2.cpp:233`); GLib's documented contract pads after the first NUL
([g_strndup](https://docs.gtk.org/glib/func.strndup.html);
`gstrfuncs.c:397-407` at 2.86.1).
Each candidate span retains its explicit length, while the index's string
comparison sees only the prefix (`:169-210`). Tools retain that padding,
and the shared graph's spelling check truncates at NUL after tone scanning.
A dedicated test covers the extended span and erased trailing apostrophe.

Data loading retains heap-owned payloads, as these programs'
`MemoryChunk::load` calls do. The nonshipping adapters do not change the C
ABI or file formats.

### Candidate strings across reparsing

`pinyin.cpp:1497-1609` and `zhuyin.cpp:1017-1081` replace the
parse matrix without touching the candidate vector or its strings. The
candidate string getters return the stored pointer (`pinyin.cpp:2873-2878`,
`zhuyin.cpp:1865-1870`). Candidate guesses free the preceding list, even on
an empty-matrix refusal (`pinyin.cpp:2183-2196`, `zhuyin.cpp:1460-1471`);
reset also frees it (`pinyin.cpp:2693-2704`, `zhuyin.cpp:1718-1729`).

Prediction invalidates the old phonetic candidate list before resolving
prefixes, including when there is no matching prefix (`pinyin.cpp:2420-2427`).
The punctuation entry calls that same path (`:2459`). oxpinyin releases
the old window alongside the snapshot it indexed.

Both Rust ABI parse modules therefore retain their `CandidateSnapshot` until
those invalidation boundaries. Parse resets also preserve its anchored engine
window: a subsequent normal choose uses the retained token/span
(`pinyin.cpp:2580-2587`, `zhuyin.cpp:1646-1666`), not a freshly decoded row at
the same index. Full reset discards that window. For N rows and B string
bytes, its O(N + B) storage remains live across parsing.

Zhuyin also keeps the window after a successful choose. The pin's
`zhuyin_choose_candidate` reads the retained token and span on each call
and leaves the list intact (`zhuyin.cpp:1635-1667`). Further choices reuse
the same rows until a new guess or reset invalidates them.

A retained normal span may overrun the new constraint array after a shorter
parse. `ForwardPhoneticConstraints::add_constraint` refuses `end > len`
without a write (`lookup/phonetic_lookup.cpp:61-65`). Both choose functions
still validate existing constraints before and after that refusal. Pinyin
returns `offset + 0` (`pinyin.cpp:2578-2589`); Zhuyin returns the candidate's
original begin or end (`zhuyin.cpp:1646-1666`). The engine reports
`Selection::Refused` before clipping or recording the span. Its existing
constraint validator handles both passes; the facades retain their distinct
cursor rules and the refused candidate's window. Zhuyin stores the original
end in its snapshot's existing padding (the 64-bit row remains 32 bytes).

The refusal and validation size comes from the retained parse: zero when
the selected key path is empty, otherwise the parsed matrix bound plus its
tail slot (`storage/phonetic_key_matrix.cpp:30-48`). Unconsumed input bytes
do not enlarge it, and consumed apostrophes alone do not create a matrix.
The check applies to token-bearing candidates that write constraints;
tokenless engine fallbacks do not call the pin's `add_constraint` route.
No additional matrix or parse-length state is retained.

The refusal test is O(1). Two validation passes reuse the retained parse and
the existing spelling check: each scans C constraint cells and probes F
forcings, with O(F) temporary forcing storage plus the existing spelling
matrix and search costs. The pin also validates twice.

## Open

- Constraint revalidation over a phrase of the pinned model forced over its abbreviation, one initial-only key per syllable.
- The [§9](#9-constants) value inventory is complete for the source sweep at `074a2219`. The sweep covered `src/pinyin.cpp`, `src/zhuyin.cpp`, `src/lookup/`, `src/storage/` and `src/include/`, and traced the corresponding C facade, storage and utility call paths in Rust. It counted named and inline coefficients, comparison and loop bounds, scanner widths and integer widths that affect results or saved bytes. It excluded pure indexes, capacity-only buffers, identity operands, unused table weights and unreachable legacy paths.
- Equivalence not argued: candidate dedup for malformed phrase text.
- Equivalence not argued: the table-text pinyin parser (`crates/oxpinyin-utils/src/storage/pinyin.rs`) against `PinyinDirectParser2::parse` with `USE_TONE`.
- Equivalence not argued: double pinyin with a table whose first yunmu slot is empty and second is not (no shipped table has one).

N-best constraint writes follow `phonetic_lookup.cpp:172-205`: the decoded
result supplies the tail; `changed` is set before each differing write;
a zero return from `add_constraint` aborts at `:203`. The store is sized
by guess-time validation (`:120-138`), not by the NBEST choose.

### Defects from §§1–8

- <a id="fini-native-handles"></a>**Boundary, highest priority — native handles outlive fini.** Source-derived.
  The pin closes successful attachments at fini (`pinyin.cpp:1194-1222`,
  `zhuyin.cpp:741-757`). Rust retains them until the last owner drops
  (`oxpinyin-capi/src/context.rs:141-161`, `oxpinyin-runtime/src/lib.rs:1506-1519`).
  A second-process lock check with stock tkrzw data is a proposed witness.
  Close native resources at fini and preserve memory that surviving Rust owners still use.
- <a id="configured-user-file-dispatch"></a>**Configured `USER_FILE` dispatch.** Source-derived.
  In a custom valid configuration with `GB_DICTIONARY` as `USER_FILE`,
  the pin reads a present slot and accepts a new import
  (`pinyin.cpp:303-321,589-606,2821-2833`, `storage/phrase_index.h:646-652`).
  Rust returns false with zero for the read and refuses the import
  (`oxpinyin-facade/src/instance.rs:418-429`, `oxpinyin-facade/src/import.rs:82-91`).
  Dispatch both calls from the configured library type.
- <a id="chewing-table-tkrzw-read"></a>**tkrzw pinyin-table read error.** Source-derived.
  On stock data with a native I/O fault, the pin treats a failed `Get` as a miss
  (`storage/chewing_large_table2_tkrzwdb.cpp:143-159`). Rust propagates the error
  out of sentence lookup (`oxpinyin-store/src/tkrzw/mod.rs:257-262,409-419`,
  `oxpinyin-data/src/chewing_table.rs:186-196,361-370`, `oxpinyin-runtime/src/lib.rs:968-982`).
  Restore the miss under the [external I/O rule](compatibility-policy.md#external-io-uses-the-pins-call).
  The system bigram native-read ruling covers a different table.
- <a id="chewing-table-kyoto-checked-read"></a>**Kyoto Cabinet pinyin-table body read.** Source-derived.
  A native body-read fault after a positive size check on a stock row makes the pin assert
  (`storage/chewing_large_table2_kyotodb.cpp:159-176`). Rust uses one visitor with no second read
  (`oxpinyin-data/src/chewing_table.rs:186-196`, `oxpinyin-store/src/kyotocabinet/ffi.rs:472-492`).
  Restore the checked read under the [failure rule](compatibility-policy.md#failure-behaviour).
  The system bigram native-read ruling covers a different table.
- <a id="remember-packed-alternate"></a>**Remember drops a packed fuzzy alternate.** Source-derived.
  In a crafted system item with only the packed `shung3` reading, fuzzy `shun3`
  lets the pin remember the character (`storage/phonetic_key_matrix.cpp:285-306`,
  `pinyin.cpp:3594-3606,3647-3663`). Rust keeps the key in the matrix and refuses the write
  (`oxpinyin-core/src/vocab.rs:115-129`, `oxpinyin-capi/src/user_data.rs:125-143`).
  Keep the packed key through the store write.
- <a id="predecessor-row-reuse"></a>**Merged rows skip required system bigram reads.** Source-derived.
  The pin reads the attached system DBM for each load (`lookup/phonetic_lookup.h:588-599`).
  `NbestStepRow` and `get_or_try_init_in_generation` reuse a loaded row or miss
  (`oxpinyin-engine/src/nbest.rs:1382-1389`, `oxpinyin-runtime/src/lib.rs:1464-1475`).
  This can skip a later native read and its failure with stock data.
  Read the attached system DBM every time, including for `sentence_start`, as the
  [native-read ruling](compatibility-policy.md#sentence-start-bigram-caching) requires.
- <a id="mask-out-slot-bits"></a>**Mask-out compares the full token in the user arena.** Source-derived.
  With an imported phrase in the stock user library,
  `mask_out(0x10000000, 0x10000000)` removes its body at the pin and keeps it in Rust
  (`include/novel_types.h:41-44`, `storage/phrase_index.cpp:677-715`,
  `oxpinyin-user/src/store_arena.rs:665-680`). Use the pin's separate library and slot masks.
- <a id="compact-slot-zero"></a>**Import-end compaction retains slot zero.** Source-derived.
  Ending an import drops a readable item at reserved slot 0 in a crafted user chunk
  at the pin (`pinyin.cpp:655-660`, `storage/phrase_index.cpp:624-675`).
  Rust retains it and its unigram remains readable
  (`oxpinyin-user/src/user_library.rs:178-185,474-485,533-542`).
  Start user compaction at slot 1.
- <a id="compact-system-slot-aliases"></a>**Import-end compaction retains system slot aliases.** Source-derived.
  For two slots sharing a body in a crafted system chunk, the pin's compaction copies
  each body independently (`storage/phrase_index.cpp:208-217,648-675`).
  Rust keeps the offsets, so a later unigram increment can reach both slots
  (`oxpinyin-user/src/store.rs:1358-1402`, `oxpinyin-user/src/system_items.rs:135-143,223-229`).
  Make system compaction separate the live bodies.
- <a id="export-packed-tone-assertion"></a>**Export skips the tone assertion on system items.** Source-derived.
  Exporting an otherwise readable system item's corrupt `ni` reading with tone 6
  aborts at the pin (`pinyin.cpp:721-733`, `storage/chewing_key.cpp:47-57`).
  Rust emits `ni6` (`oxpinyin-facade/src/export_rows.rs:195-204,248-264`).
  Check packed system readings at the export boundary.
- <a id="import-aliased-reading-edit"></a>**In-place reading edits reach aliased user slots.** Source-derived.
  With two slots sharing a body in a crafted user chunk, an exact-reading import
  targeting the first slot leaves the second slot's exported count intact at the pin
  (`pinyin.cpp:563-582`, `storage/phrase_index.cpp:208-234`). Rust updates both counts
  before import end (`oxpinyin-user/src/user_library.rs:323-361,498-511`).
  Give the edited slot an independent body.

### Existing separator issue

- <a id="index-parse-separator-fill"></a>**Index-parse separator fill.** Source-derived follow-up to
  [policy row 122](upstream-divergences.md#index-parse-separator-column-a-clear-at-the--after-a-guess-at-the-next-key-policy-row-122).
  Luoma `'ni` with stock data leaves column 0 empty and produces no sentence at the pin
  (`storage/pinyin_parser_table.h:906`, `storage/phonetic_key_matrix.cpp:46-79,410-418`,
  `lookup/phonetic_lookup.h:753-771`). Rust decodes canonical `ni` and can produce a row
  (`oxpinyin-facade/src/parse.rs:288-323`). Follow row 122's route to close by
  giving index-parsed sessions the pin's separator columns and revisiting the #602 mapping.

### Removal candidates

These source-derived candidates follow the
[internal implementation and performance rule](../source-policy.md#internal-implementation-and-performance).

- **Owned records in `search_continued`.** The pin filters packed records in place
  (`storage/chewing_large_table2.h:91-152`). Rust allocates keys per record
  (`oxpinyin-data/src/chewing_table.rs:486-503`, `oxpinyin-data/src/row_format.rs:251-260`).
  Filter borrowed records in the same order.
- **Duplicate full matrix records.** The pin stores one key and rest per entry
  (`storage/phonetic_key_matrix.h:123-155`). Rust retains a second full copy
  (`oxpinyin-engine/src/session/matrix.rs:10-119,157-175`).
  Share the key payload while preserving both views' indexing and order.
- **Repeated separator suffix scan.** The pin remembers the next separator
  (`storage/pinyin_parser2.cpp:233-269`). Rust scans the suffix from each byte
  (`oxpinyin-core/src/full_pinyin_index.rs:219-247`). Reuse the separator position.
- **Repeated mask-out validation.** The pin walks and masks the user tables in order
  (`pinyin.cpp:1224-1230`). Rust repeats the preflight reads before mutating the tables
  (`oxpinyin-user/src/store.rs:2379-2445`). Reuse validation while preserving first-fault order.
- **`CurrentUserItem` export fallback.** The pin reads the current item directly
  (`pinyin.cpp:705-724`). Rust does redundant token and generation checks for a cache
  whose fallback reads the same sources that just missed
  (`oxpinyin-facade/src/export_rows.rs:550-590`, `oxpinyin-user/src/store.rs:1801-1806`).
  Remove the fallback and its retained state.
