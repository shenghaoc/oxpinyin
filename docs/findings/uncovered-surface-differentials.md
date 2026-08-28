# Findings — uncovered-surface differentials (paging, punct modes, option profiles, cursor moves)

Date: 2026-08-24, amended twice 2026-08-25 · Status: W12 live-typing
enumeration, part 2; classes assigned, no fix. The 2026-08-25 amendment
verifies B1's user-visible consequence against both frontends, reframes
C2 as the candidate-window divergence it is, and records the review
priorities (§ at the end).

`docs/findings/live-typing.md` measured the post-choose surfaces (choose,
continue-typing, backspace — since closed by the W14 constraint port).
This note measures the four surfaces the W12 parking list names beyond
those: **deep paging**, **punctuation modes**, **FORCE_TONE and the
remaining option-bit profiles**, and **mid-composition cursor moves**. It
is measured by `tools/bisection/uncovered-surface-diff.c` /
`tools/bisection/run-uncovered-surface-diff.sh` against the pin-built
oracle (libpinyin 2.11.91 @ 0c5e80e) and `libpinyin_capi.so` over the
matched model20 tables, under the parity word `0x18a` set on both sides
first (the bare defaults differ; without the common word a
default-config artefact would masquerade as a divergence). No fix lands
here; the differential exiting non-zero **is** the deliverable.

## Reproduction

```bash
# assemble a real-unigram system dir once: the four live-typing files
# (pinyin_index.redb, phrase_index.redb, bigram.redb, interpolation2.text)
# PLUS punct.redb — the Option A export (token LE → NUL-terminated UTF-8,
# docs/findings/prediction-punct.md) of the same model20 punct.table the
# oracle's punct.bin was built from (370 rows / 272 tokens), so the punct
# rows compare over matched tables
UNCOVERED_SYSTEM=/tmp/uncovered-surface-system \
tools/bisection/run-uncovered-surface-diff.sh
```

Exit 2 is the measured state below. The driver honours the oracle caller
contracts: `pinyin_get_sentence` is asked only for proved indices (row 0
after a successful guess; rows 1..2 only when the walked window's NBEST
`nbest_index` values prove them — the corpus discipline, `live.rs:551-560`;
a past-the-rows index on a non-empty set aborts at the pin,
`pinyin.cpp:1474`); predicted candidates are never passed to
`pinyin_choose_candidate` (`pinyin.cpp:2506-2508` asserts); no scheme call
is made anywhere (zhuyin 7 / double 30 never reach the oracle).

## Method

One 836-line oracle log against an 856-line capi log (the capi answers
where the pin's FORCE_TONE rejection leaves nothing — class C1 below).
45 line-aligned change hunks, no pure additions or deletions: 161
diverging oracle-side log positions, 181 capi-side (offset 8 of the
word-move probes is measured since the harness update). Phases, in log
order:

| Phase | Lines (oracle) | Surface |
|---|---:|---|
| A `page:` | 338 | deep paging: parse shi/yi/ji/nihao, decode, walk pages 0..11 (page size 5, the fork default `PYPConfig.cc:148`) plus the last page, sentence rows, a deep choose at index 10, and a choose of the very last row |
| B `punct` | 149 | punct-table prediction for 好/的/一/你/中国/我/是/了 (head rows with types, every PRED_PUNCT row, counts) plus punctuation bytes in the composition (`nihao,` `ni,hao` `ni'hao` `ni hao` `ni2hao` `，nihao`) |
| C `opt:` | 205 | option profiles: `0x18a` control (offsets 0 and 2), `0x38a` (DYNAMIC_ADJUST set, offsets 0/2/5), `0x1ca` (parity+FORCE_TONE, 7 inputs), `0x60` (USE_TONE+FORCE_TONE, 4 inputs) |
| D `cur:` | 144 | cursor moves on `nihaoshijie`: auxiliary text and lookup offset at every byte cursor 0..11, word-level left/right offsets (0/2/5/8), the candidate window at every moved cursor (left walk then right walk), and one mid-buffer choose at cursor 5 |

## Measured agreement (the seams that hold)

- **Deep paging — surface (a): 338/338 lines identical.** The full-list
  walk (12 pages plus the last page per input), the deep choose at index
  10, and the tail-row choose all agree, cursors and post-choose windows
  included (`shi` n=280, `yi` n=597, `ji` n=481, `nihao` n=126; the deep
  and tail chooses return the same cursors and identical re-decode
  surfaces). Paging is frontend index-walking over one library window
  (`ibus_lookup_table_page_down`, `PYPPhoneticEditor.cc:300-307`); the
  window itself was already pinned at offset 0, and every deep row of it
  matches too.
- **The punct rows themselves — the punct-table path of surface (b):
  identical for all eight prefixes.** Prediction retvals, window counts
  `n`, every `PREDICTED_PUNCTUATION` row, and the punct counts match:
  好 → `，。`; 的 → `，。“、；`; 一 → `、`; 你 → none; 中国 → `，`;
  我 → `，`; 是 → `“，：`; 了 → `。，“！`. The Option A model20
  `punct.redb` reproduces the oracle's `punct.bin` predictions exactly.
- **Punctuation bytes that agree:** the trailing comma (`nihao,` → 5),
  the mid comma (`ni,hao` → 2), the apostrophe separator (`ni'hao`), and
  the mid tone digit (`ni2hao`) parse identically.
- **Option profiles that hold:** the `0x18a` control at offset 0
  (corpus-proven, re-confirmed here); `0x38a` at offset 0 — DYNAMIC_ADJUST
  bit-set produced **no divergence of its own** at these probes (at
  offset 0 `prev_token` is null so no gram merge runs; the offset-2/5
  probes are dominated by class C2 below); `0x1ca` on all seven inputs —
  FORCE_TONE is **inert** there because `0x18a` carries no `USE_TONE` bit
  and the pin's force-tone rejection sits inside the `USE_TONE` branch
  (`pinyin_parser2.cpp:176-190`); `0x60` on the toned inputs (`ni3hao3`
  parsed=6, `zai4` parsed=4, `zhuang4` parsed=7, `ni3`, `shi4jie4`).
- **Cursor-move readouts that hold:** the auxiliary text matches at all
  twelve cursors of `nihaoshijie` (including the mid-syllable splits
  `ni h|ao shi jie`); the left-word move at offset 0; and the mid-buffer
  choose — cursor 11 both sides with an identical post-choose surface
  (n=3: 你好世界/你好时节/你好是届).

Full/half-width and Chinese/English punctuation **mode toggles are not
expressible through the pinned C ABI**: the 2.11.91 export list has no
such symbols (they are ibus-frontend state — `PYHalfFullConverter.cc`,
`PYPunctTable.h`, `PYPinyinProperties.cc`). The ABI punct surface is the
punct-table prediction path plus punctuation bytes in the composition,
both measured above; the mode toggles stay frontend territory.

## The divergence, enumerated

Six classes over 161 oracle-side positions (181 capi-side) — offset 8 of the
word-move probes is measured since the harness update (2026-08-26).

| Class | Surface | Positions (oracle) | Example |
|---|---|---:|---|
| B1 | predicted-phrase rows: text not prefix-sliced, head order differs | 79 | `punct-hao:head[2]` 莱坞 vs 好不好 |
| C2 | unconstrained mid-offset candidate window | 60 | `opt:0x18a-nihao@2:n=` 94 vs 126 |
| C1 | FORCE_TONE toneless rejection missing | 8 | `opt:0x60-nihao@0:parsed=` 0 vs 5 |
| D1 | cursor → lookup-offset normalization | 6 | `cur:3 off=` 2 vs 3 |
| B2 | parse past space / full-width punct bytes | 4 | `punctparse-space-mid:parsed=` 2 vs 5 |
| D2 | word-move step: syllable vs byte | 4 | `cur:left-right@2 right=` 5 vs 3 |

### B1 — predicted-phrase rows not prefix-sliced, head order differs (79 positions)

For every prefix with predicted phrases, the pin's `PREDICTED_PREFIX`
rows carry the phrase string **sliced from `m_begin`** — the prefix is
removed for display: prefix 好 yields 莱坞/奇心/日子 (from
好莱坞/好奇心/好日子), prefix 你 yields 的爱/是谁/的心. The capi emits
the **full phrase** (好莱坞, 你是谁). Mechanism: the pin template sets
`m_begin = m_prefix_len` (`pinyin.cpp:2399` at the pin) and the string
computation slices from it (`_token_get_phrase(..., candidate->m_begin,
...)`, `pinyin.cpp:2018-2023`); the capi's prediction path
(`predict.rs`) emits the whole phrase. The head **order** also differs
(好莱坞 is row 2 on the pin, row 6 on the capi; 好不好/好半天 lead the
capi list and sit outside the pin's first ten), while the window count
`n` agrees per prefix (180/288/592/71/127/169/101/60) — same-count,
differently-ordered lists with a different text shape. ~~Whether the two
180-row sets coincide beyond the captured head-12 is unmeasured~~
(**measured 2026-08-25, see the root-cause note below: the sets
coincide exactly after slicing**). This surface was never compared
before: the W11 prediction drivers ran mini-table capi against
full-table oracle and skipped the phrase rows (`prediction-punct.md`:
"those drivers … the punctuation list is compared … on prefixes present
in both tables").

**Verified consequence (2026-08-25): an engine-side correctness bug,
user-visible today — not a ranking nuance to ledger.** Both known
frontends commit the library's predicted-candidate string **verbatim**:
ibus-libpinyin's suggestion editor commits
`candidate.m_display_string` as-is
(`PYPSuggestionEditor.cc:212-230`), and fcitx5-oxpinyin's Phase 5
`selectPredicted` commits the string from `pinyin_get_candidate_string`
directly (`src/oxpinyin.cpp:899-927`) — after `enterPredicting` was
called with the **already-committed** prefix text as the prediction
prefix (`src/oxpinyin.cpp:857-887`, entered from `commitSentence`).
Under the pin's stripped text the two halves compose (commit 你好,
select the 世界 row → document reads 你好世界); under the engine's
full-phrase text the prefix is duplicated (commit 你好, select the
你好世界 row → the document reads 你好你好世界), and the panel itself
already displays the typed prefix inside every suggestion. The fix
(slice the prefix from the predicted text, `m_begin = prefix_len`) is
independently fix-worthy regardless of the order half of this class.

**Root cause (2026-08-25): one seam, three laws — fix in one pass, not
two PRs — amended again 2026-08-25 after the tie measurement below; see
the correction at the end of this section.** All of B1 lives in
`predict.rs` (`guess_predicted` + `append_predicted_prefix`). The
original three-law reading:

1. **The prefix subtraction.** The pin subtracts `m_begin` twice —
   from the display string (`_token_get_phrase` slices,
   `pinyin.cpp:2018-2023`) *and* from the sort key
   (`_compute_phrase_length` sets `m_phrase_length =
   get_phrase_length() − m_begin`, `pinyin.cpp:1976-1980`). The capi
   neither slices nor subtracts: one omission drives both the text
   corruption and the length component of the order.
2. **The amplified frequency law.** The pin's predicted sort key is
   `(1−λ)·unigram_freq/total·2²⁴` truncated (`pinyin.cpp:1811-1824`,
   the PREDICTED_PREFIX early-continue branch). The engine ports the
   same law as `amplified_frequency` (`session.rs:1835`) but the
   predicted path never calls it — `append_predicted_prefix` sorts on
   the raw `unigram_count` (`predict.rs:201`).
3. **The tie basis.** The pin's within-tie order is the insertion
   order; the capi pre-sorts prefix rows by token ascending
   (`predict.rs:196`, and again inside `suggest_after`,
   `dict.rs:215`).

Measured, not just code-read: a one-off dump of every PREDICTED_PREFIX
row for prefix 好 (178 rows a side) shows the **sets coincide exactly
after slicing** — every sorted-set difference is the missing
subtraction itself (好东西 vs 东西) — so the suggestion search is
provably correct. Slicing alone does not close the order.

**Correction (2026-08-25, follow-up measurement): law 2 is
order-neutral on the system store, and law 3 is not a law at all — it
is the store's physical iteration order.** Three measurements:

- **The frequency key ties.** The baked phrase-index counts across a
  prefix's suggestion set are uniform: 好 = 177 rows at count 100 plus
  one at 200; 的 = 281×100 + 2×99; 一 = 587×100 + 2×99 + 2×200; 我 =
  167×100 + 1×200. And the amplified law is monotone in the count —
  strict inequalities stay strict, ties stay ties — so
  raw-count-vs-amplified ordering is **identical** for every pair a
  system store can produce (the two scales only diverge for counts
  below the truncation collapse, ≈4, i.e. fresh user-store phrases).
  Wiring `amplified_frequency` in is still right for exactness, but it
  moves zero rows on this surface.
- **The tie order is the store walk.** Simulating laws 1+2 (sliced
  length + amplified) re-sorts nothing — the position mismatches vs
  the pin stay at **174 of 178** (position metric; the "138" quoted in
  the first amendment was oracle-side diff-edit lines, a different
  meter — the position count is 174/178 at baseline and after laws
  1+2 alike). The pin's within-tie order is not token-ascending (174
  off), not text-ascending (177 off), not library-blocked (177 off;
  the observed order switches libraries 27 times — one physical store
  holds all libraries' tokens). It is the **Tkrzw HashDBM bucket-walk
  order**: `phrase_index.bin` is `TkrzwHDB`, and
  `PhraseLargeTable3::search_suggestion` iterates it with
  `MakeIterator`/`Jump(prefix)`/`Next`
  (`phrase_large_table3_tkrzwdb.cpp:155-190`) — a deterministic order
  for this exact file and tkrzw version, with no sort-key expression.
  glib's `g_array_sort_with_data` preserves it verbatim: measured on a
  178-element array with grouped ties, within-group insertion order
  survives untouched (0 inversions), so the comparator never scrambles
  the walk.
- **The engine's store cannot walk that order.** `suggest_after`
  builds a `BTreeMap<String, Vec<u32>>` and walks text order
  (`dict.rs:196-217`); the physical bucket order of a Tkrzw hash file
  is not derivable from any key. Matching the pin's row order exactly
  means replicating the Tkrzw hash layout or freezing per-prefix
  orders as fixture data.

**Revised fix shape.** One PR lands the prefix subtraction (the
corruption) and wires the amplified law for exactness (a one-line
call); both are deterministic. The row ORDER behind the tie is a
store-layout divergence, not a comparator bug — recorded in
`docs/findings/upstream-divergences.md` (the sentence-trellis float
entry's sibling category: deterministic upstream, not reproducible
without importing the foreign store's physical layout). Expected state
for that PR: the phase-B probes go identical on text shape; the row
order stays divergent (174/178 on 好) by recorded divergence unless the
maintainer chooses fixture-frozen parity. The moving-number gate is
`tools/bisection/pred-order-diff.c` + `run-pred-order-diff.sh`
(per-prefix position-mismatch counts; raw metric 1571/1571 pre-fix —
text shape and order both count — dropping to the order-only residual,
174/178 on 好, once the slice lands), added with this amendment.

**Landed state (2026-08-25, the B1 fix PR) — measured against the
prediction, then the discriminator, written so a future reader cannot
mistake a real set regression for the windowing artifact.** Every
predicted number hit: text-shape **0 set differences on all eight
prefixes** (verified on the FULL lists — the 178-row 好 dump and its
siblings, 1571 rows total); hao **exactly 174/178** (the amplified
wiring moved not one row); raw total **1541/1571**, inside the
predicted band. The maintainer decided the same day: **a defined order
(text-ascending), not fixture-frozen parity** — see the divergence
entry for the decision and its consequence that the pred-order gate
becomes a defined-order assertion, not a parity one.

*The discriminator, explicit:* the phase-B `head[i]` rows are a
**head-12 window** over each prefix's list. Because the two engines'
row orders differ (the recorded divergence), their first-12 windows
cover different slices of the same set, and the head rows show **set
differences that are pure windowing** (86 rows across the eight
prefixes at the post-fix measurement). That is NOT a text regression.
The authoritative text-shape check is the **full-list comparison** —
`pred-order-diff`'s complete per-prefix dumps, sorted-set diff == 0 on
every prefix. Rule for future runs: a head-window set difference alone
means nothing; escalate only if the **full-list sorted-set diff is
non-zero** (that is a real regression — a suggestion missing or
invented), or if a prefix's row count `n` changes (the counts are
identical per prefix, 178/283/591/71/126/168/98/56, and a change there
is a search regression the order divergence cannot explain).

### C2 — the unconstrained mid-offset candidate window (60 positions)

This is a **candidate-window divergence, not a paging finding**: the
parity word `0x18a` on the most ordinary input (`nihao`), at the one
candidate-lookup argument the corpus never varied — the offset. It
stayed invisible behind two blind spots at once: every frozen pin asks
at offset 0 only, and post-choose windows agree because the W14
constraint machinery re-seeds the session at the cursor.

Guessing at a mid-buffer offset **without a prior choose** re-runs the
pin's span search from that offset: `nihao` at offset 2 returns the hao
window (n=94, rows 好/号/豪/浩/…); `nihaoshijie` at offset 5 returns the
shijie window (n=304, rows 世界/时节/…). The capi validates the caller
offset and then iterates the session's existing candidate list
(`sentence.rs:342-351`) — the window stays head-anchored wherever the
cursor is: offset 2 returns the same n=126 你/尼/呢 list as offset 0,
and every cursor of the phase-D walks returns n=129. Mechanism: the pin
anchors `start = offset` in `pinyin_guess_candidates`
(`pinyin.cpp:2224-2262` at the pin); the engine's candidate construction
has no offset parameter — `Session::candidates()` is the decode-anchored
list. No frontend drives mid-composition windows today — fcitx5-oxpinyin's
cursor moves only via `pinyin_choose_candidate` returns and backspace
(`src/oxpinyin.cpp:563-585, 644-675`) — but any Left/Right editing
feature hits this immediately and wrongly (the head window offered at
every cursor position). Affected probes: `opt:0x18a-nihao@2` (8
positions), `opt:0x38a-nihao@2` (8), `opt:0x38a-nihaoshijie@5` (6), and
the phase-D windows (38: every `cur:left@` / `cur:right@` walk probe and
`cur:mid`, including `cur:left@10` where the pin's window at the 'e'
tail column returns n=193 with 阿-family rows while the capi repeats its
head list).

### C1 — FORCE_TONE toneless rejection missing (8 positions)

Under `USE_TONE|FORCE_TONE` (`0x60`) the pin rejects every toneless
syllable in `parse_one_key` (`pinyin_parser2.cpp:176-190`: the check
lives inside the `USE_TONE` branch), so `nihao` parses 0 bytes (empty
matrix, n=0) and `zai6` parses 0 ('6' is not a tone, the toneless `zai`
is rejected). The engine has no FORCE_TONE handling at all — the bit is
absent from the capi header by design (`types.rs:91`) and from the
engine — so it parses `nihao` fully (5, n=126) and `zai6` as `zai`
(3, n=32, with sentence rows). The toned inputs agree (see above), and
`0x1ca` shows the bit alone is inert without `USE_TONE`. The fix shape
(a force-tone rejection in the parser under both bits) is parked with
this measurement. No fork GSettings key maps to `USE_TONE` or
`FORCE_TONE` (`PYPConfig.cc` maps only the incomplete/fuzzy/correct/
dynamic keys), so no frontend profile a user can produce sends `0x60`
bare — the class matters for ABI parity, not fork defaults.

### D1 — cursor → lookup-offset normalization missing (6 positions)

`pinyin_get_pinyin_offset` walks the cursor back to the nearest
non-empty matrix column at the pin (`pinyin.cpp:3010-3029` at the pin):
mid-syllable cursors normalize to the syllable start (cursor 1 → 0;
cursors 3, 4 → 2; 6, 7 → 5; 9 → 8). The capi returns the identity
mapping clamped to the raw length (`cursor.rs:90-111`, documented
"Provisional"): cursor 3 → 3, cursor 4 → 4, and so on — 6 diverging
table rows. The cursors 10 and 11 agree (their columns are non-empty on
both models). The auxiliary text at the same cursors matches, so the
frontend's visible preedit is unaffected; the offset feeds the window
anchor (class C2) and the word moves (class D2).

### B2 — parse past space and full-width punctuation bytes (4 positions)

For punctuation bytes inside the composition, the pin's parser stops at
the first non-pinyin byte: `ni hao` parses 2 (the ni window, n=125) and
`，nihao` parses 0 (n=0). The engine's parser consumes past the byte:
`ni hao` reports parsed=5 with the n=126 nihao window, and `，nihao`
reports parsed=5 with the same window — the space and the full-width
comma are skipped rather than stopping the parse. The half-width comma
and the apostrophe agree (both stop/parse identically), so the gap is
specific to the space and multi-byte (non-ASCII) punctuation bytes.

### D2 — word-move steps: syllable vs byte (4 positions)

The pin's word-level cursor moves step **syllable to syllable**:
`get_left/right_pinyin_offset` walk to the key that ends at / the first
key that starts after the offset (`pinyin.cpp:3031-3095` at the pin) —
at offsets 0/2/5/8 of `nihaoshijie` the (left, right) pairs are
(0,2)/(0,5)/(2,8)/(5,11). The capi returns `offset±1` bytes
(`cursor.rs:118-175`, "Provisional"): (0,1)/(1,3)/(4,6)/(7,9). Four
probe lines diverge (left at offset 0 happens to agree).

## Harness notes (pin behaviour, not engine divergences)

- **The pin aborts on word moves at tail offsets.** `get_right_pinyin_offset(11)`
  on `nihaoshijie` trips the second `_check_offset` — the one on the
  COMPUTED result (`pinyin.cpp:3090` → `assert` at `:2175` at the pin;
  measured SIGABRT on the first smoke run, re-measured fork-per-probe on
  the rebuilt pin 2026-08-25). ***Correction (2026-08-26):*** the first
  smoke run attributed the abort to `get_left_pinyin_offset(11)` via
  `pinyin.cpp:3055` — wrong call site. The fork-per-probe measurement
  (every offset in its own child, so the abort is a datum) shows
  `get_left(11)` genuinely answers `left=10` — its walk halts at column
  10, where the `e` key ends — and the abort lives at `get_right(11)`:
  the first check passes (column 10 is the lone non-zero `e`), column 11
  is the trailing singleton zero key, and the second check at its raw
  end 12 asserts. Offset 8 is also fully measurable (`left=5`,
  `right=11`); the driver's skip there is over-cautious. Both
  `fix/cursor-offset-normalization` docs record the same correction:
  `oracle-bisect-differential-abort.md` addendum. Upstream fixed this
  after the pin (commit `95e3af7` "Fix _check_offset function" turns the
  assert into `return false`). The driver probes word moves at the
  offsets the smoke run proved safe (0/2/5/8 — offset 8 is fully
  measurable); offset 11 is not probed, a frontend Ctrl+Right at cursor
  11 aborts the pinned library, and that is the pin's landmine, not a
  divergence to close.
- **DYNAMIC_ADJUST bit-set ranking** (the deferred #99 bigram-fold into
  candidate frequency) is **not** isolated by these probes: at offset 0
  no previous token exists (no gram merge), and the offset-2/5 windows
  are inside class C2. Measuring #99 needs a probe that isolates order
  under a non-null `prev_token` on agreeing windows — parked with C2.

## Priorities (review 2026-08-25, order inverted on second review)

Sequencing recorded for the closing workstreams. The first review
ranked C2 first while both classes were "divergences"; the B1
verification (user-visible text corruption in a shipping frontend path,
vs C2 latent-until-a-frontend-adds-cursor-editing) inverts it:

1. **B1 — landed in #173 (resolved).** The prefix subtraction closed
   the user-visible committed-text corruption (你好 + 你好世界) and the
   amplified-law wiring landed as the one-line exactness call. The
   residual row order behind the uniform-count tie is the recorded
   store-layout divergence (174/178 on 好; see the root-cause correction
   and `upstream-divergences.md`), not a comparator to port, and the
   maintainer chose the intentional text-ascending order over
   fixture-frozen parity — so B1's only standing item is that recorded
   order divergence, not open work. `pred-order-diff` reports the order
   number so the PR could show what moved and what is recorded
   divergence.
2. **C2 next.** A genuine `pinyin_guess_candidates` divergence under
   the parity word on ordinary input, invisible only because the corpus
   never varied the offset and post-choose windows are constrained. No
   frontend drives mid-composition windows today (fcitx5's cursor moves
   only via choose returns and backspace); its blast radius grows the
   moment a second frontend adds mid-composition editing. Starting
   point: `sentence.rs` passes the caller offset to
   `validate_lookup_offset` but the span search never sees it.
3. **C1** is a feature port (the FORCE_TONE rejection in the parser
   under both bits), cleanly measured here.
4. **D1/D2** track as provisional implementations of the documented
   cursor functions, low priority (9 positions).

Also noted in review: the out-of-tree `punct.redb` regeneration this
measurement needed (the `oxpinyin-migrate` exporter is gone from main)
was a reproducibility gap — since closed by `oxpinyin-datagen`
(`docs/findings/datagen-model20.md`): full-table production, punct
included, is now in-tree from the canonical model20 archive.

## Scope and pins

Measurement only: no engine, capi, data, or parser code is touched, and
no pin, gate, or CI policy changes. The differential is not wired into
CI; exit 2 is its measured state. B1 (prefix slicing) landed in #173 —
resolved there, leaving only the intentional text-ascending order
divergence (the recorded Tkrzw bucket-walk residual, 174/178 on hao;
see `upstream-divergences.md`) as B1's standing item. The still-open
classes are C1 parser rejection, C2 offset-anchored windows, B2
stop-byte parse, and D1/D2 the provisional cursor functions. Pins
re-verified bit-identical after this change (see the PR report);
fmt/clippy/tests green. The 2026-08-25 amendment (B1 consequence, C2
framing, priorities) is docs-only and re-verified the same way: the
branch delta vs `origin/main` is `tools/bisection/` + this doc +
`docs/findings/upstream-divergences.md` + `AGENTS.md` + one
`.gitignore` line, so no pin can move.

## Amendment (2026-08-27) — every class but B1's order closed; the standing residual measured

The "still-open" list above is the 2026-08-25 state and is closed as
follows: C1 and B2 by `678f325` (the parser-termination classes; the
FORCE_TONE double/zhuyin scope boundary is recorded in
`upstream-divergences.md`), C2 by `a15fc7e` and its review follow-ups
(`2382bdd`, `6721ab0`, `09e30b2`, `207aecd`, `239070e`, `9dd6018` —
the offset-anchored window and its anchor contracts; the mid-syllable
offset residue is likewise recorded, not chased), D1/D2 by `94822ec`
(the pin's cursor-offset laws). Re-measured on 2026-08-27 against the
pin over freshly compiled datagen tables: the differential still exits
2, with **zero non-`PRED_PREFIX` diverging lines** — paging, punct rows,
every option profile, and all cursor probes agree, and the only
divergence is B1's head-order window over the predicted rows. The
discriminator holds: per-prefix row counts equal on all eight prefixes
(178/283/591/71/126/168/98/56) and the full-list sorted-set diff is
**0** on every prefix (verified by dumping both engines' complete
predicted lists), so the standing item remains exactly the recorded
order divergence — text-ascending by decision, the pin's Tkrzw
bucket-walk not reproducible without importing the foreign store's
physical layout (`upstream-divergences.md`).
