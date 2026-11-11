# Upstream divergences

## Purpose and rules

This register lists accepted differences and outstanding pin defects. The
[compatibility policy](compatibility-policy.md) gives each row's current disposition; Git history holds retired entries.

A new language or runtime-quirk entry also needs the full record from
`docs/findings/language-runtime-discrepancy-template.md`. The register
keeps the short entry and links the record. Each entry names its
compatibility-policy row where it has one.

## Entry template

```markdown
### <short name>

- **Upstream source cite:** `path:lines` in the pinned libpinyin source.
- **Mechanism:** what the C++ does.
- **What oxpinyin does instead:** the Rust behaviour.
- **Externally observable:** yes/no and how a caller would see it.
- **Record and disposition (quirk entries only):**
  `docs/findings/<record>.md`; contract case `<id>`, disposition
  `POLICY-ACCEPTED`.
```

## Accepted divergences — C facades (class (b))

### Bigram export iterator's pinyin buffer

Policy row 1, class (b) (#530).

- **Upstream source cite:** `src/pinyin.cpp:842-872`
  (`pinyin_bigram_iterator_has_next_phrase`): `:844-850` and `:857-863`
  build each pronunciation with `g_ptr_array_new`, free the array with
  `g_ptr_array_free(…, FALSE)` and pass the result to `g_strjoinv` and
  `g_strfreev`; nothing adds the terminating NULL. The unigram export
  does add it (`:726-734`, `g_ptr_array_add(array, NULL)` at `:730`).
- **Mechanism:** `g_strjoinv`/`g_strfreev` walk a `gchar **` until a
  NULL. The array has none, so both read past its end into whatever
  the heap holds there. This is an out-of-bounds read whose outcome depends
  on the neighbourhood of the allocation. Any export that reaches a row with a
  pronunciation takes the path.
- **What oxpinyin does instead:** `CapiContext::export_bigram_rows`
  renders every row into owned Rust strings when the iterator is created
  (`crates/oxpinyin-capi/src/iterators.rs:576`), so no join over a C
  array exists to over-read, on the first cycle or any later one.
- **Externally observable:** yes. Recorded evidence from the audit's
  execution (`bigram-before-save.c`): after one train, the first export
  cycle SIGSEGVs in `pinyin_bigram_iterator_has_next_phrase` on tkrzw and kc
  whether or not the user dir was saved, and on bdb unsaved (4 of 4
  runs). oxpinyin completes the cycle. Class (b): the pin's behaviour is
  undefined, and no safe construction reproduces a read past an
  allocation. Also cross-indexed in `reference/memory-safety-bugs.md`.

### Chewing auxiliary substring over-read

- **Upstream source cite:** `src/pinyin.cpp:3555-3560`
  (`pinyin_get_chewing_auxiliary_text`): after `len = cursor - begin`
  (`:3556`), `g_utf8_substring(zhuyin, len, end)` (`:3558`) passes the raw
  byte delta as `start_pos` and `m_raw_end`, an **absolute** raw-input
  offset, as `end_pos`. GLib 2.89.4-2 (recorded evidence: the Debian testing package that this reading used; tag `2.89.4`) `g_utf8_substring`
  (`glib/gutf8.c:298-322`) resolves
  `start = g_utf8_offset_to_pointer(str, len)` and
  `end = g_utf8_offset_to_pointer(start, end_pos - len)`;
  `g_utf8_offset_to_pointer` (`glib/gutf8.c:376-386`) steps
  `g_utf8_next_char` `end_pos - start_pos` times with no terminator check.
- **Mechanism:** the second call asks for `m_raw_end - len` scalars from
  scalar `len`, not for the `C - len` scalars remaining. `m_raw_end > C` for
  essentially every interior cursor (a later key's absolute end exceeds `C`;
  a first key's end is its raw span, at least `C` for every splittable
  content row and usually larger), so the call is outside GLib's documented
  domain and the walk passes the NUL. The copy reads beyond the allocation
  once the end pointer is at least one byte past the NUL
  (`m_raw_end >= C + 2`); for `nihao@3` (`hao`, `C = 2`, `len = 1`,
  `m_raw_end = 5`) it walks three scalars past and copies the NUL plus the
  bytes after it. While the pin returns, `g_strconcat` sees the copied NUL
  and the visible right half is the spelling from `len` to `C` (empty at
  `len == C`). For `len > C` the copy starts past the NUL, so the visible
  right half is heap bytes. The over-read can fault.
- **What oxpinyin does instead:** `crates/oxpinyin-capi/src/aux_matrix.rs`
  `split_zhuyin` cuts the spelling at scalar `len` and returns an empty
  right half when `len` is at or past the scalar count; the zhuyin-parser
  branch in `crates/oxpinyin-capi/src/text.rs` `chewing_aux_text` uses the
  same cut. For `len <= C` its bytes are the pin's visible right half.
- **Externally observable:** for an interior cursor the visible bytes match
  only when the pin returns; the difference is the absent out-of-allocation
  read and possible fault, plus the out-of-domain walk at `m_raw_end == C + 1`.
  For `len > C` the visible bytes differ (heap vs empty). Class (b): no safe,
  deterministic reproduction; no differential case reproduces the read.
- **Record:** [G2 closure](glib-libc-verification.md#g2--c-strings-joins-and-vectors);
  verified on bdb.

### Unterminated punctuation value over-read

- **Upstream source cite:** `src/storage/punct_table.cpp:57-72`
  (`PunctTableEntry::unescape`) and `:74-94` (`get_all_punctuations`),
  reached from `pinyin_guess_predicted_candidates_with_punctuations`
  (`src/pinyin.cpp:2474`). The disassembly of the bdb reference build
  (`unescape` at `0x72020`, `get_all_punctuations` at `0x72080`) has the
  same control flow.
- **Mechanism:** `unescape` appends `punct[index]`, increments `index`,
  then reads `punct[index]` to test for the terminator before the loop
  checks `index < maxlen`. When a run reaches the value's last whole word,
  that read is one `ucs4_t` past the value. A zero breaks the loop and any
  other word ends it at the loop test, so both outcomes return
  `index + 1`: the run, and so the visible punctuation, does not depend
  on the word read. `get_all_punctuations` passes `end - begin` as
  `maxlen`, an arithmetic right shift of the byte difference. It
  compares `begin < end` on bytes. Thus a value whose length is not a
  multiple of four is walked as its whole words. When 1–3 bytes remain
  after a terminated run, the walk adds one more empty punctuation
  (`maxlen` zero).
- **What oxpinyin does instead:** `crates/oxpinyin-data/src/row_format.rs`
  `punct::decode_puncts` ends the run at the value's last whole word without
  reading past it, and ignores 1–3 trailing bytes, so a misaligned value
  gets no extra empty punctuation. Otherwise its result is the pin's
  visible result.
- **Externally observable:** when the pin's read faults, which needs
  the copied value to end where mapped memory ends, and in the pin's
  extra empty punctuation when 1–3 bytes follow a terminated run or form
  the whole value. Otherwise the rows are identical.
  Class (b): the out-of-bounds read and its possible fault are not
  reproduced.
- **Cases:** `punct-decode/unterminated`, `punct-decode/unterminated-zero`
  and `punct-decode/misaligned-unterminated` MATCH on bdb in the recorded
  evidence (#1144). `punct-decode/misaligned-short` and
  `punct-decode/misaligned-terminated` are POLICY-ACCEPTED on every cell:
  the pin returns the empty punctuation and oxpinyin does not
  ([1297-punct-words](1297-punct-words.md)).

### Aliased prediction prefix use-after-free

- **Upstream source cite:** `src/pinyin.cpp:2413-2427`
  (`pinyin_guess_predicted_candidates`): `_free_candidates` (`:1335-1344`)
  `g_free`s every candidate string and empties the list (`:2420`),
  `m_prefixes` is set to size 0 (`:2423`), then `_compute_prefixes` reads
  `prefix` with `g_utf8_to_ucs4` (`:2424`, `:1389-1398`).
  `pinyin_guess_predicted_candidates_with_punctuations` makes that call
  first and ignores its answer (`:2459`), walks `m_prefixes` (`:2463-2486`)
  and returns `true` (`:2498`).
- **Mechanism:** when `prefix` is one of the instance's candidate strings
  (`pinyin_get_candidate_string`, `:2876`), `_compute_prefixes` reads
  freed storage. Defined: the emptied list, the emptied `m_prefixes`
  before the read, untouched sentence rows, and the punctuation entry's
  `true`. Undefined: the prefixes computed from the freed bytes, and so
  the predicted and punctuation rows.
- **What oxpinyin does instead:** `crates/oxpinyin-capi/src/predict.rs`
  `clear_borrowed_prefix` finds the alias by pointer identity before
  reading it, empties the candidates and prefixes, and gives #542's
  no-prefix outcome. The plain entry answers `false`; the punctuation
  entry (`crates/oxpinyin-capi/src/sentence.rs`) answers `true` with an
  empty list, since an empty `m_prefixes` prepends nothing.
- **Externally observable:** only where the pin's freed bytes would decode
  to a prefix with phrase-table tokens, or the read faults. Class (b): the
  read is not reproduced.
- **Cases:** `punct-alias/borrowed-candidates` and
  `punct-alias/borrowed-predicted` (`pin_not_run`: the pin is not driven
  into the read; the subject is held to the defined parts) pass on bdb;
  `punct-alias/owned-*` pass a copy of the same text and MATCH the pin
  (#1272).

### NULL prediction prefix

- **Upstream source cite:** `src/pinyin.cpp:1389-1398` (`_compute_prefixes`)
  passes `prefix` to `g_utf8_to_ucs4`, whose first act is `*in`
  (GLib 2.84.4 `gutf8.c:950`); there is no NULL guard. Its callers reset
  first: `pinyin_guess_predicted_candidates` frees the candidates and
  empties `m_prefixes` (`:2420-2423`), the punctuation entry makes that
  call (`:2459`), and `pinyin_guess_sentence_with_prefix` sets
  `m_prefixes` to `[sentence_start]` (`:1431-1432`).
  `zhuyin_guess_sentence_with_prefix` has the same shape
  (`src/zhuyin.cpp:912-918`, `:949-953`).
- **Mechanism:** a NULL `prefix` with a live instance passes those defined
  resets, then the conversion dereferences NULL and the pin dies of
  SIGSEGV. Row 41's guard covers a NULL instance only, where nothing
  defined precedes the fault.
- **What oxpinyin does instead:** by maintainer ruling (#1277), a NULL `prefix` is a prefix that adds no token, like one
  `g_utf8_to_ucs4` rejects or an empty one.
  `oxpinyin-capi-marshal`'s `ucs4::prefix_text` turns all three into
  `None` at the C boundary, and the prediction and prefixed-sentence paths
  take that `Option`. The plain entry answers `false` with the candidates
  and prefixes emptied, the punctuation entry answers `true` with an empty
  list, and both sentence entries guess on `[sentence_start]` and answer
  the n-best result.
- **Externally observable:** yes. The pin faults; oxpinyin answers the
  no-prefix outcome. Class (b): the NULL dereference is not reproduced.
- **Cases:** `prefix-convert/{predicted,punctuation,pinyin-sentence,zhuyin-sentence}-null`
  (`crash=`: the pin dies of SIGSEGV after the subject's stderr, and the
  subject answers the outcome above) pass on bdb in the recorded evidence.

### NULL pointer arguments: the pin dereferences, oxpinyin null-guards (policy row 41)

- **Upstream source cite:** 68 `pinyin_*` exports, each dereferencing a
  NULL argument before any check at `074a2219`. #526's body tabulates the
  first dereference of each export. By pattern: the instance
  first (`src/pinyin.cpp:1312`); the context or iterator first (`:509`,
  `:665`, `:777`, `:1196`); an out-parameter written before any check
  (`:2847`, `:2876`, `:2982`); a NULL candidate inside a live `assert`
  (`:2507`, `:2593`); a NULL key or key-rest (`:2711`, `:2722`,
  `:2733`, `:2744`).
- **Mechanism:** no export checks its pointer arguments. A NULL one is
  dereferenced: undefined behaviour, SIGSEGV in practice.
- **What oxpinyin does instead:** every export (or the helper it
  delegates to) opens with an `is_null()` guard and returns
  `false`/`0`/NULL/void. #526 lists the guard line per export (e.g.
  `crates/oxpinyin-capi/src/instance.rs:19`, `iterators.rs:64`,
  `candidates.rs:309`, `cursor.rs:241`, `keys.rs:219`).
- **Externally observable:** yes. Audit D-04 (recorded evidence): of 83
  NULL-class probes, 70 crash the pin (68 distinct exports, all
  `signal=11`), and all 83 return on oxpinyin. The libzhuyin exports show
  the same shape (#526).
  Class (b).

### A guess on an instance whose context was finalised (policy row 42)

- **Upstream source cite:** `src/pinyin.cpp:1194-1222` (`pinyin_fini`
  deletes the context and its members, not its instances);
  `:1372-1380` (`pinyin_guess_sentence` reads
  `instance->m_context->m_pinyin_lookup`).
- **Mechanism:** a use after free. The instance keeps a pointer to the
  deleted context.
- **What oxpinyin does instead:** each instance holds its own handles to
  the shared engine state (`crates/oxpinyin-capi/src/state.rs:69-77`),
  so the guess completes and answers `true`.
- **Externally observable:** yes. Audit D-06, probe
  `instance_outlives_context`: SIGSEGV on the pin, `true` on oxpinyin.
  Class (b). The inverse probe (`alloc_after_fini`) is policy row 60.
  By ruling, the orphan guess needs no change: the pin's use after free
  stays unreproducible.

### Zhuyin import into library index 16 (policy row 43)

- **Upstream source cite:** `src/zhuyin.cpp:392-398`
  (`zhuyin_begin_add_phrases` stores any index), `:475` (`_add_phrase`
  calls `phrase_index->get_range(index, …)`),
  `src/storage/phrase_index.cpp:611` (`m_sub_phrase_indices[phrase_index]`
  with no bound), `src/storage/phrase_index.h:441` (the array has
  `PHRASE_INDEX_LIBRARY_COUNT` = 16 entries).
- **Mechanism:** index 16 reads one element past the array and calls
  through the value found there. This is an out-of-bounds read.
- **What oxpinyin does instead:** `zhuyin_begin_add_phrases` stores any index
  as the pin does (`crates/oxpinyin-zhuyin-capi/src/iterators.rs:33`). The
  following `zhuyin_iterator_add_phrase` takes the `USER_FILE` libraries and
  the loaded system libraries 1..=4. It refuses every other index, library
  16 included, with `false`
  (`crates/oxpinyin-zhuyin-capi/src/iterators.rs:132`).
- **Externally observable:** yes. The audit's libzhuyin battery
  (#526): SIGSEGV on the pin on all three cells,
  `false` on oxpinyin. Class (b). The pinyin facade's `_add_phrase`
  has the same unchecked index (`src/pinyin.cpp:589`). But the
  audit recorded library 255 *accepted* there (D-12). Thus that side is
  not a stable crash, and it is not part of this entry.
- **Held by:** `oxpinyin-zhuyin-capi`'s
  `boundary_tests::null_arguments_answer_the_declared_value`, and the
  recorded pin observation `zhuyin-add-phrase-library-16` (#563): on bdb
  the pin exits -11 (SIGSEGV) at library 16 after writing the same stderr
  bytes the subject writes, and the subject answers `false` with no
  warning. The case builds no fixture, so it also runs on kc and tkrzw.

### `pinyin_token_get_nth_pronunciation` past the last reading (policy row 51)

- **Upstream source cite:** `src/pinyin.cpp:2801-2819`
  (`ChewingKey buffer[MAX_PHRASE_LENGTH]` at `:2808`, never initialised;
  the return value of `item.get_nth_pronunciation(nth, buffer, freq)` at
  `:2815` ignored; `g_array_append_vals(keys, buffer, len)` at `:2817`);
  `src/storage/phrase_index.cpp:33-44` (`get_nth_pronunciation` refuses an
  out-of-range read through `MemoryChunk::get_content` and writes nothing).
- **Mechanism:** an `nth` past the last reading leaves `buffer` as it was.
  The function appends `phrase_length` keys of it and returns `true`:
  an uninitialised read. What the stack held depends on earlier calls
  (bdb: `0000 0000` for `nth` 1 and 2, `604f0000` for `nth` 2^32−1).
- **What oxpinyin does instead:** returns `true` and appends
  `phrase_length` zeroed keys (`crates/oxpinyin-capi/src/dict.rs`,
  `pinyin_token_get_nth_pronunciation`). The return value and the number of
  keys are the pin's.
- **Externally observable:** the content only. Class (b).

### `zhuyin_token_get_nth_pronunciation` past the last reading (policy row 70)

- **Upstream source cite:** `src/zhuyin.cpp:1793-1811`
  (`ChewingKey buffer[MAX_PHRASE_LENGTH]` at `:1800`, never initialised;
  the return value of `item.get_nth_pronunciation(nth, buffer, freq)` at
  `:1807` ignored; `g_array_append_vals(keys, buffer, len)` at `:1809`);
  `src/storage/phrase_index.cpp:33-44`, the same refusing read as the
  pinyin entry above.
- **Mechanism:** as the pinyin entry: an `nth` past the last reading leaves
  `buffer` as it was, and the function appends `phrase_length` keys of it
  and answers `true`. On bdb, `nth` 1, 2 and 2^32−1 on `你好`
  answer `true` with two keys.
- **What oxpinyin does instead:** answers `true` and appends
  `phrase_length` zeroed keys (`crates/oxpinyin-zhuyin-capi/src/dict.rs`).
- **Externally observable:** the content only. Class (b).

### `pinyin_alloc_instance` on a finalised context (policy row 60)

- **Upstream source cite:** `src/pinyin.cpp:1194-1222` (`pinyin_fini`
  deletes the context), `:1310-1333` (`pinyin_alloc_instance` reads
  `context->m_phrase_index` at `:1322` to build the instance's
  constraints, `phonetic_lookup.h:427`).
- **Mechanism:** a use after free. valgrind (bdb): "Invalid
  read of size 8 at `pinyin_alloc_instance (pinyin.cpp:1322)`, address 48
  bytes inside a block of size 1,464 free'd". The call returns a non-NULL
  instance whose constraints hold a dangling pointer, and survives because
  the freed block is still mapped.
- **What oxpinyin does instead:** `pinyin_init` records each context's
  address and `pinyin_fini` forgets it (`crates/oxpinyin-capi/src/live.rs`);
  `pinyin_alloc_instance` answers NULL for an address that is not live, and
  logs nothing, because the pin does not abort.
- **Externally observable:** yes — the recorded case
  `alloc-instance-after-fini`: the pin returns an instance and exits 0;
  oxpinyin returns NULL and exits 0. Class (b).
  A new context at a reused address is live again, which the registry cannot
  tell from the old one.
- **The zhuyin twin:** `zhuyin_alloc_instance` reads the
  freed context the same way (`zhuyin.cpp:845-857`). `zhuyin_init` and
  `zhuyin_fini` keep their own registry (`crates/oxpinyin-zhuyin-capi/src/live.rs`)
  and the call answers NULL, silently (the pin observation
  `zhuyin-alloc-instance-after-fini`); `oxpinyin-zhuyin-capi`'s
  `live::tests::a_context_is_live_from_init_to_fini` holds the answer.

### `pinyin_bigram_iterator_get_next_phrase` after a walk that ends on a real predecessor (policy row 75)

- **Upstream source cite:** `src/pinyin.cpp:896-911` (`get_next`: the
  assert at `:902`, then `g_strdup(iter->m_phrase)` at `:904` and
  `g_strdup((gchar *) g_ptr_array_index(iter->m_pinyins,
  iter->m_pinyin_index))` at `:905`, then `return
  pinyin_bigram_iterator_has_next_phrase(iter)` at `:910`); `:789-894`
  (`has_next` frees `m_pinyins` and replaces it with an empty array at
  `:798-801` before it looks for the next row, and leaves `m_index_token` at
  the predecessor it loaded last, `:885-886`); `:779` and
  `src/storage/ngram_bdb.cpp:178-200` (the predecessors come in the user
  bigram's hash order, `DB_HASH`).
- **Mechanism:** a walk's `false` row leaves `m_pinyins` empty. If the
  predecessor loaded last is `null_token` or `sentence_start`, a further
  `get_next` asserts (policy row 62). If it is a real phrase token the assert
  passes and `:905` indexes the empty array: on GLib 2.90 `g_ptr_array_new`
  allocates no storage, so the read goes through a NULL pointer and the pin
  dies of SIGSEGV. Undefined behaviour.
- **What oxpinyin does instead:** answers `false` with `phrase`, `pinyin`
  and `count` untouched and logs nothing
  (`crates/oxpinyin-capi/src/iterators.rs:641`; the walk names the state
  `BigramStep::Undefined`, `crates/oxpinyin-facade/src/export_rows.rs:1036-1038`).
- **Externally observable:** yes: the pin's process dies, and oxpinyin
  answers `false`. Class (b). No contract case holds it, because which
  predecessor a walk loads last depends on the backend's hash order.
- **Recorded evidence:** executed on bdb, linux/amd64 under Rosetta. After
  two trains (你好 and 世界), the pin answers `has_next` `true` and
  `get_next` `false` with 你好, then dies of SIGSEGV with a NULL fault
  address in `pinyin_bigram_iterator_get_next_phrase` at `pinyin.cpp:905`.
  A pin built with the `bigram-export-strjoinv` patch, whose walk cannot
  over-read (row 1), faults at the same statement. oxpinyin answers the
  same two calls, then answers the third with `false`, the three
  out-params untouched and no log record.

### A second `pinyin_fini` / `zhuyin_fini` on one context (#573; policy row 105, class (b), 2026-10-17 UTC)

- **Upstream source cite:** `src/pinyin.cpp:1194-1222` (`pinyin_fini`),
  `src/zhuyin.cpp:741-757` (`zhuyin_fini`), at
  `074a2219c90feaf962d0d24f034514033ece5f99`.
- **Mechanism:** a double free. The second call reads the freed context
  (`pinyin_fini` first reads `m_user_table_info`) and deletes its members and
  the context again.
- **What oxpinyin does instead:** both entry points return at once for a
  context the live-context registry does not hold (row 60's registries), with
  no log, since the pin has no path to mirror.
- **Externally observable:** yes. In the recorded cases `fini-twice` and
  `zhuyin-fini-twice` on bdb, the pin dies of SIGSEGV every run on both
  facades and the subject returns. The cases hold the exit status and
  leave stderr out, since the pin reads freed memory.
- **Left as is:** a second `pinyin_free_instance` / `zhuyin_free_instance`
  (`pinyin.cpp:1347-1356`, `zhuyin.cpp:876-884`) is caller UB on both sides
  (the audit saw SIGSEGV on one pin run and SIGABRT on another). A
  registry of live instances would worsen time and space on every
  alloc/free, so there is no case and no change.

### A prediction candidate whose phrase item a journal REMOVE took out (#1231; policy row 106a, class (b), 2026-10-25 UTC)

- **Upstream source cite:** `src/pinyin.cpp:2389-2405` (prefix filter),
  `:2018-2022` and `:1635-1637` (NULL phrase string),
  `:2041-2068` and `:2082` (dedup `strcmp`), `:2071-2080` (singleton
  dedup), `:2608-2616` (prefix choose), and
  `src/storage/phrase_index.h:628-634` (facade-total increment), at
  `074a2219c90feaf962d0d24f034514033ece5f99`.
- **Mechanism:** `_compute_predicted_prefix_candidates` ignores
  `get_phrase_item`'s result. The loop-wide `PhraseItem` keeps the last
  successful read's length (or the default zero), so the
  `2 * prefix_len + 1` gate uses a stale length: every success replaces it
  before the check, including one the check then skips. An appended missing
  token keeps `m_phrase_string = NULL`. With two or more pre-dedup rows
  the dedup sort's `strcmp` reads it and the pin dies of SIGSEGV; with one
  row neither comparison runs, so the NULL row survives and the getters
  expose count 1, prefix type, a successful string getter writing NULL, a
  failed `token_get_phrase` with untouched outputs, and `false` from
  `is_user_candidate`. Choosing that row calls
  `add_unigram_frequency(token, 483)`: the loaded facade total advances by
  483 modulo 2^32, the missing sub-index write fails, and the prefix
  branch still returns true without a bigram; an unloaded library returns
  before the total moves and still returns true.
- **What oxpinyin does instead:** `select_prefix_rows`
  (`crates/oxpinyin-capi/src/predict.rs`) keeps one plain
  `last_read_length` with the pin's update-before-check order. It keeps a
  sole missing row in the snapshot with `TextSpan::NULL`; with any other
  pre-dedup row it drops missing rows under row 106's class (b) answer
  (no candidate, normal true, no warning). `pinyin_choose_predicted_candidate`
  (`crates/oxpinyin-capi/src/candidates.rs`) advances the loaded facade
  total through `RuntimeDict::add_unigram_delta` and returns true, or
  leaves it unchanged when the library is unloaded. An absent item reaches
  the adapter as `None`.
- **Externally observable:** yes. The recorded cases are
  `removed-predict-sole-pinyin`, `removed-predict-choose-pinyin`,
  `removed-predict-class-b-pinyin` (crash: pin SIGSEGV, subject true with
  the live candidate) and `removed-predict-stale-skip-pinyin`. The stale
  case is bdb-only by registration; the other three run on bdb, kc and
  tkrzw and MATCH on all three. The C ABI has no facade-total getter; the 483-total and
  unloaded-unchanged effects are also held by the in-crate test
  `choosing_a_sole_missing_prefix_row_advances_the_facade_total`.
- **Related parity:** the implementation applies the pin's
  `pinyin_is_user_candidate` type gate (`src/pinyin.cpp:3713-3715`), so a
  predicted-bigram or predicted-prefix row whose token is in
  `USER_DICTIONARY` answers false; `oxpinyin-capi`'s
  `boundary_tests::a_predicted_bigram_row_is_not_a_user_candidate` holds it.
- **Not changed:** the zhuyin facade has no predicted-candidate path.

### NULL string conversion after defined prior work (#1279)

Class (b), under the #1272/#1277 no-input ruling. At 074a2219
`src/pinyin.cpp:1450-1452` and `src/zhuyin.cpp:969-971`, NULL sentence
first calls `g_utf8_strlen`, which emits its precondition critical
(GLib 2.84.4 `glib/gutf8.c:240`, GLib domain from `glib/meson.build:404`),
then `g_utf8_to_ucs4` dereferences NULL (`gutf8.c:950`). Both adapters'
`phrase_segment` entries preserve that critical, then segment no input:
true with the prior phrase-result list cleared (`lookup/phrase_lookup.cpp:36-52,119-157,382-428`).

NULL lookup phrase faults before `reduce_tokens` at
`src/pinyin.cpp:2658` / `src/zhuyin.cpp:1684`; the adapters' `lookup_tokens`
entries take the no-input lookup, return false and clear the caller array.
Batch parse NULL strings fault at strlen before instance mutation
(`pinyin.cpp:1509,1552,1599`; `zhuyin.cpp:1032,1070`); their existing
no-input path returns zero and clears parse state. Single-key NULL strings
fault at strlen before key mutation (`pinyin.cpp:1491,1534,1575`;
`zhuyin.cpp:1011,1052`); the existing guard returns false and leaves the
key untouched. NULL `parse_full_pinyin` is the single-key case in point:
the pin faults at strlen (`pinyin.cpp:1491`, `zhuyin.cpp:1011`) before
`parse_one_key` touches the key, so the key stays untouched. No UB is
reproduced. Row 41 remains the guard classification; this entry
records the state-sensitive string outcomes.

Cases: `c-string-input/*-segment-state-null`, `*-lookup-state-null`,
`*-parse*-null`, `*-character-offset-null`, `*-import-null` and
`*-remember-null`, with the pin not driven into UB (`pin_not_run`), plus
non-empty valid controls.

### Training a token whose library is unloaded raises a stale cached item's reading counts (policy row 118)

- **Upstream source cite:** `src/storage/phrase_index.cpp:261-269`,
  `src/lookup/phonetic_lookup.h:924-927`, `src/storage/phrase_index.h:649-650`
  and `src/storage/phrase_index.cpp:204`, at `074a2219`.
- **Mechanism:** after `pinyin_unload_phrase_library` (GBK, 2) or
  `zhuyin_unload_phrase_library` (any library but TSI, 1) deletes a
  sub-index (`phrase_index.cpp:261-269`), a sentence that holds a token of
  it still trains. `train_result3` reads the item unchecked
  (`phonetic_lookup.h:924`). `FacadePhraseIndex::get_phrase_item` answers
  `ERROR_NO_SUB_PHRASE_INDEX` and leaves `m_cached_phrase_item` as the last
  successful read left it (`phrase_index.h:649-650`). The call then raises
  that item's reading counts (`:925-927`). The cached chunk is non-owning
  (`phrase_index.cpp:204`), so the write lands wherever that read pointed.
- **What oxpinyin does instead:** it trains no reading of a token whose
  library has no live buffer, as `arena_train` leaves a journal-removed
  item alone. It keeps what the pin defines: the bigram trains
  (`:875-909`), the unigram add finds no sub-index and moves no total
  (`phrase_index.h:631-632`), and `pinyin_train` and `zhuyin_train` answer
  `true`. A predicted bigram or prefix row of the unloaded library does the
  same (`pinyin.cpp:2609-2639`). `_add_phrase` finds no range
  (`pinyin.cpp:589-591`) and `compact` skips the library
  (`phrase_index.cpp:651-652`), so its total stays as the unload left it.
- **Externally observable:** the outcome has two arms.
  - **Write into the unloaded library's freed chunk:** the pin's write is
    a use-after-free. Class (b): no safe construction reproduces it, and
    oxpinyin trains no reading. This arm is source-derived. No contract
    case runs it, because the pin's write there is undefined.
  - **Write into a library that is still loaded:** the pin defines the
    result and raises that live item's reading counts. oxpinyin declines
    that write too. This is an explicitly approved divergence, the
    "Unloaded-token pronunciation uses no cached item" entry below. The
    executed cases `unloaded-cached-pronunciation-pinyin` and
    `unloaded-cached-pronunciation-zhuyin` hold it.
- **Held by:** `unloaded_library_tests` (`crates/oxpinyin-capi/src/candidates.rs`)
  and `unloaded` (`crates/oxpinyin-user/src/store_libpinyin/unloaded.rs`).
- **Disposition:** [compatibility policy](compatibility-policy.md), row 118.

## Accepted divergences — store and data files

### The pinyin index DBMs carry uninitialized struct padding

- **Upstream source cite:** `src/storage/chewing_large_table2.h:217-231`
  (`ChewingTableEntry::add_index` inserts a whole `PinyinIndexItem2<L>`
  struct with `m_chunk.insert_content(offset, &add_elem, sizeof(IndexItem))`)
  and `pinyin_phrase3.h:181-195` (`PinyinIndexItem2<L>` = `{u32 m_token,
  ChewingKey m_keys[L]}` — for odd L the struct carries 2 bytes of
  tail padding to the token's 4-byte alignment).
- **Mechanism:** `add_elem` is a stack-allocated `PinyinIndexItem2<L>`
  whose constructor `memmove`s only the keys and never touches the tail
  padding; the whole `sizeof` bytes — padding included — are copied into
  the DBM value. The padding bytes are whatever happened to sit on that
  stack region during `gen_binary_files` — unreproducible garbage
  (observed `17, 236` and `90, 237` in the pin's real files).
- **What oxpinyin does instead:** the table generators zero the padding
  (`oxpinyin_data::row_format::pinyin_index::encode_item`, written from
  `crates/oxpinyin-data/src/table_entries.rs`), and the runtime reader
  (`pinyin_index::decode_items` via `ChewingTable::search`,
  `crates/oxpinyin-data/src/chewing_table.rs`) reads only the token and
  key fields within each stride, never the padding.
- **Externally observable:** no — the reader never touches the padding
  bytes, so a libpinyin runtime consuming either file decodes identical
  index items; only the file bytes differ, in two bytes per odd-L value
  record. Class (b): upstream's stored bytes *are* uninitialized memory;
  no safe construction reproduces specific garbage, and none should.

### `mmap %s failed!`: a system library the pin cannot map, and the empty chunk it dies on (#545; policy row 76)

- **Upstream source cite:** `src/pinyin.cpp:256`, `:290`, `:956`, `:1265` and
  `src/zhuyin.cpp:200`, `:234`, `:589`, `:800` (`if (!chunk->mmap(chunkfilename))
  fprintf(stderr, "mmap %s failed!\n", chunkfilename);`, the `open %s failed!`
  branches beside them being the `#else` of `LIBPINYIN_USE_MMAP`, defined by
  `src/include/memory_chunk.h:31-34` whenever `HAVE_MMAP` is, and
  `config.h:42` of the pin's build defines it); `src/include/memory_chunk.h:470-520`
  (`MemoryChunk::mmap` resets the chunk, then answers `false` for a file that
  will not open, one shorter than the 8-byte header, a length word that is not
  the payload length, or a checksum that does not verify);
  `src/storage/phrase_index.cpp:244` and `:278` (`FacadePhraseIndex::load` and
  `diff` hand the chunk to `SubPhraseIndex::load`, `:335-363`, whose
  `g_return_val_if_fail(*(buf_begin + offset) == c_separate, FALSE)` at `:354`
  reads `buf_begin + 16`). Also `src/storage/facade_chewing_table.h:115` and
  `src/storage/facade_phrase_table2.h:96`, which no public call reaches (below).
- **Mechanism:** every site prints the line and carries on with the chunk
  `mmap` reset, which is empty: `begin()` is NULL and `size()` is 0, so
  `SubPhraseIndex::load` reads the separator byte at `NULL + 16` before its own
  bounds check can refuse the chunk. The pin dies of SIGSEGV. The sites are
  reached by `pinyin_init` / `zhuyin_init` (each default `SYSTEM_FILE`
  library, in index order), `pinyin_load_phrase_library` /
  `zhuyin_load_phrase_library` (a library that was unloaded),
  `pinyin_load_addon_phrase_library` (`:290`, the only entry to a `DICTIONARY`
  library), a modified `pinyin_save` / `zhuyin_save` (every loaded system
  library is mapped again to `diff` it) and `pinyin_mask_out` /
  `zhuyin_mask_out` (every loaded system library is reloaded). A library
  already loaded, an unloaded one in a save or mask-out, an unmodified save
  and a reload of a whole file stay silent. `zhuyin.cpp:234` is reached by no
  call: `zhuyin_init` and `zhuyin_load_phrase_library` `assert` that the default
  table is not a `DICTIONARY` (`:330`, `:372`; the pin is built without
  `NDEBUG`) and the zhuyin facade has no addon entry point.
  `FacadeChewingTable` and `FacadePhraseTable2` are included by
  `src/pinyin_internal.cpp` into the uninstalled `libpinyin_internal.a` and
  instantiated nowhere in `src/`, `tests/` or `utils/`; `libpinyin.so.15` and
  `libzhuyin.so.15` define no symbol of either class.
- **What oxpinyin does instead:** writes the pin's `mmap <path> failed!` line,
  the path as its bytes, at each reachable site on both facades, with the
  pin's own `fprintf(stderr, …)` through `oxpinyin_data::c_stderr` (stage 4
  of `external-io-boundary.md`), and answers what it answered before: NULL with
  one GLib warning from an init whose library is short or corrupt, non-NULL
  from one whose library is absent (the library stays unloaded), `true` from a
  reload, a modified save and a mask-out, `false` from the addon load. It
  never reads through the empty chunk. The failure is the typed
  `LibraryError::Unmappable` the chunk reader raises for exactly what
  `MemoryChunk::mmap` refuses; a separator or index outside the payload
  is `LibraryError::Format` and has no line; a payload shorter than 17
  bytes is `LibraryError::Layout`, and the caller loads the empty
  sub-index and logs one critical, as `SubPhraseIndex::load`'s
  `g_return_val_if_fail` does. With several libraries broken it writes one
  line for each, in library order; the pin's bytes are the first. An init that
  fails on a library judges the user profile first, as the pin does, so the pin's
  `open <user>/user.conf failed.`, wipe and marker write precede its line and its NULL.
- **Externally observable:** yes: the pin's process dies, with the line as its
  last output; oxpinyin writes the line and goes on. Class (b).
- **Recorded evidence:** executed on bdb, native amd64. The
  `stderr-library-*` cases break `merged.bin`, `gbk_char.bin` or `art.bin`
  in a private copy of the system directory (absent, cut to four bytes,
  last payload byte flipped) and compare stderr up to the crash. In 39
  crash cases (21 pinyin, 18 zhuyin, fresh and non-conforming user dirs
  among them) the pin dies of SIGSEGV after the same bytes, and oxpinyin
  gives the answers listed above; the 11 controls match. The pin's fault
  address is `0x10` for the init, load, save, mask-out and addon paths
  alike, on both facades.

**Separator and index outside the payload (#865):** the same `SubPhraseIndex::load`
separator/index reads at `phrase_index.cpp:354-356` also run past the chunk
for an in-bounds header whose separator/index lies outside the payload, or for
a payload shorter than the four-word header. The pin reads the separator/index
outside the mapped chunk; on the first shape it dies of SIGSEGV before any
line. oxpinyin's `PhraseLibrary::open` refuses a separator or index outside the
payload as `LibraryError::Format` without the read and without a critical,
and a payload shorter than 17 bytes as `LibraryError::Layout`; the caller
loads the empty sub-index and logs one critical. Both are class (b), the same
site as row 76. Held by `crash-oob-layout-{pinyin,zhuyin}` (pin SIGSEGV,
subject `init` false; no critical) and `short-chunk-layout-{pinyin,zhuyin}`
(the pin's empty sub-index + one critical matched by the subject's Layout
empty sub-index + one critical; the out-of-object read itself is not
performed), all MATCH on bdb in the recorded evidence.

### A sparse chunk's whole mapping is retained after `fini` (#865; policy row 109)

Policy row 109: the mapping is **IMPLEMENTED** and follows the pin; the
wrapped-size residual is **POLICY-ACCEPTED** under the external API misuse
ruling.

- **Upstream source cite:** `src/include/memory_chunk.h:475-524`
  (`MemoryChunk::mmap` maps the whole `file_size`) and `:72-78`
  (`MemoryChunk::freemem` unmaps only `header + capacity()`).
- **Mechanism:** `mmap` does not keep the length it mapped. `freemem`
  passes `munmap` a length recomputed from the chunk, `header + capacity()`.
  When `guint32 data_len = file_size - header` wraps, that length is short
  of the mapping. A 2^32+8 file whose length word is 0 gives a payload
  length of 0, so `freemem` releases only the 8-byte header extent and the
  remaining ~4 GiB stays mapped after `pinyin_fini`/`zhuyin_fini`. The #865
  report measured this on bdb: 4,242,620 KiB VmSize after init and
  4,228,488 KiB after fini.
- **What oxpinyin does instead:** `oxpinyin-input::chunk` owns one writable
  private whole-file memmap2 mapping for each loaded system or addon chunk,
  as the pin maps the whole file. `PendingMapping` keeps the descriptor
  through checksum validation, then closes it. The data crate transfers a
  system chunk to its live sub-index and forbids unsafe code. The
  mapping's `Drop` releases the entire mapping. No manual unmap or
  `ManuallyDrop` is used. Trailing-byte interpretation is unchanged.
- **Externally observable:** only as process address space after `fini`
  for a file whose payload length wraps. For every file whose size passes
  the pin's length check without wrapping, the release is the pin's own
  `munmap(base, file_size)`.
- **Disposition:** the mapping is **IMPLEMENTED**. The wrapped-size
  residual is **POLICY-ACCEPTED**: the pin passes `munmap` a length that
  contradicts the mapping it made, and oxpinyin releases exactly what it
  mapped ([compatibility policy](compatibility-policy.md), row 109 and the
  external API misuse ruling).
- **Held by:** `a_sparse_chunk_maps_the_whole_file`
  (`crates/oxpinyin-data/src/phrase_library.rs`).

### `table.conf` follows its rows; malformed ones fail the triggering call (#694 and #525 batch B; policy rows 78, 79, 80 and 83)

Only policy row 80 is an accepted divergence, class (b). Rows 78 and 83
are **CLOSED** (oxpinyin reads the rows and λ as the pin does), and row 79
is **pin behaviour reproduced**; see the
[compatibility policy](compatibility-policy.md).

- **Upstream source cite:** `src/storage/table_info.cpp:209-229`
  (`SystemTableInfo2::load`), `src/pinyin.cpp:1635-1637`, `:2001-2007` and
  `:2053`, at `074a2219`.
- **Mechanism:** (1) a missing `source table format:` directive leaves
  `fscanf` returning 0 with `str` never written, and
  `to_table_phonetic_type` compares it (`table_info.cpp:228-229`):
  undefined behaviour. In execution it ends in SIGABRT on both facades. A
  missing `database format:` line is deterministic and is row 79's `:132`.
  A missing line 1–3 is the pin's ordinary `false` (`:209-224`). (2) A
  `table.conf` that points a loaded library at another file (the
  `gb`↔`gbk` swap, a second `GB_DICTIONARY` row, `GB_DICTIONARY` as
  `USER_FILE`) leaves index tokens whose library is loaded but whose item
  is not there. `_token_get_phrase` returns `false` (`pinyin.cpp:1635-1637`,
  result ignored at `:2001-2007`), and
  `_remove_duplicated_items_by_phrase_string` compares the unset
  `m_phrase_string` in `compare_indexed_item_with_phrase_string` (`:2053`).
  A native gdb backtrace on bdb (Linux x86-64) shows SIGSEGV in
  `__strcmp_avx2` called from that comparator under
  `g_array_sort_with_data`, from `pinyin_guess_candidates`: a NULL or wild
  read, with no assertion.
- **What oxpinyin does instead:** (1) `TableConfError::UninitialisedSourceFormat`:
  init answers NULL with one warning. A missing line 1–3 answers NULL with
  the raw `load %s/table.conf failed!` line and no warning
  (`table-conf-lambda-line-missing`). (2) `pinyin_guess_candidates` and
  `zhuyin_guess_candidates_{after,before}_cursor` answer `false` with one
  warning when the lookups they ran met such a token
  (`SystemDictionary::unreadable_items`); the sentence calls answer as
  before. A library file that is absent or corrupt is row 76.
- **Externally observable:** yes. The pin dies; oxpinyin answers the
  values above. Class (b): no safe construction reproduces the read.
- **Held by:** `table-conf-candidates-*` (the pin dies of SIGSEGV after its
  stderr line; the subject answers `false` with one warning) and
  `table-conf-abort-source-format-line-missing`, on both facades.

### A system journal record names an absent token or leaves an over-long item header (#850; policy row 107)

- **Upstream source cite:** `SubPhraseIndex::merge`
  (`src/storage/phrase_index.cpp:439-517`), reached at init through
  `_load_phrase_library` (`src/pinyin.cpp:276`, return value ignored;
  `zhuyin.cpp` likewise); `remove_phrase_item` (`:219-234`),
  `get_phrase_item` (`:180-206`) and `add_phrase_item` (`:208-217`), at
  `074a2219`.
- **Mechanism:** `LOG_REMOVE_RECORD` (`:460-475`) calls
  `remove_phrase_item`, which returns at `:222-224` when `get_phrase_item`
  fails and leaves `tmpitem` NULL; `olditem != *tmpitem` (`:468`)
  dereferences it. MODIFY (`:477-501`) ignores `get_phrase_item`'s failure
  at `:478` and compares a stale `item`. After an earlier MODIFY, a growing
  MODIFY of an absent token calls `remove_phrase_item` and
  `assert(olditem == *tmpitem)` (`:489`) dereferences the NULL. Neither a
  MODIFY's new chunk nor an ADD payload is checked against its own header,
  so the stored header can name more bytes than were stored, and a later
  `get_phrase_item` hands that length out. A read that ends inside the
  content buffer returns the following bytes and is reproduced. A read
  that ends past the buffer's end is an over-read.
- **What oxpinyin does instead:** the REMOVE of an absent token is a no-op
  and the merge goes on. The growing stale MODIFY and the absent default's
  growing form stop at the record like any failed compare; the earlier
  records stay applied. The all-zero default's same-size writes go on and
  create no entry. Under the maintainer ruling (option 1, explicit class
  (b) safe outcome, not parity), a system journal item whose stored header
  names a read past the content buffer is unreadable and answers no item
  to every getter, lookup and range, as rows 102 and 106 do. All other
  valid journal changes and the replay's sub-index totals stay; the item's
  original bytes are not restored and the journal is not rejected. The
  replay's finish marks such a slot as removed, in the eager and the lazy
  finish alike. A failed MODIFY comparison keeps its prefix; a failed
  REMOVE comparison keeps its removal and prefix. A payload that runs past
  the log's end is a different class (b) site: the walk stops, the applied
  records are reversed from the undo vector, and the library keeps its
  pre-replay content.
- **Externally observable:** yes. Executed on the bdb cell, linux/amd64:
  `pinyin_init`/`zhuyin_init` die of SIGSEGV on the four absent-token
  cases and on `replay-modify-header-overlong-{pinyin,zhuyin}`; oxpinyin
  keeps its answers above. Class (b): NULL dereference and over-read; no
  safe construction reproduces them.
- **Held by:** `replay-remove-absent-{pinyin,zhuyin}`,
  `replay-modify-absent-stale-grow-{pinyin,zhuyin}`,
  `replay-modify-absent-default-grow-{pinyin,zhuyin}` and
  `replay-modify-header-overlong-{pinyin,zhuyin}` (`crash=`); the MATCH
  controls `replay-modify-absent`, `replay-modify-absent-default-{pinyin,zhuyin}`,
  `replay-modify-absent-default-twice-{pinyin,zhuyin}` and the in-buffer
  over-read `replay-modify-header-overlong-small-{pinyin,zhuyin}`; bdb only.
  `replay-over-read-tail-{pinyin,zhuyin}` (`pin_not_run`) and
  `oxpinyin_user::replay::tests::rollback_restores_the_buffer_for_every_kind_of_mutation`
  hold the log-end site. Unit tests include
  `an_over_read_item_answers_no_item_and_the_records_around_it_stay`,
  `valid_records_before_and_after_an_over_read_item_stay` and
  `a_mismatch_stop_keeps_every_applied_record`.
- **Disposition:** [compatibility policy](compatibility-policy.md), row 107.

### Bigram export iterator reads past a partial user-gram item (#677; policy row 98, class (b), 2026-10-13 UTC)

- **Upstream source cite:** `src/storage/ngram.cpp:133-143`
  (`SingleGram::retrieve_all`) at
  `074a2219c90feaf962d0d24f034514033ece5f99`, read from the pinned
  libpinyin source; `src/pinyin.cpp:789-889` reaches the walk.
- **Mechanism:** a value with a four-byte total, `k` complete eight-byte
  items and 1–7 trailing bytes has a partial `SingleGramItem` at its end.
  `retrieve_all` walks with `cur != end`, then reads an entire item. This
  can over-read the value on any backend. The partial tail is distinct
  from the complete-item count that `get_length` and indexed reads use.
- **What oxpinyin does instead:** the user loader decodes the total and
  complete items for defined reads, and the raw DBM keeps the original
  value for save. The export iterator refuses a predecessor with a
  partial item tail, returning NULL and emitting one `libpinyin` warning.
  A mask or candidate removal refuses with `false` and one warning in the
  facade domain when a surviving predecessor has such a tail; a mask
  deleting that predecessor wholesale never reads its value.
  The raw tkrzw store returns libtkrzw's actual results, including a
  retained removal record if the linked library returns one.
- **Externally observable:** a caller sees NULL and one warning where
  upstream can read outside the value. The source guard and ordinary
  backend-independent valid-row differential are prepared. The
  classification is source-derived: no malformed crash probe has run
  against the pin.

### Training on a malformed user-gram value reads past a partial item (#677; policy row 100, class (b), 2026-10-14 UTC)

`SingleGram::insert_freq` (`src/storage/ngram.cpp:178`, loop `:190` at
`074a2219c90feaf962d0d24f034514033ece5f99`), `get_freq` (`:231`, loop
`:241`) and `set_freq` (`:252`, loop `:261`) walk the existing items with
`cur_item != end`. `pinyin_train`/`zhuyin_train` reach them through
`train_result3` (`pinyin_lookup2.cpp:611`, `phonetic_lookup.h:889`). A value
of length `4 + 8k + r`, `1 <= r <= 7`, leaves a partial eight-byte item at
its end that these loops read past: the same out-of-bounds read as row 98,
on the training path. The maintainer accepted this source-based class (b)
classification; no crash probe was run.

`pinyin_choose_predicted_candidate` reaches the same walks
(`pinyin.cpp:2630-2633`: `get_total_freq`, `get_freq`, `insert_freq` on the
previous token's gram).

- **What oxpinyin does instead:** `pinyin_train`/`zhuyin_train` return
  `false` with exactly one facade-domain warning, and observe nothing,
  whenever the training step would load a predecessor for which
  `has_oob_bigram_value_for` is true; `pinyin_choose_predicted_candidate`
  answers `false` with one warning under the same condition. The check
  reads the live raw value at each call. The raw value is untouched and
  survives save and reopen byte for byte.
- **Held by:** `training_over_a_partial_tail_refuses_and_keeps_the_raw_value`
  and `a_partial_tail_created_in_session_refuses_training`
  (`crates/oxpinyin-user/src/store_libpinyin.rs`), at store level.

### A partial phrase-table token tail cannot terminate the entry walk (#799; policy row 101, class (b), 2026-10-15 UTC)

- **Upstream source cite:** `src/storage/phrase_large_table3.cpp:32-39`
  (`PhraseTableEntry::search`), `:57-65` (`add_index`), `:74-81`
  (`remove_index`) and `:103-117` (`mask_out`) at
  `074a2219c90feaf962d0d24f034514033ece5f99`, read with `git show`
  from the libpinyin clone.
- **Mechanism:** the entry casts its raw byte end to a token pointer and
  walks four-byte tokens with `iter != end`. For a value of length
  `4k + r`, `1 <= r <= 3`, the walk cannot reach that end by token
  increments and can read beyond the record. This is an out-of-bounds
  read, not a defined instruction to ignore the trailing bytes. Early
  exits in add/remove may avoid the tail on a particular call; search
  walks it. The source-based class (b) safe answer is user-authorized;
  this entry does not claim a measured crash or a stable pin output.
- **What oxpinyin does instead:** `phrase_table.rs`, `validate` and
  `PhraseTableEntry::from_bytes`, reject the whole incomplete record with
  `StoreError::InvalidInput`; entry operations do not process its complete
  tokens while discarding its tail. `lookup.rs`, `UserLookup::from_store`,
  substitutes an empty entry in the lookup cache, retaining the key and
  exposing no tokens from that record while unrelated lookups remain
  available. `persistence.rs`, `load_user_phrase_index` and `stage_rest`,
  retain and save the raw key/value bytes without normalization. The
  refusal/cache policy does not repair or truncate the stored value.
- **Externally observable:** direct entry consumers receive an error;
  cached lookup exposes no tokens from the malformed record. This records
  those existing behaviors, not a uniform C ABI return or new warning
  guarantee. No malformed pin crash probe was executed. Rows 98 and 100
  concern `SingleGram` tails and do not cover this distinct phrase-table
  record layout.

### A reloaded library's dropped item stays in the user index tables, and a candidate walk dereferences it (#805; policy row 102, class (b), 2026-10-16 UTC)

- **Upstream source cite:** `src/storage/phrase_index.cpp:261-268`
  (`FacadePhraseIndex::unload` deletes the sub-index and takes its total out
  of the facade), `src/pinyin.cpp:264-277` (`_load_phrase_library` maps the
  system chunk and merges only the saved `.dbin`), `src/zhuyin.cpp:298-312`
  (the same path), at `074a2219c90feaf962d0d24f034514033ece5f99`, read from
  the libpinyin checkout at that commit.
- **Mechanism:** the unload frees every item the session had added to the
  library, but the user phrase and pinyin index tables are not unloaded. An
  imported item that was never saved therefore keeps its token in the index,
  and after the reload `pinyin_lookup_tokens` still returns it while the
  sub-index holds no item for it. `pinyin_guess_candidates` (and
  `zhuyin_guess_candidates_after_cursor`) read the item through that token and
  die of SIGSEGV. Measured on the bdb cell: library 2 (GBK), `龘龘` imported with
  count 50, unload, load, then `dada` (pinyin) or `283283` (zhuyin). A dereference
  of an item the sub-index no longer owns has no safe construction.
- **What oxpinyin does instead:** the reload and the unload reproduce the
  pin's state (`GenericUserStore::load_system_libraries` and
  `unload_system_libraries`): the library's rows, overrides and share of the
  total revert to the system chunk plus the saved log, and the user index
  tables are left as they are. `lookup_tokens` answers the dropped token, the
  item reads answer false, and a candidate walk treats the token as an item
  that does not exist: it adds no candidate and the walk completes. The
  candidate list is the one the same input gives before the import.
- **Externally observable:** yes. The pin dies of SIGSEGV in the candidate
  call; oxpinyin returns the list without the dropped item. Every other
  observation of the path (frequencies, readings, ranks, the facade total while
  the library is unloaded, library 1 staying loaded) matches the pin.
- **Held by:** the recorded `reload-*` cases
  (`reload-dangling-token-candidates` for each facade as the class (b) case;
  the rest as plain differentials with their saved-first controls), and
  `reloading_a_system_library_drops_the_unsaved_session`
  (`crates/oxpinyin-user/src/store_libpinyin.rs`) at store level. The
  unload and reload themselves reproduce the pin (policy row 103,
  [library-transitions-in-memory](library-transitions-in-memory.md)).

### A wrapped phrase-content item extends past the content (#1514; policy row 111)

- **Upstream source cite:** `SubPhraseIndex::get_phrase_item`
  (`src/storage/phrase_index.cpp:180-206`) at `074a2219`; compaction at
  `src/storage/phrase_index.cpp:624–675` and `src/pinyin.cpp:655–659`.
- **Mechanism:** `get_phrase_item` constructs a header-derived view
  without validating its full extent. A read beyond the original item but
  inside the content returns the following content bytes. A read that
  passes the content end is unsafe. The logical content boundary can come
  before the owning allocation boundary, so not every read beyond content
  is C++ undefined behaviour.
- **What oxpinyin does instead:** `UserLibrary::item` and the replay buffer
  answer a header-derived read from the content bytes, and answer no item
  where the read passes the content end; the other changes stay, as in
  row 107 option 1. The original item end is not the safety boundary.
  `append_offset`, used by `UserLibrary` and the replay's
  `Content::append`, reproduces the defined content offset narrowing
  modulo 2^32, including zero → 8, the overwritten bytes, aliases and
  totals. Compaction runs at the pin's points only (end-import, save and
  mask-out), in the pin's slot order and with its totals, with no
  dead-over-live automatic trigger.
- **Externally observable:** only where a wrapped item's read passes the
  content end: the pin performs an unsafe read; oxpinyin answers no item.
  Class (b). Unit tests with a test-only modulus hold both wrap sites.
- **Disposition:** class (b), implemented (#1514);
  [compatibility policy](compatibility-policy.md), row 111.

### An oversized user chunk or journal bulk save (#1514; policy row 112)

- **Upstream source cite:** `src/pinyin.cpp:1132-1146`,
  `src/zhuyin.cpp:547-554` (both saves compact first);
  `src/storage/phrase_index.cpp:365-390` (`SubPhraseIndex::store`);
  `src/include/memory_chunk.h:95-98,290-296`;
  `src/storage/phrase_index.cpp:271-283` and
  `src/storage/phrase_index_logger.h:115-117` (`PhraseIndexLogger::store`),
  at `074a2219`.
- **Mechanism:** a `USER_FILE` `SubPhraseIndex::store` copies compacted
  content C at B = 18 + I in a fresh chunk. The growth narrows C to `int`,
  so the copy is unsafe when C ≥ 2^31, with destination end B+C. A
  `SYSTEM_FILE`/`DICTIONARY` library produces a diff whose
  `PhraseIndexLogger::store` copies the whole J-byte journal to a fresh
  chunk; that copy is unsafe when J ≥ 2^31.
- **What oxpinyin does instead:** it reproduces the pin's observable
  operations and filesystem effects up to the oversized copy, including
  the library-write order, baseline reads, staged temporary files and
  completed renames. At the copy it refuses without performing the
  undefined memory operation: it stops the save, keeps earlier filesystem
  changes, performs no later operation, answers `false` and keeps the
  session dirty for retry. Each check runs at its own copy in
  `persistence.rs`, `stage_rest`, with the data produced there; the error
  is `PersistenceError::SaveTooLarge`.
- **Externally observable:** process survival and the kept unsaved session
  state are deliberate class (b) departures. If the pin crashes at the
  overflowing copy before any further filesystem operation, the resulting
  on-disk state is reproduced. The pin's behaviour after the undefined copy
  is not otherwise claimed. The thresholds are source-derived.
- **Disposition:** class (b); [compatibility policy](compatibility-policy.md),
  row 112.

### A negative `MemoryChunk::load` payload read (policy row 114)

- **Upstream source cite:** `src/include/memory_chunk.h:452-453` at
  `074a2219`; in the tkrzw object, `pinyin::MemoryChunk::load` at offsets
  `+0x116` to `+0x13b`.
- **Mechanism:** `load` converts a negative `read` result, including
  `EINTR`, to `guint32` (`0xffffffff`) and checksums that length past an
  ordinary smaller allocation. The result is undefined: an out-of-bounds
  read.
- **What oxpinyin does instead:** it takes the checksum-failure path. It
  frees the buffer, closes the descriptor and answers false, without a
  retry.
- **Externally observable:** only when `read` fails. Class (b). The
  negative-read path is `pin_not_run`: the evidence is source- and
  disassembly-derived, not a live comparison. Rust-level single-read tests
  cover it and the defined nonnegative short-read result.
- **Disposition:** [compatibility policy](compatibility-policy.md), row 114
  (maintainer ruling).

### Berkeley DB handles whose open failed (2026-11-02 UTC, #1516)

[bdb-failed-handle](bdb-failed-handle.md) lists the accepted cases.
The explanation is [compatibility policy, row 113](compatibility-policy.md#bdb-failed-handles).

## Accepted divergences — utilities

`oxpinyin-utils` ports the pin's utilities under their own names. Every
edge call goes through `oxpinyin-input`'s `tools` boundary
([external-io-boundary.md](external-io-boundary.md), stage 5). The
`gen_pinyin_table` citations are `utils/storage/gen_pinyin_table.cpp` at
`074a2219c90feaf962d0d24f034514033ece5f99`. Policy rows 115–117 cover
the pin's storage test programs.

### gen_pinyin_table invalid UTF-8 phrase (class (b), 2026-10-30 UTC)

- **Upstream source cite:** `main` (`:97-131`), `feed_line` (`:160-232`),
  `phrase_item_compare` (`:85-94`), `gen_phrase_file` (`:259-322`).
- **Mechanism:** a phrase that is not valid UTF-8 can pass both of
  `feed_line`'s checks (`g_utf8_strlen` counts lead bytes, and a pinyin of
  that many keys parses). `g_utf8_to_ucs4` then returns NULL (`:174`). The
  item enters the tree with that NULL `uniphrase`. `main` reads every input
  file (`:110-112`), prints `nnodes` (`:114`), and stores and sorts
  (`:116-127`). `gen_phrase_file` opens the output with `fopen(outputfile,
  "w")` (`:261`) before `g_ucs4_to_utf8` reads through the NULL
  (`:280-281`). This is undefined behaviour, a SIGSEGV on the reference host.
  The buffered `nnodes` line and any rows already printed are lost with the
  process. Where the tree compares the item with another phrase of its
  length, `phrase_item_compare`'s `memcmp` reads through the NULL earlier,
  before the output is opened, at a point that GLib's tree shape decides.
- **What oxpinyin does instead:** it keeps the item unconverted, reads
  every input and opens the output at the pin's point (`:261`). At the row
  where the pin reads through the NULL (`:280-281`), it writes
  `gen_pinyin_table: invalid UTF-8 phrase: <phrase bytes>` on `stderr` and
  exits 1 through `exit`, which flushes what it wrote. The pin crashes
  there (a SIGSEGV on the reference host); oxpinyin exits 1 with that
  message. stdout and any partial output may differ. The pin's crash
  discards whatever its C streams still buffer: the `nnodes` line and rows
  already printed, depending on whether stdout is a pipe or a terminal.
  That loss is the effect of the undefined behaviour and libc buffering,
  not behaviour that oxpinyin reproduces. An unconverted phrase sorts
  before the converted phrases of its length, so oxpinyin does not
  reproduce the earlier `memcmp` death.
- **Externally observable:** the exit status, oxpinyin's message, and
  stdout and the output file where the pin's buffers were lost. Held by
  `an_unconvertible_phrase_is_refused_at_the_pins_row_after_the_output_opens`
  (`crates/oxpinyin-utils/tests/gen_pinyin_table.rs`). Against the pin-built
  utility (#1436) the two invalid-UTF-8 cases are POLICY-ACCEPTED under
  this record.
- **Also class (b), no observable choice made:** `g_utf8_strlen` over a
  truncated sequence at the end of a `%1023s` word steps past its NUL into
  the indeterminate bytes of the pin's `char phrase[1024]` (`:136`, `:162`);
  oxpinyin's buffer keeps the earlier words' bytes as the pin's frame does,
  starts zeroed and has eight zero bytes past the 1024. A `-t` index of 128
  or more, or a negative one, overflows the pin's `int` shift in
  `PHRASE_INDEX_MAKE_TOKEN` (`src/include/novel_types.h:45-46`); oxpinyin
  wraps the shift and keeps the masked bits.
- **Record:** [eio-table-generator-invalid-utf8.md](eio-table-generator-invalid-utf8.md)
  lists the four accepted invalid-UTF-8 cases. The tests in
  `crates/oxpinyin-utils/tests/gen_pinyin_table.rs` and `gen_zhuyin_table.rs`
  hold oxpinyin's side.
  The zhuyin utility's missing-frequency null read is recorded separately
  in [eio-zhuyin-missing-frequency.md](eio-zhuyin-missing-frequency.md).

### k_mixture_model_to_interpolation undefined lines (2026-10-31 UTC)

**Class (b).** The pin's `taglib_read` gives `strcmp` the NULL `tokens[0]`
of a line without a token. It also reads through the NULL that `split_line`
returns for an escape at the end of the line (`src/storage/tag_utility.cpp:137`,
`:181-190`). The reference pin dies with SIGSEGV. oxpinyin's utility makes
the pin's calls up to that read, GLib's criticals included, then refuses
with exit 1. Recorded in
[eio-kmm-to-interpolation-undefined-lines.md](eio-kmm-to-interpolation-undefined-lines.md).
A truncated UTF-8 sequence at the end of the line, whose next `*cur` reads
whatever follows the string, is refused the same way. The pin has no
reproducible outcome there to bind.
`undefined_lines_are_refused_normally` holds the refusal.
The utility's `abort()` calls are not divergences: both sides abort, and
`tool-diff` compares them as ordinary cases.

### g_utf8_next_char from a byte that leads no sequence (2026-10-31 UTC)

**POLICY-ACCEPTED.** `split_line` steps with `g_utf8_next_char`, which
GLib documents for valid UTF-8 and implements with its internal
`g_utf8_skip` table. oxpinyin steps the length the lead byte gives, and 1
for a byte that leads no sequence. Only `0xF8..=0xFD`, which the table
steps 5 or 6, differ. Recorded with its cases in
[eio-utf8-next-char-invalid-lead.md](eio-utf8-next-char-invalid-lead.md).

### Utility check macros write no message (2026-10-31 UTC)

**Language artefact.** Under the maintainer ruling the utilities' `assert` (`check_result`) and `g_return_val_if_fail` checks are
plain Rust: on the pin's condition oxpinyin answers the pin's value or
aborts where the pin aborts, without glibc's assertion line or GLib's
critical. Stdout and the SIGABRT exit are the pin's. Recorded in
[eio-tool-check-messages.md](eio-tool-check-messages.md).

The same ruling covers the seven KMM utilities' asserts:
`merge`'s missing result attach, `estimate`'s zero magic total and `gen`'s
missing container header abort with SIGABRT on both sides and no
subject assertion line. Recorded in
[eio-kmm-tools.md](eio-kmm-tools.md).

### Pinyin-index item padding is indeterminate in the pin (class (b), 2026-11-01 UTC)

`ChewingTableEntry<N>::add_index` (`src/storage/chewing_large_table2.h:
208-232`) copies a stack `PinyinIndexItem2<N>` (`src/storage/pinyin_phrase3.h:
181-196`) into the DBM value whole. For an odd `N`, the object's last two
bytes are tail padding that no constructor sets. The subject's
`row_format::pinyin_index::encode_item` writes zeros there. Tokens and keys
are identical and no reader interprets the padding. Recorded for the
`gbf-toned-model` case in
[eio-pinyin-index-padding.md](eio-pinyin-index-padding.md). The case is
POLICY-ACCEPTED on bdb, kc and sound tkrzw. The comparison masks only the
two tail bytes of each odd-`N` item and compares whole DBM rows, so
container metadata does not enter it. Every other mismatch is DIFFER.

### FlexibleBigram malformed record lengths

- **Upstream source cite:** `src/storage/flexible_ngram_bdb.h:165-187`
  and its Kyoto Cabinet and tkrzw peers at 074a2219;
  `src/storage/flexible_single_gram.h:88-105,151-181`.
- **Mechanism:** the pin wraps the whole stored value in a
  `FlexibleSingleGram` and reads the header and item tail without
  checking that the value holds whole records.
- **What oxpinyin does instead:** `SpecGram::decode` answers
  `StoreError` for a value shorter than its header or with a partial
  item tail.
- **Externally observable:** only for a corrupted or hand-written
  container; a pin-written file never takes this path.
- **Record:** [eio-flexible-bigram-malformed-records](eio-flexible-bigram-malformed-records.md),
  class (b).

### Consumed invalid UTF-8 in family E text rows or phrase-table queries (policy row 115)

- **Upstream source cite:** `tests/storage/test_phrase_table.cpp:45-59`,
  `src/storage/phrase_large_table3_tkrzwdb.cpp:133-136`,
  `phrase_large_table3.cpp:131-135`, and `FacadePhraseIndex::load_text`
  (`phrase_index.cpp:541-586`), at `074a2219`.
- **Mechanism:** the phrase-table test passes failed `g_utf8_to_ucs4`
  output to search, and phrase-table text loading also consumes it. In
  `FacadePhraseIndex::load_text`, the conversion returns NULL without
  setting the uninitialized `glong written` (`:541-543`). Only
  `cur_token == 0` or a changed token consumes it in
  `set_phrase_string(written, NULL)` (`:545-556`), which copies an
  uninitialized length from NULL: undefined behaviour. Token 0 keeps
  `cur_token` at 0, so every token-0 row consumes its conversion. A same
  nonzero token never uses the failed conversion: it parses the reading,
  adds it when its key count matches, otherwise prints the source skip
  line (`:577-582`), then frees NULL (`:586`). That path is defined.
- **What oxpinyin does instead:** it stops the consuming failures with a
  normal `exit(EINVAL)` and no added diagnostic, and keeps earlier
  observable work. The defined same-token path continues as in the pin.
- **Externally observable:** only for invalid UTF-8 input. Class (b). The
  two same-token cases are compared against the pin. The undefined
  consuming paths never run the pin (`pin_not_run`); the subject guard
  checks that earlier timed output stays.
- **Disposition:** maintainer-approved safe outcome;
  [compatibility policy](compatibility-policy.md), row 115.

### More than sixteen keys passed to the chewing-table test search (policy row 116)

- **Upstream source cite:** `tests/storage/test_chewing_table.cpp:71-86`,
  `src/storage/chewing_large_table2.cpp:161-172` and
  `src/storage/pinyin_phrase3.h:146-177`, at `074a2219`.
- **Mechanism:** the test passes all parsed keys to the search, whose
  local `ChewingKey index[MAX_PHRASE_LENGTH]` has sixteen slots.
  `pinyin_phrase3.h:160-177` writes phrase_length keys before
  search_internal's switch can abort: a stack-buffer write.
- **What oxpinyin does instead:** after it checks only the first
  incomplete key's zero tone (`pinyin_phrase3.h:146-157`), it stops
  immediately before the unsafe copy with a normal `exit(EINVAL)` and no
  added diagnostic, and keeps earlier observable work. This is separate
  from row 84's stored-key dispatch.
- **Externally observable:** only for an over-long query. Class (b).
  `chewing-table-overlong-query` is `pin_not_run`: the pin is not driven
  into the write. The subject guard after a valid timed query checks that
  earlier stdout stays.
- **Disposition:** maintainer-approved safe outcome;
  [compatibility policy](compatibility-policy.md), row 116.

### The logger test ignores a failed required phrase-chunk load (policy row 117)

- **Upstream source cite:** `tests/storage/test_phrase_index_logger.cpp:28-30,45-48,52-54`
  and `:55-57`; `phrase_index.cpp:271-282,335-354`;
  `phrase_index_logger.h:94-133`, at `074a2219`.
- **Mechanism:** the three required `../../data/gb_char.bin` loads ignore
  `MemoryChunk::load` returning false. The first and third enter
  `SubPhraseIndex::load` directly; `diff` loads the second as its old
  sub-index. Each reaches row 76's NULL read.
- **What oxpinyin does instead:** it stops a failed required phrase-chunk
  load with a normal `exit(ENOENT)` and no added diagnostic, and keeps
  earlier observable work. The separate journal reload at `:55-57` is
  defined on failure: `PhraseIndexLogger::load` keeps the empty chunk and
  `has_next_record` finds nothing, so nothing is merged. oxpinyin does not
  apply the refusal to the journal.
- **Externally observable:** only when a required chunk is missing or
  short. Class (b). `phrase-index-logger-missing` and
  `phrase-index-logger-short` (`crates/oxpinyin-utils/tests/storage_tools.rs`)
  hold the prescribed result. The pin is not run into the undefined read.
- **Disposition:** maintainer-approved safe outcome;
  [compatibility policy](compatibility-policy.md), row 117.

## Approved product and ruling exceptions

### Unloaded-token pronunciation uses no cached item

- **Upstream source cite:** `phonetic_lookup.h:924–929` at `074a2219`.
- **Mechanism:** a failed item fetch leaves the context's shared lookup cache
  in place; training can mutate a different loaded item or access freed bytes.
- **What oxpinyin does instead:** skips the unloaded token's pronunciation
  step, preserving bigram training, the constraint walk, modified flag and result.
- **Externally observable:** yes; the defined witness saves 甲's pronunciation
  count as 79 in the pin and leaves it at 10 in oxpinyin.
- **Record and disposition:** [compatibility policy](compatibility-policy.md),
  ruling “Unloaded-token pronunciation cache” and row 118;
  `unloaded-cached-pronunciation-pinyin` and
  `unloaded-cached-pronunciation-zhuyin`, `POLICY-ACCEPTED`.

### Pinyin trellis adjusted cost — S1/S2 repair (2026-10-28 UTC)

[Pinyin trellis adjusted-cost comparison](trellis-adjusted-cost-s1-s2.md)
records the repair authorized by the witnessed algorithm-precondition amendment:
ordered adjusted cost for node heaps and preselection batch-position ties for
beam/final-tail selection. Four BDB cases report `POLICY-ACCEPTED`,
each attributed in that record. The record also accepts, on bdb, kc and tkrzw
(#1400), the two gb table.conf cases that the S2 tie rule moves and the two
negative-lambda cases that the special-cost completion moves. It accepts the
three lambda cases on kc and tkrzw too. This is the amendment's product-level ruling,
not a new exception class. Zhuyin, tail ordering, node front eviction,
frequency wrap and lambda validation are unchanged.

### Library `g_return_val_if_fail` criticals are not reproduced (2026-10-31 UTC)

**Maintainer decision D1 of #1297**, a narrow exception to the library
stderr ruling (#545) under the no-shims ruling. Both
facades' `phrase_segment` on invalid UTF-8 (`pinyin.cpp:1454`,
`zhuyin.cpp:973`) and `SubPhraseIndex::load`'s layout checks
(`phrase_index.cpp:354-356`, `:361`) keep the pin's return values, state
and continuation and log no critical. Lost: the critical a caller's log
handler or stderr receives, and the pin's abort under GLib fatal-critical
settings. Recorded in
[1297-segment-critical.md](1297-segment-critical.md) and
[1297-layout-criticals.md](1297-layout-criticals.md).

### Reproduced library aborts write no assertion line (2026-10-31 UTC)

**POLICY-ACCEPTED, stderr only.** With class (c) retired, oxpinyin
reproduces each pin `assert` site with `std::process::abort()`. It does
not rebuild glibc's `Assertion '…' failed.` line. The SIGABRT status, the
earlier stderr bytes and the files left are MATCH. Recorded in
[pin-abort-assert-line.md](pin-abort-assert-line.md).

### Pin aborts reproduced in the pinyin C facade (2026-10-31 UTC)

**Pin behaviour reproduced.** Member 3 of the class (c) retirement: the
pinyin C facade aborts the process with `std::process::abort()` at the
pin's `assert` and `abort()` sites of rows 4, 5a, 10, 14, 38, 45, 63, 64,
81, 88, 93 and 95, and on the pinyin arm of rows 5c, 5d, 6, 19, 21, 44,
61, 62, 66, 74, 77, 79, 82, 84, 86, 87 and 94. `abort()` sites are MATCH;
`assert` sites are POLICY-ACCEPTED for the missing assertion line only.
Recorded, with the pin sites, items and cases, in
[pin-aborts-pinyin-facade.md](pin-aborts-pinyin-facade.md).

### Pin aborts reproduced in the zhuyin C facade (2026-10-31 UTC)

**Pin behaviour reproduced.** Member 4 of the class (c) retirement: the
zhuyin C facade aborts the process with `std::process::abort()` at the
pin's `assert` and `abort()` sites of rows 89, 90 and 104, and on the
zhuyin arm of rows 5c, 5d, 6, 19, 21, 44, 61, 66, 74, 77, 79, 82, 84, 86,
87 and 94. `abort()` sites are MATCH; `assert` sites are POLICY-ACCEPTED
for the missing assertion line only. Recorded, with the pin sites, items
and cases, in [pin-aborts-zhuyin-facade.md](pin-aborts-zhuyin-facade.md).

### Pin aborts reproduced in the journal replay (2026-10-31 UTC)

**Pin behaviour reproduced.** Member 5a of the class (c) retirement. A
system library's user `.dbin` is replayed in the pin's order at init, at a
library reload and at `mask_out`. The assertions that the pin's walk reaches
(rows 91, 92, 97 and 108: `phrase_index_logger.h:202`, `:245`, `:257`,
`phrase_index.cpp:745`, `memory_chunk.h:390`) return a typed fault, on which
both C facades call `std::process::abort()`. There is no up-front log
validation and no refusal for it. All sites are `assert`s: POLICY-ACCEPTED
for the missing assertion line only. Recorded, with the pin sites, items and
cases, in [pin-aborts-journal-replay.md](pin-aborts-journal-replay.md).

### Pin aborts reproduced: the short user-bigram total and the directory library (2026-10-31 UTC)

**Pin behaviour reproduced.** Member 5c of the class (c) retirement, the
last two rows.

- Row 85: a user-bigram value shorter than its `guint32` total
  (`memory_chunk.h:390`) is a typed fault at mask-out, candidate removal,
  predicted guessing and training, `*_train`, the lookups and the bigram
  export walk. Both C facades abort on it. The export aborts where
  `has_next` loads that predecessor, after the earlier rows. The partial
  item tail of rows 98 and 100 is a separate fault and keeps its class (b)
  refusals.
- Row 96: a system library that is a directory (`memory_chunk.h:493`) is
  the fault where the library loops of init, the save and the mask map it.
  The fault comes after the work of the earlier libraries (the save's
  earlier `.tmp` files, renamed by libzhuyin). It is also the fault at the
  reload and the addon load.

All sites are `assert`s: POLICY-ACCEPTED for the missing assertion line
only. Every class (c) row is reproduced.
Recorded, with the pin sites, items and cases, in
[pin-aborts-remaining.md](pin-aborts-remaining.md).

## Language-artefact records

### Candidate sorting guards the wrapping frequency order (#1297 item A)

A maintainer decision preserves libpinyin's frequency
comparison as `rhs.wrapping_sub(lhs) as i32`. Stable std sorting replaces
GLib's merge after guarding each enabled length/span tie group. Only cyclic
groups may use stable numeric-descending fallback; equal complete keys keep
collection order, and dedup-only `m_freq` never breaks ties. The guard proof,
source derivation and verification are in
[1297-candidate-numeric-sort](1297-candidate-numeric-sort.md). The four
`row58-signed-{score,duplicate}-{off,on}` cases must MATCH and are not
accepted by this registration.

### Zhuyin selection uses length, raw score and batch position (#1378)

The “No ported external-library internals” ruling orders zhuyin
selection by length, descending raw possibility (NaN last, signed zeros
equal), then earlier batch position. The equal-count user-phrase case
`nbest-user-table-order-zhuyin` and the three NaN-producing λ cases are
POLICY-ACCEPTED. The derivation and the rank-0 results are in
[1378-zhuyin-selection](1378-zhuyin-selection.md).
Single-slot admission stays unchanged.

### Native scanner and tool parsing (#1297 Phase 2)

- [Punctuation values contain whole words](1297-punct-words.md): language
  artefact, POLICY-ACCEPTED. `decode_puncts` walks whole words, so a
  misaligned value gets no extra empty punctuation. The record stands under
  the external-I/O ruling. The unterminated read is the class (b) entry
  [Unterminated punctuation value over-read](#unterminated-punctuation-value-over-read).

### Stored gram partitions: phrase segmentation — inventory C

- **Record:** [1297-gram-partition-segmentation](1297-gram-partition-segmentation.md).
- **Status:** accepted. `user-gram-unsorted-phrase-segment` is
  POLICY-ACCEPTED on bdb, kc and sound tkrzw.
- **Upstream source cite:** `src/storage/ngram.cpp:231–250,308–349`
  and `src/lookup/phrase_lookup.cpp:268–290` at `074a2219`.
- **Mechanism:** the stored-order merge of an out-of-order gram violates
  the search predicate's partition requirement. `single_gram::lower_bound_records`
  uses `slice::partition_point` on whole stored records without sorting or cleaning.
- **Externally observable:** the pin segments 今天天气 as one phrase; Rust
  selects 今天 and 天气 separately after reaching the boosted 今天 record.
  Both segmenters return true. Ordered duplicates retain first-not-less
  semantics; existing refusals, persistent formats and C ABI are unchanged.

### Stored gram partitions: training — inventory C

- **Record:** [1297-gram-partition-training](1297-gram-partition-training.md).
- **Status:** accepted. `abort-train-unsorted-set-freq-miss-pinyin` and
  `abort-train-unsorted-set-freq-miss-zhuyin` are POLICY-ACCEPTED on bdb,
  kc and sound tkrzw.
- **Upstream source cite:** `src/storage/ngram.cpp:178–205,231–271`
  and `src/lookup/phonetic_lookup.h:888–903` at `074a2219`.
- **Mechanism:** an out-of-order user gram violates the search predicate's
  partition requirement. `single_gram::lower_bound_records` uses
  `slice::partition_point` on whole stored records without sorting or cleaning.
- **Externally observable:** the pin's final set-freq search misses after
  insertion and training aborts; Rust reaches the inserted item and training
  returns true with no warning. An actual post-insertion miss still uses the
  existing refusal. Persistent formats and C ABI are unchanged.

### The full-pinyin parse's 16-bit steps (2026-11-01 UTC, #1501)

**Language artefact, POLICY-ACCEPTED.** The pin's full-pinyin parse stops at byte 32767 because its steps are `gint16`s (`pinyin_parser2.cpp:67-70`); oxpinyin parses the whole input, and policy row 59 is reopened: [1501-gint16-input-cap.md](1501-gint16-input-cap.md).

### Key positions past byte 65535 (2026-11-01 UTC, #1501)

**Contract for the getters, POLICY-ACCEPTED for the matrix.** The C getters wrap key-rest positions past 65535 mod 2^16 as the pin's `guint16` fields do; oxpinyin keeps real positions where the pin's `fill_matrix` places keys at the wrapped begin (`phonetic_key_matrix.cpp:51-55`): [1501-guint16-key-positions.md](1501-guint16-key-positions.md).

### FlexibleBigram unsorted-record search (2026-11-01 UTC)

[eio-flexible-bigram-unsorted-records](eio-flexible-bigram-unsorted-records.md)
records the standard-library search contract and the malformed KC KMM
fixture's provenance. The implementation uses Rust partitioning, and
there is no copied external probe sequence. Sorted results remain exact.
Any distinguishing unsorted result is a language artefact under the
“No ported external-library internals” ruling, and needs concrete
POLICY-ACCEPTED evidence.

## Outstanding upstream defects (report-back)

These entries are pin defects that oxpinyin collects for the libpinyin
report-back batch. The drafts are in
[upstream report drafts](upstream-report-drafts.md); none is filed.

On arm64 the pin departs from its own amd64 reference because it assigns
an out-of-range `float` expression to a `guint32`, a conversion C leaves
undefined. oxpinyin stores the score with Rust's `as u32`, which in range
truncates like both pins and out of range stores what the arm64 pin's
`fcvtzu` stores (decision-order limb 3,
[source policy](../source-policy.md#internal-language-artefacts)). Against
the amd64 pin the out-of-range cases are therefore class (b) divergences,
`POLICY-ACCEPTED` and held by the cases named in each entry; in range
nothing changes. There is one entry per independent defect. The full
diagnosis, the instruction sequences and the proposed issue texts are in
[`arm64-pin-portability-1137.md`](arm64-pin-portability-1137.md).

The `LC_NUMERIC` entry is closed locally: oxpinyin reproduces the pin.
The defect is not filed upstream.

### `pinyin_init`/`zhuyin_init` leave the process `LC_NUMERIC` at "C" (policy row 39)

- **Upstream source cite:** `src/storage/table_info.cpp:328` and `:372`
  (`UserTableInfo::load`), `:197` and `:291` (`SystemTableInfo2::load`),
  `:378` and `:394` (`UserTableInfo::save`).
- **Mechanism:** each function saves `setlocale(LC_NUMERIC, "C")`'s
  return value and restores it at the end. `setlocale` returns the name
  of the *new* locale (C11 7.11.1.1p7), so the "restore" applies `"C"`
  again. Every early return, such as a failed `fopen` (`:330-333`,
  `:199-202`) or a failed version directive (`:339-348`), skips the
  restore. In both cases, the process's `LC_NUMERIC` is `"C"`
  after `pinyin_init` or `zhuyin_init`, successful or not.
- **What oxpinyin does instead:** it reproduces the pin. The `setlocale`
  calls are the pin's own, made inside `oxpinyin-input`'s
  `table.conf`/`user.conf` reader and writer at the pin's sites and in the
  pin's number: before `fopen`, and again after `fclose` on the success
  path only.
- **Externally observable:** no difference from the pin. With
  `LC_ALL=zh_CN.UTF-8` set by the host, `setlocale(LC_NUMERIC, NULL)`
  reads `C` after init on both sides (audit D-17, #539).
- **Status:** ruled bug-for-bug and **CLOSED** locally (#618; policy
  row 39). The upstream defect is outstanding: it is drafted as item 6
  of [upstream report drafts](upstream-report-drafts.md) and is not
  filed.

### Pinyin candidate frequency: out-of-range `float`-to-`guint32` store (#1137, #1416; class (b))

- **Upstream source cite:** `src/pinyin.cpp:1813-1826` (predicted-prefix
  branch), `:1829-1843` (addon branch) and `:1855-1868` (ordinary branch)
  at `074a2219c90feaf962d0d24f034514033ece5f99`; the conversions are the
  `guint32 freq = ...` initializations at `:1821-1823`, `:1838-1840` and
  `:1862-1865`, and the stores are `item->m_freq = freq` at `:1824`,
  `:1841` and `:1866`.
- **Mechanism:** each branch computes a `gfloat` candidate frequency and
  assigns it to `guint32`. When the value is negative or at least 2³²,
  the conversion is undefined (C++ [conv.fpint]/1). GCC 15.3.0 emits
  `cvttss2si` to a 64-bit register plus a 32-bit store for x86-64 and
  `fcvtzu` for arm64; the latter saturates, so a negative value becomes 0
  and a value at least 2³² becomes `0xffffffff`. For the observed case
  the arm64 conversion is `libpinyin.so.15.0.0+0x72644`
  (predicted-prefix) and the shared store is `+0x72530`; the amd64
  conversion is `+0x8fcce` and the store is `+0x8fb6e`.
- **What oxpinyin does instead:**
  `oxpinyin_engine::session::amplified_frequency_with_bigram`, which the
  predicted rows (`oxpinyin_capi::predict::predicted_frequency`) and the
  ordinary candidate lists share, stores the `f32` score with Rust's
  `as u32`. In range, it truncates like the pin. A negative or NaN score
  is 0, and a score at or above 2³² is `0xffffffff`. oxpinyin does not
  imitate the amd64 `cvttss2si` result (decision-order limb 3, #1416).
  The pin's `guint32` comparator still orders the rows
  (`oxpinyin_engine::candidate::rank_by_frequency`).
- **Externally observable:** yes, in the candidate order, only when a
  score is out of range. `row58-unsigned-score-off` and
  `row58-unsigned-score-on`: import `我甲甲` with
  count 1431655631, bring the facade total to 10000000 through the
  absent-token delta, then call
  `pinyin_guess_predicted_candidates("我")`. The frequency is
  `4952525312.0f`. The amd64 pin stores `0x27318a00 = 657558016` and
  lists `甲甲` first. oxpinyin stores `0xffffffff` and lists it last
  among the two-character rows, as the arm64 pin does. The other 167
  rows are in the same order on both sides.
- **Classification:** class (b). C leaves the conversion undefined, and
  the pin's own result differs between x86-64 and AArch64. oxpinyin's
  saturating store is the language's defined result and is not tuned to
  either target. Also a pin portability defect to report upstream.
- **Held by:** `oxpinyin-engine`'s
  `session::tests::candidate_scores_follow_rusts_saturating_conversion`,
  which holds oxpinyin's value. The pin observations `row58-unsigned-score-off`
  and `row58-unsigned-score-on` are `POLICY-ACCEPTED` on bdb, kc and sound tkrzw against the amd64 pin,
  as [`arm64-pin-portability-1137.md`](arm64-pin-portability-1137.md)
  records (#1416).
- **Proposed upstream report:** issue A in
  `arm64-pin-portability-1137.md` ("pinyin candidate frequency:
  undefined float-to-`guint32` conversion is target-dependent").

### Zhuyin candidate frequency: negative `float`-to-`guint32` store at λ > 1 (#1137, #1416; class (b))

- **Upstream source cite:** `src/zhuyin.cpp:1229-1270`
  (`_compute_frequency_of_items`), the conversion in the
  `guint32 freq = ...` initialization at `:1264-1267` and the store at
  `:1268`, at `074a2219c90feaf962d0d24f034514033ece5f99`.
- **Mechanism:** with the default options `DYNAMIC_ADJUST` is clear, so
  the frequency is `(1 - lambda) * unigram / (gfloat) total_freq *
  256 * 256 * 256`. For λ > 1 the coefficient is negative, every
  positive-unigram candidate gets a negative `float`, and the assignment
  to `guint32` is undefined. GCC 15.3.0 emits `cvttss2si` plus a 32-bit
  store for x86-64 (`libzhuyin.so.15.0.0+0x8e06d`, store `+0x8e072`) and
  `fcvtzu` for arm64 (`+0x70938`, store `+0x7093c`). amd64 keeps the
  low 32 bits of `-k` (for `0 < k < 2³²`, that is `2³² - k`); arm64
  stores 0, so every negative key is equal.
- **What oxpinyin does instead:** the shared engine law
  `oxpinyin_engine::session::amplified_frequency_with_bigram` stores a
  negative score as 0 (Rust's `as u32`). Thus, for λ > 1, every
  positive-unigram candidate has the key 0, and the rows keep their
  collection order under the pin's comparator, as on the arm64 pin.
  oxpinyin does not imitate the amd64 low-32 key (decision-order limb 3,
  #1416).
- **Externally observable:** yes, against the amd64 pin (the reference), in
  `zhuyin_guess_candidates_after_cursor`. oxpinyin stores 0, as the arm64
  pin does, so its length-1 rows keep their collection order. With
  `lambda parameter:1.5`, `2` and `1e6` in `table.conf`, the arm64 pin's
  candidate rows 3–12 are the collection order within the length-1 group
  `疒 祢 禰 妳 呢 堄 逆 猊 霓 儗` and are identical for all three λ
  values. The amd64 pin orders them by the wrapped key and differs per
  λ. The pin's sentences are identical on both targets. The λ < 0, `inf` and
  `nan` cases match because their keys are in range or equal on both
  architectures.
- **Classification:** class (b). C leaves the conversion undefined and
  the pin's own result differs between x86-64 and AArch64 (policy row
  83 keeps λ outside `[0, 1]` itself as reproduced input handling). Also
  a pin portability defect to report upstream.
- **Held by:** `table-conf-lambda-million-zhuyin`,
  `table-conf-lambda-one-and-a-half-zhuyin` and
  `table-conf-lambda-two-zhuyin`, `POLICY-ACCEPTED` on bdb, kc and
  sound tkrzw against the amd64 pin under
  [`1378-zhuyin-selection.md`](1378-zhuyin-selection.md), which
  already accepted the #1378 selection difference in the same cases (#1416).
- **Proposed upstream report:** issue B in
  `arm64-pin-portability-1137.md` ("libzhuyin candidate frequency: λ > 1
  orders candidates differently on arm64").
