# First candidate after init (#1031), from source

Date: 2026-11-02 UTC (captured with `date -u`). Status: source investigation;
fixes below are proposals, not implementations or performance measurements.

## Conclusion and scope

The first candidate passes through more representations in oxpinyin: graph,
matrix projections, owned dictionary entries, engine candidates, ranking keys,
and a C-facing snapshot. The pin retains its parse matrix and gathers token
ranges before reading phrase fields. Current oxpinyin still repeats topology
work, probes table existence separately from search, decodes user-row keys it
then discards, and reads unigram overlays through maps. It also converts a
path's syllables to keys four times by string search, takes three mutexes and
clones a library handle twice per candidate, and in the measured profiles has
its count gates open. These are identifiable
costs; they do **not** establish a current 2× ratio or a numerical decomposition
of the old ratio.

Trees read:

- oxpinyin `origin/main` at `f3d08577b7a5f159f0d1624c52aedfb7d8462c26`,
  in a new `wip/1031-first-candidate` worktree. Rust citations below name files
  and items in that tree, not line numbers.
- libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`, verified as the HEAD
  of `/Users/shenghaochen/repos/libpinyin`. All pin `path:line` citations refer
  to that commit; source links use its full hash.
- Open [#1517](https://github.com/shenghaoc/oxpinyin/pull/1517), head
  `7028eea95de6bf4345c3d8db1b659ee09725d2fa`, read with `git show`.
  [#1518](https://github.com/shenghaoc/oxpinyin/pull/1518) is merged as
  `f3d08577`; its implementation is the main tree above.

The [issue and its four comments](https://github.com/shenghaoc/oxpinyin/issues/1031)
were read first. Their old observations and rulings remain historical:
Memo A at `52125a73` was rejected, neither fixed hasher was adopted, the attribution
located the excess mainly in guess, and the later snapshot-append experiment
stopped on init-plus-first-candidate wall time. This document revives none of
those experiments and imports no old absolute figure as a current baseline.

The issue's phase is **parse + candidate guess + first candidate observation**,
after init, configuration and instance allocation. It is not
`pinyin_guess_sentence`. The maintained [startup worker][worker],
`startup_first_candidate_unit`, makes the boundary explicit:

1. `parse_more_full_pinyins("nihao")` on **both** facades, options `0x18a`.
2. Pinyin: `pinyin_guess_candidates(instance, 0, 0x1e)`.
   Zhuyin: `zhuyin_guess_candidates_after_cursor(instance, 0)`.
3. Count, fetch candidate zero, read its type/string, and copy that string.

`0x18a` has no `DYNAMIC_ADJUST`; `0x1e` disables LONGER candidates. No sentence
guess has populated sentence rows. Thus sentence trellis construction,
predecessor bigram reads/merges, prediction, and LONGER suggestion walks are
outside this first request. The worker's `phrases` and `journals` profiles
change user state, not this API sequence. Both the original
[init attribution](init-attribution-2026-10-22.md) and the issue's attribution
comment describe this boundary.

## Cost notation

Bounds describe the issue's Linux path and successful reads of ordinary,
well-formed data unless stated otherwise. They do not estimate elapsed time,
resident pages or instructions.

- `n`: input bytes; `e`: parser graph edges; `k`: materialized matrix keys,
  including alternates and intermediate duplicates; `q`: completed key paths
  searched across the expanding windows; `v`: visited recursive scan nodes.
  A path has `l ≤ 16` keys. The parser's maximum spelling length is 7 including
  tone; split/fuzzy inventories are fixed. Enumeration is output-sensitive in
  `q`; it must not be called linear in `n` when alternatives branch.
- `M = n + e + k + a`: work of one Rust scan-matrix build, where `a` counts
  the resplit key-pair checks and in-column duplicate comparisons. In
  [session/mod.rs][engine-mod], `resplit_step` clones columns and visits pairs;
  `inner_split_step` clones columns; `fuzzy_additions` snapshots the keys;
  `keep_first_in_column` compares against preceding retained keys. Snapshot
  copies cost O(`k`), while `a` is at most O(`k²`). Fixed spelling/fuzzy
  inventories do not justify silently treating those nested loops as linear.
- `r`, `b`, `m`: records, bytes and matching records in one probed pronunciation
  row. Sum per-row costs over `q` and the participating system/user/addon tables.
  `c_w` is candidates appended in a window; `c` is the total before text dedup;
  `s` is survivors; `t` is total phrase-text size processed.
- `u`: relevant user-table entries; `f`: mapped chunk
  bytes; `j`: journal bytes. `D_get`, `D_exists`, and `D_open` denote backend
  operations, not assumed constant-time disk I/O. Balanced-map lookup is
  worst-case logarithmic; hash lookup/insertion is expected amortized constant
  time in entry count plus key hashing, not a worst-case guarantee.

## Init and the retained state

At the pin, [src/pinyin.cpp:325–443][pin-init] loads configuration, constructs
parsers, attaches system pinyin/phrase DBMs, loads user tables into their
in-memory backend containers, loads default phrase chunks and replays journals,
opens the system/user bigrams, constructs lookup objects, and opens the pinyin
addon indexes and punctuation DBM. It leaves addon phrase libraries unloaded.
[src/zhuyin.cpp:269–358][pin-zh-init] has the corresponding default-table and
bigram work without pinyin's addon/punctuation setup.

[src/storage/facade_chewing_table2.h:75–89][pin-facade] distinguishes system
`attach` from user `load_db`. Loading user rows is already init work on both
sides, not a first-guess database open. Its cost includes the bytes loaded and
the backend insertions; no backend-independent bound for `D_open` is asserted.
The pin's [src/include/memory_chunk.h:475–520][pin-map] makes one writable
private mapping, scans the interpreted payload for its checksum, then closes
the descriptor. Checksum work is O(`f`), mapping setup is one OS operation,
and mapping reserves address space without eagerly copying `f` heap bytes.
[src/pinyin.cpp:234–323][pin-load] merges `.dbin` before returning from init.
Journal processing reads O(`j`) bytes, with additional compare/write/append
cost determined by the records; appending beyond capacity can copy the content
buffer ([src/storage/phrase_index.cpp:207–217,439–517][pin-replay]).

Oxpinyin [runtime/lib.rs][runtime], `Runtime::open_with_law_reporting`, opens
the same classes of runtime tables, calls `SystemDictionary::open_with`,
`BigramLanguageModel::open_with_mask`, `open_user_store`, and the facade-specific
addon/punctuation setup. [data/phrase_library.rs][library], `map_chunk` and
`PhraseLibrary::open`, map/checksum and retain bounded item views rather than
decoding every system item. [user/persistence.rs][persistence],
`system_originals` and `load_logs_with_mask`, retain the originals and replay
views; [user/replay.rs][replay], `origin_and_content`, makes a second private
writable mapping for a mapped library whose journal reaches replay, falling
back to an item-area copy if that mapping fails. This is conditional on replay,
not two mappings for every library in every profile.

The extra mapping and retained descriptor belong to **init**, outside the
first-candidate window. Both implementations checksum their original mapping
at init. Later lookup touches the selected DBM rows and item offset/header/text
bytes; the source alone does not identify which of those touches faults.
Do not equate two mappings with twice the physical memory or twice the latency.

Instance allocation is also outside the window
([src/pinyin.cpp:1310–1340][pin-alloc]). Oxpinyin's
`Runtime::new_session` shares handles and skips the fallback key-cost walk for
the production real-unigram model. [facade/instance.rs][instance],
`InstanceCore::new`, enables `set_defer_list_builds(true)`: parsing does not
eagerly build a candidate list that the guess then builds again.

## First-request comparison

The pin entry points are [src/pinyin.cpp:1497–1527][pin-parse] and
[src/pinyin.cpp:2184–2308][pin-guess]; the Zhuyin equivalents are
[src/zhuyin.cpp:1017–1042][pin-zh-parse] and
[src/zhuyin.cpp:1460–1539][pin-zh-guess]. The table follows their execution
order. “Extra” includes moved work and representation overhead, not just an
asymptotic regression. Common work is identified explicitly.

The Rust entry chain is [capi/parse.rs][capi-parse], `parse_c_string` →
[facade/parse.rs][facade-parse], `InstanceCore::parse_full_more` →
[session/state.rs][engine-state], `Session::replace_raw` →
[session/lookup.rs][engine-lookup], `refresh_after_parse`.
Guess enters [capi/sentence.rs][pinyin-sentence], `pinyin_guess_candidates`,
or [zhuyin-capi/sentence.rs][zhuyin-sentence], `guess_candidates`, then
`Session::candidates_at`/`scan_window` in [session/lookup.rs][engine-lookup].
Each shortened file label links to its full crate path.

| Step | Pin | Oxpinyin on the stated main | Extra work and source-derived cost |
| --- | --- | --- | --- |
| 0. Chunk ownership before the query | One private writable mapping; journal edits that view ([memory_chunk.h:501–519][pin-map], [phrase_index.cpp:439–517][pin-replay]). | `MappedFile::open_len` retains a read-only mapping and file; `origin_and_content`/`private_content` create the replay mapping ([library], [replay]). | Per replayed mapped library: another mapping of the chunk extent and retained original ownership; O(1) owner/setup operations plus OS mapping cost, O(`f`) extra virtual extent. Mapping failure fallback copies the item area in O(`f`) time/heap. **Init only; #1517 prepares its removal, not yet active.** |
| 1. Parse input and retain topology | `strlen`, one `g_strndup` of the input ([pinyin_parser2.cpp:234][pin-parser-input]) and one `g_strndup`/`g_free` pair per parser trial, up to seven trials per byte ([pinyin_parser2.cpp:169,187,198,212][pin-parser-trial], [pinyin_parser2.cpp:268–280][pin-parser-loop]), O(`n`) parser DP/backtrace with fixed spelling inventory, then matrix fill/transforms (key output plus resplit pair visits); pinyin adds resplit/divided/fuzzy passes, zhuyin only fuzzy ([pinyin_parser2.cpp:217–343][pin-parser], [phonetic_key_matrix.cpp:30–80][pin-matrix]). | `parser_prefix` validates UTF-8 and allocates a `String`; `InputBuffer::refill` retains another raw copy ([buffer]). `SegmentGraph::build_with_options`, `fewest_keys`, `build_scan_matrix`, `ParsedMatrix::from_scan` build graph, selected keys, temporary columns, retained columns and zero flags ([input], [graph], [engine-mod], [engine-matrix]). | Extra validation/copy O(`n`); graph and intermediate representations O(`n+e+k`) space and O(`M`) build work. Column clones/fuzzy snapshots add O(`k`) copies; `keep_first_in_column` adds duplicate comparisons absent from the pin's bag append. `full_parsed_len` runs `fewest_keys` again, O(`n+e`) plus path allocation. Zhuyin `ensure_matrix` builds both `ending_matrix` and `matrix` even for this after-cursor request: another O(`M`) pass and O(`n+k`) retained storage. Graph construction itself is already shared, not repeated by each caller. The pin's per-trial heap pair has no Rust counterpart: `emit_edges` resolves each span without a heap operation ([graph]). |
| 2. Empty-matrix/offset checks and parse length during guess | Reuse matrix, inspect size/columns, then scan from offset zero ([pinyin.cpp:2189–2228][pin-guess], [zhuyin.cpp:1465–1495][pin-zh-guess]). | `Session::matrix_keys` lazily builds another scan/projection; `InstanceCore::mode_key_list` copies keys for the pinyin shell. `spans_a_matrix_key` rebuilds a scan matrix; `scan_window` runs `fewest_keys` for its parse length ([offsets], [facade-cursor], [engine-lookup]). | First key-cache fill is O(`M`) plus `in_column_order`'s O(`k`) check and conditional O(`k log k`) stable sort ([engine-cursor]); temporary space O(`n+k`), cached key storage O(`k`), key-list copy O(`k`). The span check and length DP repeat per guess even when the graph is cached. Zhuyin's Hanyu `matrix_has_keys` avoids a matrix build, but its offset validation and shared engine still reach key/topology work. Each new parse invalidates these caches. `validate_abi_lookup_offset_over` reads its columns through `AbiColumns::get`, which converts each read key's spelling with `ChewingKey::from_pinyin`, a string binary search over the content table ([facade-cursor], [chewing-key]). |
| 2b. Enumerate expanding windows | `search_matrix_recur` walks matrix paths into per-library ranges; pinyin makes a separate addon traversal whose every completed path issues a `DB->get` on the addon pinyin index even with no addon library loaded, so `convert` finds every range `NULL` ([pinyin.cpp:2237][pin-guess], [chewing_large_table2_bdb.cpp:182][pin-bdb], [chewing_large_table2.h:107–109][pin-search]). Each nonempty-window search allocates/frees a `GArray` key path ([phonetic_key_matrix.cpp:350–437][pin-scan]). | `collect_window_scan`/`scan_parsed_paths` traverse retained columns, reusing the key/entry/addon scratch vectors; `ScanBuf` also keeps tones in a `SmallVec` ([engine-lookup]). | Common O(`n+v`) window/path traversal before row costs, with one table search per completed path and O(`n`) worst-case recursion storage. Rust shares the system/addon path traversal and bounds key depth; this is not extra dictionary-wide work. First use can grow scratch capacities. Rust's `AddonDictionary::fill_lookup` and `prefix_exists` return before any read when no addon library is loaded ([dict]): one fewer `D_get` per path than the pin here. |
| 3. System pronunciation row read | BDB borrows DB-owned bytes; KC fills its retained length-specific buffer; tkrzw gets a string then copies to a retained buffer ([backend citations below](#backend-row-ownership)). | `SystemDictionary::fill_lookup` converts syllables to keys; `ChewingTable::search` allocates a key buffer, fresh value `Vec`, and matching-token `Vec` ([dict], [chewing]). | BDB adds O(`b`) copy/allocation versus borrowing. KC/tkrzw already copy rows at the pin: the definite difference is fresh Rust row/token buffers, not a universally extra O(`b`) copy. Temporary space O(`b+m+l`); backend read cost remains `D_get`. No new DBM open. |
| 4. Filter system row | `ChewingTableEntry::search` equal-range searches, then `convert` compares packed keys and appends token ranges ([chewing_large_table2.h:91–152][pin-search]). | `pinyin_index::item_views` validates row stride and borrows packed keys; `ChewingTable::search` linearly tests records and collects matching tokens ([chewing], [row-format]). | Pin O(`l log r + m l`) on ordered rows, Rust O(`r l`), O(`m`) token storage. Already no per-system-record owned key vector. For the complete, toneless `ni`/`nihao` rows, many/all row records can match, so linear work may be shared; this bound alone cannot explain “2×”. Unsorted/malformed rows are not silently sorted by a proposed repair. |
| 5. Continue to the next window | `SEARCH_CONTINUED` comes from the **same** successful row get, including empty prefix markers ([chewing_large_table2_bdb.cpp:182–191][pin-bdb]). | After lookup, `search_scan_path` calls `phrase_prefix_exists` and the addon equivalent; `key_exists` makes a separate backend existence probe. User queries reconstruct keys and probe again; KC/tkrzw also call `note_user_search` in both lookup and continuation ([engine-lookup], [runtime], [chewing], [user-index]). | Up to an additional `D_exists` and O(`l`) key construction per searched table/path, plus O(`l log u`) user key probes and short guards. The note stores a key for the compatibility side effect; it does not copy the whole row. These repeat each guess. The O(`l`) key construction is a string binary search per syllable: `ChewingKey::from_pinyin` runs `spelling_key` and a binary search over `CONTENT_KEYS` by string compare ([chewing-key]), and one path's syllables are converted four times, each into its own `Vec`: `syllables_to_chewing_keys` in `SystemDictionary::fill_lookup` and again in `SystemDictionary::phrase_prefix_exists` ([dict]), `query_keys` in `UserLookup::lookup` and again in `UserLookup::phrase_prefix_exists` ([user-lookup]). The pin's keys are structs in its matrix ([phonetic_key_matrix.cpp:52–56][pin-matrix]) and its index key a stack array ([chewing_large_table2.cpp:164–172][pin-table-search]). |
| 6. User lookup and matched text | Search the already loaded user table in place, gather token ranges; phrase slots are direct offsets ([facade_chewing_table2.h:120–126][pin-facade], [phrase_index.cpp:180–204][pin-item]). | `GenericUserStore::lookup` now lends `SessionPhrases`. `UserIndex::record` borrows its arena/overlay row; `search` still allocates/decodes `Vec<ChewingKey>` for **every** record and returns matched `(token, keys)` pairs; `UserLookup::entries_for` discards those keys and resolves text ([user-store], [user-index], [user-lookup]). | **#1518 removes the whole-user-table snapshot/build:** lookup acquisition O(1). Remaining row search O(`l log u + r l`), O(`r`) key-vector allocation events, O(`m l`) retained matching keys before disposal, then arena text resolution: O(`m+t`) for dense slots, O(`m log u+t`) for sparse slots. No dictionary-wide first-use build on this session path. |
| 7. Resolve phrase fields | Append lightweight token/span rows; read length and frequency fields; build strings after ranking ([pinyin.cpp:1769–1867,1990–2038][pin-fields]). | `resolve_items`/`resolve_item` decode `PhraseText` before window ordering/token dedup; `append_scan_entries` moves that text into candidates. `PhraseText::phrase_length` counts scalar characters when rank keys are built ([dict], [engine-mod], [ucs4]). | Text construction O(`t`) exists on both sides. Rust performs it earlier, including duplicate token hits later removed, and retains engine text alongside the later C text. Scalar length counting adds O(`t`) traversal versus the pin's byte field. Short `CompactString` text can be inline; do not count every phrase as a heap allocation. `resolve_item` reaches each item through `PhraseLibraries::library`, an `RwLock` read and a `PhraseLibrary` clone (two `Arc` increments and decrements) per token ([libraries]); row 9's frequency read takes the same lock and clone again and rebuilds the `PhraseItemView` for the same token ([libraries], [library]). The pin's two `get_phrase_item` reads are about four loads each and hold no handle ([pinyin.cpp:1972,1857][pin-fields], [phrase_index.cpp:185–204][pin-item]). |
| 8. Restore per-window encounter order | `convert` files ranges by library, `_append_items` walks the fixed library array; no per-window global sort ([chewing_large_table2.h:101–131][pin-search], [pinyin.cpp:1769–1791][pin-fields]). | `order_window_tail` stable-sorts by library nibble and creates a `HashSet` to retain each token's first hit; addon batch is appended and ordered separately ([engine-mod]). | O(`c_w log c_w`) worst-case sort plus expected O(`c_w`) membership work; O(`c_w`) auxiliary space. Fixed 16 library buckets could preserve encounter order without comparison sorting. This is separate from required final frequency ranking. Duplicate tokens arise only when more than one key path reached the window; for a single-path window — both `nihao` windows, `[ni]` and `[ni hao]` — the set is allocated and every row hashed for nothing. |
| 9. Compute unigram frequencies | Direct slot/header and `get_unigram_frequency` reads from the already merged phrase index; total is a stored field ([pinyin.cpp:1855–1866][pin-frequency], [phrase_index.h:100–125][pin-fields-header]). | `candidate_frequencies` → `RuntimeLm::unigram_freq` → `GenericUserStore::unigram_delta`, then base item and bump overlay. `CountCache` retains present count rows; absent rows are reread. `unigram_total` reads session values, and the C shell's zero-total check reads it again ([engine-lookup], [runtime], [user-store], [session-values]). | Extra guards, atomic gates, map lookups and base-plus-delta arithmetic per token. With user data: expected hash O(1) hits; misses O(`log u`) in session `BTreeMap` rows, plus key work. Up to O(`c log u`) miss work; at most O(`min(c,8192)`) present entries retained from a cold cache (`memo_insert` clears at capacity). User-item reads also take the arena guard and an O(1) dense-slot or O(`log u`) sparse-slot lookup ([store-arena], [user-library]). Absent misses recur next guess. Session `get_u64` now borrows with `read_with`: neither native DBM I/O nor an eight-byte value allocation is implied. Empty-user and inactive-bump gates skip much of this. Those gates are open in the worker's `phrases` and `journals` profiles: `populate_user` imports 10,000 rows into libraries 1–4 ([teardown], `select_rows`), which the pin's import turns into reading and unigram changes ([pinyin.cpp:602–605][pin-add-phrase]) saved as MODIFY records; at open, `seed_txn_with_replay` → `seed_system_unigram_deltas` seeds one `UNIGRAM` row per changed slot and `count_tables` sets `has_user_data`, and the non-empty replay sets `has_system_items` ([user-store-libpinyin]). Every candidate then takes the memo path, and a cold first guess inserts every present row (`memo_insert`), the insert cost the rejected memo also paid. Before the memo, `unigram_delta` takes the `SessionPhrases` mutex through `with_user_item` even for a library 1–4 token (a `BTreeMap<u8, _>` miss, [store-arena]), then `with_count_cache` takes the `count_cache` and `db` mutexes and moves the memo out and back ([user-store]): three mutex pairs per candidate. |
| 10. Final ranking | Stable frequency/length sort of candidate rows ([pinyin.cpp:2281–2288][pin-guess], [zhuyin.cpp:1524–1529][pin-zh-guess]). | Allocate frequency and `(RankKey,index)` vectors; `rank_by_frequency` sorts length/span, guards each frequency group against cycles, then sorts groups. `rank_candidates` permutes candidates using the sorted indices ([candidate], [engine-mod]). | Ranking is common O(`c log c`) comparison work, not entirely extra. Rust adds O(`c`) key construction/guards and O(`c`) key storage, then a linear permutation: each displaced index is followed once before its chain is redirected to an unprocessed index. It moves compact keys during sorting rather than full candidates. The cycle guard implements the current policy and must remain; it is not a reason to restore GLib internals. |
| 11. Text dedup and prepends | Construct phrase strings, sort an index array by string, mark zombies, remove each with `g_array_remove_index`, O(`c`) per removal ([pinyin.cpp:2154][pin-dedup]); no sentence rows on this request ([pinyin.cpp:1990–2162][pin-dedup]). | `dedup_ranked_by_text` uses borrowed `PhraseText` keys in a `HashMap`, a keep bitmap and `retain`; prepends find no stored sentence rows; LONGER is disabled ([engine-mod], [engine-lookup]). | Expected O(`c+t`) hashing/compaction and O(`c`) scratch, versus string-comparison sorting and zombie compaction at the pin. This is a trade, **not** a wholly additional sort or string-copy cost. RandomState remains production policy. Collision worst cases are not covered by the expected bound. |
| 12. C snapshot and first observation | Candidate array already owns its strings; count/row/type/string getters return fields/pointers ([pinyin.cpp:2845–2880][pin-getters]). | Both facade builders reserve snapshot row/text buffers, call `phrase_text_bytes`, `CandidateSnapshot::intern`, then `push`/`span_ptr`. Engine window remains retained. Getters borrow the snapshot ([pinyin-sentence], [zhuyin-candidates], [snapshot], [marshal-text]). | Additional O(`s+t`) row/text storage and O(`t`) copy/NUL checks; `span_ptr` validates the terminated slice again. Ordinary scalar text is borrowed by the encoder, not re-encoded during reservation. Stored-word text is encoded once for sizing and again for insertion, adding O(`t`) temporary bytes/work. The first call grows buffers; later calls retain snapshot capacity. Getter fields are O(1); the worker's `strlen`/copy of candidate zero is O(first text bytes) on both sides. |

### Backend row ownership

The copy claim above is deliberately backend-specific. Read:

- [src/storage/chewing_large_table2_bdb.cpp:164–193][pin-bdb]: `DB::get`
  and `set_chunk(..., NULL)` borrow the result. Oxpinyin
  [store/bdb/ffi.rs][bdb-ffi], `Db::get`, copies returned bytes
  into the caller's vector.
- [src/storage/chewing_large_table2_kyotodb.cpp:148–178][pin-kc]: check size,
  resize the entry's chunk, get into it. Oxpinyin
  [store/kyotocabinet/mod.rs][kc-store], `RawReadStore::get_raw`, uses its
  existing backend adapter, while `ChewingTable::search` supplies a fresh vector.
- [src/storage/chewing_large_table2_tkrzwdb.cpp:132–161][pin-tkrzw]: `Get`
  into `std::string`, then `memcpy` into the entry's chunk. Oxpinyin
  [store/tkrzw/mod.rs][tkrzw-store], `RawReadStore::get_raw`, calls `db_get`.
  No claim that the pin borrows this row is valid for tkrzw.

### Outside the unit: a sort word with bit 0x2 clear

The unit's `0x1e` sets `SORT_WITHOUT_LONGER_CANDIDATE` ([pinyin.h:56–65][pin-sort]).
Under a word with that bit clear both sides run a suggestion walk, and the
comparison changes. The pin's `_prepend_longer_candidates`
([pinyin.cpp:1870–1933][pin-longer]) calls `search_suggestion`
([chewing_large_table2_bdb.cpp:550–611][pin-suggest]): a cursor from the
prefix key, each row binary-searched in place by `search_suggestion_internal`,
then per token `get_phrase_item`, one length byte and one unigram word
([pinyin.cpp:1890–1921][pin-longer]). Oxpinyin's `Session::longer_candidate`
([engine-lookup]) builds a fourth scan matrix, `ChewingTable::walk_suggestions`
([chewing]) runs `decode_items` on every row — an owned `Vec<ChewingKey>` per
record — before `prefix_keys_match`, and `phrase_text_for_token` ([runtime])
decodes each token's text to learn its length. Extra under that word: one
O(`M`) build; O(rows · `r`) record decodes with allocation against the pin's
O(rows · (log `r` + `m`)); O(tokens · `t`) text decodes against one byte each.
Proposal 1 (one retained parse) and proposal 3 (borrowed row views) cover the
first two; the length is `PhraseLibrary::item` plus `phrase_length` ([library])
without the text.

## What is first-use work, and what is already gone

The graph and retained scan matrix are created by the first **parse** and
invalidated by input/options changes. The key projection is filled on its first
getter/validation use after that parse. These are input caches, not global
dictionary initialization. Candidate, token, row, rank and hash buffers are
query work; scratch vectors and C snapshots can retain capacity, but
`candidates_at` starts a new result vector and replaces `anchored_window`.
There is no lazy system-table scan or DBM open in this successful guess path.
The lazy `Loaded::item_count` in [data/phrase_libraries.rs][libraries] and
`PhraseLibrary::item_total` in [data/phrase_library.rs][library] are not called
here. Candidate frequency reads use the stored/atomic total, not an item scan.

`CountCache` can allocate on its first present-count miss in a write generation;
its absent-row path does not become warm. This is distinct from the pin's
merged-at-init direct field reads. A second identical guess can reuse graph,
key cache, count hits and capacities while still doing search, ranking, text
dedup and snapshot copies. The old observation that steady typing was cheaper
does not make every row in the table a one-time cost.

The historical attribution's largest named pricing path is **already absent**
from current candidate collection. Main commit `ebc9d4f5` added
`lookup_candidates_into`; `RuntimeDict::fill_lookup(..., false)` and
`SystemDictionary::fill_lookup(..., false)` bypass pronunciation possibility
and effective-system-item repricing. Consequently [user/system_items.rs][system-items],
`SystemItems::get`'s lazy `ChunkItem` decode/cache, is not charged to this
candidate request through that old path. It still exists for other callers,
including sentence/pronunciation work. The pin's candidate frequency loop
also does not compute pronunciation possibility.

Main commit `3b1b7153` already shares the whole-input `Arc<SegmentGraph>`;
`Session::build_graph_at(0, ...)` returns it. The remaining scan projections
and `fewest_keys` calls are distinct from reparsing spellings. The candidate
path already borrows system pronunciation-key views, moves entry text into
candidates, and retains one packed C text buffer rather than one C allocation
per row. Those improvements must not be counted again as proposals.

| Related work | Confirmed state | What it removes; what remains |
| --- | --- | --- |
| #1518 / #1515 | Merged in the examined main. | Session lookup acquisition is O(1), borrowing canonical index/phrase/arena owners; no full user snapshot or prediction dictionary rebuild. Standalone derivation remains generation-cached but is outside `pinyin_init`'s session path. Per-query user record/key/text work in row 6 remains. |
| #1517 single-mapping stack | Root PR open at `7028eea9`; preparation only. | [input/chunk.rs at that head][mapping-prep], `MappingSource::map` consumes the descriptor owner and `PendingMapping::finish` returns one writable mapping after closing it. A later coherent production transition is intended to remove original/replay dual ownership. **The root PR alone removes none of main's runtime costs.** It does not yet establish removal of `SystemItems` decoded caches, count overlays, or candidate copies. |

## Proposed follow-ups, without implementation

Each proposal is internal work subject to the current [source policy][policy];
none changes ranking keys, stable ties, flags, file formats or C pointer lifetime.

1. **Read every topology question from one retained parse.** Keep the selected
   path/parsed length with the graph, project key getters from retained columns,
   and use those columns for `spans_a_matrix_key`. Build Zhuyin's second topology
   only if it differs or a before-cursor operation requires it. Expected effect:
   remove repeated O(`M`) builds, O(`n+e`) length DPs and temporary allocations;
   key/offset queries become projections or direct column reads. Preserve facade-specific split
   rules, zero-key placement and option invalidation. This saves repeated work,
   rather than moving it from guess into init. Let the C-string adapter lend
   the valid UTF-8 prefix to the retained input buffer instead of first
   allocating a temporary `String`: this removes one O(`n`) copy/allocation,
   while retaining validation and the invalid-prefix rule. One constraint on the
   zhuyin facade: `build_matrix_keys` builds with `divided = true`, the zhuyin
   scan in `ensure_matrix` with `full_pinyin() && !collapse` (false) and
   `spans_a_matrix_key` with `!collapse` ([offsets], [engine-matrix],
   [engine-lookup]), so its key list cannot be projected from the retained scan
   matrix without its own retained projection or the parse's own keys; the
   pinyin facade's three builds take identical inputs.
2. **Return continuation with the same search.** Pass `(found/continued,
   matches, unreadable)` through the candidate lookup seam, including the user
   side effect. Expected effect: remove the separate `D_exists` and repeated
   O(`l`) conversions/O(`l log u`) user probes per path. Keep empty markers,
   masks, missing items, and KC/tkrzw last-entry effects exactly as required.
   Reuse the existing record-view search rather than the older owned
   `search_continued` decoder. Convert a path's syllables once: keep the
   `ChewingKey`s beside `ScanBuf::path` in the engine's existing fixed-capacity
   inline vector type (`smallvec` is already an `oxpinyin-engine` dependency,
   so `SmallVec<[ChewingKey; 16]>` adds none) or in a `[ChewingKey; 16]` array
   with a length, pushed and popped with the path, and give the dictionary
   seams a keys-taking entry (`SystemDictionary::trellis_keys` and
   `trellis_item_keys` exist, [dict]); or make `ChewingKey::from_pinyin` O(1)
   through a `SyllableKey`-indexed table over the frozen inventory
   ([chewing-key]). Expected effect: −3`l` string searches and −4 `Vec`s per
   path, O(1) per syllable with the table. No new dependency either way.
3. **Keep pronunciation rows borrowed or in reusable scratch storage.** A
   scoped BDB read callback under the existing owner guard could eliminate
   its extra O(`b`) row copy; the callback must finish before another read on
   that handle, and retain the pin's external call. Reusing backend row/token
   buffers avoids fresh allocations on all backends without claiming to
   eliminate KC/tkrzw's existing copies. Return borrowed user record views or
   tokens for candidates, as the system key-decoding path already does.
   Expected effect: remove O(`r`) per-record key allocations and O(`m l`)
   discarded matching-key storage from user lookup;
   a linear filter can retain O(`r l`) time with O(`m`) token output. An ordered
   range search could additionally reduce selective rows to
   O(`l log r + m l`), but gives no asymptotic gain when the toneless query
   matches the whole row. Preserve raw order/duplicates and recorded malformed
   input dispositions; do not clean or sort stored input to justify a search. Read `unigram_count`
   under the `PhraseLibraries` guard instead of cloning the library, and build
   one `PhraseItemView` per token for text and frequency ([libraries],
   [library]): −2 lock/clone pairs and −1 view per candidate.
4. **Gather lightweight rows in fixed library buckets before text decoding.**
   Replace the per-window library sort with stable bucket append, preserving
   system/user/addon encounter order and first-token retention. Resolve text
   and retain stored length once for surviving token rows. Expected effect:
   grouping O(`c_w`) instead of O(`c_w log c_w`), fewer duplicate text reads,
   no UTF-8 recount for a known length, and smaller intermediate rows. Keep
   unreadable-item checks at their current observable point, even for rows
   later removed; do not skip required errors as an optimization. Count the key paths a window
   searched in `ScanBuf`: a single-path window needs no token dedup at all, and
   when the set is needed keep it in `Scratch`, cleared rather than allocated.
   Expected effect: −1 allocation and −`c_w` hash inserts per single-path
   window, the `RandomState` policy unchanged.

   **Measured and dropped.** The windows arrive already in library order, so
   the stable sort finds one run and moves nothing: on `nihao`, `phrases`,
   tkrzw it costs 2,863 Ir inclusive for the first candidate. Stable bucket
   placement with a retained token set (`a8e7b631`, branch
   `wip/1031-buckets`) measured +44,769 to +45,683 Ir (pinyin) and +47,074 to
   +47,849 Ir (zhuyin) against base `851a6304`. The cost came from the
   retained set's rehash as it grew from empty (`reserve_rehash`, 37,848 Ir
   inclusive, where the per-window set had been allocated at its final size)
   and from moving every row into a bucket and back (+16,206 Ir exclusive in
   `order_window_tail`, plus bucket growth and drain). The sort stays
   required while each key path's lookup appends its system rows and then
   its user rows (`RuntimeDict::fill_lookup`), so a window that several
   paths reach interleaves libraries. Removing the sort belongs with item 5:
   collect each window's rows library by library, as the pin's
   `FacadeChewingTable2::search` files ranges per library and
   `_append_items` walks the libraries in order
   ([facade_chewing_table2.h][pin-facade], [pinyin.cpp:1769–1791][pin-fields]),
   then drop the sort.
5. **Unify live phrase fields with the single-mapping transition.** After its
   activation, expose narrow current-item length/frequency/text reads through
   the canonical owner; eliminate base-plus-session-row reconstruction where
   that item already holds the effective value. Expected effect: remove
   per-candidate count-map/memo misses and dual ownership, without eagerly
   decoding every item. The total must preserve wrapping and failed-call total
   updates. This is a coordinated ownership proposal, not a new negative-cache
   memo: the rejected memo and hasher experiments remain rejected. Keep
   #1517's prepared boundary and its later activation as separate deliverables.
   A narrower option on the same path: at open, fold the seeded libraries 1–4
   unigram deltas into a per-library slot vector or a sorted `(slot, delta)`
   list owned beside the language model and kept current by
   `mark_committed_write`, so `RuntimeLm::unigram_freq` reads one element (or
   one binary search over the changed slots) with no lock when a library
   carries no deltas and inserts nothing on the read path ([runtime],
   [user-store]). Expected effect: −3 mutex pairs, −1 hash operation and −1
   b-tree descent per candidate in a profile with seeded deltas; memory
   O(changed slots), already held as `UNIGRAM` rows. **Sequencing:** this moves
   where the user store's counts are owned, so it is sequenced with #1517's
   members 2 and 3, which rework the user store's ownership; it does not land
   ahead of them as a separate overlay. It is not the rejected memo: no
   read-path insert.
6. **Reuse result/rank storage and reduce snapshot passes.** Refill the anchored
   candidate buffer and retain scratch rank storage where lifetimes permit;
   keep only data needed for selection in the engine. Use a boundary encoding
   length operation or retained encoded bytes for Stored text so reservation
   does not encode twice, and establish NUL validity once before appending.
   Expected effect: fewer per-guess allocations and O(`t`) passes/copies;
   construction still needs O(`s+t`) C-visible output. Preserve candidate
   pointer invalidation rules and all Stored-word encodings. This is not
   adoption of the stopped snapshot-append checkpoint. The frequency cycle
   guard and stable ranking remain required; final sorting is shared work.

No timing priority or percentage saving is inferred from these proposals.
The precise question source cannot settle is: **on the same data and backend,
what portion of the historical first-candidate wall/Ir excess survives at
`f3d08577`, and what portion comes from each remaining cost?** That requires
execution and is left unanswered. Investigation stops at the source arguments;
no workload, profiler, benchmark, build or differential was run.

Documentation gate: review this text and its source/issue links, and run
`git diff --check`. This finding references #1031; it neither closes the issue
nor claims its numerical acceptance criteria have been met.

[policy]: ../source-policy.md#internal-implementation-and-performance
[worker]: ../../tools/workers/startup.c
[runtime]: ../../crates/oxpinyin-runtime/src/lib.rs
[library]: ../../crates/oxpinyin-data/src/phrase_library.rs
[libraries]: ../../crates/oxpinyin-data/src/phrase_libraries.rs
[persistence]: ../../crates/oxpinyin-user/src/persistence.rs
[replay]: ../../crates/oxpinyin-user/src/replay.rs
[instance]: ../../crates/oxpinyin-facade/src/instance.rs
[input]: ../../crates/oxpinyin-capi-marshal/src/input.rs
[capi-parse]: ../../crates/oxpinyin-capi/src/parse.rs
[facade-parse]: ../../crates/oxpinyin-facade/src/parse.rs
[buffer]: ../../crates/oxpinyin-engine/src/session/buffer.rs
[engine-state]: ../../crates/oxpinyin-engine/src/session/state.rs
[engine-cursor]: ../../crates/oxpinyin-engine/src/cursor.rs
[zhuyin-sentence]: ../../crates/oxpinyin-zhuyin-capi/src/sentence.rs
[store-arena]: ../../crates/oxpinyin-user/src/store_arena.rs
[user-library]: ../../crates/oxpinyin-user/src/user_library.rs
[graph]: ../../crates/oxpinyin-core/src/graph.rs
[engine-matrix]: ../../crates/oxpinyin-engine/src/session/matrix.rs
[offsets]: ../../crates/oxpinyin-engine/src/session/offsets.rs
[facade-cursor]: ../../crates/oxpinyin-facade/src/cursor.rs
[engine-lookup]: ../../crates/oxpinyin-engine/src/session/lookup.rs
[engine-mod]: ../../crates/oxpinyin-engine/src/session/mod.rs
[candidate]: ../../crates/oxpinyin-engine/src/candidate.rs
[dict]: ../../crates/oxpinyin-data/src/dict.rs
[chewing]: ../../crates/oxpinyin-data/src/chewing_table.rs
[row-format]: ../../crates/oxpinyin-data/src/row_format.rs
[user-store]: ../../crates/oxpinyin-user/src/store.rs
[user-index]: ../../crates/oxpinyin-user/src/user_index.rs
[user-lookup]: ../../crates/oxpinyin-user/src/lookup.rs
[session-values]: ../../crates/oxpinyin-user/src/session_values.rs
[system-items]: ../../crates/oxpinyin-user/src/system_items.rs
[ucs4]: ../../crates/oxpinyin-core/src/ucs4.rs
[pinyin-sentence]: ../../crates/oxpinyin-capi/src/sentence.rs
[zhuyin-candidates]: ../../crates/oxpinyin-zhuyin-capi/src/candidates.rs
[snapshot]: ../../crates/oxpinyin-facade/src/snapshot.rs
[marshal-text]: ../../crates/oxpinyin-capi-marshal/src/phrase_text.rs
[bdb-ffi]: ../../crates/oxpinyin-store/src/bdb/ffi.rs
[kc-store]: ../../crates/oxpinyin-store/src/kyotocabinet/mod.rs
[tkrzw-store]: ../../crates/oxpinyin-store/src/tkrzw/mod.rs
[mapping-prep]: https://github.com/shenghaoc/oxpinyin/blob/7028eea95de6bf4345c3d8db1b659ee09725d2fa/crates/oxpinyin-input/src/chunk.rs
[pin-init]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L325-L443
[pin-zh-init]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/zhuyin.cpp#L269-L358
[pin-facade]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/facade_chewing_table2.h#L75-L128
[pin-map]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/include/memory_chunk.h#L475-L520
[pin-load]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L234-L323
[pin-replay]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_index.cpp#L207-L517
[pin-alloc]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L1310-L1340
[pin-parse]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L1497-L1527
[pin-guess]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L2184-L2308
[pin-zh-parse]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/zhuyin.cpp#L1017-L1042
[pin-zh-guess]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/zhuyin.cpp#L1460-L1539
[pin-parser]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/pinyin_parser2.cpp#L217-L343
[pin-matrix]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phonetic_key_matrix.cpp#L30-L80
[pin-search]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2.h#L91-L152
[pin-bdb]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_bdb.cpp#L164-L193
[pin-kc]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L148-L178
[pin-tkrzw]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_tkrzwdb.cpp#L132-L161
[pin-item]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_index.cpp#L180-L204
[pin-fields]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L1769-L2038
[pin-frequency]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L1855-L1866
[pin-fields-header]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_index.h#L100-L125
[pin-dedup]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L1990-L2162
[pin-getters]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L2845-L2880
[pin-scan]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phonetic_key_matrix.cpp#L350-L437
[chewing-key]: ../../crates/oxpinyin-chewing/src/chewing_key.rs
[teardown]: ../../tools/bisection/teardown.py
[user-store-libpinyin]: ../../crates/oxpinyin-user/src/store_libpinyin.rs
[pin-parser-input]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/pinyin_parser2.cpp#L234
[pin-parser-trial]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/pinyin_parser2.cpp#L164-L214
[pin-parser-loop]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/pinyin_parser2.cpp#L268-L280
[pin-table-search]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2.cpp#L161-L176
[pin-add-phrase]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L602-L605
[pin-sort]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.h#L56-L65
[pin-longer]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L1870-L1933
[pin-suggest]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_bdb.cpp#L550-L611
