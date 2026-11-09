# Pin aborts reproduced in the pinyin C facade

Status: implemented, 2026-10-31 UTC (member 3 of the stack that retires
class (c)). Rule: [compatibility policy, class (c)
retired](compatibility-policy.md#failure-behaviour).
Pin: libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`, read from a
`git archive` of the pin; `check_result` is `assert` in the reference build
(`src/include/pinyin_utils.h:27-31`).

The pinyin C facade (`crates/oxpinyin-capi`) used to answer `false`,
NULL or `-1`/`0` with one `libpinyin` warning at each site below. It now
calls `std::process::abort()` where the pin asserts or aborts. The shared
Rust crates still return the typed error at the pin's point (the engine,
facade and user store are unchanged except where noted), and the C entry
point turns that error into the abort. Every refusal text these sites
logged is deleted. The zhuyin facade is member 4 and is unchanged here;
the rows member 5 owns (85, 91, 92, 96, 97, 103, 108 and row 62's
store-read failures) keep their refusals.

**Disposition.** An `abort()` site writes nothing on either side: the
case is MATCH. An `assert` site writes glibc's assertion line at the pin
only: the case is POLICY-ACCEPTED for that line
([pin-abort-assert-line.md](pin-abort-assert-line.md)) and MATCH on the
SIGABRT status, the earlier stderr and, with `userdir`, the files. The
cases are registered `sigabrt=True` in `tools/bisection/contract_*.py`.

**Preceding work.** For every site the pin's path from the API entry to
the abort was read. Before the abort the pin changes only in-memory state
(its user tables are in-memory DBMs written at save), except at row 77,
where it has written the `.tmp` files of earlier libraries, and at
`pinyin.cpp:388` and `memory_chunk.h:434` of row 79, where `check_format`
has written `user.conf`. oxpinyin's facade stops at the same point with the
same files in both cases, and the abort skips every destructor, so nothing
else is written.

## Sites

| Row | Pin site(s) | Pinyin entry points | oxpinyin item that aborts | Cases | Disposition |
|---|---|---|---|---|---|
| 4 | `storage/pinyin_phrase3.h:152` `assert`; `pinyin.cpp:2769` `assert` | `pinyin_guess_candidates`, `pinyin_guess_sentence`, `pinyin_guess_sentence_with_prefix`; `pinyin_get_pinyin_is_incomplete` | the engine's table searches return `EngineError::TonedIncompleteKey` (`EngineError::check_toned_incomplete`) where the pin's search runs, and `sentence.rs` aborts on it (`refuse_lookup_gram`, the `candidates_at` arm); `keys.rs` `pinyin_get_pinyin_is_incomplete` | `abort-guess-candidates-toned-initial`, `abort-guess-candidates-toned-initial-before-offset`, `abort-guess-candidates-toned-initial-no-prepends`, `abort-guess-sentence-toned-initial`, `abort-guess-sentence-toned-initial-after-key`, `abort-is-incomplete-toned-initial`; controls `guess-candidates-toned-initial-past-key-no-prepends`, `guess-candidates-toned-initial-before-offset-no-prepends` | POLICY-ACCEPTED; controls MATCH |
| 5a | `storage/pinyin_parser2.cpp:611` `abort()` | `pinyin_set_double_pinyin_scheme(30)` | `config.rs` `pinyin_set_double_pinyin_scheme` | `abort-set-double-pinyin-scheme-30` | MATCH |
| 5c | `storage/zhuyin_parser2.cpp:295` `abort()` | `pinyin_set_zhuyin_scheme(7)` | `config.rs` `pinyin_set_zhuyin_scheme` | `abort-set-zhuyin-scheme-7` | MATCH |
| 5d | `pinyin.cpp:1189` `abort()`; `storage/pinyin_parser2.cpp:398` `abort()` | `pinyin_set_zhuyin_scheme` out of the enum; `pinyin_set_full_pinyin_scheme` outside 1–3 | `config.rs` `pinyin_set_zhuyin_scheme`, `pinyin_set_full_pinyin_scheme` | `abort-set-zhuyin-scheme-0`, `-10`; `abort-set-full-pinyin-scheme-0`, `-4` | MATCH |
| 6 | `lookup/phonetic_lookup.h:868` `assert` | `pinyin_train` | `candidates.rs` `pinyin_train` on `EngineError::StaleTrainingConstraint` | `abort-train-stale-forcing` | POLICY-ACCEPTED |
| 10 | `pinyin.cpp:1474` `assert` | `pinyin_get_sentence` past the rows | `sentence.rs` `pinyin_get_sentence` | `abort-get-sentence-past-rows` | POLICY-ACCEPTED |
| 14 | `pinyin.cpp:3035`, `:3057`, `:3067`, `:3092` `assert`; `storage/phonetic_key_matrix.h:103` `assert` past the matrix | `pinyin_get_left_pinyin_offset`, `pinyin_get_right_pinyin_offset` | `cursor.rs`, on `ZeroKeyOffsetCheck` and `LookupOffsetOutOfRange` only (the former catch-all is narrowed; a graph failure keeps `false` and a warning) | `abort-get-left-pinyin-offset-after-separator`, `abort-get-right-pinyin-offset-after-separator`, `abort-get-right-pinyin-offset-nihao-5`, `separator/pinyin/*/left/*`, `separator/pinyin/*/right/*` | POLICY-ACCEPTED |
| 19 | `pinyin.cpp:3147`, `:3161`, `:3203`, `:3204` `assert` | `pinyin_get_character_offset` | `sentence.rs` `pinyin_get_character_offset`, on `MatrixColumnAssert`, `LookupOffsetOutOfRange` and `ZeroKeyOffsetCheck` only | `abort-get-character-offset-past-matrix`, `abort-get-character-offset-after-separator`, `separator/pinyin/*/character/*`, `c-string-input/pinyin-character-offset-null-past-matrix`, `-invalid-past-matrix` | POLICY-ACCEPTED |
| 21 | `storage/pinyin_parser2.cpp:170` `assert`; `pinyin.cpp:466`, `:499` `assert` | `pinyin_parse_full_pinyin` with `'`; `pinyin_unload_phrase_library` and `pinyin_unload_addon_phrase_library` with an index of 16 or more | `keys.rs` `pinyin_parse_full_pinyin`; `config.rs` `pinyin_unload_phrase_library`; `state.rs` `CapiContext::unload_addon` | `abort-parse-full-pinyin-apostrophe`, `c-string-input/pinyin-full-key-apostrophe-valid`, `-invalid`, `abort-unload-phrase-library-16`, `abort-unload-addon-phrase-library-16` | POLICY-ACCEPTED |
| 38 | `pinyin.cpp:2684` `assert` | `pinyin_train` past nonempty results | `candidates.rs` `pinyin_train` on `CandidateIndexOutOfRange` | `abort-train-index-past-results` (`userdir`); [archived train index diff source](https://github.com/shenghaoc/oxpinyin/blob/813a495efaadd55fe372611b4592b5cdbf78a919/tools/bisection/train_index_diff.py) cases `len`, `255` | POLICY-ACCEPTED |
| 44 | `storage/table_info.cpp:132` `abort()`, reached from `:353-354` | `pinyin_init` | `context.rs` `init_context` on `OpenFailure::unknown_database_format` | `user-conf-abort-database-format-unknown` (`userdir`); `open-counter-diff` `abort-*` (pinyin) | MATCH |
| 45 | `pinyin.cpp:2566` `assert` | `pinyin_choose_candidate` with a nonzero offset under `SORT_WITHOUT_SENTENCE_CANDIDATE` | `candidates.rs` `pinyin_choose_candidate` | `abort-choose-candidate-without-sentence-offset-1` | POLICY-ACCEPTED |
| 61 | `pinyin.cpp:1859` `assert` | `pinyin_guess_candidates`; `pinyin_guess_predicted_candidates` and `_with_punctuations` (a predicted bigram row takes the same scorer branch, `:2432` into `:1855-1859`) | `sentence.rs` `pinyin_guess_candidates` on `CandidateList::ranked_phrase_row`; `predict.rs` `guess_predicted_result` | `facade-total-zero`, `nonzero-total-candidates`, `row58-total-zero-off`, `-on`, `-punct-off`, `-punct-on` | POLICY-ACCEPTED |
| 62 | `pinyin.cpp:457`, `:709`, `:721`, `:902` `assert`; `:714`, `:724` `check_result`; `storage/chewing_key.cpp:48`, `:50` `assert` | `pinyin_load_phrase_library`; `pinyin_iterator_get_next_phrase`; `pinyin_bigram_iterator_get_next_phrase` | `config.rs` `pinyin_load_phrase_library` on `LibraryRowAssert::NotLoadable` (the catch-all is narrowed); `iterators.rs` on `ExportStep::Assert` and `BigramStep::Aborts` | `abort-load-phrase-library-0`, `-8`, `abort-iterator-get-next-phrase`, `abort-bigram-iterator-get-next-phrase`, `table-conf-abort-load-library-*`; the unspellable reading by `e2e_tests::export_iterator_aborts_on_a_reading_it_cannot_spell` | POLICY-ACCEPTED |
| 63 | `storage/phonetic_key_matrix.h:103` `assert`; `pinyin.cpp:3311` `assert`; `pinyin.cpp:3488` `abort()` | `pinyin_get_pinyin_key`, `pinyin_get_pinyin_key_rest` on an empty matrix; the three auxiliary-text getters | `cursor.rs` `abort_on_empty_matrix`; `text.rs` `render_shared`, `abort_on_full_walk` | `abort-get-pinyin-key-empty-matrix`, `abort-get-pinyin-key-rest-empty-matrix`, the four `abort-*-auxiliary-text-*leading-apostrophe` (POLICY-ACCEPTED); `abort-double-auxiliary-text-cut-three-bytes-in` (MATCH) | as stated |
| 64 | `pinyin.cpp:2507`, `:2593`, `:2883`, `:3734`, `:3738` `assert` | `pinyin_choose_candidate`, `pinyin_choose_predicted_candidate`, `pinyin_get_candidate_nbest_index`, `pinyin_remove_user_candidate` on the wrong row type or a non-user token | `candidates.rs` | `abort-choose-candidate-predicted-prefix-row`, `abort-choose-candidate-predicted-bigram-row`, `abort-choose-predicted-candidate-normal-row`, `-sentence-row`, `abort-get-candidate-nbest-index-normal-row`, `abort-remove-user-candidate-normal-system-token`, `-sentence-row` | POLICY-ACCEPTED |
| 66 | `storage/phonetic_key_matrix.h:103` `assert` (through `pinyin.cpp:3085`'s loop, and `_check_offset`'s `get_column_size` at `:2226`) | `pinyin_get_right_pinyin_offset` on an empty matrix; `pinyin_guess_candidates` two or more past the reserved slot | `cursor.rs` `abort_on_empty_matrix`; `sentence.rs` `validated_lookup_offset` (`parsed_len + 1` still answers `true`) | `abort-get-right-pinyin-offset-empty-matrix`, `abort-guess-candidates-past-matrix` | POLICY-ACCEPTED |
| 74 | `storage/phrase_large_table3.h:95` `assert` (`reduce_tokens`, directly or through `get_first_token`, `:106`) | `pinyin_iterator_add_phrase`; `pinyin_lookup_tokens` (`pinyin.cpp:2664`); `pinyin_guess_sentence_with_prefix` and `pinyin_guess_predicted_candidates` (`_compute_prefixes`, `:1415`); `pinyin_get_character_offset` and `pinyin_remember_user_input` (`_pre_compute_tokens`, `:3117`) | `iterators.rs` `plan_import` → `ImportPlan::Abort`; `dict.rs` `pinyin_lookup_tokens` above four; `sentence.rs` and `predict.rs` on `oxpinyin_facade::compute_prefixes`' `EngineError::ReduceTokensCount`; `sentence.rs` on `EngineError::ReduceTokensCount`; `user_data.rs` per character | `combined-library-sixth-add-pinyin`, `combined-library-sixth-add-long-reading-pinyin`, `combined-library-unused-index-pinyin`, `combined-library-sixth-add-key-past-parse-bound-pinyin`, `prefix-guess-five-libraries-pinyin`, `predicted-candidates-five-libraries-pinyin`, `character-offset-five-libraries-pinyin`, `remember-user-input-five-libraries-pinyin`, `-unparsed-pinyin`; controls `prefix-guess-four-libraries-pinyin`, `character-offset-four-libraries-pinyin` | POLICY-ACCEPTED; controls MATCH |
| 77 | `include/memory_chunk.h:543`, `:547` `assert` | `pinyin_save` | `context.rs` `save_context` on `SaveOutcome::ChunkWriteFailed` | `abort-save-chunk-header-length`, `-checksum` (`userdir`) | POLICY-ACCEPTED |
| 79 | `storage/table_info.cpp:119`, `:132`, `:142`, `:156`, `:175` `abort()`; `:276`, `pinyin.cpp:388`, `:457`, `:491`, `include/memory_chunk.h:434`, `:493` `assert` | `pinyin_init`; `pinyin_load_phrase_library`; `pinyin_load_addon_phrase_library` | `context.rs` `table_conf_aborts`; `config.rs` as row 62; `state.rs` `CapiContext::load_addon` on `NotDictionary`, and on `NullChunkName` where `oxpinyin_facade::null_chunk_asserts` | `table-conf-abort-source-format-unknown`, `-database-format-unknown`, `-target-unknown`, `-default-name-unknown`, `-file-type-unknown`, `-919-database-format-line-missing`, `-919-source-format-value-missing`, `-919-database-format-value-missing`, `-919-word-width-overlong` (MATCH); `-addon-index-99`, `-16`, `-negative`, `-919-addon-positive-overflow`, `-default-dictionary`, `-default-dictionary-gbk`, `-load-library-*`, `-load-addon-system-file`, `-load-addon-user-file`, (POLICY-ACCEPTED) | as stated |
| 81 | `pinyin.cpp:3743`, `:3750`, `:3759` `assert` | `pinyin_remove_user_candidate` | `candidates.rs`, on the store's `Ok(false)` only | `abort-remove-user-candidate-twice`, `-missing-phrase-table`, `-multi-pron` | POLICY-ACCEPTED |
| 82 | `storage/phonetic_key_matrix.cpp:661`, `:663`, `:664` `assert` | `pinyin_train` | `candidates.rs` `pinyin_train` on `EngineError::StaleTrainingSpan` | `abort-train-after-reparse-past-matrix-end`, `-empty-start-column`, `-empty-end-column`; control `train-after-reparse-and-choose-control` | POLICY-ACCEPTED; control MATCH |
| 84, 94 | `storage/chewing_large_table2_bdb.cpp:529` `abort()` (kc `:499`, tkrzw `:466`) | `pinyin_mask_out` | `config.rs` on `MaskOutOutcome::OverlongIndexKey` | `abort-mask-out-overlong-index-key`, `-overlong-odd-index-key`, `-empty-index-key`, `-one-byte-index-key` | MATCH |
| 84, 95 | `storage/chewing_large_table2_bdb.cpp:282` `abort()` (kc `:269`, tkrzw `:252`) | `pinyin_guess_candidates` | `sentence.rs` on `EngineError::OverlongUserIndexKey` | `abort-guess-candidates-overlong-index-key`, `-overlong-odd-index-key`, `-overlong-system-index-key`, `-overlong-odd-system-index-key`, `-overlong-incomplete-index-key` | MATCH |
| 86 | `storage/ngram_bdb.cpp:199` `assert` (kc `:173`, tkrzw `:150`) | `pinyin_mask_out`, `pinyin_remove_user_candidate`, `pinyin_begin_get_bigram_phrases` | `config.rs`, `candidates.rs`, `iterators.rs` on `NonTokenUserBigramKey` / `has_non_token_bigram_key` | `abort-mask-out-non-token-bigram-key`, `abort-remove-user-candidate-non-token-bigram-key`, `abort-begin-bigram-phrases-non-token-key` | POLICY-ACCEPTED |
| 87 | `storage/ngram.cpp:70` `assert` via `ngram_bdb.cpp:243` | `pinyin_mask_out`, `pinyin_remove_user_candidate` | `config.rs`, `candidates.rs` on `ResidualUserBigramGram` | `abort-mask-out-residual-bigram-gram`, `abort-remove-user-candidate-residual-bigram-gram` | POLICY-ACCEPTED |
| 88 | `storage/ngram.cpp:70` `assert` via `pinyin.cpp:2332` | `pinyin_guess_predicted_candidates` | `predict.rs` `predicted_bigram_items` (the short-value arm of row 85 stays a refusal) | `abort-guess-predicted-empty-bigram-gram` | POLICY-ACCEPTED |
| 93 | `pinyin.cpp:554`, `:568`, `:571` `assert` | `pinyin_iterator_add_phrase` | `iterators.rs` `plan_import` → `ImportPlan::Abort` | `abort-add-phrase-duplicate-library-token`, `-index-length-mismatch`, `-index-item-missing`, `-index-text-mismatch` (`userdir`) | POLICY-ACCEPTED |

Row 4's predicate (corrected 2026-10-31 UTC, external review). The pin's
assert is inside the phrase-table search: `contains_incomplete_pinyin`
(`storage/pinyin_phrase3.h:146-156`) returns at the first initial-only key
of the searched path and asserts that it carries no tone. It runs at the top
of `ChewingLargeTable2::search` (`chewing_large_table2.cpp:166-167`) for each
key path `search_matrix_recur` hands to the table (`phonetic_key_matrix.cpp:350-373`,
paths of at most `MAX_PHRASE_LENGTH` keys), and of `search_suggestion`
(`chewing_large_table2_bdb.cpp:560`, kc `:534`, tkrzw `:504`) for each path
the LONGER walk hands out (`phonetic_key_matrix.cpp:440-475`). So
`pinyin_guess_candidates` reaches it only through the span loop from the
offset (`pinyin.cpp:2228-2262`, which runs no search when `offset + 1` is
not below the matrix size) or, when `SORT_WITHOUT_LONGER_CANDIDATE` is clear,
through `_prepend_longer_candidates` (`:2292-2293`), whose walk starts at
column 0. The sentence guesses reach it through `get_nbest_match`'s searches
(`lookup/phonetic_lookup.h:763-800`). The earlier facade pre-scanned the whole
matrix before the lookup; the abort now fires where oxpinyin's own search
leaf meets such a path: the n-best `GenerationView::walk` leaf, the explicit
guess's `search_scan_path`, and the explicit LONGER walk (an eager refresh,
which the pin never runs, does not check). `n4` at offset 2 with both
`SORT_WITHOUT_*` bits searches nothing and answers `true` with no
candidates, as the pin does. The zhuyin facade reaches the same engine
searches and aborts on the same error.

Notes:

- Row 61's `pinyin_guess_candidates` decides on the searched list before
  the dedup. `_compute_frequency_of_items` runs on the freshly searched rows
  (`pinyin.cpp:2283`), before the sort, the LONGER and sentence prepends
  and `_remove_duplicated_items_by_phrase_string` (`:2300`), and skips
  addon and predicted-prefix rows (`:1812-1840`). The engine's window
  reports whether that list held such a row
  (`CandidateList::ranked_phrase_row`), so a searched row the dedup removes
  for duplicating a sentence row still aborts, and an addon-only window or
  the engine's raw-input fallback row does not. A lookup strictly inside a
  key searches nothing at the pin and does not abort. The predicted path
  already checks its bigram rows before the prefix rows join them
  (`:2438` against `:2451`). The edge is witnessed by
  `session::tests::a_searched_row_the_dedup_removes_is_still_a_ranked_row`;
  no contract case can build it from the pin's system tables, where every
  key offers many single-character rows.
- Row 44's two `fscanf`-0 shapes (junk after the model line, no
  `database format:` line) leave the pin's `str` unwritten and read it
  anyway. oxpinyin's reader answers no token, which no format matches, so
  `UserTableInfo::load` returns `UserConfError::UnknownDatabaseFormat` for
  all five `open-counter-diff` shapes (`crates/oxpinyin-data/src/user_files.rs`),
  and the pinyin init aborts. The `open-counter-diff` runner
  ([archived runner protocols source](https://github.com/shenghaoc/oxpinyin/blob/6c4467dc826d11a0cbd8517e907b7027ae440fd9/tools/dev/runner_protocols.py)) now expects SIGABRT, no refusal line
  and an unchanged user directory from the pinyin subject; the zhuyin
  expectation is member 4's.
- Row 79 keeps two class (b) shapes: the missing `source table format:`
  line (row 80, `table-conf-abort-source-format-line-missing` and its
  `919` twin), and a chunkless row on a directory that does not read as a
  chunk (the pin writes `mmap <dir> failed!` and dies of SIGSEGV). Those
  keep their warning; `oxpinyin_facade::null_chunk_asserts` tells the two
  directory shapes apart with the facade's own probe.
- Row 63's empty-matrix predicate is `sentence::matrix_has_keys`: false
  exactly when the active parse placed no key, which is when the pin's
  `fill_matrix` leaves the matrix at size 0
  (`storage/phonetic_key_matrix.cpp:34-38`; apostrophes are not keys).
- `pinyin_mask_out` checks the system libraries and the system `.dbin`
  logs before it masks the user tables, while the pin masks the pinyin
  table first (`pinyin.cpp:1228-1230`). Where a profile has both an
  unmappable system library and an over-long user index key, oxpinyin
  writes the `mmap %s failed!` line before it aborts and the pin does not.
  The order belongs to member 5's work on those checks (rows 91, 92, 96,
  97).

## Corrections after the external review (2026-10-31 UTC)

Each of these carried the old refusal's predicate or order into the abort:

- Row 4: see the paragraph under Sites; the abort is decided by the search
  that reaches the pin's assert, not by a pre-scan of the matrix.
- Row 62: `pinyin_iterator_get_next_phrase` writes `*phrase = NULL;
  *pinyin = NULL; *count = -1;` first (`pinyin.cpp:703`), before every assert
  of the read (`:709`, `:714`, `:721`, `:724`, `chewing_key.cpp:48`/`:50`).
  The facade writes the same values to each non-null out-param before it
  aborts. `export_iterator_aborts_on_a_reading_it_cannot_spell` reads them
  back from a page the child shares (`test_support::assert_aborts_sharing`).
- Rows 63 and 66 (pinyin): the key getters test `offset >= matrix.size() - 1`
  first (`pinyin.cpp:2927-2928`, `:2950-2951`); on an empty matrix that is
  `offset >= SIZE_MAX`, so `SIZE_MAX` answers `false` with the out-param
  NULLed and only a smaller offset reaches `get_column_size`'s assert
  (`cursor.rs` `abort_on_empty_matrix_below`). Held by
  `e2e_tests::the_key_getters_answer_false_at_size_max_on_an_empty_matrix`.
  The right-offset call keeps the
  abort at every offset: `_check_offset` or its loop asserts first
  (`:3067-3075`).
- Rows 74 and 93: `plan_import` walks the tokens and decides `:554`, `:568`
  and `:571` for any library index, as the pin does (`pinyin.cpp:544-571`);
  the index gate, the pin's `get_range` in its `else` branch (`:586-588`),
  now comes after. Fixed 2026-11-01 UTC: the import parsed the whole
  reading, and the segment graph (`MAX_GRAPH_INPUT`) refused one longer than
  65535 bytes before the token walk. Since #1501 `parse_import_pinyin`
  parses the whole reading with no graph refusal; the pin's 32767-byte stop
  (`pinyin_parser2.cpp:67-70`) is a language artefact
  ([1501-gint16-input-cap](1501-gint16-input-cap.md)). `pinyin.cpp:614-650`
  does no observable work before `_add_phrase`, so the asserts follow on the
  parsed keys.
  Held by `boundary_tests::an_import_reading_parses_past_long_separator_runs`
  (a 70008-byte reading parses whole to its two keys) and
  `boundary_tests::an_import_aborts_on_five_tokens_before_the_library_refusal`
  (the token walk's abort). In
  `combined-library-sixth-add-key-past-parse-bound-pinyin`, the 40007-byte
  reading `ni3` + 40000 apostrophes + `hao3` parses to one key: the backward
  scan in `final_step` starts at step 40007 and stops at the first step whose
  sign-extended `gint16` `m_parsed_len` equals its index, 32767
  (`pinyin_parser2.cpp:353-357`; `movswl 0x8(%rax),%r13d` at `0x5caac`,
  `cmp %esi,%r13d` at `0x5cab4`). The two-character phrase then fails the
  length gate (`pinyin.cpp:640`) and the add answers `false`, leaking
  `keys`, `key_rests` and `ucs4_phrase` (`:639-652`). The case's first
  read is `tokens_of` on 你好; `pinyin_lookup_tokens` (`pinyin.cpp:2652-2668`)
  calls `reduce_tokens` at `:2664`, whose `assert(0 <= num && num <= 4)`
  (`phrase_large_table3.h:95`) sees the five libraries that already hold the
  phrase. That is the abort, settled 2026-11-01 UTC from the pin-built
  object: `pinyin_lookup_tokens+0x13a` (`0x97aea`) is `cmp $0x4,%r15d; ja`
  to the cold stub `0x2e7ff`, which calls `reduce_tokens`' outlined assert
  `0x2de92` (`__assert_fail@plt`, file `../src/storage/phrase_large_table3.h`
  at `0xa4238`, line `0x5f` = 95, expression `0 <= num && num <= 4` at
  `0xa9be3`). It is a defined assert, not heap corruption: the apostrophe
  copy writes `m_parse_steps` at `data + (i+1)*14` with `i+1 <= len`
  (`0x5cf48`), the DP store uses `data + n*14` with `n <= next_sep`
  (`0x5cd6a`, `0x5cdea`), `final_step` writes `keys`/`key_rests` only at
  `m_num_keys - 1` with the array sized to `m_num_keys` (`0x5caed`,
  `0x5cb0a`), and the refusing import writes nothing; the leak is the only
  imbalance. `pinyin_save`, `pinyin_end_add_phrases`, the instance alloc and
  free and `pinyin_fini` have no other abort. oxpinyin parses the whole
  reading (#1501): two keys, so the add runs and `_add_phrase`'s own
  `reduce_tokens` aborts at `pinyin.cpp:541`, one call before the pin; both
  die of SIGABRT with the same stderr. The zhuyin import is
  unbounded at both, as `ZhuyinDirectParser2::parse` is
  (`zhuyin_parser2.cpp:914-961`); `zhuyin_lookup_tokens` (`zhuyin.cpp:1690`)
  carries the same assert and is not changed here.
- Row 74's other `reduce_tokens` callers (2026-11-01 UTC). Every validating
  call asserts `0 <= num && num <= 4` (`phrase_large_table3.h:95`) after it
  has cleared and filled its array, and oxpinyin completed each one
  silently:
  - `_compute_prefixes` (`pinyin.cpp:1415`) reduces each tail substring of
    the prefix, for `pinyin_guess_sentence_with_prefix` (`:1434`) and
    `pinyin_guess_predicted_candidates` (`:2424`), before anything uses a
    token. `oxpinyin_facade::compute_prefixes` now makes the pin's one pass:
    search a suffix, check its count, append, continue. It returns
    `EngineError::ReduceTokensCount` at the first search with more than four
    tokens, and both entry points abort on it.
  - `_pre_compute_tokens` takes each character's first token through
    `get_first_token` (`:3117`, `phrase_large_table3.h:106`), in order, and
    the assert comes before the empty search's `false`. The engine's
    character-offset walk returns `EngineError::ReduceTokensCount` there,
    which `pinyin_get_character_offset` aborts on. `pinyin_remember_user_input`
    runs the same loop first, before it reads the composition keys, as the
    pin reads no matrix before it (`:3685-3700`).
  - `pinyin_lookup_tokens` now aborts after the clear and the appends, so a
    NULL array's GLib criticals come first, as at the pin.
  - The non-validating calls (`_prepend_longer_candidates`, `:1885`, and
    `_compute_predicted_prefix_candidates`, `:2386`) have no assert, and
    oxpinyin has no bound there.
- Row 82 (2026-11-01 UTC): `train_result3` walks the result positions
  below `constraints->length()` (`phonetic_lookup.h:858`). The pin-built
  `pinyin_train` reloads the `GArray`'s `len` at each step and leaves the
  loop at it. A NORMAL choose validates the store to the current matrix
  first (`pinyin.cpp:2579`, `phonetic_lookup.cpp:120-160`), clearing a
  forcing whose end reaches the new length. The engine's choose only resizes
  the store, so after a re-parse to a shorter input such a forcing survives,
  and `train_next` carries from it to the retained row's next phrase, which
  starts at that forcing's end, past the store. The pin never reaches that
  phrase, but `Session::plan_spans` walked every span and aborted on it at
  `:663`. The walk now stops at the store's length. A span right after a
  forcing starts at the forcing's end, so with the bound the surviving
  forcing changes nothing the walk does. Held by `oxpinyin-engine`'s
  `session::tests::train_after_a_reparse_and_a_choose_stops_the_walk_at_the_store_length`:
  `nihaoma` with 好 forced at 2..5, re-parsed to `niha`, 你 chosen at 0, then trained.
- Row 79: the default rows are checked in index order, and the libraries the
  pin's loop maps before the fatal row write their `mmap %s failed!` lines
  first (`pinyin.cpp:253-256`, then `:388`; through the row itself for a row
  without a user file, whose chunk is mapped before its log `load` asserts,
  `:268-271`). `Runtime::open_with_law_reporting` reports them through the
  facade's `c_stderr::mmap_failed`. At the pin an absent library dies of
  SIGSEGV on its empty chunk (row 76, class (b)) before a later row is
  reached, so the order shows only after that class (b) outcome. Held by
  `a_default_dictionary_row_reports_only_the_libraries_before_it`.
- Row 81: `UserStore::remove_user_phrase`
  decides the removal asserts (`pinyin.cpp:3743`, `:3750`, `:3759`) before the
  bigram fault of the `mask_out` that follows (`:3766`), with nothing changed
  before either (`phrase_table::index_removable_ucs4` reads `:3750`'s test
  without writing). Held by `a_removal_assert_comes_before_the_bigram_fault`.

## Fuzzing

`fuzz/fuzz_targets/capi_commands.rs` keeps every call off these
preconditions. It skips double-pinyin scheme 30 and a predicted row passed
to `pinyin_choose_candidate` (argument conditions), and asks the library
before `pinyin_train` (`oxpinyin_train_aborts`: the engine's
`Session::train_nbest_preflight`, the row lookup and span walk
that `Session::prepare_training` also runs, through `InstanceCore::train_aborts`) and before
`pinyin_iterator_add_phrase` (`oxpinyin_import_aborts`: `plan_import`, the
checks the add runs). Both are Rust functions compiled only with
`fuzz-api` and never into the shipped object. The other converted sites
are not reachable from the harness's alphabet.

## Verification

The commands and results are in the change's report.
