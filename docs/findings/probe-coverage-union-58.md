# Findings — probe coverage over the 58-symbol consumer union

Date: 2026-09-16 · amended 2026-09-17 · Status: recorded; the union
probe's oracle run **diverges at every sort word measured** — the
residues are committed unclassified and the work is STOPped per the
measurement rules (no fix without approval). The 2026-09-16 lead (the
divergence was "post-import/save state") was wrong: the probe
hard-coded sort 0, and the sort word caused most of it (measured
below). The matrix was also reclassified 2026-09-17 under the
maintainer's stricter coverage rule.

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

## Classification rules (amended 2026-09-17, maintainer's ruling)

- **COVERED** requires a differential that asserts the symbol's whole
  observable surface — return status, out-params and the data they
  point to, written lengths, handle state — **and ran IDENTICAL at the
  pin in this session** (the debian:testing container below). Rust
  tests (T) and the leak gates are supporting evidence only, never
  coverage on their own. The 2026-09-16 draft counted T as coverage;
  that rule is withdrawn.
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
- The void destructors (`pinyin_fini`, `pinyin_free_instance`): whether
  the leak gates may count as their coverage is **pending the
  maintainer's ruling** — marked PENDING RULING below, not COVERED.
- The capture-replay suite (`pinyin-oracle` tests) replays recorded pin
  captures against the pin-built `.so` — pin-side regression evidence,
  not an oxpinyin differential; its citations are withdrawn.

## The new probe

`union-probe-diff.c` resolves exactly the 58 union symbols (refusing to
run when any is missing, so a completed run also proves the export
surface) and walks every symbol's observable surface in deterministic
phases — setup, import, full parse ×2, choose/nbest/train/remember,
prediction, double (incl. the row-5b out-of-enum probe), chewing,
export → mask_out → export, teardown — printing one `label=value` row
per observation: return status, out-param values AND the data they
point to (texts, packed keys, symbol strvs, begin/end/length), written
counts, and handle state. The **sort word is the driver's third
argument** (`UNION_PROBE_SORT` in the runner, default 0x1e, printed
into every log): the first run hard-coded 0, and that word — not
user-store state — caused most of its divergence. `run-union-probe-diff.sh`
drives it into oxpinyin-capi and the pin and diffs the logs (0 =
identical, 1 = build/run failure, 2 = divergence), env-gated on the pin
oracle and resolving its system dir through `system-dir.sh`.

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

Evidence codes: **D** = the differentials that ran IDENTICAL at the pin
this session (2026-09-16/17, the container below) and assert the row's
surface — option-sweep (21 cases at 0x1e on main + 24 cases on #472's
tree), scheme-diff double, chewing ×8 keyboards, live-typing (317
lines), uncovered surfaces (993), key-surface (2131), import (16),
nbest-train (56), predict (5), punct (17), pred-order (1588),
addon-candidate (4), user-candidate (1). **T** = Rust tests on the
oxpinyin ABI — supporting evidence only. Post-probe class: the union
probe diverges at every sort word measured, so it confers **no**
coverage — every symbol's post-probe class equals its class here.

| # | Symbol | Class | D | T |
|---|--------|-------|---|---|
| 1 | `pinyin_init` | COVERED | every runner (non-NULL or the run dies) | context.rs:183,214 |
| 2 | `pinyin_fini` | **PENDING RULING** (void; leak gates only) | — | — |
| 3 | `pinyin_alloc_instance` | COVERED | every runner (non-NULL) | common.rs:56 |
| 4 | `pinyin_free_instance` | **PENDING RULING** (void; leak gates only) | — | — |
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
| 20 | `pinyin_get_character_offset` | UNCOVERED | nothing this session (bisect stale; the probe prints it but its run diverges) | sentence.rs:441-499 |
| 21 | `pinyin_get_n_candidate` | COVERED | n= rows in every runner above | e2e:1657-1722 |
| 22 | `pinyin_get_candidate` | COVERED | candidate rows in every runner above | exact_scheme.rs:32-35 |
| 23 | `pinyin_get_candidate_string` | COVERED | texts in every runner above | exact_scheme.rs:34-42 |
| 24 | `pinyin_get_candidate_type` | COVERED | type rows in pred-order/uncovered/live/nbest/dict-surface | phrase.rs:191-201 |
| 25 | `pinyin_get_candidate_nbest_index` | COVERED | nbest= rows in nbest-train + live-typing + uncovered, all IDENTICAL | — (none on the oxpinyin ABI) |
| 26 | `pinyin_is_user_candidate` | PARTIAL | user-candidate-diff (1 line): the gated user row; false path implicit; union-diff's is_user rows sit in a DIVERGENT run (below) | e2e:262 |
| 27 | `pinyin_remove_user_candidate` | **PARTIAL** | no differential; true path pin assert-fenced (pinyin.cpp:3734,3738) | e2e:356-358 (false path) |
| 28 | `pinyin_choose_candidate` | COVERED | live-typing cursor= rows; uncovered deep/tail-choose cursors | e2e + guess_offset_tests |
| 29 | `pinyin_choose_predicted_candidate` | UNCOVERED | nothing this session (bisect stale) | e2e:229-287 |
| 30 | `pinyin_train` | COVERED | live-typing train= + the 你好\|ni'hao\|1242 export matched; nbest-train rows | e2e:79-115 |
| 31 | `pinyin_get_pinyin_key_rest` | UNCOVERED | nothing this session (bisect stale; capture replay is pin-side) | cursor.rs:656-704 |
| 32 | `pinyin_get_pinyin_key_rest_positions` | UNCOVERED | same | cursor.rs:662-665 |
| 33 | `pinyin_get_pinyin_offset` | COVERED | uncovered cursor table: off= per byte cursor | cursor.rs:437-530 |
| 34 | `pinyin_get_left_pinyin_offset` | COVERED | uncovered left probes (safe cursors) | cursor.rs:452-536 |
| 35 | `pinyin_get_right_pinyin_offset` | COVERED | uncovered right probes (safe cursors; tail assert = pin landmine, upstream 95e3af7) | cursor.rs:457-543 |
| 36 | `pinyin_get_full_pinyin_auxiliary_text` | COVERED | option-sweep aux rows; uncovered | pipeline.rs:85-92, text.rs:571-577 |
| 37 | `pinyin_get_double_pinyin_auxiliary_text` | COVERED | scheme-diff per-cursor double_aux (the A2d run) | — |
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
| 54 | `pinyin_get_pinyin_key` | UNCOVERED | nothing this session (capture replay is pin-side; bisect stale) | cursor.rs:650-701 |
| 55 | `pinyin_get_pinyin_string` | COVERED | key-surface render rows | keys.rs:115-436 |
| 56 | `pinyin_get_pinyin_key_rest_length` | UNCOVERED | no driver prints it, stale or current | cursor.rs:668-671 |
| 57 | `pinyin_get_pinyin_strings` | COVERED | key-surface | keys.rs:397-427 |
| 58 | `pinyin_get_zhuyin_string` | COVERED | key-surface | keys.rs:121-442 |

Tally: **43 COVERED · 5 PARTIAL (rows 9, 10, 17, 26, 27) · 2 PENDING
RULING (rows 2, 4) · 8 UNCOVERED (rows 20, 29, 31, 32, 39, 40, 54,
56) — sums to 58.** The 2026-09-16 draft's "56 COVERED" tally is
superseded: it counted Rust tests as coverage, cited runners that had
not run in the session (key-surface/import/train/capture among them —
now re-run or dropped as above), and missed `pinyin_in_chewing_keyboard`
plus §1h's `pinyin_get_offset`→`pinyin_get_pinyin_offset` name typo.

Three runners could not be re-run and their citations are dropped:
`run-bisect.sh`, `run-dynamic-adjust-diff.sh` and `run-train-diff.sh`
all drive the capi against the pre-split flat `fixtures/w3` root and
exit 1 since the per-backend fixture split — stale, reported. One
runner of the re-run set diverged: **union-diff**, by one line (a
`pred: type=4 text=你` row the capi emits after the pin's `你好` under
its import/choose/train flow — same user-store-state family as the
probe's residues below, unclassified); the other eleven same-data-dir
drivers re-ran IDENTICAL.

## The oracle runs — measured per sort word (STOP)

All runs: 2026-09-17 UTC, `debian:testing` container `a15849ef7dd2`
(image
`docker.io/library/debian@sha256:5056ab8a99336d6d71390d640f72229649f12e3d38e987cf6b24dc8675325d73`),
pin oracle prefix read-only, both sides tkrzw (oracle: the pin's own
`data/`; capi: a datagen tkrzw compile, native names +
`interpolation2.text`). The 2026-09-16 run was on `main`; every run
below is on a throwaway stack of **main + #476 + #471 + #472** (the
probe carried as uncommitted files), so the row-5b out-of-enum rows
agree by construction. Both sides complete the whole walk at every
word (197–199 log lines, no SKIP, no crash). Logs:
`~/.local/share/oxpinyin-evidence/2026-09-17/w3/{sort-runs,sort-runs-extra,perturbed-1e,post-revert-1e}.log`.

| sort word | verdict | shape |
|---|---|---|
| `0x1e` (parity; every runner's word) | exit 2 — **21 lines, 2 hunks** | (A) the nbest-row set: the pin's list carries 2 sentence rows (nbest 0, 2 — it prepends ALL nbest rows then dedups by phrase string, `pinyin.cpp:1943-1948` + `:2298-2299`), the capi's carries 3 (nbest 0, 1, 2, incl. an extra 你好是届) — the sides' nbest result sets differ; (B) the train/bigram export: the capi lands `你好世界\|ni'hao'shi'jie\|414`, the pin nothing |
| `0x1c` (ibus preset 1, the default) | exit 2 — 75 lines, 6 hunks | A + B plus the **longer-candidate hunks**: the pin prepends LONGER(7) rows (`现代`, `阿尔`, `你们`; `pinyin.cpp:2292-2293`) the capi never produces — the sort-option gap below |
| `0x14` (ibus preset 0) | exit 2 — 75 lines, 6 hunks | same shape; the pin's window differs from 0x1c's (without the 0x8 pinyin-length key `西`/`系` rise — measured), the capi's is unchanged |
| `0x0` (the first run's word; fcitx5-oxpinyin's) | exit 2 — 73 lines, 6 hunks | same plus the unsorted rare-char tails (the keyless comparator returns 0 for every pair, `pinyin.cpp:1678-1709`) |
| `0x1f` (ibus preset 2) | exit 2 — 52 lines, 4 hunks | both sides honour the sentence suppression (the capi reads bit 0x1, `sentence.rs:286-287`); the pin surfaces the imported user phrase as an `is_user=true` NORMAL row (你好世界 at [0]) — the capi surfaces no user row; counts 127 vs 126 |
| `0x16` (fcitx's other word) | exit 2 — 31 lines, 3 hunks | A + the 0x8 window effect + B |

**Measured cause of the 2026-09-16 record:** the sort word. The
pre-registered expectations held in part — at 0x1e the LONGER-row and
unsorted-tail hunks (the old hunks 2–4 and the rare tails inside hunk
1) vanish, and they return at 0x1c/0x14/0x0 exactly as the
longer-candidate gate predicts; the old "post-import/save state"
argument is withdrawn (it compared a 0x1e runner with a 0 run). What
does NOT vanish at 0x1e: residue (A) — the nbest-row-set gap, the
nearest registered neighbour being the sentence-surface §3/§5
residuals (different decode tie-handling), though its direction (capi
carrying the extra row) was not previously measured — and residue (B),
which persists at **every** word measured and is not sort-induced. The
0x1f user-row surfacing is a third, separate shape. All three stay
**unclassified**; no fix is included or implied. Non-vacuity: a
perturbed capi build (sentence rows force-suppressed — one line in
`sentence.rs`) at 0x1e grows the diff to 56 lines with the NBEST rows
absent on the capi side; reverted, the baseline re-measures at 21
(`perturbed-1e.log`, `post-revert-1e.log`).

## The sort-option gap (measured; register row DRAFTED, not committed)

`pinyin_guess_candidates`' `sort_option_t` input, per bit, pin vs
oxpinyin (source + the measured words above):

| bit | pin | oxpinyin |
|---|---|---|
| `0x1` SORT_WITHOUT_SENTENCE_CANDIDATE | set → sentence rows dropped (`pinyin.cpp:2295-2296`) | honoured (`sentence.rs:286-287`; measured at 0x1f: rows gone on both sides) |
| `0x2` SORT_WITHOUT_LONGER_CANDIDATE | clear → LONGER rows prepended (`:2292-2293`; measured: 现代/阿尔/你们 at 1c/14/0) | **ignored** — no longer-candidate code exists; the rows are never produced at any word |
| `0x4` SORT_BY_PHRASE_LENGTH | comparator key 1 (`:1683-1688`) | ignored; the engine's own order matched the pin at 0x1e in every probe window |
| `0x8` SORT_BY_PINYIN_LENGTH | comparator key 2 (`:1690-1695`) | ignored; measured: the pin's window membership changes without it (0x14/0x16 vs 0x1c), the capi's does not |
| `0x10` SORT_BY_FREQUENCY | comparator key 3 (`:1697-1702`) | ignored |

The `test/sort-option-sweep` branch parameterises `option-sweep.c`
(`OPTION_SWEEP_SORT`) and measures the corpus: **0x1e PASS 21/21;
0x1c and 0x14 STOP on every case** (the pin's TEXT sets carry the
LONGER rows). Consumer reachability, verified in the pinned consumers:
ibus-libpinyin 1.16.5's presets are 0x14, 0x1c and 0x1f
(`PYPConfig.cc:225-230`) with **0x1c the default** (`:151`) — presets
0 and 1 both leave `0x2` clear, so default-settings ibus users see
LONGER rows on libpinyin that oxpinyin never produces, and ibus maps
them (`CANDIDATE_LONGER`/`CANDIDATE_LONGER_USER`,
`PYPLibPinyinCandidates.cc:56-62`). fcitx-libpinyin 0.5.4 passes
0x16/0x1e (`enummap.cpp:159-166`) — both suppress longer, not exposed.
fcitx5-oxpinyin passes literal 0 (`src/oxpinyin.cpp:1282`, separate
repo, report-only): against real libpinyin that word yields an
unsorted list with sentence AND longer rows; it only "works" today
because oxpinyin ignores the bits.

Register row **drafted for the maintainer's decision** (not committed):
> **Sort-option input of `pinyin_guess_candidates`** — oxpinyin
> honours only `SORT_WITHOUT_SENTENCE_CANDIDATE` (0x1,
> `sentence.rs:286-287`); `SORT_WITHOUT_LONGER_CANDIDATE` (0x2) and the
> three sort keys (0x4/0x8/0x10) are ignored, and no longer-candidate
> row is ever produced. The pin gates both preprends on the word
> (`pinyin.cpp:2292-2296`) and orders by the keys (`:1678-1709`).
> Consumer-reachable through ibus-libpinyin's presets 0 (0x14) and 1
> (0x1c, the GSettings default); ibus maps the LONGER types
> (`PYPLibPinyinCandidates.cc:56-62`). Measured 2026-09-17: union
> probe exit 2 at 0x1c/0x14/0x0/0x1f/0x16 (LONGER rows, 0x8 window
> order, user-row surfacing), 21-line residue at 0x1e; option-sweep
> STOP on all 21 cases at 0x1c and 0x14. Proposed class: defect to
> close (no (a)-(c) class fits); the fix shape is a maintainer
> decision (port the longer-candidate production and the sort keys, or
> register a scoped divergence).

Also reported (code comment, no change made): `sentence.rs:258` cites
`pinyin.cpp:2292-2293` as the sentence-candidate gate; the sentence
gate is `:2295-2296` — `2292-2293` is the longer-candidate gate.
