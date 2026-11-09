# Pin aborts reproduced: the short user-bigram total and the directory library

Status: implemented, 2026-10-31 UTC (member 5c of the stack that retires
class (c); `date -u` at writing: 2026-10-31 13:31 UTC). Rule: [compatibility
policy, class (c)
retired](compatibility-policy.md#amendment--class-c-retired-pin-aborts-reproduced-2026-10-31-utc).
Pin: libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`, read from a
`git archive` of the pin; `check_result` is `assert` in the reference build
(`src/include/pinyin_utils.h:27-31`). The earlier members' records are
[pin-aborts-pinyin-facade.md](pin-aborts-pinyin-facade.md),
[pin-aborts-zhuyin-facade.md](pin-aborts-zhuyin-facade.md),
[pin-aborts-journal-replay.md](pin-aborts-journal-replay.md) and
[library-transitions-in-memory.md](library-transitions-in-memory.md).

Rows 85 and 96 were the last two registered class (c) rows; row 110, the
Kyoto Cabinet read after a size check, was a class (c) refusal the register
did not list, and this member registers and reproduces it too. Each refusal (`false`,
NULL or a refused export, with one warning) is deleted. The shared Rust
facade returns a typed fault at the pin's point, after the pin's preceding
work; both C adapters call `std::process::abort()` on it. Every site is an
`assert` (or `check_result`): the missing glibc assertion line is
POLICY-ACCEPTED, the SIGABRT is MATCH.

## Row 85: a user-bigram value shorter than its `guint32` total

`SingleGram::get_total_freq` (`src/storage/ngram.cpp:48-51`) reads the
four-byte total through `MemoryChunk::get_content<guint32>`, whose
`assert(get_content(offset, &value, sizeof(T)))` (`include/memory_chunk.h:390`)
fails on a value of 0 to 3 bytes. No public call writes such a value; a
foreign `user_bigram.db` can hold one. The pin's user bigram is an in-memory
DBM written only at save, so no path below has observable work before its
abort: training and predicted training change memory only.

| Path | Pin site | oxpinyin item that returns the fault | Adapter |
|---|---|---|---|
| `pinyin_mask_out`, `zhuyin_mask_out` | `pinyin.cpp:1230`, `zhuyin.cpp:765` → `Bigram::mask_out` (`ngram_bdb.cpp:217-254`) → `SingleGram::mask_out` `check_result(get_total_freq)` (`ngram.cpp:80`), for a key the mask leaves to be loaded | `gram_mask_fault` → `UserStoreError::ShortUserBigramValue` → `MaskOutOutcome::ShortUserBigramValue` | both `config.rs` abort |
| `pinyin_remove_user_candidate` | `pinyin.cpp:3766` → the same mask walk | `GenericUserStore::remove_user_phrase` (`arena_remove`) → `ShortUserBigramValue` | `candidates.rs` aborts |
| `pinyin_guess_predicted_candidates` (and the punctuation variant) | `_compute_predicted_bigram_candidates`, `pinyin.cpp:2321-2341`: the prefixes from last to first, `merge_single_gram` then `get_length`, which reads the total of an item-less merged gram (`ngram.cpp:62-70`) | `predict.rs` `predicted_bigram_items`, at the same step of the walk | aborts in the walk |
| `pinyin_choose_predicted_candidate` | `pinyin.cpp:2630`, after the unigram add (`:2609-2612`, whose overflow returns `false` first) | `GenericUserStore::update` (`SeedPolicy::Predicted`): the unigram add, then `ShortUserBigramValue` | `candidates.rs` aborts |
| `pinyin_train`, `zhuyin_train` | `train_result3`: `get_total_freq` (`phonetic_lookup.h:884`), and `check_result(user->set_freq(token, freq + seed))` (`:903`) on an unsorted gram | `check_training_step` → `TrainingFault::{ShortValue, SetFreqMiss}` → `EngineError::{ShortTrainingPredecessor, TrainingSetFreqMiss}`, per step in the pin's order | both `candidates.rs` abort |
| `*_guess_sentence`, `*_guess_sentence_with_prefix`, `*_guess_candidates`, `*_phrase_segment` | `merge_single_gram` reading both totals (`ngram.cpp:303-304`), `SingleGram::search` (`:165`), the `DYNAMIC_ADJUST` term (`pinyin.cpp:1848`, `zhuyin.cpp:1250`) | `EngineError::ShortLookupPredecessor` from the merged-gram reads (unchanged) | `refuse_lookup_gram` (both `sentence.rs`, used by `phrase.rs`) aborts on it |
| the bigram export iterator | `pinyin_bigram_iterator_has_next_phrase` → `load(…, true)` and `retrieve_all`'s `check_result(get_total_freq)` (`pinyin.cpp:889`, `ngram.cpp:133-141`) | `BigramExportWalk::has_next` → `ShortGramTotal`; `get_next` → `BigramStep::ShortTotal` | `iterators.rs` aborts in `has_next` and `get_next` |

### The row 98 / row 100 split

The old `UserStoreError::ShortUserBigramValue` and `MaskOutOutcome::ShortUserBigramValue`
carried two conditions: a value shorter than four bytes (row 85) and a
value of `4 + 8k + r` bytes, `1 <= r <= 7`, whose partial item the pin's
`cur != end` walks read past (rows 98 and 100, class (b)). They are split:
`ShortUserBigramValue` is now the row 85 value only, and the new
`UserStoreError::PartialUserBigramTail` / `MaskOutOutcome::PartialUserBigramTail`
is the partial tail. The class (b) outcomes are unchanged: `*_mask_out`,
`pinyin_remove_user_candidate` and `pinyin_choose_predicted_candidate` answer
`false` with the same single warning as before, `*_train` keeps
`UnsafeTrainingPredecessor`'s warning, the lookups keep
`UnsafeLookupPredecessor`'s, the predicted walk keeps its partial-tail
warning, and `pinyin_begin_get_bigram_phrases` keeps its NULL for a partial
tail under any predecessor. The engine already split the two
(`TrainingFault::{ShortValue, PartialTail}`, `oxpinyin_core::GramFault::{Short, OverRead}`).
A three-byte sentence-start gram read by a one-step constraint's `get_freq`
is still row 98 (`get_freq` starts four bytes in and never reads the total,
`ngram.cpp:239-248`), as before.

### The export walk position

The pin's begin (`pinyin.cpp:775-787`) only calls `get_all_items`, whose
key-size assert is row 86 (still a begin-time abort). It reads no value.
`has_next` takes the predecessors from the front of that array, loads each
gram and calls `retrieve_all`, which reads the total before its item walk,
so the abort comes at the first short predecessor in walk order, after the
rows of every predecessor before it were returned to the caller (and
`get_next` returns `has_next`, so it can abort inside `get_next` too,
after it copied out its own row). The old begin preflight refused the whole
export with NULL. `BigramExportWalk` now returns `ShortGramTotal` where its
state machine loads the predecessor, and begin answers the iterator.

The walk order is the backend's in-memory container order: BDB's
in-memory hash, Kyoto Cabinet's `StashDB` and tkrzw's `TinyDBM`
(`ngram_bdb.cpp`, `ngram_kyotodb.cpp`, `ngram_tkrzwdb.cpp` `load_db` and
`get_all_items`). oxpinyin's session keeps its grams in the same backend's
container and `bigram_predecessors` walks it (unchanged by this member; the
export cases of rows 36 and 86 and #1114 hold the order). The new case is built so its verdict needs no order.

Every row the pin hands out goes through row 1's `g_strjoinv` over an
unterminated `GPtrArray` (`pinyin.cpp:844-863`, class (b)): it reads
uninitialised heap past the array, so the pin may join a garbage word,
fault, or abort in `g_strfreev`'s `free`. A first version of the case
exported whole rows before the short value and wrote each to stderr; on
tkrzw one pin run handed out 15 rows and the next 8, both ending in SIGABRT,
while the subject always handed out 15. That is the row 1 undefined read,
not shown to be a varying walk: the pin's tkrzw user bigram is a `TinyDBM`
filled by `copy_tkrzwdb` from the `HashDBM` file in that file's iterator
order (`ngram_tkrzwdb.cpp:48-63`, `tkrzwdb_utils.h:48-74`) and walked by
`ProcessEach` (`:161-172`), a hash table of a fixed default bucket count
(`DEFAULT_NUM_BUCKETS = 1048583`, `tkrzw_dbm_tiny.h:42` of the provisioned
libtkrzw at `9ee84164`), and the subject's container is the same library.
The tkrzw `.cc` sources are not on this host, so the walk's determinism is
argued from that structure, not read line by line; the row 1 read alone
accounts for the pin's varying count. The case therefore renders no row: its twelve whole predecessors
each hold one item of count 1, under the export threshold (68,
`pinyin.cpp:792-793`), so `has_next` loads and skips them in whatever order
the backend walks them and loads the three-byte value under `0x02001234` in
the same call (the load precedes the loop's `m_items->len` test, so its
position does not matter), and both sides abort with no row handed out, on
bdb, kc and tkrzw alike. The probe asserts that begin answers an iterator,
so the old begin-time refusal differs. The rows handed out before the abort
are held by `a_short_predecessor_faults_where_the_walk_loads_it`
(`export_rows.rs`), where no C-side join is involved.

## Row 96: a system library that is a directory

`MemoryChunk::mmap` (`include/memory_chunk.h:470-520`) takes
`file_size = lseek(fd, 0, SEEK_END)`; below the 8-byte header it answers
`false` (row 76, class (b), unchanged). From 8 up it reads the length word,
`read` fails with `EISDIR`, and `assert(ret_len == sizeof(length))`
(`:493`) kills the pin. Which branch a directory reaches is the file
system's: ext4 reports the htree end of file for any directory, btrfs twice
the summed entry-name length, tmpfs refuses `SEEK_END`. The size probe is
Unix only, as before.

| Path | Pin site and preceding work | oxpinyin item that returns the fault | Adapter |
|---|---|---|---|
| `pinyin_init`, `zhuyin_init` | The default-library loop (`pinyin.cpp:377-392`, `zhuyin.cpp:318-334`) maps each library (`_load_phrase_library`, `pinyin.cpp:255`, `zhuyin.cpp:199`) and merges its `.dbin` before the next. Preceding work: `check_format`'s writes; the earlier libraries are mapped and merged in memory, and one of their merges can assert first (rows 91, 108). | `library_loop::run` walks the default rows in order, so it maps the library only after the earlier logs merged, and stops there with `LibraryLoopStop::Library`; the session open returns `UserStoreError::LibraryLoop` before its fini guard is armed (so `check_format`'s raised open counter stays on disk, as the pin leaves it), and `Runtime::open_with_law` returns `OpenError::Dict(…ChunkHeaderRead)` (or the earlier `OpenError::Journal`) before the language model is opened | both `context.rs` abort (the warning is deleted) |
| `*_load_phrase_library` | `pinyin.cpp:255`, `zhuyin.cpp:199` at the reload, before the merge; nothing precedes it | `ContextCore::load_phrase_library` → `LibraryLoadFault::Row(ChunkHeaderRead)` (unchanged point) | both `config.rs` abort |
| `pinyin_load_addon_phrase_library` | `pinyin.cpp:289` | `load_system_addon_reporting` → `LibraryRowAssert::ChunkHeaderRead` (unchanged point) | `state.rs` aborts |
| `*_save` | `_write_files` maps each loaded system library in the library loop (`pinyin.cpp:955`, `zhuyin.cpp:588`). libpinyin has written the earlier libraries' `.tmp` files and renamed none (`_rename_files` runs after, `pinyin.cpp:1141`); libzhuyin has renamed each earlier library's file as it wrote it (`zhuyin.cpp:609`, `:635`). | the save's baseline hook (`Runtime::save_disk_baseline` → `SaveRemapFault::ChunkHeaderRead` → `SaveBaseline::ChunkHeaderRead`) at `persistence::stage_rest`'s library loop, which returns `PersistenceError::LibraryChunkHeaderRead` there → `UserStoreError::LibraryChunkHeaderRead` → `SaveOutcome::ChunkHeaderRead` | both `context.rs` abort |
| `*_mask_out` | The user tables first (`pinyin.cpp:1228-1230`, `zhuyin.cpp:763-765`; rows 84 to 87 abort there), then each loaded library is mapped (`pinyin.cpp:1264`, `zhuyin.cpp:799`) before its `merge_with_mask` | `Runtime::reload_system_libraries` stops at the library and answers it; the facade merges the libraries mapped before it (a journal fault of theirs first), then returns `MaskOutOutcome::ChunkHeaderRead` | both `config.rs` abort |

The `mmap %s failed!` line of a library the pin cannot map (row 76) is now
written where the save's and the mask's loops reach that library, through
the same hooks, instead of in an up-front pass. A row 76 library before a
directory library is written and the loop continues (class (b): the pin
dies of SIGSEGV there), so the later directory aborts. Directory size decisions are covered by filesystem-independent Rust tests.

### The save's staged files

The save goes through `UserStore::save_reporting_with_baseline` →
`persistence::save_reporting`, the lenient save the pin's `_write_files`
and `_rename_files` model. On `LibraryChunkHeaderRead` it returns from
`stage_all` at once: no rename pass, no `user.conf`, and no clean-up, so the
user directory keeps exactly the pin's files (libpinyin: the earlier
libraries' `.tmp`; libzhuyin: the earlier libraries renamed, by
`Staged::push` as each was written). A failed write or rename leaves every
staged `.tmp` in place as the pin does, and the save carries on and reports
each failure. The strict Rust save that stopped at its first failure was
retired (2026-11-09 UTC). Row 96 therefore needs no change.

## A Kyoto Cabinet bigram read after its size check

Row 110. The KC build's `Bigram::load` (`src/storage/ngram_kyotodb.cpp:124-143`
at 074a2219) sizes the record with `m_db->check` and returns `false` for a
miss, then reads the body with `check_result(vsiz == m_db->get(kbuf,
sizeof(phrase_token_t), vbuf, vsiz))` (`:136-137`). `check_result` is
`assert` in the reference build, so a body read that fails or returns
another size after a successful check aborts. Only the KC build has this
two-step read; BDB and tkrzw read once.

oxpinyin's KC store reports it as `StoreError::ReadAfterCheck`
(`KcDb::get_bigram`, `crates/oxpinyin-store/src/kyotocabinet/ffi.rs`). It
reaches the engine only through the system bigram
(`BigramTable::load_raw_successors`): the user bigram is read from memory
after the open, so the pin's user-bigram loads cannot meet it.

| Path | Pin site | oxpinyin item that returns the fault | Adapter |
|---|---|---|---|
| `*_phrase_segment` | `PhraseLookup` loads both grams (`phrase_lookup.cpp:192`, `:269`) | `step_costs` → `EngineError::row_read` → `ReadAfterCheck` (since #1166) | `refuse_lookup_gram` aborts (was: `false` and one warning) |
| `*_guess_sentence`, `*_guess_sentence_with_prefix` | `search_bigram2` (`phonetic_lookup.h:595`) | the n-best predecessor rows (`nbest.rs`) → `EngineError::row_read` (was: a scoring error) | `refuse_lookup_gram` aborts (was: `false`, no warning) |
| `pinyin_guess_candidates`, `zhuyin_guess_candidates_{after,before}_cursor` | the `DYNAMIC_ADJUST` row (`pinyin.cpp:2212`, `zhuyin.cpp:1485`, `:1579`) | `Session::dynamic_adjust_gram` → `EngineError::row_read` (was: a scoring error) | `refuse_lookup_gram` aborts (was: `false`, no warning) |

The pin's other `Bigram::load` callers load only the user bigram: the export
(`pinyin.cpp:888`), predicted guessing (`:2326`) and choosing (`:2623`),
training (`phonetic_lookup.h:878`), and KC `Bigram::mask_out`'s
`check_result(load(...))` (`ngram_kyotodb.cpp:216`) under `*_mask_out` and
`pinyin_remove_user_candidate`. oxpinyin answers those from memory, so no
C entry point there can meet the fault.

No differential can make a KC read fail after its check succeeded, so no
contract case holds it. The death tests
`phrase::tests::a_read_failure_after_the_size_check_aborts` (both adapters)
hold the abort, and the engine tests
`row_read_names_only_the_checked_read_failure` and
`a_checked_read_failure_is_the_predecessors_read_after_check_fault` hold the
fault the engine returns.

### Audit of the C adapters

Every site in `oxpinyin-capi` and `oxpinyin-zhuyin-capi` that logs a warning
or answers `false`/NULL on an error was read against the pin. Apart from row
110, each is class (b) (rows 76, 80, 98, 100, 106) or an oxpinyin failure the
pin has no counterpart for (a graph, dictionary or store failure, an
allocation failure in the returned-string encoders, which member 6
replaces). Every site where the pin asserts or aborts already calls
`std::process::abort()`. Stale doc comments that still described a class (c)
refusal (`predict.rs`, `aux_matrix.rs`, both `state.rs`, both `lib.rs`) are
corrected. `UserStoreError::EmptyUserBigramGram` (`ngram.cpp:70`) is never
constructed.

## Corrections after the external review (2026-10-31 UTC)

- Row 85, the bigram export's `get_next`: the pin writes `*phrase`,
  `*pinyin` and `*count` (`pinyin.cpp:904-906`), advances, and only then runs
  the `has_next` that loads the next predecessor and asserts on a short total
  (`:910`, `:889`). `BigramStep::Row` now carries that `has_next`'s answer or
  fault, and the C adapter writes the row before it aborts. Held by
  `bigram_get_next_writes_its_row_before_the_short_predecessor_aborts`, which
  reads the out-params back from a page the child shares.
- Rows 85 and 98, the export begin: the begin no longer refuses when any
  predecessor has a partial item tail; the pin's begin reads no gram
  (`:775-787`). `BigramExportWalk` reports `GramLoadFault::ShortTotal`
  (abort) or `GramLoadFault::PartialTail` (class (b): one warning and
  `false`, the walk ends) where it loads that predecessor, so a short value
  earlier in the walk aborts first. Held by
  `bigram_export_aborts_at_a_short_predecessor_before_a_partial_one` and
  `bigram_export_ends_at_a_partial_predecessor`; policy row 98 is amended.
- Rows 91, 92, 96, 97 and 108 at `mask_out`: the pin maps, loads and merges
  one library at a time in index order (`pinyin.cpp:1234-1285`,
  `zhuyin.cpp:769-820`), so a merge that asserts in an earlier library comes
  before a later library's `mmap %s failed!` line. `Runtime::reload_system_libraries`
  now lists each unmappable file with its sub-index (`SystemRemap`), and the
  facade writes the lines the loop reached: through a journal fault's
  library, before a directory library, all of them otherwise. Init reports
  the same way (`Runtime::open_with_law_reporting`, through the journal
  fault's library or before the directory library); a reload maps one
  library, writes its line and then merges it.
- Row 110: `pinyin_guess_candidates` sends `EngineError::ReadAfterCheck` from
  the `DYNAMIC_ADJUST` row to `refuse_lookup_gram`, which aborts; it had
  fallen to the generic arm and answered `false`.

## Audit of the reproduced rows (2026-10-31 UTC)

Read against the pin at 074a2219, for each row: does oxpinyin abort under
exactly the inputs, options and offsets for which the pin reaches its assert
or `abort()` (predicate scope), and is the pin's earlier observable work done
first (order)? "Fixed" names the change in this stack's members that made it
so; every other row read as exact.

| Row | Predicate scope | Order | Verdict |
|---|---|---|---|
| 4 | Fixed (member 3): the engine's search leaves decide it, for the key paths the pin searches (guess candidates' span loop and LONGER walk, the sentence searches); zhuyin reaches the same searches | nothing observable first | OK after fix |
| 5a, 5c, 5d | exact | in-memory only | OK |
| 6 | exact | in-memory only | OK |
| 10, 14, 19, 21 | exact | nothing written first | OK |
| 38, 44, 45 | exact | nothing written first | OK |
| 61 | exact | in-memory only | OK |
| 62 | exact | Fixed (member 3): the export iterator writes NULL, NULL, -1 first (`pinyin.cpp:703`); the bigram half is row 85's (member 5c) | OK after fix |
| 63, 66 | Fixed (members 3, 4): `SIZE_MAX` on an empty matrix answers `false` in the key getters | nothing written first | OK after fix |
| 74, 93 | Fixed (members 3, 4): the token asserts are decided for any library index; the pinyin import parses the whole reading, where it had refused one over 65535 bytes before the walk; the pin stops at byte 32767 (`pinyin_parser2.cpp:67-70`, `:248`, `:292`), a language artefact since #1501 removed row 59's bound from every entry point with the graph's 65535-byte refusal ([1501-gint16-input-cap](1501-gint16-input-cap.md), 2026-11-01 UTC); the zhuyin import is unbounded at both. Row 74's other `reduce_tokens` callers completed silently and now abort (members 3, 4; see below) | the plan only reads; the lookups abort after their clear and appends | OK after fix |
| 77 | exact | the `.tmp` files are written per library in loop order | OK |
| 79 | exact | Fixed (member 3): the lines of the libraries mapped before the fatal row come first | OK after fix |
| 81 | exact | Fixed (member 3): the removal asserts come before the bigram fault | OK after fix |
| 82 | Fixed (member 3, 2026-11-01 UTC): `Session::plan_spans` stops at the store's length, where the pin's walk stops (`phonetic_lookup.h:858`; the pin-built `pinyin_train` reloads `m_constraints->len` at each step). A NORMAL choose after a shorter re-parse validates the store in the pin (`pinyin.cpp:2579`, `zhuyin.cpp:1647`), clearing a forcing whose end reaches the new length; the engine's choose only resizes it, so that forcing survived, `train_next` carried from it to the retained row's next phrase past the store, and the walk aborted at `:663` where the pin trains. With the bound the surviving forcing changes nothing the walk does | in-memory only | OK after fix |
| 84, 94, 95 | exact | nothing written first | OK |
| 85 | exact | Fixed (member 5c): the export's out-params, the begin's partial-tail scan, the removal order | OK after fix |
| 86, 87, 88, 89, 90 | exact | nothing written first | OK |
| 91, 92, 97, 108 | Fixed (member 5a): the masked walk keeps why it ended | Fixed (members 5a, 5c): init and `mask_out` write only the lines the loop reached | OK after fix |
| 96 | exact | Fixed (member 5c): as rows 91 and 108 at `mask_out` and init; an earlier library's journal fault (row 91) is the one reached before a later directory library, at both, with no line first (checked 2026-11-01 UTC) | OK after fix |
| 103 | the pin has no failing site | nothing written | OK |
| 104 | exact | nothing written first | OK |
| 110 | Fixed (member 5c): guess candidates aborts too | nothing written first | OK after fix |

### `reduce_tokens` call sites (2026-11-01 UTC)

Every call in both facades, at 074a2219. A validating call (the default
`validate = true`) asserts `0 <= num && num <= 4` (`phrase_large_table3.h:95`)
after it has cleared and filled its array. "More than four" means more than
four loaded libraries hold the searched string. Before this pass, the 40-row
audit checked for sites that warn or refuse where the pin aborts; the sites
marked "silent" instead completed normally where the pin aborts.

| Pin call site | Validates | Reached from | oxpinyin before | Now |
|---|---|---|---|---|
| `pinyin.cpp:541` (`_add_phrase`) | yes | `pinyin_iterator_add_phrase` | aborts (`plan_import`) | unchanged |
| `zhuyin.cpp:427` (`_add_phrase`) | yes | `zhuyin_iterator_add_phrase` | aborts (`plan_import`) | unchanged |
| `pinyin.cpp:2664` | yes | `pinyin_lookup_tokens` | aborts, before the clear and appends | aborts after them (member 3) |
| `zhuyin.cpp:1690` | yes | `zhuyin_lookup_tokens` | silent | aborts after the clear and appends (member 4) |
| `pinyin.cpp:1415` (`_compute_prefixes`) | yes | `pinyin_guess_sentence_with_prefix` (`:1434`), `pinyin_guess_predicted_candidates` and `_with_punctuations` (`:2424`) | silent | aborts on `compute_prefixes`' count error, in its single pass (member 3) |
| `zhuyin.cpp:933` (`_compute_prefixes`) | yes | `zhuyin_guess_sentence_with_prefix` (`:953`), before the matrix is read | silent | aborts before the empty-matrix refusal (member 4) |
| `phrase_large_table3.h:106` (`get_first_token`) via `pinyin.cpp:3117` | yes | `pinyin_get_character_offset` (`:3221`), `pinyin_remember_user_input` (`:3688`) | silent; `remember_user_input` read the keys first | aborts on `EngineError::ReduceTokensCount` and per character, before the keys (member 3) |
| the same via `zhuyin.cpp:2080` | yes | `zhuyin_get_character_offset` (`:2176`) | silent | aborts on `EngineError::ReduceTokensCount` (member 4) |
| `pinyin.cpp:1885` (`_prepend_longer_candidates`) | no | `pinyin_guess_candidates` | completes, no bound | unchanged: no assert |
| `pinyin.cpp:2386` (`_compute_predicted_prefix_candidates`) | no | the predictions | completes, no bound | unchanged: no assert |

Cases: `combined-library-sixth-add-key-past-parse-bound-pinyin`,
`combined-library-sixth-add-one-key-zhuyin`, `prefix-guess-five-libraries-*`,
`predicted-candidates-five-libraries-pinyin`,
`character-offset-five-libraries-*`, `remember-user-input-five-libraries-pinyin`
and `-unparsed-pinyin` (all `sigabrt=True`); controls
`prefix-guess-four-libraries-*` and `character-offset-four-libraries-*`. A NULL
`tokenarray`'s GLib criticals go to the driver's log handler, which an
abort loses, so no case compares the lookups' order against them.
`storage/tag_utility.cpp:350` (`taglib_string_to_token`) is outside the
facades: the pin's tools reach it, and it was not audited here.

Not settled by reading, outside these rows: a user phrase-table token with
high bits set (the pin files it under `PHRASE_INDEX_LIBRARY_INDEX`'s four
bits), `pinyin_choose_candidate` with a candidate pointer outside the current
list (oxpinyin answers `-1` before row 45's check), and a system library's
user `.dbin` that is a directory.

## Cases and dispositions

`*` is `pinyin` and `zhuyin`. All are `sigabrt=True` cases of the existing
`contract-diff.py` driver.

| Row | Cases | Disposition |
|---|---|---|
| 85 | `abort-mask-out-short-bigram-value`, `abort-zhuyin-mask-out-short-bigram-value`, `abort-remove-user-candidate-short-bigram-value`, `abort-guess-predicted-short-bigram-value`, `abort-guess-sentence-short-user-gram-{system-row-*,no-system-row}`, `abort-guess-candidates-dynamic-adjust-short-user-gram-*`, `abort-phrase-segment-short-user-gram`, `abort-phrase-segment-prefix-short-user-gram-*`, new `abort-bigram-export-short-predecessor` | POLICY-ACCEPTED (assertion line) |
| 85 controls | `mask-out-short-bigram-value-masked-key`, `guess-predicted-unrelated-short-bigram-value`, `guess-unrelated-short-user-gram`, `guess-candidates-short-user-gram-dynamic-adjust-off`, `parse-dynamic-adjust-short-user-gram-*`, `phrase-segment-prefix-unrelated-short-user-gram-*` | MATCH |
| 96 | Directory-as-chunk size and header-read decisions | Rust unit tests exercise the size gate and failed read on every filesystem |
| 96 controls | `library-directory-guard-{reserved,unloaded}-{save,mask-out}[-zhuyin]` | MATCH |
| 91 before 96 | `abort-init-log-header-token-later-directory-*` (`userdir=True`), `abort-mask-out-log-header-token-later-directory-*`, both with `stderr=True`; library 2's chunk is missing too, so a subject that reached library 4 first would write its `mmap %s failed!` line | POLICY-ACCEPTED on every filesystem; the pin dies in library 1 before examining library 4, and so does the subject, which walks the rows in the same order. |

`abort-train-unsorted-set-freq-miss-{pinyin,zhuyin}` stay POLICY-ACCEPTED
under [1297-gram-partition-training.md](1297-gram-partition-training.md):
oxpinyin's `partition_point` search finds the item `insert_freq` added, so
training answers `true` where the pin's `lower_bound` misses and asserts.
That difference is the 2026-10-29 UTC "no ported external-library
internals" ruling's, recorded there; the subject never reaches its
`TrainingSetFreqMiss` on that fixture, so the cases cannot be `sigabrt`.
A miss that oxpinyin's own search does reach is the abort
(`check_training_step`, held by the store test).

The save cases of both facades hold the user directory at the abort: the
directory library is `merged.bin`, library 4, after libraries 1 to 3.

## The init library loop (2026-11-06 UTC)

The pin's init handles each default row completely before it looks at the
next: it judges the row (`pinyin.cpp:388`), maps the chunk (`:254-256`),
merges the log (`:268-276`) or reads the `USER_FILE` chunk (`:303-318`).
`zhuyin.cpp:318-334` and `:198-220` do the same. Read at `074a2219`.
Before this change oxpinyin judged every row first, mapped every library and
attached the bigram, and replayed the logs afterwards. Where library N's log
asserts, that differed from the pin as follows. No differential recorded
these inputs; they follow from the source and are held by the tests below.

| Input | Pin | oxpinyin before |
|---|---|---|
| Later library unmappable | SIGABRT, no line | That library's `mmap %s failed!`, NULL and a warning, no abort |
| Later library with a layout read past its chunk (`LibraryError::Format`) | SIGABRT | NULL and a warning |
| Later row the pin dies on | SIGABRT at N, no line | Lines for absent libraries up to that row, then its abort, or NULL and a warning for a null system file whose directory does not read as a chunk |
| Later files sound | Untouched | Mapped, checksummed, read and attached, with no change in answer or stderr |

Without any bad log, an absent library followed by an unmappable one lost the
absent library's line, because the `Err` dropped the list.

`library_loop::run` is now the one loop. `TableConf::default_row_steps` gives
each row's step, `SessionSteps` and `MapSteps` run it, and
`LibraryLoopStop` is the one record of where it stopped. The runtime reports
the lines of the absent libraries the loop reached, then returns the stop.
It attaches the bigram after the loop (`pinyin.cpp:394-397`).

The remaining differences on these paths are the existing class (b) answers:
the pin dies of SIGSEGV on an absent or unmappable library, and oxpinyin goes
on from an absent one and answers NULL for an unmappable one. An absent
library's own log is still merged, so its assertion can be reached there,
where the pin has already died.

## Tests

- The loop, each under both laws, with real replay and injected map faults
  (`library_loop.rs`): `a_bad_log_aborts_before_a_later_library_that_cannot_map`,
  `a_bad_log_aborts_before_a_later_library_with_a_bad_layout`,
  `a_bad_log_aborts_before_a_later_fatal_row`,
  `an_absent_library_keeps_its_line_before_a_later_unmappable_one`,
  `the_loop_touches_nothing_after_the_faulting_log`,
  `each_library_maps_before_its_log_and_before_the_next`,
  `a_library_that_cannot_map_never_reads_its_log`,
  `an_absent_library_keeps_its_line_and_the_fault_of_its_log` and
  `a_directory_library_stops_the_loop_after_the_earlier_logs`.
  `init_reports_only_what_the_library_loop_reached` (`library_directory.rs`)
  holds the lines through the runtime, and
  `init_aborts_on_a_log_before_an_unmappable_library` is the death test on both
  adapters.
- Death tests: `mask_out_aborts_on_a_short_bigram_value` (both adapters),
  `bigram_export_aborts_where_the_walk_loads_a_short_predecessor`, and
  `{init,save,mask_out,reload}_aborts_on_a_directory_library` (both),
  `addon_load_aborts_on_a_directory_library` (`e2e_tests.rs`). The row 96
  tests assert row 76's outcome instead where a directory does not read as a
  chunk. Controls: `mask_out_of_a_selected_short_key_completes` (both) and
  the class (b) `mask_out_refuses_a_partial_bigram_tail`.
- Typed faults: `library_directory.rs` (facade: init returns the earlier
  library's journal fault first and the directory's fault before a later
  bad log; the save's file set for both laws; mask-out),
  `a_short_predecessor_faults_where_the_walk_loads_it` (`export_rows.rs`),
  `a_short_predecessor_value_is_the_fault_of_both_training_paths` and the
  partial-tail store tests, now on `PartialUserBigramTail`
  (`store_libpinyin.rs`). The facade's `instance_laws.rs` short-gram tests and
  the core and testsupport detector tests are unchanged.

## Fuzzing

No harness reaches either row: `capi_commands.rs` opens a fresh user
directory, trains only through `pinyin_train` (which writes sorted grams of
at least four bytes) and has no mask, save, export or library call;
`user_store_ops.rs` drives a standalone store through its writers, which
never write a value under four bytes; the system-file targets map regular
files only. No filter is needed.

## Verification

The commands and results are in the change's report.
