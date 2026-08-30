# Revert plan — the seven incompatible divergences

Date: 2026-08-28 · Status: **work order; nothing reverted yet** · Branch:
`claude/pr5-revert-incompatible-divergences`.

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

## The seven

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
- **Now:** raw-text fallback gives `n=1` where the pin gives `n=0`; the
  divided-table inventory is `n=756` for `xian` where the pin gives
  `n=337` (the pin drops `xi'an`-style phrases without
  `USE_DIVIDED_TABLE`).
- **Target:** the pin's gating at a literal `0x0` option word.
- **Probe:** `run-option-sweep.sh` at the `0x0` word, currently outside
  its exclusion list because no frozen gate runs that word.
- **Open question first:** both reference consumers OR
  `USE_DIVIDED_TABLE | USE_RESPLIT_TABLE` unconditionally
  (ibus `PYLibPinyin.cc:195-196`, fcitx `eim.cpp:941`), so neither can
  produce the word. Whether exception (d) covers consumer-unreachable
  *inputs* as well as uncalled *symbols* decides whether this entry
  exists at all. One line from the maintainer retires or keeps it.

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
  it: `'` → 1, `''` → 2, `'''` → 3 (measured, `oracle-apostrophe-abort.md`
  F-E-14).
- **Probe:** `pinyin_parse_more_full_pinyins` return and
  `pinyin_get_parsed_input_length` on apostrophe-only input.
- **Do not revert past the class-(c) boundary:** the cursor helpers'
  `false` at the `_check_offset` abort shapes stays. Only the parse
  length moves. Class B2 of `uncovered-surface-differentials.md` inherits
  this entry.

## Order to execute

6, 7, 4, 5, 3, 2, 1 — smallest blast radius first, and 1 last because it
alone waits on the BDB path. Each lands with its own differential
flipped to IDENTICAL and the frozen pins re-measured, per the standing
gate.
