# Findings — probe coverage over the full exported pinyin ABI

Date: 2026-09-17 · Status: recorded. The union probe's oracle run
**diverges at every sort word measured**; the measured causes per word
and the four open residues are recorded below. Classification of the
residues is the maintainer's; none is fixed here.

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
