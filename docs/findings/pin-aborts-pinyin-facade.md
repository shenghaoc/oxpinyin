# Pin aborts reproduced in the pinyin C facade

Status: implemented, 2026-10-31 UTC (member 3 of the stack that retires
class (c)). Rule: [compatibility policy, class (c)
retired](compatibility-policy.md#amendment--class-c-retired-pin-aborts-reproduced-2026-10-31-utc).
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
| 4 | `storage/pinyin_phrase3.h:152` `assert`; `pinyin.cpp:2769` `assert` | `pinyin_guess_candidates`, `pinyin_guess_sentence`, `pinyin_guess_sentence_with_prefix`; `pinyin_get_pinyin_is_incomplete` | `sentence.rs` `abort_on_toned_matrix`, `abort_on_toned_initial`; `keys.rs` `pinyin_get_pinyin_is_incomplete` | `abort-guess-candidates-toned-initial`, `abort-guess-candidates-toned-initial-before-offset`, `abort-guess-sentence-toned-initial`, `abort-guess-sentence-toned-initial-after-key`, `abort-is-incomplete-toned-initial` | POLICY-ACCEPTED |
| 5a | `storage/pinyin_parser2.cpp:611` `abort()` | `pinyin_set_double_pinyin_scheme(30)` | `config.rs` `pinyin_set_double_pinyin_scheme` | `abort-set-double-pinyin-scheme-30` | MATCH |
| 5c | `storage/zhuyin_parser2.cpp:295` `abort()` | `pinyin_set_zhuyin_scheme(7)` | `config.rs` `pinyin_set_zhuyin_scheme` | `abort-set-zhuyin-scheme-7` | MATCH |
| 5d | `pinyin.cpp:1189` `abort()`; `storage/pinyin_parser2.cpp:398` `abort()` | `pinyin_set_zhuyin_scheme` out of the enum; `pinyin_set_full_pinyin_scheme` outside 1–3 | `config.rs` `pinyin_set_zhuyin_scheme`, `pinyin_set_full_pinyin_scheme` | `abort-set-zhuyin-scheme-0`, `-10`; `abort-set-full-pinyin-scheme-0`, `-4` | MATCH |
| 6 | `lookup/phonetic_lookup.h:868` `assert` | `pinyin_train` | `candidates.rs` `pinyin_train` on `EngineError::StaleTrainingConstraint` | `abort-train-stale-forcing` | POLICY-ACCEPTED |
| 10 | `pinyin.cpp:1474` `assert` | `pinyin_get_sentence` past the rows | `sentence.rs` `pinyin_get_sentence` | `abort-get-sentence-past-rows` | POLICY-ACCEPTED |
| 14 | `pinyin.cpp:3035`, `:3057`, `:3067`, `:3092` `assert`; `storage/phonetic_key_matrix.h:103` `assert` past the matrix | `pinyin_get_left_pinyin_offset`, `pinyin_get_right_pinyin_offset` | `cursor.rs`, on `ZeroKeyOffsetCheck` and `LookupOffsetOutOfRange` only (the former catch-all is narrowed; a graph failure keeps `false` and a warning) | `abort-get-left-pinyin-offset-after-separator`, `abort-get-right-pinyin-offset-after-separator`, `abort-get-right-pinyin-offset-nihao-5`, `separator/pinyin/*/left/*`, `separator/pinyin/*/right/*` | POLICY-ACCEPTED |
| 19 | `pinyin.cpp:3147`, `:3161`, `:3203`, `:3204` `assert` | `pinyin_get_character_offset` | `sentence.rs` `pinyin_get_character_offset`, on `MatrixColumnAssert`, `LookupOffsetOutOfRange` and `ZeroKeyOffsetCheck` only | `abort-get-character-offset-past-matrix`, `abort-get-character-offset-after-separator`, `separator/pinyin/*/character/*`, `c-string-input/pinyin-character-offset-null-past-matrix`, `-invalid-past-matrix` | POLICY-ACCEPTED |
| 21 | `storage/pinyin_parser2.cpp:170` `assert`; `pinyin.cpp:466`, `:499` `assert` | `pinyin_parse_full_pinyin` with `'`; `pinyin_unload_phrase_library` and `pinyin_unload_addon_phrase_library` with an index of 16 or more | `keys.rs` `pinyin_parse_full_pinyin`; `config.rs` `pinyin_unload_phrase_library`; `state.rs` `CapiContext::unload_addon` | `abort-parse-full-pinyin-apostrophe`, `c-string-input/pinyin-full-key-apostrophe-valid`, `-invalid`, `abort-unload-phrase-library-16`, `abort-unload-addon-phrase-library-16` | POLICY-ACCEPTED |
| 38 | `pinyin.cpp:2684` `assert` | `pinyin_train` past nonempty results | `candidates.rs` `pinyin_train` on `CandidateIndexOutOfRange` | `abort-train-index-past-results` (`userdir`); `train_index_diff.py` cases `len`, `255` | POLICY-ACCEPTED |
| 44 | `storage/table_info.cpp:132` `abort()`, reached from `:353-354` | `pinyin_init` | `context.rs` `init_context` on `OpenFailure::unknown_database_format` | `user-conf-abort-database-format-unknown` (`userdir`); `open-counter-diff` `abort-*` (pinyin) | MATCH |
| 45 | `pinyin.cpp:2566` `assert` | `pinyin_choose_candidate` with a nonzero offset under `SORT_WITHOUT_SENTENCE_CANDIDATE` | `candidates.rs` `pinyin_choose_candidate` | `abort-choose-candidate-without-sentence-offset-1` | POLICY-ACCEPTED |
| 61 | `pinyin.cpp:1859` `assert` | `pinyin_guess_candidates`; `pinyin_guess_predicted_candidates` and `_with_punctuations` (a predicted bigram row takes the same scorer branch, `:2432` into `:1855-1859`) | `sentence.rs` `pinyin_guess_candidates` on `CandidateList::ranked_phrase_row`; `predict.rs` `guess_predicted_result` | `facade-total-zero`, `nonzero-total-candidates`, `row58-total-zero-off`, `-on`, `-punct-off`, `-punct-on` | POLICY-ACCEPTED |
| 62 | `pinyin.cpp:457`, `:709`, `:721`, `:902` `assert`; `:714`, `:724` `check_result`; `storage/chewing_key.cpp:48`, `:50` `assert` | `pinyin_load_phrase_library`; `pinyin_iterator_get_next_phrase`; `pinyin_bigram_iterator_get_next_phrase` | `config.rs` `pinyin_load_phrase_library` on `LibraryRowAssert::NotLoadable` (the catch-all is narrowed); `iterators.rs` on `ExportStep::Assert` and `BigramStep::Aborts` | `abort-load-phrase-library-0`, `-8`, `abort-iterator-get-next-phrase`, `abort-bigram-iterator-get-next-phrase`, `table-conf-abort-load-library-*`; the unspellable reading by `e2e_tests::export_iterator_aborts_on_a_reading_it_cannot_spell` | POLICY-ACCEPTED |
| 63 | `storage/phonetic_key_matrix.h:103` `assert`; `pinyin.cpp:3311` `assert`; `pinyin.cpp:3488` `abort()` | `pinyin_get_pinyin_key`, `pinyin_get_pinyin_key_rest` on an empty matrix; the three auxiliary-text getters | `cursor.rs` `abort_on_empty_matrix`; `text.rs` `render_shared`, `abort_on_full_walk` | `abort-get-pinyin-key-empty-matrix`, `abort-get-pinyin-key-rest-empty-matrix`, the four `abort-*-auxiliary-text-*leading-apostrophe` (POLICY-ACCEPTED); `abort-double-auxiliary-text-cut-three-bytes-in` (MATCH) | as stated |
| 64 | `pinyin.cpp:2507`, `:2593`, `:2883`, `:3734`, `:3738` `assert` | `pinyin_choose_candidate`, `pinyin_choose_predicted_candidate`, `pinyin_get_candidate_nbest_index`, `pinyin_remove_user_candidate` on the wrong row type or a non-user token | `candidates.rs` | `abort-choose-candidate-predicted-prefix-row`, `abort-choose-candidate-predicted-bigram-row`, `abort-choose-predicted-candidate-normal-row`, `-sentence-row`, `abort-get-candidate-nbest-index-normal-row`, `abort-remove-user-candidate-normal-system-token`, `-sentence-row` | POLICY-ACCEPTED |
| 66 | `storage/phonetic_key_matrix.h:103` `assert` (through `pinyin.cpp:3085`'s loop, and `_check_offset`'s `get_column_size` at `:2226`) | `pinyin_get_right_pinyin_offset` on an empty matrix; `pinyin_guess_candidates` two or more past the reserved slot | `cursor.rs` `abort_on_empty_matrix`; `sentence.rs` `validated_lookup_offset` (`parsed_len + 1` still answers `true`) | `abort-get-right-pinyin-offset-empty-matrix`, `abort-guess-candidates-past-matrix` | POLICY-ACCEPTED |
| 74 | `storage/phrase_large_table3.h:95` `assert` | `pinyin_iterator_add_phrase` | `iterators.rs` `plan_import` → `ImportPlan::Abort` | `combined-library-sixth-add-pinyin`, `combined-library-unused-index-pinyin` | POLICY-ACCEPTED |
| 77 | `include/memory_chunk.h:543`, `:547` `assert` | `pinyin_save` | `context.rs` `save_context` on `SaveOutcome::ChunkWriteFailed` | `abort-save-chunk-header-length`, `-checksum` (`userdir`) | POLICY-ACCEPTED |
| 79 | `storage/table_info.cpp:119`, `:132`, `:142`, `:156`, `:175` `abort()`; `:276`, `pinyin.cpp:388`, `:457`, `:491`, `include/memory_chunk.h:434`, `:493` `assert` | `pinyin_init`; `pinyin_load_phrase_library`; `pinyin_load_addon_phrase_library` | `context.rs` `table_conf_aborts`; `config.rs` as row 62; `state.rs` `CapiContext::load_addon` on `NotDictionary`, and on `NullChunkName` where `oxpinyin_facade::null_chunk_asserts` | `table-conf-abort-source-format-unknown`, `-database-format-unknown`, `-target-unknown`, `-default-name-unknown`, `-file-type-unknown`, `-919-database-format-line-missing`, `-919-source-format-value-missing`, `-919-database-format-value-missing`, `-919-word-width-overlong` (MATCH); `-addon-index-99`, `-16`, `-negative`, `-919-addon-positive-overflow`, `-default-dictionary`, `-default-dictionary-gbk`, `-system-file-without-chunk`, `-system-file-without-user-file`, `-user-file-without-user-file`, `-load-library-*`, `-load-addon-system-file`, `-load-addon-user-file`, `-load-addon-without-chunk` (POLICY-ACCEPTED) | as stated |
| 81 | `pinyin.cpp:3743`, `:3750`, `:3759` `assert` | `pinyin_remove_user_candidate` | `candidates.rs`, on the store's `Ok(false)` only | `abort-remove-user-candidate-twice`, `-missing-phrase-table`, `-multi-pron` | POLICY-ACCEPTED |
| 82 | `storage/phonetic_key_matrix.cpp:661`, `:663`, `:664` `assert` | `pinyin_train` | `candidates.rs` `pinyin_train` on `EngineError::StaleTrainingSpan` | `abort-train-after-reparse-past-matrix-end`, `-empty-start-column`, `-empty-end-column` | POLICY-ACCEPTED |
| 84, 94 | `storage/chewing_large_table2_bdb.cpp:529` `abort()` (kc `:499`, tkrzw `:466`) | `pinyin_mask_out` | `config.rs` on `MaskOutOutcome::OverlongIndexKey` | `abort-mask-out-overlong-index-key`, `-overlong-odd-index-key`, `-empty-index-key`, `-one-byte-index-key` | MATCH |
| 84, 95 | `storage/chewing_large_table2_bdb.cpp:282` `abort()` (kc `:269`, tkrzw `:252`) | `pinyin_guess_candidates` | `sentence.rs` on `EngineError::OverlongUserIndexKey` | `abort-guess-candidates-overlong-index-key`, `-overlong-odd-index-key`, `-overlong-system-index-key`, `-overlong-odd-system-index-key`, `-overlong-incomplete-index-key` | MATCH |
| 86 | `storage/ngram_bdb.cpp:199` `assert` (kc `:173`, tkrzw `:150`) | `pinyin_mask_out`, `pinyin_remove_user_candidate`, `pinyin_begin_get_bigram_phrases` | `config.rs`, `candidates.rs`, `iterators.rs` on `NonTokenUserBigramKey` / `has_non_token_bigram_key` | `abort-mask-out-non-token-bigram-key`, `abort-remove-user-candidate-non-token-bigram-key`, `abort-begin-bigram-phrases-non-token-key` | POLICY-ACCEPTED |
| 87 | `storage/ngram.cpp:70` `assert` via `ngram_bdb.cpp:243` | `pinyin_mask_out`, `pinyin_remove_user_candidate` | `config.rs`, `candidates.rs` on `ResidualUserBigramGram` | `abort-mask-out-residual-bigram-gram`, `abort-remove-user-candidate-residual-bigram-gram` | POLICY-ACCEPTED |
| 88 | `storage/ngram.cpp:70` `assert` via `pinyin.cpp:2332` | `pinyin_guess_predicted_candidates` | `predict.rs` `predicted_bigram_items` (the short-value arm of row 85 stays a refusal) | `abort-guess-predicted-empty-bigram-gram` | POLICY-ACCEPTED |
| 93 | `pinyin.cpp:554`, `:568`, `:571` `assert` | `pinyin_iterator_add_phrase` | `iterators.rs` `plan_import` → `ImportPlan::Abort` | `abort-add-phrase-duplicate-library-token`, `-index-length-mismatch`, `-index-item-missing`, `-index-text-mismatch` (`userdir`) | POLICY-ACCEPTED |

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
  (`tools/dev/runner_protocols.py`) now expects SIGABRT, no refusal line
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

## Fuzzing

`fuzz/fuzz_targets/capi_commands.rs` keeps every call off these
preconditions. It skips double-pinyin scheme 30 and a predicted row passed
to `pinyin_choose_candidate` (argument conditions), and asks the library
before `pinyin_train` (`oxpinyin_train_aborts`: the engine's
`Session::train_nbest_preflight`, the row lookup and span walk
`train_nbest` itself runs, through `InstanceCore::train_aborts`) and before
`pinyin_iterator_add_phrase` (`oxpinyin_import_aborts`: `plan_import`, the
checks the add runs). Both are Rust functions compiled only with
`fuzz-api` and never into the shipped object. The other converted sites
are not reachable from the harness's alphabet.

## Verification

The commands and results are in the change's report.
