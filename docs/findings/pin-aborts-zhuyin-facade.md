# Pin aborts reproduced in the zhuyin C facade

Status: implemented, 2026-10-31 UTC (member 4 of the stack that retires
class (c)). Rule: [compatibility policy, class (c)
retired](compatibility-policy.md#amendment--class-c-retired-pin-aborts-reproduced-2026-10-31-utc).
Pin: libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`, read from a
`git archive` of the pin; `check_result` is `assert` in the reference build
(`src/include/pinyin_utils.h:27-31`). The pinyin twin is
[pin-aborts-pinyin-facade.md](pin-aborts-pinyin-facade.md).

The zhuyin C facade (`crates/oxpinyin-zhuyin-capi`) used to answer `false`
or NULL with one `libzhuyin` warning at each site below. It now calls
`std::process::abort()` where the pin asserts or aborts. The shared Rust
crates still return the typed error at the pin's point, and the C entry
point turns that error into the abort. Every refusal text these sites
logged is deleted, and so is the facade's now unused
`UNKNOWN_DATABASE_FORMAT_WARNING`. The rows member 5 owns (85, 91, 92, 96,
97, 103, 108) and the class (b) sites (the unsafe predecessor, the
unreadable candidate item, the missing source-format line and the
chunkless row on a directory that does not read as a chunk) keep their
refusals.

**Disposition.** An `abort()` site writes nothing on either side: the
case is MATCH. An `assert` site writes glibc's assertion line at the pin
only: the case is POLICY-ACCEPTED for that line
([pin-abort-assert-line.md](pin-abort-assert-line.md)) and MATCH on the
SIGABRT status, the earlier stderr and, with `userdir`, the files. The
cases are registered `sigabrt=True` in `tools/bisection/contract_*.py`.

**Preceding work.** For every site the pin's path from the zhuyin entry
point to the abort was read. Before the abort the pin changes only
in-memory state (its user tables are in-memory DBMs written at save),
except at three rows. At row 77 `zhuyin_save` has written each earlier
library's `.tmp` and renamed it into place (`zhuyin.cpp:610-650`), and has
created the failing library's `.tmp`; oxpinyin's save renames each file as
soon as it is written on this facade (`persistence.rs` `Staged`, immediate
mode) and stops at the failing header with no rename, no user index or
bigram file and no `user.conf`. At row 79's `zhuyin.cpp:330` and
`memory_chunk.h:434`/`:493` sites `check_format` has unlinked a
non-conforming profile's files and, unlike libpinyin's, written no
`user.conf` (`zhuyin.cpp:126-160`); the open's `check_format` under
`UserConfLaw::Zhuyin` does the same before the default-row check. At row
44 the pin aborts in `check_format`'s first step, before it unlinks
anything, and so does the open. The abort skips every destructor, so
nothing else is written.

## Sites

| Row | Pin site(s) | Zhuyin entry points | oxpinyin item that aborts | Cases | Disposition |
|---|---|---|---|---|---|
| 5c | `storage/zhuyin_parser2.cpp:295` `abort()` | `zhuyin_set_chewing_scheme(7)` | `config.rs` `zhuyin_set_chewing_scheme` | `abort-zhuyin-chewing-scheme-7` | MATCH |
| 5d | `zhuyin.cpp:736` `abort()`; `storage/pinyin_parser2.cpp:398` `abort()` via `zhuyin.cpp:704` | `zhuyin_set_chewing_scheme` out of the enum; `zhuyin_set_full_pinyin_scheme` outside 1–3 | `config.rs` `zhuyin_set_chewing_scheme`, `zhuyin_set_full_pinyin_scheme` | `abort-zhuyin-chewing-scheme-0`, `-10`, `-minus-1`; `abort-zhuyin-full-pinyin-scheme-0`, `-4` | MATCH |
| 6 | `lookup/phonetic_lookup.h:868` `assert` via `zhuyin.cpp:1712` | `zhuyin_train` | `candidates.rs` `zhuyin_train` on `EngineError::StaleTrainingConstraint` | `abort-zhuyin-train-stale-forcing` | POLICY-ACCEPTED |
| 19 | `zhuyin.cpp:2110`, `:2158` `assert`; `:1453` via `:2159` | `zhuyin_get_character_offset` | `sentence.rs` `zhuyin_get_character_offset`, on `MatrixColumnAssert`, `LookupOffsetOutOfRange` and `ZeroKeyOffsetCheck` only (the former catch-all is narrowed; a graph failure keeps `false` and a warning) | `abort-zhuyin-get-character-offset-past-matrix`, `c-string-input/zhuyin-character-offset-null-past-matrix`, `-invalid-past-matrix`, `separator/zhuyin/*/character/*` | POLICY-ACCEPTED |
| 21 | `storage/pinyin_parser2.cpp:170` `assert` via `zhuyin.cpp:1012` | `zhuyin_parse_full_pinyin` with `'` | `keys.rs` `zhuyin_parse_full_pinyin` | `abort-zhuyin-parse-full-pinyin-apostrophe`, `c-string-input/zhuyin-full-key-apostrophe-valid`, `-invalid` | POLICY-ACCEPTED |
| 44 | `storage/table_info.cpp:132` `abort()`, reached from `:353-354` through `check_format` (`zhuyin.cpp:126-132`) | `zhuyin_init` | `context.rs` `zhuyin_init` on `OpenFailure::unknown_database_format` | `user-conf-abort-database-format-unknown-zhuyin` (`userdir`); `open-counter-diff` `abort-*` (zhuyin) | MATCH |
| 61 | `zhuyin.cpp:1261` `assert` | `zhuyin_guess_candidates_after_cursor`, `_before_cursor` | `sentence.rs` `guess_candidates` on `facade_total_is_zero` and `CandidateList::ranked_phrase_row` | `abort-zhuyin-guess-candidates-total-zero`, `nonzero-total-candidates-zhuyin` | POLICY-ACCEPTED |
| 66 | `zhuyin.cpp:372`, `:381`, `:1453` `assert`; `storage/phonetic_key_matrix.h:103` `assert` (empty matrix and past the matrix); `:1261` as row 61; the 5c/5d aborts | `zhuyin_load_phrase_library`, `zhuyin_unload_phrase_library`, `zhuyin_get_zhuyin_key`, `_key_rest`, `zhuyin_get_left_zhuyin_offset`, `_right_`, `zhuyin_guess_candidates_after_cursor`, `_before_cursor` | `config.rs` (`NotLoadable`; an unload index of 16 or more); `cursor.rs` `abort_on_empty_matrix`, `key_lookup_failed` and the narrowed offset arms; `sentence.rs` `validated_lookup_offset` | `abort-zhuyin-load-phrase-library-0`, `-8`, `abort-zhuyin-unload-phrase-library-16`, `-255`, `abort-zhuyin-get-zhuyin-key-empty-matrix`, `-key-rest-empty-matrix`, `abort-zhuyin-get-right-zhuyin-offset-empty-matrix`, `abort-zhuyin-get-left-zhuyin-offset-past-matrix`, `abort-zhuyin-get-right-zhuyin-offset-past-matrix`, `abort-zhuyin-guess-candidates-after-cursor-past-matrix`, `-before-cursor-past-matrix`, `-after-cursor-reserved-slot-plus-one`, `-before-cursor-reserved-slot-plus-one`, `separator/zhuyin/*/{left,right,key,rest,candidates,before}/*`, `zero-key/zhuyin/*`, `allocator-zero/zhuyin/xi1'an1/key/4` | POLICY-ACCEPTED |
| 74 | `storage/phrase_large_table3.h:95` `assert` | `zhuyin_iterator_add_phrase` | `iterators.rs` `plan_import` → `ImportPlan::Abort` | `combined-library-sixth-add-zhuyin`, `combined-library-unused-index-zhuyin` | POLICY-ACCEPTED |
| 77 | `include/memory_chunk.h:543`, `:547` `assert` | `zhuyin_save` | `context.rs` `zhuyin_save` on `SaveOutcome::ChunkWriteFailed` | `abort-zhuyin-save-chunk-header-length`, `-checksum` (`userdir`) | POLICY-ACCEPTED |
| 79 | `storage/table_info.cpp:119`, `:132`, `:142`, `:156`, `:175` `abort()`; `:276`, `zhuyin.cpp:330`, `:372`, `include/memory_chunk.h:434`, `:493` `assert` | `zhuyin_init`; `zhuyin_load_phrase_library` | `context.rs` `table_conf_aborts`; `config.rs` as row 66 | `table-conf-abort-source-format-unknown-zhuyin`, `-database-format-unknown-zhuyin`, `-target-unknown-zhuyin`, `-default-name-unknown-zhuyin`, `-file-type-unknown-zhuyin`, the four `-919-*-zhuyin` (MATCH); `-addon-index-99-zhuyin`, `-16-zhuyin`, `-negative-zhuyin`, `-default-dictionary-zhuyin`, `-default-dictionary-gbk-zhuyin`, `-system-file-without-chunk-zhuyin`, `-load-library-*-zhuyin` (POLICY-ACCEPTED) | as stated |
| 82 | `storage/phonetic_key_matrix.cpp:661`, `:663`, `:664` `assert` via `zhuyin.cpp:1712` | `zhuyin_train` | `candidates.rs` `zhuyin_train` on `EngineError::StaleTrainingSpan` | `abort-zhuyin-train-after-reparse-past-matrix-end`, `-empty-start-column`, `-empty-end-column` (new); control `zhuyin-train-after-reparse-internal-zero-key` | POLICY-ACCEPTED |
| 84, 94 | `storage/chewing_large_table2_bdb.cpp:529` `abort()` (kc `:499`, tkrzw `:466`) via `zhuyin.cpp:763` | `zhuyin_mask_out` | `config.rs` on `MaskOutOutcome::OverlongIndexKey` | `abort-zhuyin-mask-out-overlong-index-key`, `-empty-index-key` (`userdir`) | MATCH |
| 86 | `storage/ngram_bdb.cpp:199` `assert` (kc `:173`, tkrzw `:150`) via `zhuyin.cpp:765` | `zhuyin_mask_out` | `config.rs` on `MaskOutOutcome::NonTokenUserBigramKey` | `abort-zhuyin-mask-out-non-token-bigram-key` | POLICY-ACCEPTED |
| 87 | `storage/ngram.cpp:70` `assert` via `ngram_bdb.cpp:243` | `zhuyin_mask_out` | `config.rs` on `MaskOutOutcome::ResidualUserBigramGram` | `abort-zhuyin-mask-out-residual-bigram-gram` | POLICY-ACCEPTED |
| 89 | `zhuyin.cpp:440` `assert` | `zhuyin_iterator_add_phrase` | `iterators.rs` `plan_import` → `ImportPlan::Abort` | `abort-zhuyin-add-phrase-duplicate-library-token` | POLICY-ACCEPTED |
| 90 | `zhuyin.cpp:457` `assert` | `zhuyin_iterator_add_phrase` | as row 89 | `abort-zhuyin-add-phrase-index-text-mismatch` | POLICY-ACCEPTED |
| 104 | `zhuyin.cpp:454` `assert` | `zhuyin_iterator_add_phrase` | as row 89 | `abort-zhuyin-add-phrase-index-length-mismatch`, `-index-item-missing` (new), `reload-user-zhuyin-reuse-same` (the in-session reload trigger) | POLICY-ACCEPTED |

Notes:

- Row 84's longer-candidate shape (`chewing_large_table2_bdb.cpp:282`) has
  no zhuyin arm. libzhuyin has no longer-candidate walk: its guesses search
  the user chewing table only at the matrix's own key lengths
  (`zhuyin.cpp:1502-1503`, `:1587-1588`), so an over-long key is never read
  and the pin answers with its ordinary candidates. oxpinyin's zhuyin
  facade never clears `SORT_WITHOUT_LONGER_CANDIDATE` (the session keeps
  its `0x1e` sort word), so the engine never runs the walk and
  `EngineError::OverlongUserIndexKey` cannot reach the zhuyin guesses. The
  new control `zhuyin-guess-candidates-overlong-index-key` holds the two
  answers equal.
- Row 66's empty-matrix predicate is `cursor::matrix_has_keys`: false
  exactly when the active parse placed no key, which is when the pin's
  `fill_matrix` leaves the matrix at size 0
  (`storage/phonetic_key_matrix.cpp:37`). The key accessors and the right
  move abort on it for every offset; the left move only for an offset
  above 0, because `_check_offset(0)` reads no column and the walk answers
  0.
- `zhuyin_load_phrase_library` keeps the pin's plain `false` for an index
  of 16 or more (`zhuyin.cpp:362-363`).
- Row 61 is decided on the searched list before the dedup. libzhuyin's
  `_compute_frequency_of_items` skips no row type and runs on the freshly
  searched rows (`zhuyin.cpp:1526`, and per `len` slice before the cursor,
  `:1607`), before the sentence prepend and
  `_remove_duplicated_items_by_phrase_string` (`:1537`, `:1630`). The
  engine's window reports whether that list held a row
  (`CandidateList::ranked_phrase_row`), so a searched row the dedup removes
  for duplicating the sentence row still aborts. An after-cursor lookup
  strictly inside a key, and a before-cursor lookup at an offset no key ends
  at (the composition start, inside a key), search nothing at the pin and
  do not abort. The edge is witnessed by the
  engine test
  `session::tests::a_searched_row_the_dedup_removes_is_still_a_ranked_row`
  (after and before the cursor); no contract case can build it from the
  pin's system tables, where every key offers many single-character rows.
- The zhuyin init's table checks run after `check_format`, in the pin's
  order, but before the earlier libraries are mapped, so an earlier row's
  `mmap %s failed!` line that the pin writes before a later row's assert
  is not written. The pinyin record has the same order.

## Fuzzing

No fuzz target drives the zhuyin C API: `fuzz/fuzz_targets/chewing_parser.rs`
drives `oxpinyin_core::ZhuyinParser` only, and `capi_commands.rs` drives the
pinyin facade. No harness change is needed.

## Verification

The commands and results are in the change's report.
