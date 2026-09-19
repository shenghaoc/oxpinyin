# Findings — probe coverage over the full exported pinyin ABI

Date: 2026-09-17 · Status: recorded; **amended 2026-09-19** with
classes for residues B/C/D (B/C registered as rows 33–34; D no ABI
divergence). Residue A is **one specific absent path** on the
`nihaoshijie` dump — the pin's rank-0 user-phrase tail, which
oxpinyin's language model never prices, an instance of the wider
refusal of every user-library token (characterised 2026-09-19; the
common-root experiment against B/C refutes a shared cause) —
registered as row 35, REVERT TARGET (ruled 2026-09-19). The union
probe's oracle run **diverges at every sort word measured**; the
measured causes per word and the residues are recorded below.

The §(e) rule (`docs/findings/compatibility-policy.md`) wants, for
every exported symbol, a probe that asserts its
whole observable surface: return status, out-params and the data they
point to, written lengths, and handle state. This document is the
coverage matrix the rule's consequence 3 calls for, the description of
the whole-surface probe (`tools/bisection/abi-probe-diff.c` +
`run-abi-probe-diff.sh`), and the record of its oracle runs.

## Name correction

The scope is every symbol in `crates/oxpinyin-capi/libpinyin.ver` — 79
`pinyin_*` exports, the same set as the pin's `src/libpinyin.ver`
(pin `074a2219`, blob `964d96e0`; verified identical 2026-09-17). The
per-symbol signatures and the export lists are
`docs/findings/abi-reference.md`; one historical note there records
that its source notes once mislisted `pinyin_get_offset` — the export
is `pinyin_get_pinyin_offset`.

## Classification rules

- **COVERED** requires a differential that asserts the symbol's whole
  observable surface — return status, out-params and the data they
  point to, written lengths, handle state — **and ran IDENTICAL at the
  pin** (the `debian:testing` measurements of 2026-09-16/17 named
  below). Rust tests are supporting evidence, never coverage on their
  own.
- **Void destructors** (`pinyin_fini`, `pinyin_free_instance`) are
  COVERED when both hold: the leak gates are clean (valgrind
  `run-bisect.sh` mode 2; the alloc-pairing LSan harness of
  `tools/abi/check-alloc-pairing.sh`), and every runner completes
  without a crash. Both hold for the runs recorded here
  (maintainer ruling 2026-09-17).
- A call made only as setup counts as **UNCOVERED**.
- A symbol covered only by `bisect.c` stays **PARTIAL** — bisect's
  differential mode cannot run today: `run-bisect.sh`, like
  `run-dynamic-adjust-diff.sh` and `run-train-diff.sh`, still resolves
  the capi data to the pre-split flat `fixtures/w3` root and exits 1
  (`missing file: … pinyin_index.bin` / `fatal: redb tables not
  found`) since the fixture moved to per-backend subdirectories.
  Three stale runners, reported, not fixed here (tools, outside this
  PR's purpose).
- A runner that skips or fails provides no coverage.
- The capture-replay suite (`pinyin-oracle` tests, 6 passing on the
  pin-built `.so`) replays recorded pin captures against the oracle —
  pin-side regression evidence, not an oxpinyin differential; it never
  counts as coverage.

## The probe

`abi-probe-diff.c` resolves all 79 exported symbols (refusing to run
when any is missing, so a completed run also proves the export surface)
and walks every symbol's observable surface in deterministic phases —
setup, import, full parse ×2, choose/nbest/train/remember, prediction,
double (incl. the row-5b out-of-enum probe), chewing, export → mask_out
→ export, the remaining exported surfaces, teardown — printing one `label=value` row
per observation: return status, out-param values AND the data they
point to (texts, packed keys, symbol strvs, begin/end/length), written
counts, handle state, and every iterator predicate result. Row 0 of
`pinyin_get_sentence` is queried only after a successful
`pinyin_guess_sentence`; higher indices only through an NBEST row's own
`nbest_index` value. The **sort word is the driver's third argument**
(`ABI_PROBE_SORT` in the runner, default 0x1e, printed into every
log): 0x1e is the parity word every other differential passes, 0x1c
and 0x14 are ibus-libpinyin's presets (0x1c the GSettings default),
0x1f the third preset, 0x16 fcitx-libpinyin's phrase-length word, 0
the raw word fcitx5-oxpinyin passes. `run-abi-probe-diff.sh` drives
the binary into oxpinyin-capi and the pin and diffs the logs
(0 = identical, 1 = build/run failure, 2 = divergence), env-gated on
the pin oracle and resolving its system dir through `system-dir.sh`.

Oracle caller contracts honoured, with the pin's assert landmines the
probe documents in-source (each found by crashing exactly once):

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
  USER_DICTIONARY token (`pinyin.cpp:3734,3738`) — its false path on a
  system row is unmeasurable against the oracle; the probe calls it
  only behind a true `pinyin_is_user_candidate`, ibus's own guard;
- `get_left/right_pinyin_offset` run a second `_check_offset` on the
  computed offset (`pinyin.cpp:3055/:3090`) that asserts for tail
  cursors (upstream fixed it after the pin, `95e3af7`) — the probe uses
  `uncovered-surface-diff.c`'s smoke-proved-safe cursor set.

## Matrix

Evidence codes: **D** = the differentials that ran IDENTICAL at the
pin and assert the row's surface — option-sweep (21 cases at 0x1e on
`main`; 24 with the row-17 cases on PR #472's tree), scheme-diff double
(PR #471's out-of-enum probe), chewing ×8 keyboards (PR #478's runs),
live-typing (317 lines, PR #479), uncovered surfaces (993 lines,
PR #479), key-surface (2131), import (16), nbest-train (56), predict
(5), punct (17), pred-order (1588), addon-candidate (4),
user-candidate (1), dict-surface (168), phrase-surface (19),
fullpin scheme 1 — all 2026-09-16/17 in the `debian:testing`
container named below. **T** = Rust tests on the oxpinyin ABI —
supporting evidence only. The union probe itself diverges (below), so
it confers no coverage; every symbol's class rests on the D set.

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
| 17 | `pinyin_guess_candidates` | **PARTIAL** | rets compared at 0x1e only; the sort-option input diverges at every other word measured (below) — the input dimension is uncovered | guess_offset_tests.rs:143-186 |
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
| 72 | `pinyin_set_full_pinyin_scheme` | COVERED | fullpin scheme 1 (run-scheme-diff.sh full): ret + scheme effects, IDENTICAL | — |
| 73 | `pinyin_token_get_phrase` | COVERED | dict-surface: per-token reads | — |
| 74 | `pinyin_token_get_n_pronunciation` | COVERED | dict-surface | — |
| 75 | `pinyin_token_get_nth_pronunciation` | COVERED | dict-surface | — |
| 76 | `pinyin_token_get_unigram_frequency` | **PARTIAL** | dict-surface: add-then-read flows IDENTICAL; the probe's fresh-context read of a system token's unigram diverges (residue D below) | — |
| 77 | `pinyin_token_add_unigram_frequency` | COVERED | dict-surface: overlay semantics | — |
| 78 | `pinyin_load_phrase_library` | COVERED | dict-surface: provisioned indexes + out-of-range false | — |
| 79 | `pinyin_unload_phrase_library` | COVERED | dict-surface: in-range sweep (false for the non-GBK defaults) | — |

Tally: **64 COVERED · 6 PARTIAL (rows 9, 10, 17, 26, 27, 76) · 9
UNCOVERED (rows 20, 29, 31, 32, 39, 40, 54, 56, 62) — sums to 79.**

## The oracle runs — measured per sort word

All runs: 2026-09-16/17 UTC, `debian:testing` container `a15849ef7dd2`
(image
`docker.io/library/debian@sha256:5056ab8a99336d6d71390d640f72229649f12e3d38e987cf6b24dc8675325d73`),
pin oracle prefix read-only, both sides tkrzw (oracle: the pin's own
`data/`; capi: a datagen tkrzw compile, native names +
`interpolation2.text`). Trees: `main` for the 0x1e-baseline
reproductions; a stack of `main` + PR #476 + PR #471 + PR #472 for the
per-word walks (the row-5b out-of-enum rows agree there by
construction). Both sides complete the whole walk at every word
(249–253 log lines, no SKIP, no crash). Logs:
`~/.local/share/oxpinyin-evidence/2026-09-17/w3/` (the six-word
full-ABI runs `abi-{oracle,capi}-<word>.log` and
`abi-sixwords-summary.txt`; the perturbed builds and reverted
baselines).

With the full-ABI extension (the `abi-extras` phase driving the 21
symbols above on a fresh context, honouring the pin's caller contracts —
full-pinyin schemes stay within 1..3, `unload_phrase_library` only on
in-range indexes with `load_phrase_library` on dict-surface's
smoke-proved provisioned set {1,2,4,7}: an unprovisioned index asserts
inside the pin, `pinyin.cpp:457`), every extras row except one is
IDENTICAL at every word; the exception is residue (D) below.

| sort word | verdict | shape |
|---|---|---|
| `0x1e` (parity; every runner's word) | both sides complete; 26 diverging lines | residues (A) the nbest-row set and (B) the train/bigram export, below |
| `0x1c` (ibus preset 1, the GSettings default) | 80 diverging lines | A + B + D plus the **longer-candidate hunks**: the pin prepends LONGER(7) rows (`现代`, `阿尔`, `你们`; `pinyin.cpp:2292-2293`) the capi never produces — the sort-option gap below |
| `0x14` (ibus preset 0) | 80 diverging lines | same shape; the pin's window differs from 0x1c's (without the 0x8 pinyin-length key `西`/`系` rise — measured), the capi's is unchanged |
| `0x0` (fcitx5-oxpinyin's word) | 78 diverging lines | same plus the unsorted rare-char tails (the keyless comparator returns 0 for every pair, `pinyin.cpp:1678-1709`) |
| `0x1f` (ibus preset 2) | 60 diverging lines | both sides honour the sentence suppression (the capi reads bit 0x1, `sentence.rs:286-287`); residue (C) below |
| `0x16` (fcitx's other word) | 36 diverging lines | A + the 0x8 window effect + B + D |

Non-vacuity, both surfaces: a perturbed capi build (sentence rows
force-suppressed — one line in `sentence.rs`) at 0x1e grows the diff
with the NBEST rows absent on the capi side, and a second perturbed
build (`pinyin_get_luoma_pinyin_string` forced to answer false) grows
the full-ABI walk from 26 to 30 diverging lines with the four luoma
rows flipped; each reverted, the baseline re-measures at the recorded
figure (`w3/perturbed-1e.log`, `w3/post-revert-1e.log`,
`w3/abi-perturbed-{capi,oracle}-1e.log`).

## The open residues (measured; classification is the maintainer's)

**A — the nbest result set: paths on the pin, texts on the capi.**
After the import (你好/5, 你好世界/9, 测试/3), parse `nihaoshijie`,
`pinyin_guess_sentence`, `pinyin_get_sentence` for every proved index:
the pin answers `你好世界 / 你好世界 / 你好时节` for indices 0/1/2 —
two distinct **paths** with the same text — while the capi answers
`你好世界 / 你好时节 / 你好是届` — three distinct **texts**. The pin's
candidate list then dedups the duplicate-text row away
(`_prepend_sentence_candidates` at `pinyin.cpp:1943-1948` emits one row
per nbest result; `_remove_duplicated_items_by_phrase_string` at
`:2298-2299` collapses the second 你好世界), so its list carries nbest
0 and 2 while the capi's carries 0, 1 and 2 — the 0x1e hunk. Logs:
`item4/{oracle,stack}-1e.log` (section A).

**B — the train record after a whole-row choose.** After
`choose(row 0)` + `train(0)`: the pin's user bigram export stays
**empty** — a whole-row choose trains nothing into the user bigram
(the phrase index keeps the imported counts unchanged) — while the
capi records `你好世界|ni'hao'shi'jie|138`. A second `train(0)` moves
the capi's count to 414 (the first adds 138, the second 276 — the
user bigram's update doubles on repeat), which is the arithmetic
behind the 138 (one train call) and 414 (two) figures across the
recorded runs; `pinyin_remember_user_input("你好",1)` returns false on
both sides and changes nothing. Identical on `main` and on the stack,
at 0x1e and at 0 — not sort-induced, not introduced by PR #471 or
#472. Logs: `item4/*.log` (section B).

**C — the imported phrase as a user row.** In a fresh window (parse,
then `pinyin_guess_candidates` without a preceding
`pinyin_guess_sentence`), **both** sides surface the imported 你好世界
as row 0 typed NORMAL with `pinyin_is_user_candidate` **true**, at
0x1f and at 0x1e, for `nihaoshijie` and for `nihaoshijiema` (where the
phrase covers only a prefix) — symmetric. The asymmetry appears only
after `pinyin_guess_sentence` + `pinyin_guess_candidates`: the pin's
0x1f window then still carries the user row while the capi's does not,
and the pin's 0x1e window carries sentence rows from the
still-live nbest results across the re-parse (instance state) where
the capi's window rebuilds from the fresh parse. Sequence-dependent,
separate from the sort-option gap. Logs: `item4/{oracle,stack}-1e.log`
(section C).

**D — a system token's raw unigram read.** On the fresh context, after
`pinyin_lookup_tokens("你好")` yields the phrase's system token,
`pinyin_token_get_unigram_frequency` answers **161** on the pin and
**1610** on the capi — a 10× unit gap on the raw read of a system
token (the dict-surface driver's add-then-read overlay flows are
IDENTICAL, so the gap is specific to reading a system token's stored
unigram field directly). Every other extras row — the single-key
parsers, `get_context`, `is_incomplete`, the luoma/secondary getters
under schemes 2 and 3, `phrase_segment`, `n_phrase`, `phrase_token`,
`sentence_with_prefix`, the plain predicted variant's ret, the rest of
the introspection family, the library load/unload pair — is IDENTICAL
at every word. Logs: `w3/abi-{oracle,capi}-1e.log`.

**Union-diff baseline.** The one-line `pred: type=4 text=你` divergence
(a predicted row the capi emits after the pin's 你好 under union-diff's
import/choose/train flow) reproduces on `main` + PR #476 **alone** —
measured 2026-09-17 in the same container
(`item3/union-diff-main-476.log`), eleven of the twelve same-data-dir
drivers IDENTICAL around it. It predates PR #471 and PR #472 and
belongs with the residues above.

## The sort-option gap (measured; register row drafted in the PR description)

`pinyin_guess_candidates`' `sort_option_t` input, per bit, pin vs
oxpinyin (source + the measured words above):

| bit | pin | oxpinyin |
|---|---|---|
| `0x1` SORT_WITHOUT_SENTENCE_CANDIDATE | set → sentence rows dropped (`pinyin.cpp:2295-2296`) | honoured (`sentence.rs:286-287`; measured at 0x1f: rows gone on both sides) |
| `0x2` SORT_WITHOUT_LONGER_CANDIDATE | clear → LONGER rows prepended (`:2292-2293`; measured: 现代/阿尔/你们 at 1c/14/0) | **ignored** — no longer-candidate code exists; the rows are never produced at any word |
| `0x4` SORT_BY_PHRASE_LENGTH | comparator key 1 (`:1683-1688`) | ignored; the engine's own order matched the pin at 0x1e in every probe window |
| `0x8` SORT_BY_PINYIN_LENGTH | comparator key 2 (`:1690-1695`) | ignored; measured: the pin's window membership changes without it (0x14/0x16 vs 0x1c), the capi's does not |
| `0x10` SORT_BY_FREQUENCY | comparator key 3 (`:1697-1702`) | ignored |

PR #481's parameterised sweep measures the corpus:
`OPTION_SWEEP_SORT=1e` passes 21/21; `0x1c` and `0x14` stop on every
case (the pin's TEXT sets carry the LONGER rows). Consumer
reachability, verified in the pinned consumers: ibus-libpinyin 1.16.5's
presets are 0x14, 0x1c and 0x1f (`PYPConfig.cc:225-230`) with 0x1c the
default (`:151`) — presets 0 and 1 both leave `0x2` clear, so
default-settings ibus users see LONGER rows on libpinyin that oxpinyin
never produces, and ibus maps them (`CANDIDATE_LONGER`/
`CANDIDATE_LONGER_USER`, `PYPLibPinyinCandidates.cc:56-62`).
fcitx-libpinyin 0.5.4 passes 0x16/0x1e (`enummap.cpp:159-166`) — both
suppress longer, not exposed. fcitx5-oxpinyin passes literal 0
(`src/oxpinyin.cpp:1282`, separate repository): against real libpinyin
that word yields an unsorted list with sentence AND longer rows; it
only works today because oxpinyin ignores the bits.

Also reported (code comment, no change made): `sentence.rs:258` cites
`pinyin.cpp:2292-2293` as the sentence-candidate gate; the sentence
gate is `:2295-2296` — `2292-2293` is the longer-candidate gate.

## Amendment — residue classification (2026-09-19 UTC)

Diagnosis recorded; classes ruled 2026-09-19 (A by the settling
measurement below, sharpened the same day to the one missing tail and
ruled REVERT TARGET as row 35; B/C/D by maintainer ruling). No
shipped-crate behaviour change in this amendment. Pin cites are from the checkout
at `074a2219` (blob tree read 2026-09-19). Mechanism probe:
`tools/bisection/residue-mechanism-diff.c` +
`run-residue-mechanism-diff.sh`. Measurement environment:
`debian:testing` container
`fd969584d8c60edcdfbc22fea20a0682aa4db90874ce35915b8b14866ed0e8a1`,
image
`docker.io/library/debian@sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9`,
pin oracle tkrzw at `/work/oracle/prefix` (pin_ref
`libpinyin-2.11.92-074a2219…+dbm-tkrzw`), durable logs
`~/.local/share/oxpinyin-evidence/2026-09-19/residue-classify/`
(same-dir phase B/D under `same-dir/`).

### B — whole-row choose + train (**REVERT TARGET**, register row 33)

**Mechanism.** A row-0 `NBEST_MATCH_CANDIDATE` choose runs
`diff_result(best, best)` and installs no `CONSTRAINT_ONESTEP` —
pin `pinyin.cpp:2515-2520` / `phonetic_lookup.cpp:172-205`; oxpinyin
`constraint.rs:193-214` and `selection.rs:221-241` (comments there
already state the empty-store law). Pin `pinyin_train` always delegates
to `train_result3` (`pinyin.cpp:2670-2690`;
`phonetic_lookup.h:844-936`), which trains a phrase only when
`train_next || constraint.m_type == CONSTRAINT_ONESTEP` (`:866`). With
no OneStep cells the loop writes nothing to the user bigram. Oxpinyin's
`Session::train` (`selection.rs:298-338`) takes that constrained walk
only when some `last_result` span sits on a OneStep cell (`:314-318`);
otherwise it falls through to the selection-history record that the
row choose still filled (`:197-218`, `:333-337`). That fallback is the
divergence — not a different notion of "constrained run", and not a
deliberate class-(a)/(b)/(c) exception: it is a missing guard that
"no OneStep ⇒ observe nothing", matching `train_result3`.

**Re-measured (same-dir, 2026-09-19).** After import 你好世界 / parse
`nihaoshijie` / `guess_sentence` / choose row 0:
`clear_constraint(0)=false` on both sides (no OneStep). Pin bigram
export stays empty through two `train(0)` calls. Capi records
`你好世界|ni'hao'shi'jie|138` after the first train and `|414` after
the second — the export's stored-count×2 rendering of seeds 69 then
138 (`run-train-diff.sh` header; `user-store.md` §2.1). Logs:
`same-dir/{oracle,capi}.log`.

**User-visible consequence.** After N sessions of the same whole-row
accept + train (the ibus shape that commits the 1-best sentence
candidate), the pin's user bigram is unchanged for that pair; oxpinyin
accumulates `sentence_start → 你好世界` (exported counts 138, 414,
1242, …). DYNAMIC_ADJUST / prediction then boosts that phrase on
oxpinyin and not on the pin. This is the only residue that corrupts
stored user state and compounds with use.

**Class.** **REVERT TARGET** — missing "no OneStep ⇒ observe nothing"
guard; not (a)/(b)/(c). Registered as compatibility-policy row 33;
work order `revert-plan.md` §10.

**Fix shape (do not implement).** Drop the history fallback whenever
no OneStep cell is present — `Session::train` observes nothing, as
`train_result3` does. Pre-registered differential: phase B of
`residue-mechanism-diff` must print empty bigram rows on both sides
after `train(0)` and `train(0)again` — **and** (amended 2026-09-19,
from the common-root experiment under A) the user 你好世界 token's
unigram must stay 27 on both sides after both trains: an A fix alone
moves the fallback's pair to `sentence_start → 你好世界`, which the
export never renders, so the export line goes empty while the fallback
still writes. `residue-a-tail-diff` phase B prints both observables.

### A — the missing user-phrase tail (**REVERT TARGET**, register row 35)

**Amended 2026-09-19 UTC (characterisation).** The earlier text below
the alignment table stands as the record of the settling measurement;
what follows names the one tail it left unnamed.

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
`--apply-patches tools/bisection/patches/nbest-tails-dump` (an
env-gated `stderr` dump inside `get_nbest_match`; its stdout is
byte-identical to the unpatched pin's — the runner checks), oxpinyin
rows from `crates/oxpinyin-runtime/examples/nbest_tail_probe` (nats =
`cost / 1000 · ln 2`). Runner:
`tools/bisection/run-residue-a-tail-diff.sh` with
`RESIDUE_A_SAME_DIR=1 RESIDUE_A_DUMP_PREFIX=… RESIDUE_A_PROBE=1`.

| rank | pin tail: `m_poss`, `m_last_step`, result tokens | ox row: text, cost, nats |
|---|---|---|
| 0 | 你好世界 −14.8274994, `last_step=0`, `{0: 0x07000002}` | 你好世界 28341 = 19.644484 (`0x01006205`@0 + `0x01007a03`@5) |
| 1 | 你好世界 −19.6447334, `last_step=5`, `{0: 0x01006205, 5: 0x01007a03}` | 你好时节 35656 = 24.714856 |
| 2 | 你好时节 −24.7148857, `last_step=5`, `{0: 0x01006205, 5: 0x01007a09}` | 你好是届 35822 = 24.829918 |

ox[0] ≡ pin[1] (`|Δ| = 0.00025` nats) and ox[1] ≡ pin[2]
(`|Δ| = 0.00003` nats): oxpinyin's list is the pin's shifted up by one.
Exactly one tail — the pin's rank 0 — is absent, and the third row is
backfilled from below (你好是届; the pin's dump prints its three tails
only, so that this path sits fourth in the pin's own heap is inferred
from the aligned costs, not measured). The residual `|Δ|` of the two aligned pairs is
row 11's float band; it is not this residue.

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
the phrase into the phrase table, the pinyin table (`:597-599`, so
`search_matrix` spells it) **and** the phrase index with
`add_unigram_frequency(token, count × unigram_factor 3)` (`:604-605`,
so `get_phrase_item` prices it) — an imported phrase is a first-class
trellis token. Both sides agree on that state: `lookup_tokens` answers
`0x07000002` with unigram 27 (and `0x07000001` 你好 with 15) on the pin
and on oxpinyin alike; the facade total is 51051882 on both.

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
(`crates/oxpinyin-data/src/lm/mod.rs:532-548`): its first gate
(`:540-543`) destructures `self.unigram_count(token)` and returns the
default when it is `None`, and `unigram_count` (`:340`;
`PhraseLibraries::unigram_count`, `phrase_libraries.rs:179`) reads the
*system* chunk libraries only, where a nibble-7 token owns no item. The
user delta the same function merges one line later (`:545`; the store
carries 27 for this token and the total already includes it) is never
reached. The gate's comment says it stands in for "no installed unigram
table"; on a runtime with real unigrams it fires for exactly the tokens
the pin prices from a user-file sub-index — every USER_DICTIONARY (7)
and NETWORK_DICTIONARY (6) token. Not a seed never expanded, not a span
the expander skips, not a dictionary miss: the language model refuses
to price a token that has no system item.

Probe output (baseline, `04-baseline/ox-probe.log`):

| entry | lib | unigram_freq | `step_costs(sentence_start → token)` |
|---|---|---|---|
| `0x01006205` 你好 | 1 | 161 | unigram 18816 (13.042 nats), blended 17543 (12.160 nats) |
| `0x07000001` 你好 | 7 | 15 | **None / None** |
| `0x07000002` 你好世界 | 7 | 27 | **None / None** |

**What consumes the path rather than the text — measured.** Same-dir
ABI probe (`residue-a-tail-diff.c`, phases X/B/D):

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
row 35 (maintainer ruling 2026-09-19); work order `revert-plan.md`
§12, executing first. At its true width: every user-library token is
refused an n-best step cost, so no imported or learned phrase can
enter a sentence path; the `nihaoshijie` tail is one instance. The
pin's behaviour is reproducible: the missing
step's price is `log(27 / 51051882 · 0.6873010)`, a basic-ops ratio the
fixed-point scale already reproduces for system tokens (the
counterfactual build below lands it at 14.827804 nats against the
pin's 14.8274994 — `|Δ| = 0.0003`, row 11's band). No language
mechanism is involved and no blocker was found: the step-cost seam
already holds the user delta it needs. Not (a) — the 4.8-nat gap is an
absent step, not accumulation. Not (b), not (c).

**Fix shape (do not implement).** In
`nbest_step_costs_with_user_delta`, price a token whose library owns
no system item from its user delta alone — `count = 0 +
user.unigram_delta` — for the user-file libraries (nibbles 6 and 7)
and the promoted addon nibble, keeping `None` for a token of an
unloaded (masked) library as `get_phrase_item` failing does;
`unigram_total` already carries the user delta. Expected oxpinyin
rows: 你好世界 at `surprisal(0.6873010 × 27, 51051882)` = 21392
millibits (14.8278 nats), 你好世界 (the system pair, 19.644), 你好时节
(24.715) — the candidate windows then dedup the second to `n = 128`.

**Pre-registered differential.** `run-residue-a-tail-diff.sh` same-dir
on the pin's `data/`: phase A IDENTICAL — `sentence[0..2]` = 你好世界 /
你好世界 / 你好时节, `A-1e:n=128`, NBEST ranks 0 and 2; phase X
IDENTICAL — `clear_constraint(0)=true`, the 你好时节 row at nbest
index 2, unigram 你好 161 → 644 after the train; phase D `n=303`. The
runtime probe must print `step_costs(sentence_start → 0x07000002):
unigram=Some(21392)` and rows 14.828 / 24.715 nats with
`sentence_text(1)` the duplicate text. Phases B and C are **not** part
of A's gate: the experiment below shows B's export line going empty
under an A fix while B's defect stands, so a B gate must read the
user token's unigram, not the export.

**Side observations (not A, B or C; measured, not registered).** (i)
`pinyin_bigram_iterator_get_next_phrase` on the last export row answers
`false` on the pin (it returns `has_next_phrase`, `pinyin.cpp:910`)
and `true` on oxpinyin (row fetched) — an ABI return-value divergence
the union probe could not see while the pin's export was empty in
every probed state. (ii) After a whole-composition NBEST choose and
re-guess, `guess_candidates(0, 0x1e)` answers `n = 1` (the n-best row
only) on oxpinyin and `n = 128` on the pin (phase X2). Both need their
own rows; neither is opened here.

**Earlier text (2026-09-19, the settling measurement), kept as
recorded.**

**Mechanism.** The pin's k-best keeps up to three trellis *tails* with
no text dedup inside the search (`phonetic_lookup.h:736-836`,
`get_tails` `:330-340`); `pinyin_get_sentence` converts each
`MatchResult` by index (`pinyin.cpp:1464-1481`), so two paths can
share a display string. Candidate-list dedup
(`_remove_duplicated_items_by_phrase_string`, `:2298-2299`) is a later,
separate pass. Oxpinyin likewise does not text-dedup inside the
trellis (`nbest.rs:301-333`, `:508-535`; span expand only skips
duplicate *tokens* on the same span, `:601-622`). Candidate-list
`prepend_nbest_rows` + `dedup_by_text_keep_first`
(`lookup.rs:558-580`) is the same late pass as the pin's. The ABI
probe's `nihaoshijie` gap (pin two paths / one text vs oxpinyin three
texts) is survivor selection under that search, not search-time text
dedup.

**Which defect.** Survivor selection — **not** search-time text dedup.
The settling dump below is a wide score gap, so this is not the
float/`gfloat` unresolved band. (Sharpened above: the survivor the pin
selects and oxpinyin lacks is one specific tail, and it is absent from
the trellis rather than out-selected — the step that would create it
is never priced.)

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
sort (`trellis_value_compare`, `phonetic_lookup.h:174-178`) truncates
the float possibility difference to `gint`, so two tails with
`|Δm_poss| < 1.0` compare equal and keep heap-pop order. That
**1.0 nat** threshold is what the pin's own ordering of survivors can
resolve. A near-tie on an input where the two sides agree does not
explain a different input where they do not.

**`tuihui` vs `sentence-surface.md`.** The §12 example (oracle
`[退回, 退回, 退会]` vs port `[退回, 退会]`) is the
`sentence_tail` / `sentence_surface_parity` configuration: exported
model20 tables (`PINYIN_EXPORT_DIR`) against
`fixtures/w4/oracle-sentence-surface.txt` (fixture line: `tuihui`
sentences `退回 / 退回 / 退会`). On P6 native pin `data/` (this
same-dir dump) the port emits all three texts, matching the pin. Not
a stale record and not a search-time text-dedup: two table
configurations, two survivor sets. The claim that oxpinyin never
places the duplicate-text path among the survivors is false on pin
`data/`; it remains a possible outcome of `sentence_tail` over the
exported tables, where the frozen gate still holds 6 distinct-same.

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
rank-2 你好是届. Margin `|24.829918302 − 19.6447334| = 5.185` nats,
far above the 1.0 nat `gint` band. Pin's own `|m_poss[1] − m_poss[2]|
= 5.070` nats is the same wide gap. Ox rank-0 nats `19.644` matches
pin `|m_poss[1]|`, not pin `|m_poss[0]| = 14.827` (the
`last_step=0` user-phrase tail). Re-measured 2026-09-19 in container
`7cfeefdaf53f` with identical figures (the alignment table at the top
of this section).

**(a) does not hold.** The gap is structural, not float accumulation
inside the unresolved band. Classification is the maintainer's — no
new register row here.

**Corpus-tier coverage.** `sentence_tail` /
`sentence_surface_parity` gate the ordered `get_sentence` lists and
the distinct-same bucket on exported tables. They do not cover this
import-boosted ABI-probe shape.

**User-visible consequence.** After an import, the pin's 1-best for
the imported phrase's pinyin is the imported phrase itself — one
token — and its rank-1 the same text assembled from system phrases;
oxpinyin's 1-best is only the assembled text, with a third, different
string in the list. Same display string at rank 0; different token
path underneath: a row choose constrains fewer phrases, a train after a
non-best choose touches fewer unigrams, and every candidate window
carries one sentence row more.

### A — common-root experiment against B and C (2026-09-19 UTC)

**Question.** A (the user-phrase tail never enters the trellis), B
(whole-row choose trains through the history fallback) and C (the
imported NORMAL leaves the window after `guess_sentence`) all turn on
one imported user phrase. One defect with three symptoms, or three
defects?

**Design.** Neutralise A's mechanism alone and re-measure B and C in
the same state. The counterfactual is a scratch build of
oxpinyin-capi with
`tools/bisection/patches/residue-a-counterfactual/nbest-step-costs-user-token.patch`
applied — the first gate of `nbest_step_costs_with_user_delta`
replaced by `unwrap_or(0)`, so a token without a system item is priced
from its user delta — and nothing else changed. Never part of the
shipped tree: applied with `patch -p1` in the container, built into
`target/cf`, run, reverted (`patch -R`); the worktree carries no
counterfactual line. Both builds are driven through the same ABI
probe (`residue-a-tail-diff.c`, phases A/C/B/X/D) and the same
runtime probe, same-dir on the pin's `data/`, against the same pin.
The B observable is widened beyond the bigram export on purpose:
the export skips `sentence_start` pairs on both sides
(`pinyin.cpp:804`, `iterators.rs:323`), and the user token's unigram
is read before and after each train.

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
patch reverted). Baseline: `04-baseline/`. Command:
`RESIDUE_A_CAPI_SO=target/cf/debug/libpinyin_capi.so
RESIDUE_A_SAME_DIR=1 RESIDUE_A_OUT=… run-residue-a-tail-diff.sh`, then
`target/cf/debug/examples/nbest_tail_probe <data> <tmp>`.

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

**Verdict: refuted — no shared root.** The counterfactual closes A and
every consequence that flows from the absent path (the choose
constraints, the post-choose train, the window counts, the offset-5
window), and leaves C byte-for-byte where it was. B's *export symptom*
disappears exactly as pre-registered for the three-defects outcome —
because the fallback's trained pair moved from 你好→世界 to
`sentence_start → 你好世界`, which no export renders — while the
fallback itself keeps writing (user unigram 27 → 510 → 1476; the pin's
stays 27). Three independent defects: A is the language-model gate on
user-file tokens (`lm/mod.rs:540-543`); B is the history fallback in
`Session::train` (`selection.rs:333-337`); C is the n-best lifetime
across parse plus the NBEST-wins dedup ahead of the `0x1f` filter
(`instance.rs:145-155`, `state.rs:170-172`, `lookup.rs:558-580`).

**Which of rows 33/34 and A a single A fix would close.** A only.
Row 33 (B) survives, with its symptom moved: after an A fix the
bigram export reads empty on both sides, so **§10's pre-registered
differential ("empty bigram rows on both sides after `train(0)` and
`train(0)again`") passes without the defect being fixed** — the gate
must also read the user 你好世界 token's unigram (27 on both sides
after both trains), which `residue-a-tail-diff` phase B prints. Row 34
(C) survives unchanged: its `0x1f` probe has the same outcome with or
without A.

**Consequence for the work order (`revert-plan.md`).** Ruled
2026-09-19: A executes first (§12), then 10, 11, 9 — a one-line
ordering fix inside an invariant the same function already documents
for the bigram path, blocking a whole feature rather than perturbing
presentation, and B's corrected gate is easier to write once the
unigram observable is live. The order is safe exactly because of the
finding above: §10's gate reads the unigram, never the old export-only
line, once A has landed; §10 cross-references this section.

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

**Isolation.** dict-surface's add-then-read flows remain IDENTICAL.
The original probe opened the oracle on `ORACLE_DATA` and the capi on
`resolve_system_dir` — different trees. Same-dir re-measure
(2026-09-19): both `.so` files on the pin's `data/` answer
`token_unigram=true/161` for token `0x01006205` / 你好
(`same-dir/{oracle,capi}.log`). The 1610 figure is therefore a
harness/data mismatch from the split-dir walk, not a reader scale bug.

**User-visible consequence.** None on a shared system directory. A
consumer that pointed each library at different compiled tables would
see different raw unigrams by construction.

**Class.** **no ABI divergence** (harness/data). Settled by the
same-dir probe. If a future same-dir run ever regenerates the 10× gap,
re-open as an OPEN DEFECT in whoever writes the chunk field — not in
the ABI getter.

### C — imported phrase as a user row (**REVERT TARGET**, register row 34)

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

**User-visible consequence.** Sequence-dependent: after a sentence
guess, ibus-style `0x1f` still offers the imported user phrase on the
pin and may not on oxpinyin; after re-parse + `0x1e` the pin can still
show live sentence rows and oxpinyin rebuilds phrase-only.

**Class.** **REVERT TARGET** — nbest lifetime and candidate-rebuild
mismatch; not (a)/(b)/(c). Registered as compatibility-policy row 34;
work order `revert-plan.md` §11.

**Fix shape (do not implement).** (1) Keep nbest across parse; clear
only on full reset / a new `guess_sentence`, matching the pin.
(2) When `SORT_WITHOUT_SENTENCE` is set, rebuild the phrase window
without the prior NBEST-wins dedup (or rebuild from scratch every
`guess_candidates` as the pin does). Pre-registered differential:
import 你好世界 → parse → `guess_sentence` →
`guess_candidates(0, 0x1f)` → assert NORMAL + `is_user` on both;
then re-parse → `guess_candidates(0, 0x1e)` → assert nbest rows still
present on both.

### Stale runners — would they have caught B or D?

| Runner | Would have caught B? | Would have caught D? |
|---|---|---|
| `run-train-diff.sh` | **No.** Drives NORMAL chooses of 你 then 好 (installs OneStep); `train_result3` trains on both sides. Never exercises whole-row NBEST choose. | **No.** No `token_get_unigram_frequency` of a fresh system token. |
| `run-bisect.sh` | **Unreliable.** Chooses candidate[0] after guess and trains, but does not export the user bigram; a whole-row empty-train would look like success (`train: true`). | **No.** |
| `run-dynamic-adjust-diff.sh` | **No.** Compares post-choose candidate windows under DYNAMIC_ADJUST, not the bigram export after a constraint-free train. | **No.** |

Repairing the three runners' pre-split `fixtures/w3` paths remains
useful for their own gates; it is **not** higher priority for B or D
than the residue-mechanism probe (B) or same-dir dict reads (D) —
neither residue rides on those three scripts' intended surfaces.

### ROADMAP Stage 1 line

Applied 2026-09-19. `ROADMAP.md` Stage 1 now records the corpus claim
together with the open ABI/user-store residues (B/C as rows 33–34,
register row 32) under the frozen sentence-trellis exception (A /
row 11).
