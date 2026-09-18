# Revert plan — the incompatible divergences

Date: 2026-08-28 · Status: **work order** · Branch:
`claude/pr5-revert-incompatible-divergences` (#209; the work order
merged as a document — the reverts landed as their own PRs).

**Status at `87f25055` (2026-09-06):**

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
| 9 | #32 sort-option input of `pinyin_guess_candidates` | **open** — registered 2026-09-18: bits 0x2/0x4/0x8/0x10 ignored, no longer-candidate row produced; the port owes the pin's longer-candidate production and the three sort keys (§9) |

The sections below are the 2026-08-28 text, kept as the record of what
each revert had to prove, plus section 8 for the target the original
list omitted and section 9 for the one the register added afterwards
(row 32, 2026-09-18).

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
- **Blocked on:** nothing — the port is unstarted work, not waiting on
  an ask. The `0x1f` user-row shape is residue C
  (`docs/findings/probe-coverage-abi.md`), separate from this row.

## Order to execute

6, 7, 4, 5, 3, 2, 1 — smallest blast radius first, and 1 last because it
alone waits on the BDB path. Each lands with its own differential
flipped to IDENTICAL and the frozen pins re-measured, per the standing
gate. Section 9 is the only target still open; every earlier section
has closed, so it executes alone, first among equals of one — the
ordering question is moot until another target is registered.
