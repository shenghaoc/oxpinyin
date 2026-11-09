# Findings — probe coverage over the full exported pinyin ABI

Date: 2026-09-17 · Status: **current reference** (coverage matrix; the
residues are closed). The whole-surface probe ran 2026-09-16/17 and
2026-09-19/20. Residues A, B, C, E and F are compatibility-policy rows
35, 33, 34, 37 and 36, all **CLOSED**; residue D is no ABI divergence.
The sections below are the dated measurements behind those rows.

This ledger holds the coverage matrix and dated runs required by the
[E2E I/O compatibility rule](compatibility-policy.md#the-e2e-io-compatibility-rule).

The probe's C driver and runner are retired. Their runs are recorded
evidence.

## Name correction

The scope is every symbol in `crates/oxpinyin-capi/libpinyin.ver` — 79
`pinyin_*` exports, the same set as the pin's `src/libpinyin.ver`
(pin `074a2219`, blob `964d96e0`; verified identical 2026-09-17). The
per-symbol signatures and the export lists are
`docs/findings/abi-reference.md`. The export is
`pinyin_get_pinyin_offset`, not `pinyin_get_offset`.

## Classification rules

- **COVERED** requires a differential that asserts the symbol's whole
  observable surface (return status, out-params and the data they point
  to, written lengths, handle state) **and ran IDENTICAL at the pin** (the
  `debian:testing` measurements of 2026-09-16/17 named below). Rust tests
  are supporting evidence, never coverage on their own.
- **Void destructors** (`pinyin_fini`, `pinyin_free_instance`) are
  COVERED when both of these hold:
  - the leak gates are clean (valgrind, and the allocator-pairing gate
    `just abi alloc-pairing`,
    [`abi-allocator-pairing.md`](abi-allocator-pairing.md));
  - every runner completes without a crash.

  Both hold for the runs recorded here (maintainer ruling 2026-09-17).
- A call made only as setup counts as **UNCOVERED**.
- A symbol covered only by the bisect driver (`tools/bisection/bisect.c`)
  stays **PARTIAL**.
- A runner that skips or fails provides no coverage.

## The probe

The probe resolved all 79 exported symbols. It refused to run when any
symbol was missing, so a completed run also proves the export surface. It
walks every symbol's observable surface in deterministic phases:

1. setup;
2. import;
3. full parse ×2;
4. choose/nbest/train/remember;
5. prediction;
6. double (incl. the row-5b out-of-enum probe);
7. chewing;
8. export → mask_out → export;
9. the remaining exported surfaces;
10. teardown.

It prints one `label=value` row per observation: return status, out-param
values AND the data they point to (texts, packed keys, symbol strvs,
begin/end/length), written counts, handle state, and every iterator
predicate result. It queries row 0 of `pinyin_get_sentence` only after a
successful `pinyin_guess_sentence`, and higher indices only through an
NBEST row's own `nbest_index` value.

The **sort word is the driver's third argument** (`ABI_PROBE_SORT` in the
runner, default 0x1e, printed into every log):

- 0x1e is the parity word that every other differential passes;
- 0x1c and 0x14 are ibus-libpinyin's presets (0x1c is the GSettings
  default);
- 0x1f is the third preset;
- 0x16 is fcitx-libpinyin's phrase-length word;
- 0 is the raw word that fcitx5-oxpinyin passes.

The runner drove the binary into oxpinyin-capi and the pin and compared
the logs (0 = identical, 1 = build/run failure, 2 = divergence).

The probe honours these oracle caller contracts. The pin asserts if a
caller breaks them, and the probe documents each assert in its source:

- `pinyin_get_sentence` only for proved indices (row 0 after a
  successful guess; an NBEST row's own `nbest_index` value);
- predicted candidates never reach `pinyin_choose_candidate`; only a
  PREDICTED_PREFIX (5) row goes to `pinyin_choose_predicted_candidate`;
- double scheme 30 and zhuyin keyboard 7 are never set;
- `pinyin_guess_sentence` before `pinyin_train`;
- `pinyin_unload_addon_phrase_library` asserts
  `index < PHRASE_INDEX_LIBRARY_COUNT` (`pinyin.cpp:499`) —
  out-of-range is never sent (only `load`'s out-of-range false path is
  probed);
- `pinyin_get_candidate_nbest_index` asserts NBEST_MATCH
  (`pinyin.cpp:2883`) — asked only of type-1 rows;
- `pinyin_remove_user_candidate` asserts NORMAL type **and** a
  USER_DICTIONARY token (`pinyin.cpp:3734,3738`). Its false path on a
  system row cannot be measured against the oracle. The probe calls it
  only behind a true `pinyin_is_user_candidate`, which is ibus's own
  guard;
- `get_left/right_pinyin_offset` run a second `_check_offset` on the
  computed offset (`pinyin.cpp:3055/:3090`), which asserts for tail
  cursors (upstream fixed it after the pin, `95e3af7`). The probe uses the
  uncovered-surfaces differential's cursor set, which smoke runs proved
  safe.

## Matrix

Evidence codes:

- **D**: the differentials that ran IDENTICAL at the pin and assert the
  row's surface. All ran 2026-09-16/17 in the `debian:testing` container
  named below. They are option-sweep (21 cases at 0x1e on `main`; 24 with
  the row-17 cases on PR #472's tree), scheme-diff double (PR #471's
  out-of-enum probe), chewing ×8 keyboards (PR #478's runs), live-typing
  (317 lines, PR #479), uncovered surfaces (993 lines, PR #479),
  key-surface (2131), import (16), nbest-train (56), predict (5), punct
  (17), pred-order (1588), addon-candidate (4), user-candidate (1),
  dict-surface (168), phrase-surface (19) and fullpin scheme 1.
- **T**: Rust tests on the oxpinyin ABI. They are supporting evidence
  only.

The union probe itself diverges (below), so it gives no coverage. Every
symbol's class rests on the D set.

| # | Symbol | Class | D | T |
|---|--------|-------|---|---|
| 1 | `pinyin_init` | COVERED | every runner (non-NULL or the run dies) | context.rs:183,214 |
| 2 | `pinyin_fini` | COVERED (void; leak gates clean, every runner completes) | every runner completes | — |
| 3 | `pinyin_alloc_instance` | COVERED | every runner (non-NULL) | common.rs:56 |
| 4 | `pinyin_free_instance` | COVERED (void; leak gates clean, every runner completes) | every runner completes | — |
| 5 | `pinyin_set_options` | COVERED | option-sweep (ret + per-bit option law), scheme/chewing/live/uncovered rets | config.rs:311-383 |
| 6 | `pinyin_set_double_pinyin_scheme` | COVERED | scheme-diff: ret, scheme effect, the row-5b out-of-enum rows byte-equal | contract.rs:32-189 |
| 7 | `pinyin_set_zhuyin_scheme` | COVERED | chewing-diff ×8: ret, fatal-on-reject | contract.rs:94-189 |
| 8 | `pinyin_load_addon_phrase_library` | COVERED | addon-candidate-diff: rets + addon rows | union_e2e:69-81 |
| 9 | `pinyin_unload_addon_phrase_library` | PARTIAL | key-surface: rets per index; the unload effect unobserved by any differential | keys.rs:475-483 |
| 10 | `pinyin_save` | PARTIAL | import/nbest: rets; the written file never compared | e2e:485-500 |
| 11 | `pinyin_reset` | COVERED | live-typing/scheme/chewing: ret + parsed-after-reset | pipeline.rs:151,206 |
| 12 | `pinyin_parse_more_full_pinyins` | COVERED | option-sweep parse rows; live-typing fatal-if-wrong | pervasive |
| 13 | `pinyin_parse_more_double_pinyins` | COVERED | scheme-diff: consumed + the 5b probe rows | contract.rs:34-76 |
| 14 | `pinyin_parse_more_chewings` | COVERED | chewing-diff ×8: consumed | exact_scheme.rs:54,101 |
| 15 | `pinyin_in_chewing_keyboard` | COVERED | chewing-diff table-check ×8: ret + symbol strv per key | — |
| 16 | `pinyin_guess_sentence` | COVERED | ret + downstream sentence rows in scheme/chewing/live/uncovered/nbest | pipeline.rs:144-197 |
| 17 | `pinyin_guess_candidates` | COVERED | option-sweep at `0x1e`/`0x1c`/`0x14` (2026-09-20): parse/aux and top-10 TEXT/ORDER identical at all three; ABI probe at the same words byte-identical — 0 diverging lines (re-measured 2026-09-20 on top of #493, main `3c21641f`) | guess_offset_tests.rs:143-186, e2e_tests::longer_choose |
| 18 | `pinyin_guess_predicted_candidates_with_punctuations` | COVERED | predict + punct + pred-order: rets + rows | phrase.rs:210-213 |
| 19 | `pinyin_get_sentence` | COVERED | scheme/chewing/live/uncovered/nbest: ret + text | pipeline.rs:146-181 |
| 20 | `pinyin_get_character_offset` | UNCOVERED | nothing (bisect stale; the probe prints it but its run diverges) | sentence.rs:441-499 |
| 21 | `pinyin_get_n_candidate` | COVERED | n= rows in every runner above | e2e:1657-1722 |
| 22 | `pinyin_get_candidate` | COVERED | candidate rows in every runner above | exact_scheme.rs:32-35 |
| 23 | `pinyin_get_candidate_string` | COVERED | texts in every runner above | exact_scheme.rs:34-42 |
| 24 | `pinyin_get_candidate_type` | COVERED | type rows in pred-order/uncovered/live/nbest/dict-surface | phrase.rs:191-201 |
| 25 | `pinyin_get_candidate_nbest_index` | COVERED | nbest= rows in nbest-train + live-typing + uncovered, all IDENTICAL | — (none on the oxpinyin ABI) |
| 26 | `pinyin_is_user_candidate` | PARTIAL | user-candidate-diff (1 line): the gated user row; false path implicit; union-diff's is_user rows sit in a DIVERGENT run (below) | e2e:262 |
| 27 | `pinyin_remove_user_candidate` | **PARTIAL** | no differential; the true path is pin assert-fenced (pinyin.cpp:3734,3738) | e2e:356-358 (false path) |
| 28 | `pinyin_choose_candidate` | COVERED | live-typing cursor= rows; uncovered deep/tail-choose cursors | e2e + guess_offset_tests |
| 29 | `pinyin_choose_predicted_candidate` | UNCOVERED | nothing (bisect stale) | e2e:229-287 |
| 30 | `pinyin_train` | COVERED | live-typing train= + the 你好\|ni'hao\|1242 export matched; nbest-train rows | e2e:79-115 |
| 31 | `pinyin_get_pinyin_key_rest` | UNCOVERED | nothing (bisect stale; capture replay is pin-side) | cursor.rs:656-704 |
| 32 | `pinyin_get_pinyin_key_rest_positions` | UNCOVERED | same | cursor.rs:662-665 |
| 33 | `pinyin_get_pinyin_offset` | COVERED | uncovered cursor table: off= per byte cursor | cursor.rs:437-530 |
| 34 | `pinyin_get_left_pinyin_offset` | COVERED | uncovered left probes (safe cursors) | cursor.rs:452-536 |
| 35 | `pinyin_get_right_pinyin_offset` | COVERED | uncovered right probes (safe cursors; tail assert = pin landmine, upstream 95e3af7) | cursor.rs:457-543 |
| 36 | `pinyin_get_full_pinyin_auxiliary_text` | COVERED | option-sweep aux rows; uncovered | pipeline.rs:85-92, text.rs:571-577 |
| 37 | `pinyin_get_double_pinyin_auxiliary_text` | COVERED | scheme-diff per-cursor double_aux | — |
| 38 | `pinyin_get_chewing_auxiliary_text` | COVERED | chewing-diff per-cursor ×8 | pipeline.rs:109-116 |
| 39 | `pinyin_mask_out` | UNCOVERED | train-diff stale (pre-split fixtures/w3 root, exits 1); no other differential | e2e:293,327-346 |
| 40 | `pinyin_remember_user_input` | UNCOVERED | same | e2e:132-214 |
| 41 | `pinyin_begin_add_phrases` | COVERED | import-diff: BEGIN rows + add rets + export rows | e2e:628-638 |
| 42 | `pinyin_iterator_add_phrase` | COVERED | import-diff: add rets + rows | e2e:629-716 |
| 43 | `pinyin_end_add_phrases` | COVERED (void) | import-diff: end + save rows downstream | e2e:687-736 |
| 44 | `pinyin_begin_get_phrases` | COVERED | import-diff | e2e:738-774,839 |
| 45 | `pinyin_iterator_has_next_phrase` | COVERED | import-diff: rows + exhausted | e2e:334-843 |
| 46 | `pinyin_iterator_get_next_phrase` | COVERED | import-diff: phrase\|pinyin\|count rows | e2e:592-598 |
| 47 | `pinyin_end_get_phrases` | COVERED (void) | import-diff | leak gates |
| 48 | `pinyin_begin_get_bigram_phrases` | COVERED | nbest-train B rows | e2e:840,879 |
| 49 | `pinyin_bigram_iterator_has_next_phrase` | COVERED | nbest-train | e2e:606,844-887 |
| 50 | `pinyin_bigram_iterator_get_next_phrase` | COVERED | nbest-train B rows | e2e:610-616,881-884 |
| 51 | `pinyin_end_get_bigram_phrases` | COVERED (void) | nbest-train | leak gates |
| 52 | `pinyin_get_parsed_input_length` | COVERED | every runner's parsed rows | parse.rs:243-265 |
| 53 | `pinyin_clear_constraint` | COVERED | live-typing cr: rows | e2e:1050-1240 |
| 54 | `pinyin_get_pinyin_key` | UNCOVERED | nothing (capture replay is pin-side; bisect stale) | cursor.rs:650-701 |
| 55 | `pinyin_get_pinyin_string` | COVERED | key-surface render rows | keys.rs:115-436 |
| 56 | `pinyin_get_pinyin_key_rest_length` | UNCOVERED | no driver prints it, stale or current | cursor.rs:668-671 |
| 57 | `pinyin_get_pinyin_strings` | COVERED | key-surface | keys.rs:397-427 |
| 58 | `pinyin_get_zhuyin_string` | COVERED | key-surface | keys.rs:121-442 |

| 59 | `pinyin_get_context` | COVERED | key-surface: context match | — |
| 60 | `pinyin_get_luoma_pinyin_string` | COVERED | key-surface render rows | keys.rs:168-176 |
| 61 | `pinyin_get_secondary_zhuyin_string` | COVERED | key-surface | keys.rs |
| 62 | `pinyin_guess_predicted_candidates` | UNCOVERED | nothing (the with-punctuations variant is row 18; the plain variant has no IDENTICAL differential run) | — |
| 63 | `pinyin_guess_sentence_with_prefix` | COVERED | phrase-surface: prefix ok + rows | — |
| 64 | `pinyin_lookup_tokens` | COVERED | dict-surface: token sweeps | — |
| 65 | `pinyin_parse_full_pinyin` | COVERED | key-surface single-key parses | — |
| 66 | `pinyin_parse_double_pinyin` | COVERED | key-surface | — |
| 67 | `pinyin_parse_chewing` | COVERED | key-surface | — |
| 68 | `pinyin_phrase_segment` | COVERED | phrase-surface: segment ok + tokens | — |
| 69 | `pinyin_get_n_phrase` | COVERED | phrase-surface | — |
| 70 | `pinyin_get_phrase_token` | COVERED | phrase-surface | — |
| 71 | `pinyin_get_pinyin_is_incomplete` | COVERED | key-surface | — |
| 72 | `pinyin_set_full_pinyin_scheme` | COVERED | fullpin scheme 1 (scheme differential, full): ret + scheme effects, IDENTICAL | — |
| 73 | `pinyin_token_get_phrase` | COVERED | dict-surface: per-token reads | — |
| 74 | `pinyin_token_get_n_pronunciation` | COVERED | dict-surface | — |
| 75 | `pinyin_token_get_nth_pronunciation` | COVERED | dict-surface | — |
| 76 | `pinyin_token_get_unigram_frequency` | **PARTIAL** | dict-surface: add-then-read flows IDENTICAL; the probe's fresh-context read of a system token's unigram diverges (residue D below) | — |
| 77 | `pinyin_token_add_unigram_frequency` | COVERED | dict-surface: overlay semantics | — |
| 78 | `pinyin_load_phrase_library` | COVERED | dict-surface: provisioned indexes + out-of-range false | — |
| 79 | `pinyin_unload_phrase_library` | COVERED | dict-surface: in-range sweep (false for the non-GBK defaults) | — |

Tally: **65 COVERED · 5 PARTIAL (rows 9, 10, 26, 27, 76) · 9
UNCOVERED (rows 20, 29, 31, 32, 39, 40, 54, 56, 62) — sums to 79.**

## The oracle runs — measured per sort word

Run conditions:

- Date: 2026-09-16/17 UTC.
- Container: `debian:testing` `a15849ef7dd2` (image
  `docker.io/library/debian@sha256:5056ab8a99336d6d71390d640f72229649f12e3d38e987cf6b24dc8675325d73`).
- The pin oracle prefix is read-only. Both sides are tkrzw. The oracle
  uses the pin's own `data/`. The capi uses a tkrzw compile of the model
  text, native names + `interpolation2.text`.
- Trees: `main` for the 0x1e-baseline reproductions; a stack of `main` +
  PR #476 + PR #471 + PR #472 for the per-word walks. The row-5b
  out-of-enum rows agree there by construction.
- Both sides complete the whole walk at every word (249–253 log lines, no
  SKIP, no crash).
- Logs: `~/.local/share/oxpinyin-evidence/2026-09-17/w3/` (the six-word
  full-ABI runs `abi-{oracle,capi}-<word>.log` and
  `abi-sixwords-summary.txt`; the perturbed builds and reverted
  baselines).

The full-ABI extension is the `abi-extras` phase. It drives the 21
symbols above on a fresh context and honours the pin's caller contracts:

- full-pinyin schemes stay within 1..3;
- `unload_phrase_library` runs only on in-range indexes;
- `load_phrase_library` runs on dict-surface's smoke-proved provisioned
  set {1,2,4,7}. An unprovisioned index asserts inside the pin
  (`pinyin.cpp:457`).

With this extension, every extras row except one is IDENTICAL at every
word. The exception is residue (D) below.

| sort word | verdict | shape |
|---|---|---|
| `0x1e` (parity; every runner's word) | both sides complete; **0 diverging lines** — byte-identical (re-measured 2026-09-20 on top of #493, main `3c21641f`) | was 26 lines = A + B + X2 + D pre-#493; #493 closed A (row 35, `7c9a6923`) and the B/X2 export lines and the extras `token_unigram` line closed with it (the extras context reuses the probe's user dir — the old `1610` read was B's trained overlay; with the write gone both sides read the system `161`). Rows 33/36's dispositions stay with their own differentials |
| `0x1c` (ibus preset 1, the GSettings default) | 80 diverging lines at registration → 0 diverging lines (2026-09-20, re-measured on top of #493, main `3c21641f`) | was A + B + D plus the **longer-candidate hunks** (the pin prepended LONGER(7) rows the capi never produced) — the longer hunks gone with §9 (register row 32 closed), A with #493 (row 35), and the B/X2/D probe lines with A's closure; the probe is byte-identical at this word |
| `0x14` (ibus preset 0) | 80 diverging lines at registration → 0 diverging lines (2026-09-20, re-measured on top of #493, main `3c21641f`) | was A + B + D plus the **longer-candidate hunks** and the 0x8 pinyin-length key's window move (the 西/系 rise the capi never made) — the hunks gone with §9 (register row 32 closed), the 0x8 move with §9's ported keys, A with #493 (row 35), and the B/X2/D probe lines with A's closure; the probe is byte-identical at this word |
| `0x0` (fcitx5-oxpinyin's word) | 78 diverging lines | same plus the unsorted rare-char tails (the keyless comparator returns 0 for every pair, `pinyin.cpp:1678-1709`) |
| `0x1f` (ibus preset 2) | 60 diverging lines | both sides honour the sentence suppression (the capi reads bit 0x1, `sentence.rs:286-287`); residue (C) below |
| `0x16` (fcitx's other word) | 36 diverging lines | A + the 0x8 window effect + B + D |

Non-vacuity, on both surfaces:

- A perturbed capi build (sentence rows force-suppressed, one line in
  `sentence.rs`) at 0x1e grows the diff: the NBEST rows are absent on the
  capi side.
- A second perturbed build (`pinyin_get_luoma_pinyin_string` forced to
  answer false) grows the full-ABI walk from 26 to 30 diverging lines,
  with the four luoma rows flipped.
- After each revert, the baseline re-measures at the recorded figure
  (`w3/perturbed-1e.log`, `w3/post-revert-1e.log`,
  `w3/abi-perturbed-{capi,oracle}-1e.log`).

## The open residues (measured; classification is the maintainer's)

This section records the residues as measured before the fixes. Residues A, B, C, E and F are now CLOSED (register rows 35, 33, 34, 37 and 36). Residue D is no ABI divergence.

**A — the nbest result set: paths on the pin, texts on the capi.** The
sequence is: the import (你好/5, 你好世界/9, 测试/3), parse
`nihaoshijie`, `pinyin_guess_sentence`, then `pinyin_get_sentence` for
every proved index.

- The pin answers `你好世界 / 你好世界 / 你好时节` for indices 0/1/2: two
  distinct **paths** with the same text.
- The capi answers `你好世界 / 你好时节 / 你好是届`: three distinct
  **texts**.

The pin's candidate list then removes the duplicate-text row.
`_prepend_sentence_candidates` at `pinyin.cpp:1943-1948` emits one row per
nbest result, and `_remove_duplicated_items_by_phrase_string` at
`:2298-2299` collapses the second 你好世界. So the pin's list carries
nbest 0 and 2, and the capi's carries 0, 1 and 2. This is the 0x1e hunk.
Logs: `item4/{oracle,stack}-1e.log` (section A).

**B — the train record after a whole-row choose.** The sequence is
`choose(row 0)` + `train(0)`.

- The pin's user bigram export stays **empty**. A whole-row choose trains
  nothing into the user bigram, and the phrase index keeps the imported
  counts unchanged.
- The capi records `你好世界|ni'hao'shi'jie|138`. A second `train(0)`
  moves the capi's count to 414: the first adds 138, the second 276,
  because the user bigram's update doubles on repeat. This explains the
  138 (one train call) and 414 (two) figures across the recorded runs.
- `pinyin_remember_user_input("你好",1)` returns false on both sides and
  changes nothing.

The result is identical on `main` and on the stack, at 0x1e and at 0. So
the sort word does not cause it, and PR #471 or #472 did not introduce it.
Logs: `item4/*.log` (section B).

**C — the imported phrase as a user row.** In a fresh window (parse, then
`pinyin_guess_candidates` with no `pinyin_guess_sentence` before it),
**both** sides show the imported 你好世界 as row 0, typed NORMAL, with
`pinyin_is_user_candidate` **true**. This holds at 0x1f and at 0x1e, for
`nihaoshijie` and for `nihaoshijiema` (where the phrase covers only a
prefix). The two sides are symmetric here.

The asymmetry appears only after `pinyin_guess_sentence` +
`pinyin_guess_candidates`:

- The pin's 0x1f window still carries the user row. The capi's does not.
- The pin's 0x1e window carries sentence rows from the nbest results that
  stay live across the re-parse (instance state). The capi's window
  rebuilds from the fresh parse.

The residue depends on the call sequence and is separate from the
sort-option gap. Logs: `item4/{oracle,stack}-1e.log` (section C).

**D — a system token's raw unigram read.** On the fresh context,
`pinyin_lookup_tokens("你好")` yields the phrase's system token. Then
`pinyin_token_get_unigram_frequency` answers **161** on the pin and
**1610** on the capi: a 10× unit gap on the raw read of a system token.
The dict-surface driver's add-then-read overlay flows are IDENTICAL. So
the gap is specific to a direct read of a system token's stored unigram
field.

Every other extras row is IDENTICAL at every word: the single-key
parsers, `get_context`, `is_incomplete`, the luoma/secondary getters under
schemes 2 and 3, `phrase_segment`, `n_phrase`, `phrase_token`,
`sentence_with_prefix`, the plain predicted variant's ret, the rest of the
introspection family, and the library load/unload pair. Logs:
`w3/abi-{oracle,capi}-1e.log`.

**Union-diff baseline.** The one-line `pred: type=4 text=你` divergence
is a predicted row that the capi emits after the pin's 你好, under
union-diff's import/choose/train flow. It reproduces on `main` + PR #476
**alone** (measured 2026-09-17 in the same container,
`item3/union-diff-main-476.log`). Eleven of the twelve same-data-dir
drivers are IDENTICAL around it. It predates PR #471 and PR #472, and
belongs with the residues above.

## The sort-option gap — CLOSED (§9, register row 32, 2026-09-20)

The table compares the pin and oxpinyin on each bit of
`pinyin_guess_candidates`' `sort_option_t` input. It keeps the state
before the §9 port as the record, with the closure beside each bit:

| bit | pin | oxpinyin (pre-§9) | after §9 |
|---|---|---|---|
| `0x1` SORT_WITHOUT_SENTENCE_CANDIDATE | set → sentence rows dropped (`pinyin.cpp:2295-2296`) | honoured (`sentence.rs`; measured at 0x1f: rows gone on both sides) | unchanged |
| `0x2` SORT_WITHOUT_LONGER_CANDIDATE | clear → LONGER rows prepended (`:2292-2293`; measured: 现代/阿尔/你们 at 1c/14/0) | **ignored** — no longer-candidate code existed; the rows were never produced at any word | **ported** — the engine builds the row (`Session::longer_candidate`: the whole-composition suggestion walk, the two length caps, the max-unigram winner), prepends it at every word with the bit clear, and the ABI types it `LONGER_CANDIDATE` (7) by the zero-span marker |
| `0x4` SORT_BY_PHRASE_LENGTH | comparator key 1 (`:1683-1688`) | ignored; the engine's own order matched the pin at 0x1e in every probe window | **ported** — `RankKey::for_sort_word` zeroes a disabled key, the comparator's own law |
| `0x8` SORT_BY_PINYIN_LENGTH | comparator key 2 (`:1690-1695`) | ignored; the pin's window membership moved without it (0x14/0x16 vs 0x1c), the capi's did not | **ported** — same key |
| `0x10` SORT_BY_FREQUENCY | comparator key 3 (`:1697-1702`) | ignored | **ported** — same key |

Recorded evidence while the gap stood (PR #481's parameterised sweep):
`OPTION_SWEEP_SORT=1e` passed 21/21. `0x1c` and `0x14` stopped on every
case, because the pin's TEXT sets carried the LONGER rows.

After the §9 port (2026-09-20), all three words pass 24/24, and the ABI
probe's LONGER choose phase runs IDENTICAL:

- row `方面` typed 7, `choose=1`, `train=true`, no user-store writes;
- the trained token's unigram reads `23253` before the choose and `23736`
  after: the `+483` read back through the phrase index, identical on both
  sides. The probe's `printf` calls are sequenced for this figure
  (re-measured 2026-09-20).

Consumer reachability, verified in the pinned consumers:

- ibus-libpinyin 1.16.5's presets are 0x14, 0x1c and 0x1f
  (`PYPConfig.cc:225-230`), with 0x1c the default (`:151`). Presets 0 and
  1 both leave `0x2` clear. So default-settings ibus users see LONGER rows
  on libpinyin that oxpinyin never produces, and ibus maps them
  (`CANDIDATE_LONGER`/`CANDIDATE_LONGER_USER`,
  `PYPLibPinyinCandidates.cc:56-62`).
- fcitx-libpinyin 0.5.4 passes 0x16/0x1e (`enummap.cpp:159-166`). Both
  words suppress longer rows, so it is not exposed.
- fcitx5-oxpinyin passes literal 0 (`src/oxpinyin.cpp:1282`, separate
  repository). Against real libpinyin that word yields an unsorted list
  with sentence AND longer rows. It only works today because oxpinyin
  ignores the bits.

The `sentence.rs` doc comment cites both gates correctly: the sentence
gate is `pinyin.cpp:2295-2296`, and `2292-2293` is the longer-candidate
gate.

## The train gate under the widened law — pinyin and zhuyin evidence by arm (register row 38)

**Amended 2026-11-09 UTC (#1761).** Each train arm's evidence, by cell.
"Captured" means the pin and oxpinyin were run and compared. "Source-derived"
means the expectation is read from the pin source at 074a2219 and held by
an oxpinyin test; no pin was run for it.

| Facade | Arm | bdb | kc | tkrzw | Evidence |
|---|---|---|---|---|---|
| pinyin | fresh instance, no guess, no choose | captured | captured | captured | row 38, `train-index-diff` case `fresh`, 2026-10-04 UTC |
| pinyin | parse → NORMAL choose → train, no guess | captured | captured | captured | row 38, case `choose-only` |
| pinyin | choose → guess → train (selection arm) | captured | captured | captured | row 38, cases `0`, `1`, `2` |
| pinyin | guess → train, no choose | source-derived (pin source at 074a2219) | source-derived (pin source at 074a2219) | captured, return value only | tkrzw: pin capture 2026-11-07, `train-index/nihao/{1,2,3}`, held by `oxpinyin-capi`'s `pinned_data_tests::training_the_last_two_nbest_results_succeeds` and `training_one_past_the_nbest_results_aborts`; also the ABI probe's §9 LONGER-choose phase above, where a LONGER choose that records no selection precedes the train. bdb, kc: `pinyin.cpp:2670-2691`, `lookup/phonetic_lookup.h:858-935` |
| zhuyin | fresh instance, no guess, no choose | captured | captured | captured | row 38, case `fresh` |
| zhuyin | parse → NORMAL choose → train, no guess | captured | captured | captured | row 38, case `choose-only` |
| zhuyin | choose → guess → train (selection arm) | captured | captured | captured | row 38, case `0`, which chose 你 before `zhuyin_train` |
| zhuyin | guess → train, no choose | source-derived (pin source at 074a2219) | source-derived (pin source at 074a2219) | source-derived (pin source at 074a2219) | `zhuyin.cpp:1696-1716`, `:1707`, `:551-552`, `:697`; `lookup/phonetic_lookup.h:858-935`; `storage/phrase_index.cpp:392-437`. Held by `oxpinyin-zhuyin-capi`'s `e2e_tests::train_without_a_choose_trains_nothing_and_without_a_user_store_refuses` |

For the zhuyin arm with no choose:

- `zhuyin_train` answers `true` and logs nothing;
- the unigrams of the sentence's tokens and the sentence are unchanged;
- `zhuyin_save` answers `true` and then `false`;
- each saved journal is one `LOG_MODIFY_HEADER` record of an unchanged total;
- the saved user bigram holds no record.

This is an evidence classification, not a defect: the behaviour already
matches the pin source. The 2026-09-20 text below is the dated record of the
gap.

The train gate (now `Runtime::train_session`) is the pin's own disjunction:
a recorded selection OR a live sentence result (`pinyin.cpp:2678-2679`;
`sentence_lookup_active` is the engine's stand-in for
`results.size() > 0`). §9's port widened the gate to this law. Both
shipped C surfaces pass through it: `pinyin_train`, and `zhuyin_train`
over the same facade path. The pin's zhuyin carries the identical law
(`zhuyin.cpp:1696-1705`).

The **pinyin** surface is measured. The ABI probe's longer-choose phase
above runs IDENTICAL. Its `train(after-longer)=true` line is a train with a
live sentence result and NO recorded selection: exactly the widened arm,
on both sides, with no store writes.

The **zhuyin** surface: register row 38 records the train-index
differential (`train-index-diff`). The zhuyin cases `0`, `fresh` and
`choose-only` match the pin on bdb, kc and tkrzw. Every successful zhuyin
case of that differential chooses a candidate first. `guess_sentence`
followed by `zhuyin_train` with no choose is source-derived (#1761), as the
table above records
([`compatibility-policy.md`](compatibility-policy.md), row 38).

## Residue classification (2026-09-19 UTC)

The classes were ruled on 2026-09-19. A was ruled REVERT TARGET as row 35,
on the settling measurement below. B, C and D are by maintainer ruling.
Every REVERT TARGET below is closed (register rows 33–37). Pin cites are
from the checkout at `074a2219`. The mechanism probe and the residue-A
tail probe are retired C drivers. Their runs are recorded evidence.
Measurement environment:
`debian:testing` container
`fd969584d8c60edcdfbc22fea20a0682aa4db90874ce35915b8b14866ed0e8a1`,
image
`docker.io/library/debian@sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9`,
pin oracle tkrzw at `/work/oracle/prefix` (pin_ref
`libpinyin-2.11.92-074a2219…+dbm-tkrzw`), durable logs
`~/.local/share/oxpinyin-evidence/2026-09-19/residue-classify/`
(same-dir phase B/D under `same-dir/`).

### B — whole-row choose + train (**REVERT TARGET**, register row 33)

Current training is `Runtime::train_session` → `Session::prepare_training`
→ `TrainingPlan::observe`. A row with spans trains only the spans with a
OneStep cell. A row with empty spans replays the selection history with no
OneStep gate. The mechanism and measurements below are the 2026-09-19
state that register row 33 records.

**Mechanism.** A row-0 `NBEST_MATCH_CANDIDATE` choose runs
`diff_result(best, best)` and installs no `CONSTRAINT_ONESTEP` —
pin `pinyin.cpp:2515-2520` / `phonetic_lookup.cpp:172-205`; oxpinyin
`constraint.rs:193-214` and `selection.rs:221-241` (comments there
already state the empty-store law). Pin `pinyin_train` always delegates
to `train_result3` (`pinyin.cpp:2670-2690`;
`phonetic_lookup.h:844-936`), which trains a phrase only when
`train_next || constraint.m_type == CONSTRAINT_ONESTEP` (`:866`). With
no OneStep cells the loop writes nothing to the user bigram. oxpinyin's
`Session::train` (`selection.rs:298-338`) takes that constrained walk
only when some `last_result` span sits on a OneStep cell (`:314-318`).
Otherwise it falls through to the selection-history record that the row
choose still filled (`:197-218`, `:333-337`). That fallback is the
divergence. It is not a different concept of "constrained run", and not a
deliberate class-(a)/(b) exception. It is a missing guard,
"no OneStep ⇒ observe nothing", that would match `train_result3`.

**Re-measured (same-dir, 2026-09-19).** After import 你好世界 / parse
`nihaoshijie` / `guess_sentence` / choose row 0:
`clear_constraint(0)=false` on both sides (no OneStep). Pin bigram
export stays empty through two `train(0)` calls. Capi records
`你好世界|ni'hao'shi'jie|138` after the first train and `|414` after
the second — the export's stored-count×2 rendering of seeds 69 then
138 (`user-store.md` §2.1). Logs:
`same-dir/{oracle,capi}.log`.

**User-visible consequence.** Consider N sessions of the same whole-row
accept + train (the ibus shape that commits the 1-best sentence
candidate). The pin's user bigram is unchanged for that pair. oxpinyin
accumulates `sentence_start → 你好世界` (exported counts 138, 414,
1242, …). DYNAMIC_ADJUST / prediction then boosts that phrase on oxpinyin
and not on the pin. This is the only residue that corrupts stored user
state and grows with use.

**Class.** **REVERT TARGET** — missing "no OneStep ⇒ observe nothing"
guard; not (a)/(b). Registered as compatibility-policy row 33, CLOSED.
The closing gate has two parts:

- empty bigram rows on both sides after `train(0)` twice;
- **and** the user 你好世界 token's unigram staying 27 on both sides.

The second part is needed because an A fix alone moves the fallback's pair
to `sentence_start → 你好世界`, which the export never renders.

### A — the missing user-phrase tail (**closed in code 2026-09-19**, register row 35)

**Closed 2026-09-19 UTC** (branch `fix/nbest-step-cost-user-token`; see
the "Landed" paragraph below). This section is the record of the
settling measurement, and it names the missing tail.

**Alignment, verified.** Same-dir on the pin's `data/` inside
`debian:testing` container
`7cfeefdaf53f0809dac9f74ea800ecfa173a2ad481024d2b5a3eab3838874e06`
(image
`docker.io/library/debian@sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9`),
pin oracle tkrzw at `/inputs/oracle-tkrzw/prefix` (pin_ref
`libpinyin-2.11.92-074a2219…+dbm-tkrzw`), durable logs
`~/.local/share/oxpinyin-evidence/2026-09-19/residue-a-root/`
(`04-baseline/`). Import state as before (`set_options(0x18a)`, 你好/5,
你好世界/9, 测试/3, `save`), parse `nihaoshijie`, `guess_sentence`. Pin
tails from a second oracle prefix built with
a retired diagnostic patch (an env-gated `stderr` dump inside
`get_nbest_match`; its stdout was byte-identical to the unpatched
pin's), oxpinyin rows from a runtime probe (nats =
`cost / 1000 · ln 2`). Recorded evidence.

| rank | pin tail: `m_poss`, `m_last_step`, result tokens | ox row: text, cost, nats |
|---|---|---|
| 0 | 你好世界 −14.8274994, `last_step=0`, `{0: 0x07000002}` | 你好世界 28341 = 19.644484 (`0x01006205`@0 + `0x01007a03`@5) |
| 1 | 你好世界 −19.6447334, `last_step=5`, `{0: 0x01006205, 5: 0x01007a03}` | 你好时节 35656 = 24.714856 |
| 2 | 你好时节 −24.7148857, `last_step=5`, `{0: 0x01006205, 5: 0x01007a09}` | 你好是届 35822 = 24.829918 |

ox[0] ≡ pin[1] (`|Δ| = 0.00025` nats) and ox[1] ≡ pin[2]
(`|Δ| = 0.00003` nats). So oxpinyin's list is the pin's, shifted up by
one. Exactly one tail, the pin's rank 0, is absent. The third row is
filled from below (你好是届). The pin's dump prints its three tails only.
That this path is fourth in the pin's own heap is therefore inferred from
the aligned costs, not measured. The residual `|Δ|` of the two aligned
pairs is row 11's float band, not this residue.

**What the missing tail is.** One phrase step: the imported user token
`0x07000002` (你好世界, USER_DICTIONARY nibble 7) covering columns
0..11 from the `sentence_start` seed — `m_handles = {0x00000001,
0x07000002}`, `m_last_step = 0`, `m_sub_index = 0`,
`m_sentence_length = 4`. `m_last_step` is the column the tail's last
phrase starts at (`trellis_value_t`, `phonetic_lookup.h:46-49`);
`extract_result` walks it back to the seed (`:370-393`), so
`last_step = 0` on a whole-input tail says the sentence is one phrase.
The instrumented pin shows it created by `unigram_gen_next_step(start
= 0, end = 11, token = 0x07000002)` (`:643-668`) inside the free
widening loop at `i = 0` (`:794-812`): `elem_poss = 27 / 51051882 =
5.2887e-07`, `pinyin_poss = 1.0`, `m_poss = log(elem_poss · 1.0 ·
unigram_lambda 0.6873010) = −14.8274994`. The user 你好 token
`0x07000001` (unigram 15) enters the same way at 0..5 (`m_poss
−15.4152861`) and dies at step 5 — the unigram branch expands only
the beam head (`search_unigram2`, `:540-547`) and no merged gram
exists for a user token — so the pin's three tails are exactly: the
user single-token path, system 你好+世界, system 你好+时节.

Why the import creates it: `_add_phrase` (`pinyin.cpp:514-610`) writes
the phrase into three places:

- the phrase table;
- the pinyin table (`:597-599`), so `search_matrix` spells it;
- **and** the phrase index, with `add_unigram_frequency(token, count ×
  unigram_factor 3)` (`:604-605`), so `get_phrase_item` prices it.

So an imported phrase is a first-class trellis token. Both sides agree on
that state. `lookup_tokens` answers `0x07000002` with unigram 27 (and
`0x07000001` 你好 with 15) on the pin and on oxpinyin. The facade total is
51051882 on both.

**Where oxpinyin would generate it, and what prevents it.** The
counterpart is the free widening from position 0 in
`crate::nbest::nbest_sentences_with_seeds`
(`crates/oxpinyin-engine/src/nbest.rs:423`, `widen_free_span` `:558`).
The runtime-side probe shows the walk gets all the way there:
`phrase_prefix_exists` is true at `ni`, `ni'hao`, `ni'hao'shi` and
`ni'hao'shi'jie` (the user reverse index answers the widen probe,
`RuntimeDict::phrase_prefix_exists`,
`crates/oxpinyin-runtime/src/lib.rs:617`), and
`dictionary.lookup(ni'hao'shi'jie)` returns exactly one entry — token
`0x07000002`, text 你好世界, `pronunciation = None` (possibility 1,
so the `Some((0, _))` skip at `nbest.rs:711` does not fire). The seed
exists, the span is searched, the token is returned. The entry dies in
`expand_entry` (`nbest.rs:677`): `model.nbest_step_costs(sentence_start,
0x07000002)` answers `NbestStepCosts { unigram: None, blended: None }`,
so neither branch pushes a value. That answer is
`BigramLanguageModel::nbest_step_costs_with_user_delta`
(`crates/oxpinyin-data/src/lm/mod.rs`): its first gate
(`:540-543`) destructures `self.unigram_count(token)` and returns the
default when it is `None`. `unigram_count` (`:340`;
`PhraseLibraries::unigram_count`, `phrase_libraries.rs:179`) reads the
*system* chunk libraries only, where a nibble-7 token owns no item. The
function merges the user delta one line later (`:545`; the store carries
27 for this token, and the total already includes it), but that line is
never reached. The gate's comment says that it stands in for "no
installed unigram table". On a runtime with real unigrams, it fires for
exactly the tokens that the pin prices from a user-file sub-index: every
USER_DICTIONARY (7) and NETWORK_DICTIONARY (6) token. The cause is not an
unexpanded seed, a skipped span or a dictionary miss. The language model
refuses to price a token that has no system item.

Probe output (baseline, `04-baseline/ox-probe.log`):

| entry | lib | unigram_freq | `step_costs(sentence_start → token)` |
|---|---|---|---|
| `0x01006205` 你好 | 1 | 161 | unigram 18816 (13.042 nats), blended 17543 (12.160 nats) |
| `0x07000001` 你好 | 7 | 15 | **None / None** |
| `0x07000002` 你好世界 | 7 | 27 | **None / None** |

**What consumes the path rather than the text — measured.** Same-dir
ABI probe (residue-A tail probe, phases X/B/D):

- **Choose cursors and constraints.** `pinyin_choose_candidate` of an
  NBEST row runs `diff_result(best, other)` (`pinyin.cpp:2513-2519`;
  `phonetic_lookup.cpp:172-207`) and forces every phrase where the row
  differs from result 0. Choosing the 你好时节 row (visible on both
  sides): the pin forces both phrases — `clear_constraint(0) = true`,
  `clear_constraint(5) = true`, because its 1-best is one whole-input
  token and 你好 differs from it — where oxpinyin forces only 时节
  (`clear_constraint(0) = false`). The row's own `nbest` index differs
  as well (2 on the pin, rank 1 being the zombied duplicate text; 1 on
  oxpinyin). The cursor is `matrix.size() − 1 = 11` on both —
  unaffected.
- **Training after that choose** (re-guess first, the ibus shape): pin
  `train_result3` trains `sentence_start → 你好` and `你好 → 时节`,
  moving unigram 你好 161 → 644 and 时节 262 → 745; oxpinyin's
  constrained walk trains only `你好 → 时节` (时节 262 → 745, 你好
  stays 161). The bigram export is identical
  (`你好时节|ni'hao'shi'jie|138`) — the export skips `sentence_start`
  pairs on both sides — so the absent path shows in the unigram, not
  in the export.
- **Whole-row choose + train** (residue B's shape): the pin's
  `diff_result(best, best)` installs nothing and trains nothing;
  oxpinyin's record holds row 0's tokens 你好+世界 and the history
  fallback trains that pair (export `你好世界|…|138` then `414`,
  unigram 你好 161 → 644 → 1610, 世界 41710 → 42193 → 43159; the user
  token stays 27 on both). What differs is B's fallback, not A — but
  the tokens it trains are the pair the missing tail displaced (see
  the common-root experiment below).
- **DYNAMIC_ADJUST** (`_get_previous_token`, `pinyin.cpp:1711-1767`):
  at offset 5 the pin's result 0 holds no token at 5 (one phrase spans
  0..11), so no previous token and no bigram term; oxpinyin's result 0
  holds 世界@5, so previous 你好 and bigram-adjusted keys. Measured at
  `0x38a`, the full window at offset 5 (303 rows on the pin, 304 on
  oxpinyin) orders identically after the n-best rows; the extra row
  is the third n-best text. No observable consequence on this input —
  recorded as measured, not as impossible.
- **Every candidate window.** The pin's three tails carry two texts, so
  `_remove_duplicated_items_by_phrase_string` (`pinyin.cpp:2300`)
  leaves NBEST ranks 0 and 2 and `n = 128`; oxpinyin's three distinct
  texts give `n = 129` with ranks 0, 1, 2 — the `n=` line of every
  window at `0x1e` in this state.

**Class.** **REVERT TARGET** — registered as compatibility-policy
row 35 (maintainer ruling 2026-09-19), **closed 2026-09-19**. At its true width: every
user-library token was
refused an n-best step cost, so no imported or learned phrase could
enter a sentence path; the `nihaoshijie` tail is one instance. The
pin's behaviour is reproducible: the missing
step's price is `log(27 / 51051882 · 0.6873010)`, a basic-ops ratio the
fixed-point scale already reproduces for system tokens (the
counterfactual build below lands it at 14.827804 nats against the
pin's 14.8274994 — `|Δ| = 0.0003`, row 11's band). No language
mechanism is involved and no blocker was found: the step-cost seam
already holds the user delta it needs. Not (a) — the 4.8-nat gap is an
absent step, not accumulation. Not (b), not (c).

**Fix shape (landed).** In
`nbest_step_costs_with_user_delta`, price a token whose library owns
no system item from its user delta alone — `count = 0 +
user.unigram_delta` — for the user-file libraries (nibbles 6 and 7)
and the promoted addon nibble, keeping `None` for a token of an
unloaded (masked) library as `get_phrase_item` failing does;
`unigram_total` already carries the user delta. Expected oxpinyin
rows: 你好世界 at `surprisal(0.6873010 × 27, 51051882)` = 21392
millibits (14.8278 nats), 你好世界 (the system pair, 19.644), 你好时节
(24.715) — the candidate windows then dedup the second to `n = 128`.

**Closing differential** (recorded evidence; residue-A tail probe,
same-dir on the pin's `data/`): phase A IDENTICAL — `sentence[0..2]` = 你好世界 /
你好世界 / 你好时节, `A-1e:n=128`, NBEST ranks 0 and 2; phase X
IDENTICAL — `clear_constraint(0)=true`, the 你好时节 row at nbest
index 2, unigram 你好 161 → 644 after the train; phase D `n=303`. The
runtime probe must print `step_costs(sentence_start → 0x07000002):
unigram=Some(21392)` and rows 14.828 / 24.715 nats with
`sentence_text(1)` the duplicate text. Phases B and C are **not** part
of A's gate: the experiment below shows B's export line going empty
under an A fix while B's defect stands, so a B gate must read the
user token's unigram, not the export.

**Landed (2026-09-19 UTC).** The fix shape above is the shipped gate in
`BigramLanguageModel::nbest_step_costs_with_user_delta`
(`crates/oxpinyin-data/src/lm/mod.rs`, branch
`fix/nbest-step-cost-user-token`). `unigram_count`'s `None` splits three
ways:

- a visible `USER_FILE` nibble (5/6/7) passes with `count = 0` and prices
  from `user.unigram_delta`;
- a masked library's token keeps the default answer;
- a loaded library's missing item keeps the default answer.

The last two match the pin's failing `get_phrase_item`
(`ERROR_NO_SUB_PHRASE_INDEX` / `ERROR_NO_ITEM`). The pre-registered
differential ran IDENTICAL on its gate: same-dir on the pin's `data/`
inside `debian:testing` (image `sha256:dab11cdb…`), pin oracle tkrzw at
`/inputs/oracle-tkrzw/prefix`. Phase A is byte-equal (`A-1e:n=128`, ranks
0 and 2, `sentence[0..2]` 你好世界 / 你好世界 / 你好时节). Phase X is
byte-equal (`clear_constraint(0)=true`, 你好时节 at nbest index 2, 你好
161 → 644 after the train). `D-5:n=303`. The runtime probe prints
`step_costs(sentence_start → 0x07000002): unigram=Some(21392 =
14.827804 nats)` with rows `21392 = 14.827804487` /
`35656 = 24.714855870` nats and `sentence_text(1)` the duplicate
text. The whole-log diff's residue is exactly B, C, X2 and E (rows
33, 34, 36, 37) — no A, X or D line remains. Row 35 is closed.

**Side observations (not A, B or C).**

1. `pinyin_bigram_iterator_get_next_phrase` on the last export row
   answers `false` on the pin (it returns `has_next_phrase`,
   `pinyin.cpp:910`) and `true` on oxpinyin (row fetched). This is an ABI
   return-value divergence. The union probe could not see it, because the
   pin's export was empty in every probed state. It is registered as row
   36 (section F below).
2. After a whole-composition NBEST choose and re-guess,
   `guess_candidates(0, 0x1e)` answers `n = 1` (the n-best row only) on
   oxpinyin and `n = 128` on the pin (phase X2). This is residue E below,
   registered as row 37.

**The settling measurement (2026-09-19), as recorded.**

**Mechanism.** The pin's k-best keeps up to three trellis *tails*, with
no text dedup inside the search (`phonetic_lookup.h:736-836`, `get_tails`
`:330-340`). `pinyin_get_sentence` converts each `MatchResult` by index
(`pinyin.cpp:1464-1481`), so two paths can share a display string.
Candidate-list dedup (`_remove_duplicated_items_by_phrase_string`,
`:2298-2299`) is a later, separate pass. oxpinyin also does not text-dedup
inside the trellis (`nbest.rs:301-333`, `:508-535`). Its span expand skips
only duplicate *tokens* on the same span (`:601-622`). Candidate-list
`prepend_nbest_rows` + `dedup_by_text_keep_first` (`lookup.rs:558-580`) is
the same late pass as the pin's. The ABI probe's `nihaoshijie` gap (pin:
two paths / one text; oxpinyin: three texts) is survivor selection under
that search, not search-time text dedup.

**Which defect.** Survivor selection, **not** search-time text dedup. The
settling dump below shows a wide score gap, so this is not the
float/`gfloat` unresolved band. The section above names the cause more
exactly: the survivor that the pin selects and oxpinyin lacks is one
specific tail. That tail is absent from the trellis, not out-selected,
because the step that would create it is never priced.

**Band measurement (2026-09-19 UTC) — `tuihui`, system-only, not the
settling input.** Same-dir on the pin's `data/` inside `debian:testing`
container `fd969584d8c6` (image
`docker.io/library/debian@sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9`);
logs
`~/.local/share/oxpinyin-evidence/2026-09-19/residue-classify/tuihui-a/`.
Both sides emit the same three texts; this table is the comparator
band only.

| rank | pin text | pin `m_poss` | ox text | ox cost | ox nats |
|---|---|---|---|---|---|
| 0 | 退回 | −12.5262127 | 退回 | 18072 | 12.526555847 |
| 1 | 退回 | −13.0863819 | 退回 | 18880 | 13.086618769 |
| 2 | 退会 | −13.0903454 | 退会 | 18885 | 13.090084505 |

Pin `|m_poss[1] − m_poss[2]| = 0.0039635` nats. The pin's final tail
sort (`trellis_value_compare`, `phonetic_lookup.h:174-178`) truncates the
float possibility difference to `gint`. So two tails with
`|Δm_poss| < 1.0` compare equal and keep heap-pop order. The pin's own
ordering of survivors resolves only differences of **1.0 nat** or more. A
near-tie on an input where the two sides agree does not explain a
different input where they do not.

**`tuihui` vs `sentence-surface.md`.** The §12 example (oracle
`[退回, 退回, 退会]` vs port `[退回, 退会]`) was measured over exported
model20 tables against `fixtures/w4/oracle-sentence-surface.txt`
(fixture line: `tuihui` sentences `退回 / 退回 / 退会`). On the pin's own
`data/` (this same-dir dump), the port emits all three texts and matches
the pin. The two table configurations give two survivor sets.

**Settling measurement (2026-09-19 UTC) — `nihaoshijie`, ABI-probe
state.** Container `fd969584d8c6`, same image. Sequence:
`set_options(0x18a)`, import 你好/5, 你好世界/9, 测试/3, `save`, parse
`nihaoshijie`, `guess_sentence`. Logs
`~/.local/share/oxpinyin-evidence/2026-09-19/residue-classify/nihaoshijie-a/`.
Pin tails from instrumented `get_nbest_match` (`m_poss`); oxpinyin
rows from the session n-best cost field (nats = `cost/1000 · ln 2`).
C-ABI `get_sentence` texts on both `.so` files match the table.

| rank | pin text | pin `m_poss` | ox text | ox cost | ox nats |
|---|---|---|---|---|---|
| 0 | 你好世界 | −14.8274994 | 你好世界 | 28341 | 19.644484244 |
| 1 | 你好世界 | −19.6447334 | 你好时节 | 35656 | 24.714855870 |
| 2 | 你好时节 | −24.7148857 | 你好是届 | 35822 | 24.829918302 |

Pin rank-1 is the duplicate-text path. The ox row that displaced it is
rank-2 你好是届. The margin `|24.829918302 − 19.6447334| = 5.185` nats is
far above the 1.0 nat `gint` band. The pin's own
`|m_poss[1] − m_poss[2]| = 5.070` nats is the same wide gap. Ox rank-0
nats `19.644` matches pin `|m_poss[1]|`, not pin `|m_poss[0]| = 14.827`
(the `last_step=0` user-phrase tail). A re-measurement on 2026-09-19 in
container `7cfeefdaf53f` gave identical figures (the alignment table at
the top of this section).

**(a) does not hold.** The gap is structural, not float accumulation
inside the unresolved band. Classification is the maintainer's. This
section adds no register row.

**Corpus-tier coverage.** `sentence_tail` /
`sentence_surface_parity` gate the ordered `get_sentence` lists and
the distinct-same bucket on exported tables. They do not cover this
import-boosted ABI-probe shape.

**User-visible consequence.** After an import, the pin's 1-best for the
imported phrase's pinyin is the imported phrase itself, as one token. Its
rank-1 is the same text assembled from system phrases. oxpinyin's 1-best
is only the assembled text, with a third, different string in the list.
The display string at rank 0 is the same, but the token path underneath
differs:

- a row choose constrains fewer phrases;
- a train after a non-best choose touches fewer unigrams;
- every candidate window carries one more sentence row.

### A — common-root experiment against B and C (2026-09-19 UTC)

**Question.** A (the user-phrase tail never enters the trellis), B
(whole-row choose trains through the history fallback) and C (the
imported NORMAL leaves the window after `guess_sentence`) all turn on
one imported user phrase. One defect with three symptoms, or three
defects?

**Design.** Neutralise A's mechanism alone, and re-measure B and C in the
same state.

- The counterfactual is a scratch build of oxpinyin-capi with one scratch
  patch. The patch replaces the first gate of
  `nbest_step_costs_with_user_delta` with `unwrap_or(0)`, so a token
  without a system item is priced from its user delta. Nothing else
  changes.
- The patch was never part of the shipped tree. It was applied with
  `patch -p1` in the container, built into `target/cf`, run, and reverted
  (`patch -R`). The worktree carries no counterfactual line.
- Both builds run through the same ABI probe (residue-A tail probe, phases
  A/C/B/X/D) and the same runtime probe, same-dir on the pin's `data/`,
  against the same pin.
- The B observable is deliberately wider than the bigram export. The
  export skips `sentence_start` pairs on both sides (`pinyin.cpp:804`,
  `iterators.rs:323`), so the probe also reads the user token's unigram
  before and after each train.

**Pre-registered outcomes** (written before the counterfactual ran):

- *One root (A ⇒ B, C):* the counterfactual makes phase A IDENTICAL
  **and** phase B's export empty with every unigram matching the pin
  **and** phase C's `0x1f` window keeps the user NORMAL.
- *Three defects:* the counterfactual makes phase A IDENTICAL and
  changes nothing in C; in B the export line goes empty (row 0 is now
  the single user token, and the only pair the fallback trains is
  `sentence_start → 0x07000002`, which no export renders) **while the
  user token's unigram climbs 27 → 510 → 1476** (seed 69 × 7, then
  138 × 7) on oxpinyin and stays 27 on the pin — the fallback still
  trains, only its target moved.
- *Mixed:* any other pattern; recorded as measured.

**Run.** Container `7cfeefdaf53f` (image
`docker.io/library/debian@sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9`),
pin oracle tkrzw at `/inputs/oracle-tkrzw/prefix`; logs
`~/.local/share/oxpinyin-evidence/2026-09-19/residue-a-root/05-counterfactual/`
(ABI logs + `ox-probe.log`), driver log `05-experiment-driver.log`
(patch applied, `target/cf` built — sha256
`0d2a49d9…dcc849` against the baseline `846d0614…36ab` — probe run,
patch reverted). Baseline: `04-baseline/`.

**Result.**

| phase | observable | pin | ox baseline | ox counterfactual |
|---|---|---|---|---|
| A | `sentence[0..2]` | 你好世界 / 你好世界 / 你好时节 | 你好世界 / 你好时节 / 你好是届 | 你好世界 / 你好世界 / 你好时节 — **IDENTICAL** |
| A | `A-1e:n`, NBEST ranks | 128; 0, 2 | 129; 0, 1, 2 | 128; 0, 2 — IDENTICAL |
| A | row 0 cost (runtime probe) | −14.8274994 | 19.644484 nats (pair) | 14.827804 nats (`0x07000002`, 21392 millibits) |
| A | `step_costs(sentence_start → 0x07000002)` | priced (`unigram_step` line) | None / None | unigram Some(21392) |
| C | user NORMAL 你好世界 at `0x1f` after guess | present (`n=127`) | absent (`n=126`) | absent (`n=126`) — **unchanged** |
| C | n-best rows after re-parse at `0x1e` | 2 rows (`n=128`) | 0 rows (`n=127`) | 0 rows (`n=127`) — unchanged |
| B | bigram export after `train(0)` / again | empty / empty | `你好世界\|…\|138` / `414` | **empty / empty** |
| B | unigram user 你好世界 (`0x07000002`) | 27 / 27 | 27 / 27 | **510 / 1476** |
| B | unigram system 你好 / 世界 | 161 / 41710 | 644→1610 / 42193→43159 | 161 / 41710 (matches pin) |
| X | `clear_constraint(0)` after choosing 你好时节 | true | false | true — IDENTICAL |
| X | that row's `nbest` index; 你好 unigram after train | 2; 644 | 1; 161 | 2; 644 — IDENTICAL |
| D | `guess_candidates(5, 0x1e)` at `0x38a` | `n=303`, order | `n=304`, same order | `n=303` — IDENTICAL |

**Verdict: refuted — no shared root.**

- The counterfactual closes A and every consequence of the absent path:
  the choose constraints, the post-choose train, the window counts and
  the offset-5 window.
- It leaves C byte-for-byte unchanged.
- B's *export symptom* disappears, exactly as pre-registered for the
  three-defects outcome. The fallback's trained pair moves from 你好→世界
  to `sentence_start → 你好世界`, which no export renders. The fallback
  itself keeps writing (user unigram 27 → 510 → 1476; the pin's stays 27).

So there are three independent defects:

- A is the language-model gate on user-file tokens (`lm/mod.rs:540-543`).
- B is the history fallback in `Session::train` (`selection.rs:333-337`,
  removed 2026-11-09 UTC).
- C is the n-best lifetime across parse, plus the NBEST-wins dedup ahead
  of the `0x1f` filter (`instance.rs:145-155`, `state.rs:170-172`,
  `lookup.rs:558-580`).

**Which of rows 33/34 and A a single A fix would close.** A only.

- Row 33 (B) survives, with its symptom moved. After an A fix the bigram
  export reads empty on both sides. So **§10's pre-registered
  differential ("empty bigram rows on both sides after `train(0)` and
  `train(0)again`") passes without the defect being fixed**. The gate must
  also read the user 你好世界 token's unigram (27 on both sides after both
  trains), which `residue-a-tail-diff` phase B prints.
- Row 34 (C) survives unchanged. Its `0x1f` probe has the same outcome
  with or without A.

**Consequence for the fixes (ruled 2026-09-19).** A landed first, then B
and C. Because of the finding above, B's gate reads the user token's
unigram, never the export-only line.

### E — the candidate window behind the composition offset (**REVERT TARGET**, register row 37)

Register row 37 is CLOSED: oxpinyin rebuilds the window at the caller's offset (#604, #609, #625). The symptom and mechanism below are the state before the fix.

**Symptom.** After a choose, call `pinyin_guess_candidates(offset, …)` at
an offset at or behind the offset that the choose advanced to. oxpinyin
answers the cached, composition-anchored list. The pin answers a freshly
built window at the caller's offset.

- Whole-composition shape (residue-a-tail-diff phase X2/E1): 1 row on
  oxpinyin against 128 on the pin.
- Partial shape (phase E2): the remaining input's phrases, against the
  pin's offset-0 phrases.

**Measured (2026-09-19 UTC).** Container `70d00b22eee3` (same image
`docker.io/library/debian@sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9`
as above), same-dir on the pin's `data/`, logs
`~/.local/share/oxpinyin-evidence/2026-09-19/residue-a-root/09-phaseE-baseline/`
and `09-phaseE-cf/` (the counterfactual build of the experiment
above). Import state as before; `n` is `pinyin_get_n_candidate`.

| step | window | pin | ox baseline | ox counterfactual (A neutralised) |
|---|---|---|---|---|
| E1: NBEST row 0 chosen (cursor 11), re-guess | `(0, 0x1e)` | 128 (NBEST 0, 2 + 126 phrases: 你好 你 尼 …) | 3 (its NBEST rows only) | 2 |
| | `(0, 0x1f)` | 127 (你好世界 user NORMAL first) | **0** | **0** |
| | `(5, 0x1e)` | 303 (NBEST + 世界 时节 …) | 3 | 2 |
| | `(11, 0x1e)` | 2 (NBEST only; nothing starts at the end) | 3 | 2 — identical |
| E2: NORMAL 你好 chosen (cursor 5), re-guess | `(0, 0x1e)` | 129 (NBEST 0,1,2 + 你好 你 …) | 304 (NBEST + 世界 时节 …: the offset-5 list) | 304 |
| | `(0, 0x1f)` | 127 (你好世界 user NORMAL first) | 301 (世界 时节 使节 视界 …) | 301 |
| | `(5, 0x1e)` | 304 | 304 — identical | 304 |

At the offset the choose advanced to (E1 at 11, E2 at 5) the two sides
agree, up to A's extra n-best text. Behind it they do not: oxpinyin
answers the same list for every offset at or below the composition
offset.

**Mechanism — pin.** `pinyin_guess_candidates` frees the list and
rebuilds it from `start = offset` over the whole-composition matrix on
every call (`pinyin.cpp:2184-2262`; `_check_offset` `:2163-2180`
refuses only the lone-apostrophe column). The instance carries no
composition offset: `pinyin_choose_candidate` writes a constraint and
answers a cursor (`:2501-2590`), nothing else, so a window behind an
earlier choose is the ordinary window at that offset with the n-best
rows of the constrained re-guess prepended.

**Mechanism — oxpinyin.** Three sites. (1) `Session::select_inner`
advances the composition offset to the chosen span's end
(`crates/oxpinyin-engine/src/session/selection.rs:229`, `:252`,
`set_consumed(constraint_end)`). (2) The cached list is rebuilt at
that offset — `refresh` → `scan_window(anchor = consumed)`
(`session/lookup.rs:102-110`) — and a fully-consumed anchor yields the
n-best rows alone, which is the `n = 1` (or 3) of E1. (3) The C ABI's
`pinyin_guess_candidates` re-anchors only for a normalized offset
strictly past the composition offset
(`crates/oxpinyin-capi/src/sentence.rs:319-339`): an offset at or
below it is the "stale cursor behind the selection" branch and is
served the cached list. `Session::candidates_at(offset)` already
builds the pin's whole-matrix window at any offset without disturbing
the cached list (`lookup.rs:283-312`); it is simply never called for
an offset behind the composition. The closed mid-syllable entry
(`compatibility-policy.md`, row 13)
ported the pin's `start = offset` law only for offsets past the
composition offset.

**Not A, not row 34 — independent.** Under the counterfactual build
the counts behind the composition are unchanged (the table's last
column; only A's n-best text disappears, and `(11, 0x1e)` becomes
identical). Row 34's mechanism (`reset_parse_state` on parse, the
NBEST-wins dedup ahead of the `0x1f` filter) never runs here: no parse
happens between the choose and the guess, and the n-best rows are
present on both sides in every row of the table — the missing rows are
phrases, not sentences.

**User-visible consequence, and reachability.** ibus-libpinyin 1.16.5
`PhoneticEditor::update` (`PYPPhoneticEditor.cc:346-358`) passes
`lookup_cursor = 0` whenever the sort option carries
`SORT_WITHOUT_SENTENCE_CANDIDATE` — preset 2, `0x1f` — so under that
preset every partial choose is followed by the offset-0 window: E2's
`(0, 0x1f)`, 127 rows headed by the imported user phrase on the pin
against 301 rows of the remaining input's phrases on oxpinyin. Under
every preset `moveCursorLeft` (`:595-604`) lowers `m_cursor` freely
and `getLookupCursor` (`:380-390`) snaps it through
`pinyin_get_pinyin_offset`, so one cursor key after a choose asks for
a window behind it. The whole-composition shape E1 is ABI-reachable
only: an NBEST choose commits at once in ibus (`:485-491`). Where it
is reached, a frontend offers the wrong window — the remaining
input's phrases, or one candidate where libpinyin offers 128.

**Class.** **REVERT TARGET** — registered as compatibility-policy
row 37 (maintainer ruling 2026-09-19), CLOSED. At its worst measured point: `(0, 0x1f)` after
a whole-composition choose answers 127 candidates on the pin and 0 —
an empty list — on oxpinyin; at the ordinary partial choose oxpinyin
answers the offset-5 list where the pin answers the offset-0 one.
It is reproducible. The engine already builds the pin's window at any
offset (`candidates_at`), and the pin's law is a plain `start = offset`
re-scan. No language mechanism, abort or float is involved. The
cached-list rule was a design choice to guard the *choose* leg against a
composition regression. It was extended to the display leg, where the pin
has no such concept.

**Fix shape (landed).** Display leg: in
`pinyin_guess_candidates`, re-anchor whenever `normalized !=
composition_offset()` — both directions — through
`Session::candidates_at(normalized)`, leaving the composition offset
untouched. Choose leg, which the fix must carry with it: a choose from
a behind-window starts before the composition offset, which
`select_inner` refuses (`SelectionAnchorBeforeComposition`,
`selection.rs:157-167`); the pin's `add_constraint` clears the
overlapped forcings and the answered cursor moves back
(`phonetic_lookup.cpp:61-86`), so the record must re-anchor to the
earlier span rather than refuse. The choose leg's shape (choose 你 from
the offset-0 window after 你好 was chosen: pin cursor 2, constraint
你@0..2 replacing 你好) is phase E3.

**Closing differential** (recorded evidence; residue-A tail probe phase
E, same-dir on the pin's `data/`) matches the pin at **every row of
the E1/E2 table** — after the whole-composition choose: `(0, 0x1e)`
`n=128` with NBEST ranks 0 and 2, `(0, 0x1f)` `n=127` with `cand[0]`
the imported user 你好世界, `(5, 0x1e)` `n=303`, `(11, 0x1e)` `n=2`;
after the partial 你好 choose: `(0, 0x1e)` `n=129`, `(0, 0x1f)` `n=127`
with `cand[0..3]` 你好世界(user) / 你好 / 你 / 尼, `(5, 0x1e)` `n=304`
— not only the offsets at or past the composition. Row 35 (§12) lands
first, so the n-best counts in those rows are the pin's by then and
the whole phase runs IDENTICAL; the phase's phrase-row counts (126 at
offset 0, 301 at offset 5) are the A-independent half of the same
gate.

**Landed (2026-09-27, maintainer ruling of that date).** Both legs are
implemented:

- The display leg re-anchors at any offset other than the composition's.
- The choose leg does not hit `SelectionAnchorBeforeComposition` for a
  token-bearing row. The span is forced over the overlapped forcings, and
  the record moves back. The E3
shape is pinned in the engine test
`choosing_behind_the_composition_moves_the_record_back`
(`crates/oxpinyin-engine/src/session/tests.rs`), and recorded evidence
from the candidate-assembly differential (phase M) measured it on both
libraries.

### F — bigram export iterator, last-row return value (**REVERT TARGET**, register row 36)

Register row 36 is CLOSED: oxpinyin returns the pin's value on the last row (#607). The mechanism below is the state before the fix.

**Mechanism.** The pin's `pinyin_bigram_iterator_get_next_phrase`
fills the out-params, advances the pinyin index, and returns
`pinyin_bigram_iterator_has_next_phrase(iter)` (`pinyin.cpp:896-911`):
`false` on the last row. Oxpinyin's returns `true` whenever a row was
fetched and `false` only once exhausted
(`crates/oxpinyin-capi/src/iterators.rs:373-410`). The unigram export
iterator is not in this row — the pin's `pinyin_iterator_get_next_phrase`
returns `true` on every row (`:698-769`), as oxpinyin's does.

**Measured (2026-09-19 UTC).** `residue-a-tail-diff` phase X2, same-dir
on the pin's `data/`, container `7cfeefdaf53f`: after the constrained
train one row is exported on both sides and the line reads
`X2-train1:bigram[0]=false 你好时节|ni'hao'shi'jie|138` on the pin,
`true` on oxpinyin (`04-baseline/`). The union probe could not see it:
the pin's export was empty in every probed state (row 33).

**User-visible consequence.** ibus-libpinyin 1.16.5 wraps the call in
`check_result` (`PYLibPinyin.cc:321`), which is `assert` outside
`NDEBUG`/`G_DISABLE_ASSERT` builds (`PYUtil.h:49-53`). A debug ibus that
exports a user dictionary with at least one bigram row aborts on the pin's
last row, and completes on oxpinyin. Release builds discard the value. The
export contents are identical.

**Class.** **REVERT TARGET** — a plain ABI return-value divergence;
not (a)/(b). Registered as compatibility-policy row 36, CLOSED (#607).

**Fix shape (landed).** Return `handle.index <
handle.rows.len()` after the increment. Closing differential:
phase X2's `bigram[0]` line identical, plus a two-row export (import
two pairs, train each) asserting `true` then `false` on both sides.

### D — system token unigram 161 vs 1610 (**no ABI divergence**)

**Units/scale first.** No deliberate ×10 / ÷10 on either read path.
Pin: `pinyin_token_get_unigram_frequency` →
`PhraseItem::get_unigram_frequency()` (`pinyin.cpp:2821-2833`;
`phrase_index.h:124-126`) — the chunk `u32` field. Oxpinyin: same ABI
→ `RuntimeLm::unigram_freq` → `PhraseLibraries` item field
(`oxpinyin-capi/src/dict.rs:276-344`;
`oxpinyin-runtime/src/lib.rs:780-789`;
`phrase_library.rs:305-311`). `amplified_frequency` is sort-only
(`lookup.rs:640-657`), never this getter. Committed fixtures store
你好 unigram **161** (`fixtures/w3/*/gb_char.bin`).

**Isolation.** dict-surface's add-then-read flows remain IDENTICAL. The
first probe opened the oracle on `ORACLE_DATA` and the capi on
`resolve_system_dir`: different trees. Same-dir re-measure
(2026-09-19): both `.so` files on the pin's `data/` answer
`token_unigram=true/161` for token `0x01006205` / 你好
(`same-dir/{oracle,capi}.log`). The 1610 figure is therefore a
harness/data mismatch from the split-dir walk, not a reader scale bug.

**User-visible consequence.** None on a shared system directory. A
consumer that pointed each library at different compiled tables would
see different raw unigrams by construction.

**Class.** **no ABI divergence** (harness/data). The same-dir probe
settles it. If a future same-dir run produces the 10× gap again, re-open
it as an OPEN DEFECT in the writer of the chunk field, not in the ABI
getter.

### C — imported phrase as a user row (**REVERT TARGET**, register row 34)

Register row 34 is CLOSED: oxpinyin keeps the n-best rows across a parse (#598) and keeps the user row at `0x1f` (#596). The mechanism below is the state before the fix.

**Mechanism — what the second guess changes.**

Fresh window (parse → `guess_candidates` only): both sides surface
imported 你好世界 as NORMAL with `is_user` true — symmetric
(`pinyin.cpp:3712-3723`; `oxpinyin-capi/src/candidates.rs:194-205`).

After `guess_sentence` + `guess_candidates`:

- **Pin.** `guess_sentence` fills `m_nbest_results` only
  (`pinyin.cpp:1372-1386`). A later parse does **not** clear nbest —
  only `pinyin_reset` does (`:2693-2704` vs parse at `:1497-1524`).
  Each `guess_candidates` rebuilds from scratch (`:2184-2300`). At
  `0x1e` the live nbest prepend zombies the same-text user NORMAL
  (NBEST wins, `:2102-2125`). At `0x1f`
  `SORT_WITHOUT_SENTENCE_CANDIDATE` skips the prepend (`:2295-2296`),
  so the user NORMAL remains.
- **Oxpinyin.** `guess_sentence` → `refresh()` →
  `prepend_nbest_rows` + `dedup_by_text_keep_first` drops the
  same-text user phrase from the session cache (`guess.rs:45-103`;
  `lookup.rs:558-580`). Every `begin_parse` calls
  `reset_parse_state` → `reset_composition` → `sentence.reset()`
  (`oxpinyin-facade/src/instance.rs:145-194`;
  `session/state.rs:170-172`) — unlike the pin. `guess_candidates`
  then mostly filters that already-deduped cache
  (`sentence.rs:264-389`), so at `0x1f` the user row is gone with the
  sentences, and after a re-parse at `0x1e` the nbest rows themselves
  are gone.

**User-visible consequence.** It depends on the call sequence:

- After a sentence guess, ibus-style `0x1f` still offers the imported user
  phrase on the pin, and may not on oxpinyin.
- After re-parse + `0x1e`, the pin can still show live sentence rows, and
  oxpinyin rebuilds phrase-only.

**Class.** **REVERT TARGET** — nbest lifetime and candidate-rebuild
mismatch; not (a)/(b). Registered as compatibility-policy row 34,
CLOSED.

**Fix shape (landed).** (1) Keep nbest across parse; clear
only on full reset / a new `guess_sentence`, matching the pin.
(2) When `SORT_WITHOUT_SENTENCE` is set, rebuild the phrase window
without the prior NBEST-wins dedup (or rebuild from scratch every
`guess_candidates` as the pin does). Closing differential:
import 你好世界 → parse → `guess_sentence` →
`guess_candidates(0, 0x1f)` → assert NORMAL + `is_user` on both;
then re-parse → `guess_candidates(0, 0x1e)` → assert nbest rows still
present on both.
