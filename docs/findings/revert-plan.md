# Revert plan — the incompatible divergences

Date: 2026-08-28 · Status: **work order** · Branch:
`claude/pr5-revert-incompatible-divergences` (#209; the work order
merged as a document — the reverts landed as their own PRs).

**Status at `87f25055` (2026-09-06), amended 2026-09-19 for rows 33–34
and again for rows 35–37:**

| # | Register | Disposition |
| --- | --- | --- |
| 1 | #12 predicted-candidate tie order | superseded by P6 (345af16d): on KC/tkrzw the runtime walks the pin's own DBM and `pred-order-diff` is IDENTICAL; text-ascending stays the defined order on redb/LMDB |
| 2 | #13 mid-syllable offset | closed — the pin's empty-column law reproduced (C2 residue, 2026-08-29) |
| 3 | #17 literal `0x0` gating | **closed in code** (2026-09-16): the pin's `USE_DIVIDED_TABLE`/`USE_RESPLIT_TABLE` gating is ported; `run-option-sweep.sh` with the new all-off (`0x0`) and divided-contrast (`0x8`/`0x88`) cases ran IDENTICAL on all 24 cases inside `debian:testing` (2026-09-16) |
| 4 | #7 `validate_constraint` | closed as equivalent on model20 (4c2fe02b) |
| 5 | #8 constraints across re-parse | closed (#217) |
| 6 | #9 n-best row-choose cursor | closed (eca8d43b) |
| 7 | #15 apostrophe-only consumption | closed (678f3259, 2026-08-26 — predates this plan; the register entry was not updated until 2026-09-06) |
| 8 | #5b double out-of-enum scheme setter | closed in code (2026-09-15) — half-mutation reproduced: CAPI returns `true`, fallback cleared, shengmu/yunmu intact; contract test pinned |
| 9 | #32 sort-option input of `pinyin_guess_candidates` | **closed in code** (2026-09-20): the whole sort word reaches the engine (`Session::set_sort_options`); bit `0x2` clear prepends a LONGER row and the three `SORT_BY_*` keys order the list (§9); the choose-a-LONGER-row flow trains `+483` unigram and answers cursor 1, IDENTICAL |
| 10 | #33 whole-row NBEST choose + train | **open** — registered 2026-09-19 (probe residue B): history fallback trains when no OneStep is present; pin `train_result3` writes nothing (§10) |
| 11 | #34 imported user phrase after `guess_sentence` | **open** — registered 2026-09-19 (probe residue C): nbest cleared on parse; NBEST-wins dedup before `SORT_WITHOUT_SENTENCE` (§11) |
| 12 | #35 user-library tokens refused an n-best step cost | **closed in code** (2026-09-19): the presence gate mirrors the pin's `get_phrase_item` over the loaded sub-index — user-file tokens (nibbles 5/6/7) priced from their user delta alone, masked libraries and missing items still refused; phase A/X/D IDENTICAL, the runtime probe's `Some(21392)` measured (§12) |
| 13 | #36 bigram export iterator's last-row return value | **open** — registered 2026-09-19 (probe side observation (i)): the pin returns `has_next_phrase` after advancing, oxpinyin returns `true` for every fetched row (§13); independent of the sequence |
| 14 | #37 candidate window behind the composition offset | **open** — registered 2026-09-19 (probe residue E): the C ABI serves the composition-anchored cached list for any lookup offset at or behind a choose; an empty list at `(0, 0x1f)` after a whole-composition choose (§14); executes second |

The sections below are the 2026-08-28 text, kept as the record of what
each revert had to prove, plus section 8 for the target the original
list omitted, section 9 for row 32 (2026-09-18), sections 10–11 for
rows 33–34 (2026-09-19), section 12 for row 35, section 13 for row 36
and section 14 for row 37 (2026-09-19).

Driven by the classification table in
`docs/findings/compatibility-policy.md`. Every entry that table marks
**REVERT TARGET** is listed here with its site, the pin's behaviour, the
probe that has to flip from "recorded divergence" to "must be
IDENTICAL", and what currently blocks it.

## Why nothing is reverted in this commit

Every one of these is an externally observable behaviour change whose
acceptance gate is a differential against the pinned oracle, and the
oracle cannot be provisioned in the environment this was prepared in:
`tools/oracle/build-oracle.sh` fetches SHA-pinned archives from
`codeload.github.com`, which returns 403 under this session's egress
policy. (The `model20` archive from SourceForge *is* reachable; the two
source tarballs are not.) Substituting a git checkout would produce a
build that is not the pin, and recording pins measured against it would
be worse than not measuring.

The standing gate is *frozen pins bit-identical throughout*. Landing
seven unverified behaviour changes against that gate is the one outcome
worse than landing none, so this PR is the work order and the reverts
land with the measurements.

## The targets

### 1 — Predicted-candidate tie order (register #12)

- **Site:** `crates/oxpinyin-capi/src/predict.rs` (`guess_predicted`,
  `append_predicted_prefix`), `oxpinyin-data/src/dict.rs:196-217`
  (`suggest_after`), `oxpinyin-user/src/lookup.rs` (`suggest_after`).
- **Now:** a defined text-ascending order, by maintainer decision
  (2026-08-25), asserted in-tree by the capi e2e test
  `predicted_tie_groups_are_text_ascending_including_user_rows`.
- **Target:** the pin's store-iteration order — **and it is not the order
  the register measured.** That entry recorded the Tkrzw HashDBM bucket
  walk. Kyoto Cabinet is the reference backend (what distros ship), and
  the pin's order is KC's physical hash walk. It has to be established
  experimentally on a real file first; nothing about the Tkrzw
  measurement carries over.
- **Probe:** `tools/bisection/pred-order-diff.c` via
  `run-pred-order-diff.sh`, from a recorded-drift constant (177/178 on
  好, 1557/1571 across eight prefixes) to zero. The e2e test's predicate
  inverts with it.
- **Blocked on:** PR 4 Phase 2 (a working BDB path) *and* the oracle.
  This is the last of the seven that can move.

### 2 — Mid-syllable candidate-lookup offset (register #13)

- **Site:** `Session::candidates_at` → `Session::scan_window`
  (`oxpinyin-engine/src/session.rs`), reached through
  `pinyin_guess_candidates`.
- **Now:** rebuilds the window from the raw byte suffix `&raw[offset..]`,
  so a mid-syllable offset re-parses the tail — measured on `nihao`:
  offset 3 → `n=106`, offset 4 → `n=6`.
- **Target:** the pin anchors `start = offset` in the whole-composition
  `PhoneticKeyMatrix` (`pinyin.cpp:2224-2262`); an empty mid-syllable
  column matches nothing, so only the prepended n-best row survives —
  `n=1` at offsets 1, 3 and 4.
- **Probe:** the guess-seam differential at unsnapped offsets. Note the
  syllable-aligned offsets already agree bit-for-bit (`nihao` at 0/2/5,
  `n=126`/`94`/`1`), so the revert must not perturb them.
- **Blocked on:** the oracle. The revert itself is structural — it needs
  a persisted whole-composition matrix with empty columns, which the
  engine does not currently model.

### 3 — Literal `0x0` option gating (register #17)

- **Site:** the empty-guess fallback (`jv`/`zon`) and the divided-table
  inventory (`xian`/`fanan`/`fangan`/`tian`).
- **Now (measured 2026-09-16, both sides byte-equal):** at `0x0` (and
  at `0x2`) `jv` and `zon` return `guess=false, n=0` and `xian` returns
  `n=337` with `cand[0]=县` (no 西安); with `USE_DIVIDED_TABLE` set
  (`0x82`, `0x88`, `0x188`, `0x18a`) `xian` returns `n=756` with
  `cand[0]=西安`. The divided-table and resplit inventories are gated by
  `USE_DIVIDED_TABLE`/`USE_RESPLIT_TABLE` on both sides alike.
- **Target:** the pin's gating at a literal `0x0` option word.
- **Probe:** `run-option-sweep.sh` extended with the `all-off` (`0x0`)
  and divided-contrast (`0x8` vs `0x88`) cases ran 2026-09-16 inside
  `debian:testing`: 24/24 PASS — parse/aux identical, top-10
  TEXT/ORDER identical on every case — exit 0, no SKIP line. Per-word
  full driver logs (the sweep itself does not diff the `n=` lines)
  captured at `0x0`, `0x2`, `0x8`, `0x82`, `0x88`, `0x188`, `0x18a`
  are byte-identical between the pin and the capi, the `n=` inventory
  lines included.
- **Open question (moot):** class (d) was retired 2026-09-06. The
  consumer-unreachability argument no longer applies, so the question
  of whether (d) covers consumer-unreachable *inputs* is moot. The
  gating is ported unconditionally.

### 4 — `validate_constraint`'s drop test (register #7)

- **Site:** `span_finds_token` in the constraint validator.
- **Now:** drops a forcing when the span search no longer yields the
  forced token.
- **Target:** `compute_pronunciation_possibility(...) < FLT_EPSILON`
  (`phonetic_lookup.cpp:161-164`).
- **The work is real, not a flag flip.** The pin's function
  (`phonetic_key_matrix.cpp:534-600`) is a recursive **sum over every
  path** of `PhraseItem::get_pronunciation_possibility`, where oxpinyin's
  §3 model takes the first path per token. The revert has to port the
  all-paths sum. It is bit-reproducible — `gfloat` add and a frequency
  ratio, no transcendental — which is why the entry is not class (a).
- **Probe:** the constraint/train differentials on edits that leave a
  span marginally spellable.

### 5 — Constraints across a selection-committed re-parse (register #8)

- **Site:** `Session::parse_continues` and the reset-on-divergence rule
  (`session.rs:1432`).
- **Now:** two shapes start fresh — a composition a selection consumed,
  and a divergent buffer.
- **Target:** upstream's constraints are instance state surviving every
  re-parse; only `pinyin_reset` clears them (`pinyin.cpp:1497-1533`,
  `:2697`).
- **Probe:** the live-typing differential extended past a
  selection-consumed composition without an intervening reset. The
  backspace ladder is already measured identical and must stay so.
- **Watch:** the #141 cursor flows' pinned tests encode the current
  behaviour and will need re-basing with the revert, not around it.

### 6 — N-best row-choose cursor (register #9)

- **Site:** `crates/oxpinyin-capi/src/candidates.rs:353` — "the
  candidate's absolute end".
- **Now:** the row candidate's absolute end.
- **Target:** `matrix.size() - 1` unconditionally
  (`pinyin.cpp:2511-2519`), whatever span the row covered.
- **Probe:** the n-best choose surface on a degenerate row — the mini
  fixture's single-phrase row is the only known constructor; no
  real-table surface distinguishes them.
- **Smallest of the seven,** and the one whose revert is a two-line
  change. Its comment block at the site argues the current behaviour
  from the ibus commit branch; that argument needs answering in the
  revert, because the pin's value is what the branch actually sees.

### 7 — Apostrophe-only input consumption (register #15)

- **Site:** `SegmentGraph` (`oxpinyin-core/src/graph.rs`) — a leading
  apostrophe run is consumed only as propagation toward a following key.
- **Now:** `'` → 0, `''` → 0, `'''` → 0.
- **Target:** the pin emits a zero `ChewingKey` per separator and counts
  it: `'` → 1, `''` → 2, `'''` → 3 (measured, `docs/testing/oracle-apostrophe-abort.md`
  F-E-14).
- **Probe:** `pinyin_parse_more_full_pinyins` return and
  `pinyin_get_parsed_input_length` on apostrophe-only input.
- **Do not revert past the class-(c) boundary:** the cursor helpers'
  `false` at the `_check_offset` abort shapes stays. Only the parse
  length moves. Class B2 of `uncovered-surface-differentials.md` inherits
  this entry.

### 8 — Double-pinyin scheme setter, out-of-enum value (register #5b)

- **Site:** `pinyin_set_double_pinyin_scheme`'s Rust wrapper
  (`oxpinyin-capi`, the scheme setters; pinned by
  `tests/abi/contract.rs::double_out_of_enum_reproduces_the_half_mutation`).
- **Now:** an out-of-enum value (negatives, 0, 7–29, 31+) answers `true` (the
  upstream wrapper's lie) and clears the fallback table, reproducing
  the pin's half-mutation; the shengmu/yunmu tables stay intact. A
  following parse that would have used the fallback (the `aa` probe)
  confirms the cleared state is observable.
- **Target:** the pin clears `m_fallback_table` first
  (`pinyin_parser2.cpp:580`), the parser returns `false`, and the
  wrapper ignores it and answers `true` (`pinyin.cpp:1154–1159`) — a
  ZRM/PYJJ/XHE scheme silently loses its fallback while the caller is
  told the call succeeded. Not an abort, so not class (c): the policy's
  own boundary case (the "(c) covers aborts" paragraph), reproducible.
- **Probe:** the contract test's `aa` parse after the out-of-enum set
  pins the cleared fallback as observable; the
  `tools/bisection/run-scheme-diff.sh` oracle differential (out-of-enum
  99 and −1 under ZRM, asserting the setter's return AND the following
  fallback-dependent parse) ran 2026-09-16 in a debian:testing
  container: the probe rows are byte-identical on both sides — setter
  answers `true` throughout, baseline `aa` consumed=2/n=8, after 99 and
  after −1 consumed=0, guess false, n=0, restored ZRM consumed=2/n=8 —
  and the whole-log diff is IDENTICAL (exit 0, no SKIP lines).
- **Closed** 2026-09-15; live differential run 2026-09-16.

### 9 — Sort-option input of `pinyin_guess_candidates` (register #32)

- **Site:** `pinyin_guess_candidates`'s sort-option word
  (`crates/oxpinyin-capi/src/sentence.rs:286-287` reads only
  `SORT_WITHOUT_SENTENCE_CANDIDATE`; the three sort keys and the
  longer-candidate gate reach no code).
- **Now:** bits `0x2` (`SORT_WITHOUT_LONGER_CANDIDATE`), `0x4`
  (`SORT_BY_PHRASE_LENGTH`), `0x8` (`SORT_BY_PINYIN_LENGTH`) and `0x10`
  (`SORT_BY_FREQUENCY`) are ignored, and no longer-candidate row is
  ever produced: the engine's own candidate order answers at every
  word. Measured 2026-09-17 in a `debian:testing` container (pin oracle
  read-only, both sides tkrzw): the ABI probe diverges at
  `0x1c`/`0x14`/`0x0`/`0x1f`/`0x16` — the pin prepends LONGER rows
  (`pinyin.cpp:2292-2293`) and orders by the three keys
  (`:1678-1709`), so its windows move where the capi's do not — with a
  26-line residue at `0x1e` that is the separate residues, not this
  row; the parameterised option-sweep (`8bc31196`) stops on all 21
  cases at `0x1c` and `0x14` and passes 21/21 at `0x1e`.
- **Target:** the pin's law for the whole word — sentence candidates
  gated on `0x1` (`:2295-2296`), longer candidates prepended when
  `0x2` is clear with the LONGER/LONGER_USER typing ibus maps
  (`PYPLibPinyinCandidates.cc:56-62`), and the list ordered by the
  three enabled keys. Consumer-reachable through ibus-libpinyin's
  presets 0 (`0x14`) and 1 (`0x1c`, the GSettings default,
  `PYPConfig.cc:151`); fcitx passes `0x16`/`0x1e` (not exposed);
  fcitx5-oxpinyin passes literal 0 (`src/oxpinyin.cpp:1282`, separate
  repository).
- **Probe:** the option-sweep at `0x1c` and `0x14` must flip from STOP
  on every case to PASS; the ABI probe at `0x1c`/`0x14` must drop to
  the `0x1e` residue set; and a choose-a-LONGER-row flow (surface a
  LONGER row, `pinyin_choose_candidate` it, assert the cursor, then
  `pinyin_train` with the user stores dumped on both sides) must run
  IDENTICAL — the special-candidate unigram training whose call site
  `crates/oxpinyin-capi/src/candidates.rs:295-296` records as
  unreachable today becomes reachable with the port and must be measured.
- **Closed:** all three probes pass (2026-09-20, host measurement:
  oracle `~/.local/opt/pinyin-oracle` read-only, both sides tkrzw
  over `/home/sheng/matrix-x86/data-tkrzw`, worktree branch
  `feat/guess-candidates-sort-options`). The parameterised sweep
  (`OPTION_SWEEP_SORT`) passes 24/24 at `0x1e`, `0x1c` and `0x14`
  (0x1e identical to the recorded baseline; 1c/14 flip from
  STOP-on-every-case). The ABI probe (`ABI_PROBE_SORT`) is
  byte-identical at `0x1c` and `0x14` — and at `0x1e` (0 diverging
  lines at every word; `comm` of the sorted ± sets empty both
  directions), re-measured 2026-09-20 on top of #493 (main
  `3c21641f`): the pre-#493 26-line `0x1e` residue was A + B + X2 +
  D, and #493's closure of A (register row 35, `7c9a6923`) took the
  B/X2 export lines and the extras `token_unigram` line with it —
  the extras context reuses the probe's user dir, so the old `1610`
  read was B's trained overlay, now unwritten; rows 33/36's
  dispositions stay with their own differentials. The LONGER-choose phase
  (`tools/bisection/abi-probe-diff.c`, runs before choose-train so its
  user-bigram dump measures this flow alone) prints on both sides:
  row `方面` at index 2 type 7, `choose(longer)=1`,
  `train(after-longer)=true` twice, unigram before/after
  `23253/23736` — the trained `+483` read back through the phrase
  index on BOTH sides — and bigram rows 0. (The first §9 record said
  `0/0` with a dropped-write explanation; that was an artifact of the
  probe's unsequenced `printf` — the read call wrote `f` in one
  argument while another read it, so both sides printed the pre-call
  zero. Sequenced 2026-09-20 per the CodeRabbit finding and
  re-measured: identical `23253 → 23736` on both sides, the direct
  `+483` observable.) The `0x1f` user-row shape stays §11 (row 34).
  The facade's train gate widened to the pin's own disjunction
  (user store present AND (a live sentence result OR a recorded
  selection) — `pinyin.cpp:2674-2675` reads `results.size()`, and the
  compressed Rust e2e path records the selection the pin's
  `train_result3` would walk); the zhuyin train-no-selection test
  flipped to the pin's law with it.
- **Landed** with PR #496 (2026-09-20) — no ask was needed. The
  `0x1f` user-row shape is register #34 / §11, separate from this row.

### 10 — Whole-row NBEST choose + train (register #33)

- **Site:** `Session::train` history fallback
  (`crates/oxpinyin-engine/src/session/selection.rs:298-338`,
  especially `:314-318` and `:333-337`); reached after a row-0
  `NBEST_MATCH_CANDIDATE` choose that installs no `CONSTRAINT_ONESTEP`
  (`constraint.rs:193-214`, `selection.rs:221-241`).
- **Now:** a whole-row choose fills the selection-history record; with
  no OneStep cell, `Session::train` falls through to that history and
  seeds `sentence_start → phrase` (same-dir 2026-09-19: exported
  counts 138 then 414 for 你好世界 / `ni'hao'shi'jie`).
- **Target:** pin `train_result3` (`phonetic_lookup.h:844-936`) trains
  a phrase only when `train_next` or
  `constraint.m_type == CONSTRAINT_ONESTEP` (`:866`); a
  constraint-free whole-row train writes nothing to the user bigram.
  Drop the history fallback whenever no OneStep cell is present —
  `Session::train` observes nothing.
- **Probe:** phase B of `tools/bisection/residue-mechanism-diff.c` /
  `run-residue-mechanism-diff.sh` must print empty bigram rows on both
  sides after `train(0)` and `train(0)again` (same-dir on the pin's
  `data/`) — **and** the user 你好世界 token's unigram must stay 27 on
  both sides after both trains (phase B of
  `tools/bisection/residue-a-tail-diff.c` prints it). The export line
  alone is not a gate once §12 has landed: the common-root experiment
  (`probe-coverage-abi.md`, "A — common-root experiment") showed that
  an A fix alone moves the fallback's trained pair to
  `sentence_start → 你好世界`, which no export renders, so the old gate
  passes with the fallback still writing. Do not re-run the old gate
  after §12; read the unigram.
- **Blocked on:** nothing — the port is unstarted work. Executes after
  §12 and before §11: this residue corrupts stored user state and
  compounds with use.

### 11 — Imported user phrase after `guess_sentence` (register #34)

- **Site:** nbest lifetime across parse
  (`crates/oxpinyin-facade/src/instance.rs:145-194` —
  `reset_parse_state` → `reset_composition` → `sentence.reset()` at
  `session/state.rs:170-172`) and candidate rebuild
  (`oxpinyin-engine/src/session/guess.rs:45-103`;
  `lookup.rs:558-580` NBEST-wins dedup; `sentence.rs:264-389`
  `SORT_WITHOUT_SENTENCE` filter).
- **Now:** every `begin_parse` clears nbest; `guess_sentence` then
  `prepend_nbest_rows` + `dedup_by_text_keep_first` drops the
  same-text user NORMAL from the session cache, so at `0x1f` the
  imported user row is gone with the sentences; after re-parse at
  `0x1e` the nbest rows themselves are gone.
- **Target:** pin keeps `m_nbest_results` across parse (cleared only by
  `pinyin_reset`, `pinyin.cpp:2693-2704` vs parse at `:1497-1524`) and
  rebuilds candidates from scratch each `guess_candidates`
  (`:2184-2300`); at `0x1f` the NBEST prepend is skipped
  (`:2295-2296`) and the user NORMAL remains. (1) Keep nbest across
  parse; clear only on full reset / a new `guess_sentence`. (2) When
  `SORT_WITHOUT_SENTENCE` is set, rebuild the phrase window without the
  prior NBEST-wins dedup (or rebuild from scratch every
  `guess_candidates`).
- **Probe:** import 你好世界 → parse → `guess_sentence` →
  `guess_candidates(0, 0x1f)` → assert NORMAL + `is_user` on both;
  then re-parse → `guess_candidates(0, 0x1e)` → assert nbest rows
  still present on both (phase C of `residue-mechanism-diff`, or the
  ABI probe at those words).
- **Blocked on:** nothing — the port is unstarted work. Sequence-
  dependent presentation only; executes after §10.

### 12 — User-library tokens refused an n-best step cost (register #35)

- **Status:** **closed in code** (2026-09-19) — the port landed on
  `fix/nbest-step-cost-user-token`.
- **Site:** `BigramLanguageModel::nbest_step_costs_with_user_delta`
  (`crates/oxpinyin-data/src/lm/mod.rs`): the first gate destructured
  `self.unigram_count(token)` — the system chunk libraries only
  (`PhraseLibraries::unigram_count`, `phrase_libraries.rs`) — and
  returned the default when it was `None`, before the user delta
  merged.
- **Was:** every USER_DICTIONARY (7) / NETWORK_DICTIONARY (6) token
  answered `NbestStepCosts { unigram: None, blended: None }`, so
  `crate::nbest::expand_entry` (`oxpinyin-engine/src/nbest.rs:677`)
  pushed nothing for it: no imported or learned phrase could enter a
  sentence path. Measured same-dir on the pin's `data/` 2026-09-19
  (`probe-coverage-abi.md` A): after importing 你好世界/9, the pin's
  rank-0 tail for `nihaoshijie` was that single token
  (`m_poss = −14.8274994`, `last_step = 0`); oxpinyin's n-best was the
  pin's shifted up by one, `step_costs(sentence_start → 0x07000002)`
  `None/None`, every `0x1e` window one row longer (129 vs 128).
- **Landed fix:** the gate now mirrors the pin's
  `unigram_gen_next_step` presence test (`get_phrase_item` over the
  token's *loaded* sub-index, `phonetic_lookup.h:643-668`):
  `unigram_count`'s `None` is split three ways — a token of one of the
  default facade's three `USER_FILE` sub-indexes (the promoted addon
  nibble 5, network 6, user 7; `novel_types.h:159-161`) passes with
  `count = 0` and prices from `user.unigram_delta` alone (the
  frequency field the import or choose-promotion wrote,
  `pinyin.cpp:604-605`), while a masked (unloaded) library's token and
  a loaded library's missing item keep the default answer, exactly as
  the pin's failing `get_phrase_item` does. `unigram_total` already
  carries the user delta. A one-line ordering fix inside the invariant
  the same function already documents for the bigram path ("the bigram
  merge happens *before* the count > 0 presence gate").
- **Probe:** `tools/bisection/run-residue-a-tail-diff.sh` same-dir on
  the pin's `data/`: phase A IDENTICAL — `sentence[0..2]` = 你好世界 /
  你好世界 / 你好时节, `A-1e:n=128`, NBEST ranks 0 and 2; phase X
  IDENTICAL — `clear_constraint(0)=true`, the 你好时节 row at nbest
  index 2, unigram 你好 161 → 644 after the train; phase D `n=303`;
  the runtime probe (`crates/oxpinyin-runtime/examples/nbest_tail_probe`)
  prints `step_costs(sentence_start → 0x07000002): unigram=Some(21392)`
  and rows 14.828 / 24.715 nats with `sentence_text(1)` the duplicate
  text. **Ran IDENTICAL 2026-09-19 UTC** on phases A, X and D
  (container `debian@sha256:dab11cdb…`, pin oracle tkrzw; the diff's
  residue is exactly B/C/X2/E — rows 33, 34, 36, 37 — no A/X/D line
  remains): `A-1e:n=128` with ranks 0 and 2, `A:sentence[0..2]` the
  pin's, `X:clear_constraint(0)=true` with 你好时节 at nbest index 2
  and 你好 161 → 644 after the train, `D-5:n_cand n=303`; the probe
  prints `unigram=Some(21392 = 14.827804 nats)`, rows
  `21392 = 14.827804487` / `35656 = 24.714855870` nats,
  `sentence_text(1)` the duplicate text. Phases B and C are not this
  row's gate and remain divergent (rows 33–34).
- **Blocked on:** nothing — closed. Executed first (see the order
  below).

### 13 — Bigram export iterator's last-row return value (register #36)

- **Site:** `pinyin_bigram_iterator_get_next_phrase`
  (`crates/oxpinyin-capi/src/iterators.rs:373-410`): returns `true`
  whenever a row was fetched, `false` only once exhausted.
- **Now:** on the last exported row the capi answers `true` where the
  pin answers `false`. Measured 2026-09-19 (`residue-a-tail-diff`
  phase X2, one bigram row exported on both sides):
  `bigram[0]=false` on the pin, `true` on oxpinyin.
- **Target:** the pin fills the out-params, advances, and returns
  `pinyin_bigram_iterator_has_next_phrase(iter)` (`pinyin.cpp:896-911`)
  — whether another row follows. The unigram export iterator is out of
  scope: the pin's `pinyin_iterator_get_next_phrase` returns `true` on
  every row (`:698-769`), as oxpinyin's does. Return
  `handle.index < handle.rows.len()` after the increment.
- **Probe:** phase X2 of `run-residue-a-tail-diff.sh` — the
  `X2-train1:bigram[0]` line identical — plus a two-row export (import
  two pairs, train each) asserting `true` then `false` on both sides,
  which the ABI probe's export phase gains when this lands.
- **Blocked on:** nothing — a two-line change, independent of the
  12 → 10 → 11 → 9 sequence; lands whenever. Consumer note: ibus's
  `check_result` wrapper (`PYLibPinyin.cc:321`) asserts on the value in
  non-`NDEBUG` builds, so a debug ibus that completes an export on
  oxpinyin today aborts on the pin's last row — reproducing the pin
  reproduces that too; the release build discards the value.

### 14 — Candidate window behind the composition offset (register #37)

- **Site:** the C ABI's `pinyin_guess_candidates` re-anchor rule
  (`crates/oxpinyin-capi/src/sentence.rs:319-339`): a normalized
  lookup offset strictly past the composition offset builds
  `Session::candidates_at(normalized)`; one at or below it is served
  the composition-anchored cached list (`session/lookup.rs:32-41`
  `refresh` → `scan_window(anchor = consumed)`, `:102-110`), which the
  choose advanced (`session/selection.rs:229,252`).
- **Now (measured 2026-09-19, `probe-coverage-abi.md` E):** after a
  whole-composition NBEST choose and re-guess, `guess_candidates(0,
  0x1f)` answers **0 rows** where the pin answers 127 headed by the
  imported user phrase, and `(0, 0x1e)` the n-best rows alone where
  the pin answers 128; after an ordinary partial choose (你好, cursor
  5), `(0, 0x1e)`/`(0, 0x1f)` answer the offset-5 list (304/301) where
  the pin answers the offset-0 one (129/127). At the choose's own
  offset both sides agree. Consumer routes: ibus-libpinyin under
  preset 2 forces `lookup_cursor = 0` and calls `guess_candidates(0,
  0x1f)` after every partial choose (`PYPPhoneticEditor.cc:352-355`);
  `moveCursorLeft` (`:595-604`) reaches an offset behind a choose
  under every preset.
- **Target:** the pin rebuilds the window from `start = offset` over
  the whole-composition matrix on every call (`pinyin.cpp:2184-2262`)
  and keeps no composition offset. Display leg: re-anchor whenever
  `normalized != composition_offset()`, in both directions, through
  `Session::candidates_at(normalized)` — the target behaviour already
  exists there (`session/lookup.rs:283-312`; `scan_window` is a pure
  function of `(raw, anchor)` that touches neither the cached list nor
  the composition state), so the display leg is plausibly a routing
  change at `sentence.rs:319-339`, not decoder work, and the cache
  needs no rebuild. **Scoping caveat, stated rather than forced:** the
  choose leg is not a routing change. A choose from a behind-window
  reaches `select_anchored` → `select_inner`, which refuses a span
  starting before the composition offset
  (`SelectionAnchorBeforeComposition`, `session/selection.rs:157-167`)
  by a documented session-API decision; the pin's `add_constraint`
  clears the overlapped forcings and the cursor moves back
  (`phonetic_lookup.cpp:61-86`), so parity there needs the selection
  record to regress — consumed offset, selected text, token history and
  overlapping forcings truncated to the new span — a contract change
  to the record, small but not routing. The display leg alone closes
  every row of the measured table; the choose leg is to be scoped with
  its own probe (phase E3) when this section is executed.
- **Probe:** phase E of `tools/bisection/residue-a-tail-diff.c` /
  `run-residue-a-tail-diff.sh`, same-dir on the pin's `data/`, must
  match the pin at **every row of the E1/E2 table** — `(0, 0x1e)`,
  `(0, 0x1f)`, `(5, 0x1e)` and `(11, 0x1e)` after the whole-composition
  choose; `(0, 0x1e)`, `(0, 0x1f)` and `(5, 0x1e)` after the partial
  choose — not only the offsets at or past the composition. §12 lands
  first, so the n-best count in those rows is the pin's by then and
  the whole phase runs IDENTICAL.
- **Blocked on:** nothing — the display leg is unstarted work.
  Executes second (see the order below).

## Order to execute

Closed sections stay historical (6, 7, 4, 5, 3, 2, 1, 8, 9). Among the
open targets: **12, then 14, then 10, then 11** (maintainer rulings
2026-09-19, when §9 was still open; §9 landed with PR #496). A (§12)
goes first: it is a one-line ordering fix
inside an invariant the same function already documents for the
bigram path, it blocks a whole feature — no imported or learned phrase
can enter a sentence path — rather than perturbing presentation, and
B's corrected gate (§10: the user token's unigram, not the export
line) is easier to write once the unigram observable is live. E (§14)
goes second, ahead of 10 and 11, on two grounds: it is
consumer-reachable on a default-adjacent preset's ordinary flow
(ibus preset 2 asks for offset 0 after every partial choose) and can
present an empty candidate list; and the target behaviour already
exists in `Session::candidates_at` (`session/lookup.rs:283-312`), so
the display leg is plausibly a routing change at
`sentence.rs:319-339` rather than new decoder work — the choose leg's
record regression is the one part that is not (§14's caveat). B
before C because B corrupts stored state while C is sequence-dependent
presentation. The order is safe because the common-root experiment
showed A's fix changes B's export symptom without touching B's defect:
§10 reads the unigram, never the old export-only gate, once §12 has
landed. Row 32 (§9) landed with PR #496; row 36 (§13) is independent
of the other four and may land in parallel once it is scheduled. Each lands with its own differential flipped to
IDENTICAL and the frozen pins re-measured, per the standing gate.
