# Findings — live-typing differential (post-choose surfaces no pin gates)

Date: 2026-08-22 · Status: W12 live-typing enumeration; classes assigned, no fix.

The frozen pins gate batch surfaces: the W2 corpus candidates and the
sentence-surface fixture both parse an input, guess once, and collect —
`observe_sentence_surface` never calls `pinyin_choose_candidate`
(`crates/pinyin-oracle/src/live.rs:537-567`), so no constraint ever exists
under them. This note measures what a frontend actually hits and nothing
else does: the surfaces **after a choose** — keystroke by keystroke while
typing continues, and the self-learning that runs when the user commits a
decoded continuation instead of choosing every phrase. It is the W12 open
item ("live-typing coverage, which no frozen pin gates"), measured by
`tools/bisection/live-typing-diff.c` /
`tools/bisection/run-live-typing-diff.sh` against the pin-built oracle
(libpinyin 2.11.91 @ 0c5e80e) and `libpinyin_capi.so` over the matched
model20 tables. No fix lands here.

## Reproduction

```bash
# assemble a real-unigram capi system dir once: pinyin_index.redb,
# phrase_index.redb, bigram.redb + interpolation2.text in one directory
LIVETYPING_SYSTEM=/tmp/live-typing-system \
tools/bisection/run-live-typing-diff.sh
```

Exit 2 is the measured state below. Rounds via `LIVETYPING_ROUNDS`
(default 3). The driver sets `pinyin_set_options(0x18a)` on both sides
first: the two libraries' bare defaults differ (the capi instance starts
at `PINYIN_INCOMPLETE`, the oracle at `USE_TONE`, `pinyin.cpp:329`), and
without the common parity word a trailing-incomplete keystroke measures
that config gap instead of the live-typing surfaces (`parse` of
`"nihaos"` returns 5 under `USE_TONE` alone, 6 under the parity word).
Under 0x18a every per-keystroke parse return agrees: 6/7/8/9/10/11.

The driver's keystroke contract: every step re-parses the FULL buffer —
`pinyin_parse_more_full_pinyins` replaces the composition with the passed
string on both libraries (`pinyin.cpp:1497-1533` parses `pinyins` alone;
the capi's `parse_more` opens with `reset_parse_state`, `state.rs:848`),
which is how the pinned frontends call it.

## Measured agreement (the seams that hold)

- **baseline** (parse "nihao", guess, collect — 13 lines): identical.
- **after-choose, remaining input** (choose 你 for "ni" inside "nihao",
  then guess — 13 lines): identical — rows `你好/你好/你号`, window at the
  cursor. The W6 re-seed plus the §10 text prefix reproduces the
  constrained walk's user-visible surface here
  (`sentence-surface.md` §10), exactly as documented.
- **core rounds** (all six probes across three training rounds,
  13 lines each): identical. The §9 user-merged step costs keep the
  post-choose decode aligned even after the user bigram lands.
- cursors (`term:cursor=2`, `live:cursor=2`, `round:N cursor=2`) and
  `train` retvals agree.

## The divergence, enumerated

70 diverging log positions in three classes.

### L1 — empty-remaining constrained walk (terminal choose), 7 positions

Choosing 你 for the WHOLE input "ni" leaves the oracle's instance holding
a non-empty matrix plus the `CONSTRAINT_ONESTEP` constraint; its next
`pinyin_guess_sentence` walks the constrained matrix and emits
`你/你/尼` — the duplicate-text 你 tails (ranks 0, 1) dedup in the
candidate window to `NBEST/0/你` + `NBEST/2/尼`, `n=2`. The engine's
`Session::guess_sentence` requires remaining input (the empty-remaining
contract): `guess=0`, no rows, `n=0`. The constrained full-matrix walk
answers for a fully-consumed composition; the remaining-input model
cannot.

### L2 — selection state does not survive the re-parse, 62 positions

The frontend re-parses the full buffer every keystroke. Upstream's
constraints are **instance** state — `pinyin_parse_more_full_pinyins`
only rebuilds the matrix and never touches `m_constraints`
(`pinyin.cpp:1497-1533`), which `validate_constraint` re-syncs at the
next guess. The engine's selection record is **session** state, and the
capi's parse path opens with `reset_parse_state()` → `session.reset()`
(`state.rs:848-858`): the chosen 你, the cursor's effect, and the history
are gone before the next guess runs.

Per typed probe (label = buffer tail after "nihao"):

| probe | diverging positions | of which window anchor | of which rows |
|---|---|---|---|
| type-s | 7 | 7 | 0 |
| type-sh | 11 | 7 | 4 |
| type-shi | 11 | 7 | 4 |
| type-shij | 12 | 6 | 6 |
| type-shiji | 11 | 7 | 4 |
| type-shijie | 10 | 7 | 3 |

- **Window anchor** (the `n=` line + the NORMAL candidate tail): the
  oracle's `guess_candidates(cursor)` lists the remaining input's phrases
  (`好似/好色/耗散/耗损/号丧/好`, `n=100..102`); the engine's lists the
  full buffer's (`你好/你/尼/呢/泥/妮`, `n=128/129`) because the parse
  reset returned the scan to offset 0.
- **Rows** (sentence rows + their NBEST candidate mirrors): at type-s the
  constrained decode and the fresh decode coincide — 你 is both the forced
  token and the free 1-best, so all three rows agree. From type-sh the
  row sets diverge: the engine decodes the whole buffer fresh
  (`你好是/你好似/你好似`), the oracle pins 你 and decodes only the tail
  (`你好似/你好似/你好事`). This mixes the constraint's absence with the
  standing tail-tie class (`sentence-surface.md` §5: gfloat accumulation,
  comparator tie order), and the two cannot be separated until the
  constraint model exists.

### L3 — decoded-continuation train record, 1 position

After (choose 你 for "ni" → re-decode, 好 stays decoded → `train`) ×3,
the oracle's constraint-aware `train_result3` walks the constrained
decode and trains 你→好: the export carries
`bigram: 你好|ni'hao|1242`. The engine's `Session::train` observes only
the explicitly chosen tokens (`history=[你]`); its sole observation is
`sentence_start→你`, which the bigram export shows on neither side, so
the engine's export is empty. The §8-corrected chosen-path training
(`run-nbest-train-diff.sh`) is unaffected — every phrase there is chosen;
the gap is exclusively the decoded continuation.

## Recorded implications for the constraint port (input, not decisions)

- L2 shows the constraint store cannot live as engine-session selection
  state alone: `parse_more`'s reset wipes it on every keystroke. A store
  that survives re-parse — at the capi instance beside #141's offset
  law, or a parse path that preserves it — is a prerequisite for any
  live-typing parity, independent of the walk itself.
- L1 shows the constrained walk must also answer the fully-consumed
  composition (a row set exists there), not only a remaining input.
- L3 is the only class a train-record change alone can close; L1/L2 need
  the store plus the walk.
- The runner is deliberately not wired into CI yet: it exits 2 today by
  construction. Wiring belongs with the change that flips it green.

## Vacuity guard

The runner fails (exit 1) unless both logs prove the post-choose surface
engaged: the choose cursors, and a non-empty after-choose candidate
window on both sides. A run that never advanced past a choose cannot
claim identity.

## Closure — the constraint port (2026-08-22, `feat/constraint-machinery`)

All three classes are closed; the differential now runs IDENTICAL under
the same reproduction, and the vacuity check holds — reverting the walk
to re-seeding turns it red (63 diff lines, exit 2).

- **L1** — `Session::guess_sentence` walks the full matrix whenever the
  input is consumed but non-empty: the constrained (or, after a row-0
  choose, free) walk answers the terminal-choose surface the
  remaining-input model structurally could not.
- **L2 — closed for forward extension only.** The reset split:
  `reset_composition` (the parse path) keeps the store, the selection
  record, and the cursor; only `Session::reset`/`pinyin_reset` clears
  them (`pinyin.cpp:2697`'s rule). The parse path continues the
  composition only on the strictly extending re-parse of an incomplete
  composition — the shape the frontend's keystroke flow produces — so
  the #141 cursor contracts keep their fresh-composition semantics.
  This is **not** "constraint lifetime fixed": a shrinking re-parse
  (backspace) still drops the forcing, permanently — measured below
  (§"Backspace-after-choose, measured") and closed as its own follow-up,
  not by this port.
- **L3** — `Session::train` walks the last lookup's 1-best result
  against the store (`train_result3`): forced phrases plus the first
  decoded phrase after each run train, the predecessor threading over
  every phrase. The export lands `bigram: 你好|ni'hao|1242`, the
  oracle's own count. A result without forcings (row-0 chooses) falls
  back to the selection record — the engine's row-record divergence,
  logged in `upstream-divergences.md`.

The gates held: candidate pins 10,190/10,190/absent 0/order-only
0/prefix-10 98,930 bit-identical; sentence pins 488/385/379
bit-identical (the empty store degenerates to the old walk, as the
fixture flow requires); the scheme sweep engine-side byte-identical
before and after (its standing oracle DIVERGE is the pre-existing §5
tie-order class); the train/import/predict/union/user/addon/nbest-train
differentials all IDENTICAL; fmt/clippy/tests green. The residual §5
tail-tie mixing that PR A saw inside L2's typed probes is gone with the
class itself — those probes now compare the constrained walk against
the oracle's constrained walk, and the remaining sentence-surface
residuals (the 17 first-6 rows) are untouched, as scoped.

## Backspace-after-choose, measured (opt-in)

`LIVETYPING_BACKSPACE=1 ./live-typing-diff …` adds a `bp-*` phase: choose
你 for "ni" inside "nihaoshijie", shrink the re-parsed buffer one
keystroke at a time down to "ni", then re-type to the full buffer. It is
opt-in because its divergence is the recorded parse-survival divergence
(`upstream-divergences.md`: upstream's constraints survive every
re-parse; the engine continues only an extending one) — measuring it
must not turn the green L-class gate red.

Measured (190 diverging lines over the ladder + retype):

- **Every shrink step** — the oracle keeps 你 forced: rows
  `你好时机/你好世纪` (at "nihaoshiji"), `你和/你会` (at "nih", the
  incomplete tail), with the window anchored at the cursor (好事/好似…,
  n=101; 和/后/会…, n=1713). The engine starts the composition fresh at
  the first shrink: window at offset 0 (你好/你/尼…, n=129/133), free
  rows. The same two mechanisms as the closed L2 (window anchor, forced
  vs free rows), re-introduced by the shrink.
- **The "ni" floor** — the forcing exactly covers the buffer; the
  oracle's surface is the L1 terminal shape (rows 你/你/尼, an
  NBEST-bearing two-row window). The engine answers the free "ni".
- **The retype** — the oracle's forcing survived the whole ladder and
  constrains the re-typed buffer; the engine's fresh reset dropped it at
  the first shrink, so the re-extension continues a store-less
  composition. The forcing is not recoverable by typing.

Two driver notes from building the probe: the ladder stops at "ni"
(the cursor must never overrun one-past-end, where upstream's
`_check_offset` aborts), and `pinyin_get_sentence` asserts
`index < results.size()` on a non-empty set (`pinyin.cpp:1474`) — the
probe now asks only the indices the candidate list proves, the
frontend's own caller contract. That assert is logged in
`upstream-divergences.md` (the engine answers false).

Closing this gap is a follow-up decision, not part of the port: it
needs the parse path to keep the store through a shrink and let
validate drop what stops spelling — plus the selection-record rebuild
on the shrunk prefix.
