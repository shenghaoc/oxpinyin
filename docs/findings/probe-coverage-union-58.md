# Findings — probe coverage over the 58-symbol consumer union

Date: 2026-09-16 · Status: recorded; the union probe's first oracle run
**diverges** — the run is committed unclassified and the work is STOPped
per the measurement rules (no fix without approval).

The §(e) rule (`docs/findings/compatibility-policy.md`) wants, for every
symbol of the 58-symbol consumer union, a probe that asserts its whole
observable surface: return status, out-params and the data they point
to, written lengths, and handle state. This document is the coverage
matrix the rule's consequence 3 calls for, the description of the new
whole-surface probe (`tools/bisection/union-probe-diff.c` +
`run-union-probe-diff.sh`), and the record of its first run.

## Name correction

`abi-subset.md` §1h lists `pinyin_get_offset`. No such symbol exists in
either `libpinyin.ver` (pin `074a2219`, blob `964d96e0`) or
`crates/oxpinyin-capi/libpinyin.ver`: the export — and ibus's live call
at `PYPPhoneticEditor.cc:375,654,668` — is **`pinyin_get_pinyin_offset`**.
The matrix uses the real name; §1h's typo is reported, not edited here
(§1 is the frozen 1.16.5 characterization).

## Classification rules

- A call made only as setup counts as **UNCOVERED**.
- A symbol covered only by `bisect.c` stays **PARTIAL** unless both
  hold: bisect's log prints every out-param, written length and
  handle-state change for it, AND `run-bisect.sh`'s differential mode
  ran IDENTICAL at the pin. The second leg **cannot be satisfied
  today**: `run-bisect.sh` still resolves the capi data to the pre-split
  flat `fixtures/w3` layout (`pinyin_index.redb` at the directory root)
  and exits 1 (`fatal: redb tables not found`) since the fixture moved
  to per-backend subdirectories — a runner that fails to build or run
  provides no coverage. Reported, not fixed here (tools, outside this
  PR's purpose).
- A runner that skips provides no coverage.
- Leak gates (valgrind, the alloc-pairing LSan harness) are handle-state
  evidence and count for the void destructors, whose whole observable
  surface is release-without-leak and not crashing.

## The new probe

`union-probe-diff.c` resolves exactly the 58 union symbols (refusing to
run when any is missing, so a completed run also proves the export
surface) and walks every symbol's observable surface in deterministic
phases — setup, import, full parse ×2, choose/nbest/train/remember,
prediction, double (incl. the row-5b out-of-enum probe), chewing,
export → mask_out → export, teardown — printing one `label=value` row
per observation: return status, out-param values AND the data they
point to (texts, packed keys, symbol strvs, begin/end/length), written
counts, and handle state. `run-union-probe-diff.sh` drives it into
oxpinyin-capi and the pin and diffs the logs (0 = identical, 1 =
build/run failure, 2 = divergence), env-gated on the pin oracle and
resolving its system dir through `system-dir.sh`.

Oracle caller contracts honoured (and three new pin landmines the probe
documents in-source, all found by crashing exactly once):

- `pinyin_get_sentence` only for proved indices (row 0 after a
  successful guess; an NBEST row's own `nbest_index` value);
- predicted candidates never reach `pinyin_choose_candidate`; only a
  PREDICTED_PREFIX (5) row goes to `pinyin_choose_predicted_candidate`;
- double scheme 30 and zhuyin keyboard 7 are never set;
- `pinyin_guess_sentence` before `pinyin_train`;
- `pinyin_unload_addon_phrase_library` asserts
  `index < PHRASE_INDEX_LIBRARY_COUNT` (`pinyin.cpp:499`) — out-of-range
  is never sent (only `load`'s out-of-range false path is probed);
- `pinyin_get_candidate_nbest_index` asserts NBEST_MATCH
  (`pinyin.cpp:2883`) — asked only of type-1 rows;
- `pinyin_remove_user_candidate` asserts NORMAL type **and** a
  USER_DICTIONARY token (`pinyin.cpp:3734,3738`) — its false path on a
  system row is unmeasurable against the oracle; the probe calls it
  only behind a true `pinyin_is_user_candidate`, ibus's own guard;
- `get_left/right_pinyin_offset` run a second `_check_offset` on the
  computed offset (`pinyin.cpp:3055/:3090`) that asserts for tail
  cursors (upstream fixed it after the pin, `95e3af7`) — the probe uses
  `uncovered-surface-diff.c`'s smoke-proved-safe cursor set.

## Matrix

Coverage columns: **T** = Rust tests on the oxpinyin ABI assert it;
**D** = a differential driver prints it and that runner ran IDENTICAL at
the pin (W4's 2026-09-16 runs: live-typing, uncovered surfaces; A1's
option-sweep; A2d's scheme-diff; W1's chewing runs); **B** = bisect.c
prints it (PARTIAL per the rule above); **U** = the new union probe
prints it. Class is the pre-probe state; the union probe's own verdict
is the STOP record below.

| # | Symbol | Class | Evidence | U |
|---|--------|-------|----------|---|
| 1 | `pinyin_init` | COVERED | T: context.rs:183,214 (non-null; NULL on missing DBM) · B · D: every runner | ✓ |
| 2 | `pinyin_fini` | COVERED (void) | leak gates (valgrind run-bisect mode 2, alloc-pairing LSan) · B literal | ✓ |
| 3 | `pinyin_alloc_instance` | COVERED | T: common.rs:56, pipeline.rs:40, e2e:277 · B | ✓ |
| 4 | `pinyin_free_instance` | COVERED (void) | leak gates · B literal | ✓ |
| 5 | `pinyin_set_options` | COVERED | T: config.rs:311-383 (ret + option-word effects), keys.rs:200-241 · D: option-sweep (A1) | ✓ |
| 6 | `pinyin_set_double_pinyin_scheme` | COVERED | T: contract.rs (ret, half-mutation, restore) · D: scheme-diff incl. the row-5b probe (A2d, IDENTICAL) | ✓ |
| 7 | `pinyin_set_zhuyin_scheme` | COVERED | T: contract.rs:94-132 · D: chewing-diff (W1, 8 keyboards IDENTICAL) | ✓ |
| 8 | `pinyin_load_addon_phrase_library` | COVERED | T: union_e2e_tests.rs:69-81 (true/second-false/missing/out-of-range) · D: addon-candidate, union | ✓ |
| 9 | `pinyin_unload_addon_phrase_library` | COVERED | T: keys.rs:475-483, union_e2e:87-99 · D: key-surface | ✓ |
| 10 | `pinyin_save` | COVERED | T: e2e:284,485-500,687-735 (dirty/clean lifecycle, file written) · D: import/train/nbest runners | ✓ |
| 11 | `pinyin_reset` | COVERED | T: pipeline.rs:151,206, parse.rs:259 (length cleared) · D: every driver's reset rows | ✓ |
| 12 | `pinyin_parse_more_full_pinyins` | COVERED | T: pervasive (consumed lengths) · D: option-sweep, live-typing, uncovered | ✓ |
| 13 | `pinyin_parse_more_double_pinyins` | COVERED | T: contract.rs:34-76, guess_offset_tests · D: scheme-diff (A2d) | ✓ |
| 14 | `pinyin_parse_more_chewings` | COVERED | T: contract.rs:96-131, exact_scheme.rs:54,101 · D: chewing-diff (W1) | ✓ |
| 15 | `pinyin_in_chewing_keyboard` | COVERED | D: chewing-diff table-check per keyboard (W1, all 8 IDENTICAL, symbols printed) · B | ✓ |
| 16 | `pinyin_guess_sentence` | COVERED | T: pipeline.rs:144-197, e2e (true/false paths) · D: every driver | ✓ |
| 17 | `pinyin_guess_candidates` | COVERED | T: guess_offset_tests.rs:143-186 (refusal + range law), e2e · D: every driver | ✓ |
| 18 | `pinyin_guess_predicted_candidates_with_punctuations` | COVERED | T: phrase.rs:210-213, union_e2e:246-401, e2e:1391-1452 (tie order) · D: predict/punct/pred-order/union | ✓ |
| 19 | `pinyin_get_sentence` | COVERED | T: pipeline.rs:146-181, phrase.rs:233-238, e2e:1001-1092 · D: scheme/chewing/fullpin/live-typing/uncovered | ✓ |
| 20 | `pinyin_get_character_offset` | COVERED | T: sentence.rs:441-499 ((bool,offset) tuples, untouched out-param on false, NULL phrase) · B prints | ✓ |
| 21 | `pinyin_get_n_candidate` | COVERED | T: exact_scheme.rs:28, e2e:1657,1718-1722, guess_offset_tests:145 · D: every driver's n= rows | ✓ |
| 22 | `pinyin_get_candidate` | COVERED | T: exact_scheme.rs:32-35, pipeline.rs:52,203, phrase.rs:172-190 · D: every driver | ✓ |
| 23 | `pinyin_get_candidate_string` | COVERED | T: exact_scheme.rs:34-42 (exact texts), config.rs:296-302 · D: every driver | ✓ |
| 24 | `pinyin_get_candidate_type` | COVERED | T: phrase.rs:191-201 (kind law for every row) · D: dict-surface, pred-order, uncovered, live-typing, nbest | ✓ |
| 25 | `pinyin_get_candidate_nbest_index` | **PARTIAL** | D-only: live-typing/nbest-train/uncovered print nbest for NBEST rows (W4 IDENTICAL); no Rust assertion on the oxpinyin ABI | ✓ |
| 26 | `pinyin_is_user_candidate` | COVERED | T: e2e:262 (false for system row) · D: union-diff, user-candidate-diff (true rows printed) | ✓ |
| 27 | `pinyin_remove_user_candidate` | COVERED (false path) | T: e2e:356-358 (NULL, system token) · true path: pin assert-fence (pinyin.cpp:3734,3738) makes the oracle side unmeasurable; user-candidate-diff exercises it capi-side | ✓ (behind is_user) |
| 28 | `pinyin_choose_candidate` | COVERED | T: e2e/guess_offset_tests (returned cursors, absolute end) · D: live-typing, uncovered, dynamic-adjust | ✓ |
| 29 | `pinyin_choose_predicted_candidate` | COVERED | T: e2e:229-287 (store deltas, predecessor law, false without store) · B | ✓ |
| 30 | `pinyin_train` | COVERED | T: e2e:79-115 (store counts), :463 (false, no selection) · D: train/nbest/live-typing (W4 IDENTICAL) | ✓ |
| 31 | `pinyin_get_pinyin_key_rest` | COVERED | T: cursor.rs:656-704 (true at key starts, false elsewhere) · D: capture replay | ✓ |
| 32 | `pinyin_get_pinyin_key_rest_positions` | COVERED | T: cursor.rs:662-665 ((begin,end) pairs) · D: capture replay | ✓ |
| 33 | `pinyin_get_pinyin_offset` | COVERED | T: cursor.rs:437-530 (normalization table, separator run) · D: uncovered cursor phase (W4) | ✓ |
| 34 | `pinyin_get_left_pinyin_offset` | COVERED | T: cursor.rs:452-536 (pairs, false past end, zero-run) · D: uncovered (safe cursors) | ✓ |
| 35 | `pinyin_get_right_pinyin_offset` | COVERED | T: cursor.rs:457-543 · D: uncovered (safe cursors; tail assert = pin landmine, upstream 95e3af7) | ✓ |
| 36 | `pinyin_get_full_pinyin_auxiliary_text` | COVERED | T: pipeline.rs:85-92 (every offset), text.rs:571-577 (exact renders) · D: option-sweep, uncovered | ✓ |
| 37 | `pinyin_get_double_pinyin_auxiliary_text` | **PARTIAL** | D-only: scheme-diff per-cursor double_aux (A2d IDENTICAL); alloc-pairing leak/false-allocates; no Rust data assertion | ✓ |
| 38 | `pinyin_get_chewing_auxiliary_text` | COVERED | T: pipeline.rs:109-116 (every offset) · D: chewing-diff per-cursor (W1) | ✓ |
| 39 | `pinyin_mask_out` | COVERED | T: e2e:293,327-346 (false no store; true + export-empty + unigram_total) · D: train-diff | ✓ |
| 40 | `pinyin_remember_user_input` | COVERED | T: e2e:132-214 (token, delta, merge, 3 reject shapes) · D: train-diff, user round-trip | ✓ |
| 41 | `pinyin_begin_add_phrases` | COVERED | T: e2e:628-638 (NULL→NULL, non-null) · D: import-diff | ✓ |
| 42 | `pinyin_iterator_add_phrase` | COVERED | T: e2e:629-716 (count merge, bad pinyin, system index) · D: import-diff | ✓ |
| 43 | `pinyin_end_add_phrases` | COVERED (void) | T: e2e:687-736 (m_modified armed only at end) · leak gates | ✓ |
| 44 | `pinyin_begin_get_phrases` | COVERED | T: e2e:738-774, 839 · D: import/train/nbest/live-typing (W4) | ✓ |
| 45 | `pinyin_iterator_has_next_phrase` | COVERED | T: e2e:334,588,739-843 · D: same | ✓ |
| 46 | `pinyin_iterator_get_next_phrase` | COVERED | T: e2e:592-598,763-768 (exact rows, exhaustion NULLs) · D: same | ✓ |
| 47 | `pinyin_end_get_phrases` | COVERED (void) | leak gates · B literal | ✓ |
| 48 | `pinyin_begin_get_bigram_phrases` | COVERED | T: e2e:840,879 · D: train/nbest/live-typing (W4) | ✓ |
| 49 | `pinyin_bigram_iterator_has_next_phrase` | COVERED | T: e2e:606,844-887 · D: same | ✓ |
| 50 | `pinyin_bigram_iterator_get_next_phrase` | COVERED | T: e2e:610-616,881-884 (exact rows) · D: same | ✓ |
| 51 | `pinyin_end_get_bigram_phrases` | COVERED (void) | leak gates · B literal | ✓ |
| 52 | `pinyin_get_parsed_input_length` | COVERED | T: parse.rs:243-265 (fresh/parsed/consumed/reset/NULL) · D: every driver | ✓ |
| 53 | `pinyin_clear_constraint` | COVERED | T: e2e:1050-1240, guess_offset_tests:240-247,456-461 · D: live-typing cr: rows (W4) | ✓ |
| 54 | `pinyin_get_pinyin_key` | COVERED | T: cursor.rs:650-653,697-701 (false NULLs the out-param) · D: capture replay | ✓ |
| 55 | `pinyin_get_pinyin_string` | COVERED | T: keys.rs:115-436 (renders incl. zero-key guard) · D: key-surface | ✓ |
| 56 | `pinyin_get_pinyin_key_rest_length` | COVERED (thin) | T: cursor.rs:668-671 only (len == end−begin); no driver prints it | ✓ |
| 57 | `pinyin_get_pinyin_strings` | COVERED | T: keys.rs:397-427 (shengmu/yunmu, NULL out-param, zero-key guard) · D: key-surface | ✓ |
| 58 | `pinyin_get_zhuyin_string` | COVERED | T: keys.rs:121-442 · D: key-surface | ✓ |

Pre-probe tally: 56 COVERED, 2 PARTIAL (`pinyin_get_candidate_nbest_index`,
`pinyin_get_double_pinyin_auxiliary_text` — both differential-driver-only),
0 UNCOVERED, 0 setup-only. The union probe adds a single whole-surface
differential over all 58; after it runs IDENTICAL the two PARTIAL rows
gain a T-equivalent differential assertion of their out-params. The
earlier 38/8/3/7 tally (summing to 56) is superseded by this matrix —
the two symbols missing from it were `pinyin_in_chewing_keyboard` (15)
and the §1h name typo's real symbol `pinyin_get_pinyin_offset` (33).

## The first oracle run — STOP, unclassified

2026-09-16 UTC, `debian:testing` container `a15849ef7dd2` (image
`docker.io/library/debian@sha256:5056ab8a99336d6d71390d640f72229649f12e3d38e987cf6b24dc8675325d73`),
pin oracle prefix read-only, both sides tkrzw: the oracle on the pin's
own `lib/libpinyin/data`, the capi on a datagen tkrzw compile
(`model20-59c68e89`, libpinyin-native names) plus `interpolation2.text`.
Tree: this branch (oxpinyin at `main` + the W2 docs — #471's row-5b fix
is NOT in this tree). Both sides completed the whole walk (oracle 197
log lines, capi 203, no SKIP, no crash); the diff holds **5 hunks / 90
diverging lines → exit 2**. Logs:
`~/.local/share/oxpinyin-evidence/2026-09-16/w3/union-probe-{run,oracle,capi}.log`.

Identical surfaces worth recording: the import→export round-trip is
byte-equal (`你好|ni'hao|5`, `你好世界|ni'hao'shi'jie|9`,
`测试|ce'shi|3` on both sides, drained to `has_next=false`, and empty
again after `mask_out` on both sides); every accessor ret/out-param in
the key/offset/aux/cursor families; `choose(0,row0)=11`,
`offset(after-choose)=11`, `train(0)=true`, `sentence[nbest=0]=你好世界`,
`train(nbest=0)=true`, `remember(你好/1)=false`, `choose(ni,row0)=2`;
`remove_user=skipped(no-user-row)` on both sides (the imported phrases
never surfaced as user rows in any list, symmetrically).

The diverging hunks, **unclassified**:

1. `nihaoshijie` first list (after import+save, before any choose):
   oracle n=128 with 2 NBEST rows (nbest 0,2) and a rare-char NORMAL
   tail (疒祢禰妳呢堄逆猊); capi n=129 with 3 NBEST rows (0,1,2 — extra
   你好是届) and the common tail (你好你尼呢泥妮拟).
2. `xian` list: oracle n=757 with a LONGER(7) 现代 row at [2] and the
   rare tail (螅郗咥匚戯扢蔇); capi n=756 with the divided rows
   (西安西岸锡安) at [2..4] and the common tail (见线先仙贤). The
   clean-context option-sweep (A1, no import/save) is IDENTICAL at this
   word — the divergence is post-import/save state, not the word itself.
3. `aa`@ZRM list: oracle n=9 with LONGER(7) 阿尔 + 锕; capi n=8 without
   them. The same hunk carries the **row-5b out-of-enum rows**
   (`set(99)/set(-1)`: oracle true+cleared vs capi false+intact) —
   expected on this tree: the fix is #471, still open; this hunk must be
   re-taken after #471 merges.
4. `su` chewing list: oracle n=126 with LONGER(7) 你们 at [0]; capi
   n=125 without it.
5. bigram export after choose+train: capi carries
   `你好世界|ni'hao'shi'jie|138`; the oracle exports nothing (its
   `train(0)=true` wrote no user bigram for the whole-row choose —
   contrast the live-typing flow, where both sides export
   `你好|ni'hao|1242` IDENTICAL, W4).

Leads for the maintainer, not conclusions: hunks 1–4 share one shape —
after the user dictionary is written and saved, the pin surfaces
LONGER(7) rows and a rare-char NORMAL tail while the engine surfaces
extra NBEST/prefix rows and the common tail; the registered tail-tie
class (`sentence-surface.md` §3/§5: gfloat accumulation vs fixed-point,
comparator tie order) and live-typing's L2 window-anchor quote
(`你好/你/尼/呢/泥/妮, n=128/129`) are the nearest recorded text, but
neither was measured in this state (clean-context runners are all
IDENTICAL), so the run is recorded unclassified. Hunk 5 is a
train-record asymmetry on the whole-row choose flow. Per the measurement
rules: STOP — no fix lands without approval; the probe is committed so
the run is reproducible, and its non-vacuity is proven by construction
(it diverged on the unmodified tree).
