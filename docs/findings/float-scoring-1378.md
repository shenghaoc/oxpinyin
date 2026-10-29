# Float scoring for the sentence decoder (#1378): Phase 1 design record

Date: 2026-10-28 UTC (`date -u`). Status: record. This is the read-only
Phase 1 design for #1378. The §5 disposition and §8.1 frozen edits were approved by the maintainer
on 2026-10-29 UTC; ruled text recorded at 2026-10-29T08:32:25Z (`date -u`). Base: origin/main `0a69da749c13`. Pin: libpinyin 2.11.92 at
`074a2219c90feaf962d0d24f034514033ece5f99`; pin citations are
`file:line` at that commit. oxpinyin code is cited by file and item.

## 1. Purpose and sources

#1378 replaces the integer `Cost` scale and the rational λ interpolation
with `f32` accumulation in the pin's float sequence. This record lists
the current uses of the integer scale and the rational λ. It writes down
the pin's float sequence for each scored step and maps the 2026-10-28
S1–S3 rulings onto floats. It also proposes a policy disposition and a
PR split. Appendix A settles, from the pin-built objects, the seven
points the source could not settle (D-1 to D-7).

`.kiro/specs/oxidation-record/requirements.md` Requirement 4,
criterion 3, already requires the sentence decoder to score with the
pin's float types, conversion points and order of operations.

Documents read: #1378; `compatibility-policy.md` (class (a), the math
rule, the witnessed algorithm-precondition amendment of 2026-10-28);
the `upstream-divergences.md` entry "N-best trellis accumulates gfloat
log costs — not reproducible in fixed point, FROZEN as a permanent
Stage-1 divergence"; `trellis-adjusted-cost-s1-s2.md`;
`trellis-tail-sort-s3.md`; `scoring-spec.md`; `sentence-surface.md` §3
and §12; `libpinyin-structures-and-algorithms.md` §4, §9 and §11.

Pin sources were read with `git show` at the pin. The blobs, by
abbreviated hash:

| File | Blob |
| --- | --- |
| `src/lookup/phonetic_lookup.h` | `c092e7610cb0` |
| `src/lookup/phonetic_lookup_heap.h` | `299ddedfa55e` |
| `src/lookup/phrase_lookup.cpp` | `d42e0691e80d` |
| `src/lookup/phrase_lookup.h` | `df6a55dae705` |
| `src/lookup/lookup.h` | `d4ddc37775be` |
| `src/storage/ngram.cpp` | `e4bfe8f78458` |
| `src/storage/phonetic_key_matrix.cpp` | `ab7e8796c7f5` |
| `src/storage/phrase_index.h` | `f97ac65ccc6a` |
| `src/include/novel_types.h` | `15c8656e716d` |
| `src/pinyin.cpp` | `f27f7cf77672` |

## 2. Inventory

### 2.1 On the scoring path

The scoring path is the sentence trellis and phrase segmentation.

| Crate | File and item | What it does |
| --- | --- | --- |
| core | `lib.rs` `Cost` | `i64` alias |
| core | `cost.rs` `NAN_COST`, `POS_INF_COST`, `NEG_INF_COST`, `cost_is_nan` | integer stand-ins for IEEE NaN and ±infinity |
| core | `cost.rs` `cost_add`, `cost_gt` | IEEE-like addition with saturation; a comparison that is false on NaN |
| core | `cost.rs` `cost_of_probability` | −log₂ x for x outside (0, 1], special values included |
| core | `cost.rs` `surprisal`, `surprisal_with_memo`, `bits_to_cost`, `Log2Memo`, `log2` | −log₂(count/total) × 1000, rounded, floored at `UNKNOWN_COST` |
| core | `cost.rs` `reduce_ratio` | shrinks a `u128` ratio to `u64` |
| core | `lib.rs` `NbestStepCosts` and `step`, `NbestStepRow` (its `log2` memo) | per-step costs handed to the trellis |
| core | `lib.rs` `LanguageModel::nbest_step_costs`, `nbest_step_costs_in_row`, `nbest_item_costs_in_row`, the `&L` forwarding | trait methods returning those costs |
| data | `lm/mod.rs` `BigramLanguageModel::nbest_step_costs_using` | the step law: the rational path when λ is a decimal in [0, 1]; the raw path (`lambda_raw`) otherwise, or when the ratio exceeds 1 |
| data | `lm/mod.rs` `interpolate_ratio`, `ratio_cost` | the `u128` common-denominator blend; `None` on overflow |
| data | `lm/mod.rs` `raw_blend_cost`, `raw_unigram_cost`, `unigram_lambda`, `probability_cost`, `zero_total_bigram_cost` | the pin's f32/f64 sequence up to `log`, then converted to `Cost` |
| data | `lm/mod.rs` the `nbest_*` wrappers, `set_lambda*`, fields `lambda`, `lambda_raw`, `lambda_f32` | λ storage and entry points |
| data | `table_conf.rs` `Lambda` (`PINNED` = 312699/10⁶, `from_decimal`, `as_f64`), `parse_table_conf_lambda`, `read_table_conf_lambda` | the rational λ |
| data | `table_info.rs` `TableConf::lambda_unit` | the rational λ, built from the same text as `lambda_f32` |
| engine | `nbest.rs` `Value::cost`, `NbestRow::cost`, `LONG_SENTENCE_PENALTY` (263) | trellis values and the S1/S2 penalty |
| engine | `nbest.rs` `adjusted_cost`, `ordered_cost_compare`, `node_loses_to`, `selection_loses_to`, `tail_merge` | S1, S2, S3 and the node rule |
| engine | `nbest.rs` `zhuyin_loses_to` | the pin's zhuyin comparator, through `cost_gt` |
| engine | `nbest.rs` `expand_one` | `cost_add` of the step, plus a separate `surprisal(matched, total)` for pronunciation |
| engine | `phrase.rs` `Node::cost`, `Step::relax`, `Step::best`, `step_costs`, `phrase_segment` | `PhraseLookup` on plain `i64` |
| runtime | `lib.rs` `RuntimeLm::nbest_step_costs`, `RuntimeLm::nbest_item_costs_in_row` (`row.log2_memo()`) | forwarding |

### 2.2 Elsewhere

- **Pre-frequency fallback scorer**, reached only when
  `has_real_unigrams()` is false. Core: `LanguageModel::score`,
  `UserModel::score`, `scoring.rs` (`ScoringConfig`, `Scorer`,
  `key_cost_table`), `kbest.rs` `k_best`. Data: `model_cost`,
  `pure_bigram_cost`, `score_with_user_delta`, `score_with_merged_row`,
  `UNIGRAM_TIEBREAK_SCALE`. Engine: `session/mod.rs` `key_costs`,
  `session/state.rs`, the fallback sort in `session/guess.rs`,
  `session/lookup.rs` `collect_sentences_with_tokens`. Runtime:
  `RuntimeLm::score`, `cached_key_costs`. User: `model.rs` `score`
  (always 0).
- **Cost carried along.** Engine `candidate.rs` `Candidate::cost`: rows
  receive the trellis cost, and only the fallback sorts by it. Capi
  `e2e_tests.rs` reads it. Runtime `examples/nbest_tail_probe.rs` prints
  it.
- **Training.** `eval/model.rs` `EvalLanguageModel::model_cost` and its
  private copy of `interpolate_ratio`. `eval.rs` `lambda_from_f64` and
  `estimate_lambda` produce a rational `Lambda`; `train/pipeline.rs`
  carries `average_lambda`. `eval/decode.rs` already scores in the pin's
  float sequence and is prior art. `lambda/interpolation.rs` `Lambda` is
  an unrelated type with the same name.
- **Tests and benches.** Testsupport `fixture.rs` `model_cost` (a copy
  of `interpolate_ratio`); data `tests/language_model.rs`; core
  `tests/scoring.rs`; engine `session/tests.rs`; runtime
  `tests/assembly.rs`; benches `parse_and_graph`, `session_keystroke`,
  `scan_perf`, and a comment in `stage2`.
- **Tools.** Nothing reads `Cost`.
- **Candidate frequency.** `LanguageModel::amplification_lambda` uses
  `lambda_f32` and no `Cost`; it is unchanged.

## 3. The pin's float sequence

`gfloat` is binary32 and `gdouble` binary64. On the amd64 reference each
operation is evaluated in its own type with one rounding (Appendix A,
D-3). On arm64 one multiply-add is fused (D-2).

**Inputs.**

- **λ** is the `gfloat` that `SystemTableInfo2::load` scans with
  `fscanf("%f")`. `bigram_lambda = λ`. `unigram_lambda = (gfloat)(1. − λ)`:
  λ widened to double, subtracted in double, narrowed to float
  (`src/lookup/phonetic_lookup.h:712-713`, `src/lookup/phrase_lookup.cpp:97-98`).
- **Unigram frequency and total** are `guint32`
  (`src/storage/phrase_index.h:124-126`, `:615-617`).
- **`bigram_poss`** is `freq / (gfloat) total`: both `guint32` convert to
  float, then a float division (`src/storage/ngram.cpp:171`,
  `src/lookup/phonetic_lookup.h:604-608`, `src/lookup/phrase_lookup.cpp:292`).
  It is carried as `gfloat` (`src/include/novel_types.h:107`).
- **`pinyin_poss`** is a `gfloat`. For each matrix path it is
  `matched / (gfloat) total` over `guint32` sums, or 0 when the total is 0
  (`src/storage/phrase_index.h:141-163`). The paths are summed in float,
  depth first in column order (`src/storage/phonetic_key_matrix.cpp:554-584`).
- **Seed** `m_poss` is `log(1.f)` = +0.0f
  (`src/lookup/phonetic_lookup.h:250`, `src/lookup/phrase_lookup.cpp:40`).

**Step U, `PhoneticLookup` unigram** (`src/lookup/phonetic_lookup.h:646-667`):

1. `get_phrase_item` fails: skip.
2. `elem_poss = (double)freq / (double)total`. A total of 0 gives +inf,
   or NaN when the frequency is also 0.
3. `elem_poss < DBL_EPSILON`: skip. NaN does not skip.
4. `pinyin_poss < FLT_EPSILON`: skip.
5. `t = (elem_poss * (double)pinyin_poss) * (double)unigram_lambda`.
6. `m_poss' = (float)((double)m_poss + log(t))`.

**Step B, `PhoneticLookup` bigram** (`src/lookup/phonetic_lookup.h:674-697`):

1. `get_phrase_item` fails: skip.
2. `unigram_poss = (double)freq / (double)total`.
3. `bigram_poss < FLT_EPSILON && unigram_poss < DBL_EPSILON`: skip.
4. `pinyin_poss < FLT_EPSILON`: skip.
5. `a = bigram_lambda * bigram_poss`, a float product.
6. `s = (double)a + (double)unigram_lambda * unigram_poss`. On arm64 this
   is one `fmadd` (D-2).
7. `t = s * (double)pinyin_poss`.
8. `m_poss' = (float)((double)m_poss + log(t))`.

**`PhraseLookup`** (`src/lookup/phrase_lookup.cpp:307-346`) uses the same
two laws without `pinyin_poss` and its gate: `log(elem_poss * unigram_lambda)`
and `log(bigram_lambda * bigram_poss + unigram_lambda * unigram_poss)`. A
value is replaced only when `orig < next` (`:371`). The best-value scans
use `cur > max`, and the first value wins ties (`:229-234`, `:400-405`).

**Comparisons on `m_poss`.** `trellis_value_less_than<nstore>`
(`src/lookup/phonetic_lookup.h:66-91`) adds `LONG_SENTENCE_PENALTY`
(`:39`) only when `nstore > 1`. That is pinyin, where S1/S2 replace the
comparator. Zhuyin compares `m_poss` with `<` at equal length and
prefers the shorter sentence. `trellis_value_compare` (`:174-178`) is
replaced by S3. The node stores are `trellis_node<2>::eval_item`
(`src/lookup/phonetic_lookup_heap.h:56-81`) and `trellis_node<1>::eval_item`
(`:108-122`).

**The `log` call.** Every step calls `log@GLIBC_2.29` from `libm.so.6`
(D-7). `LONG_SENTENCE_PENALTY` is not a call: it is the folded f32
constant `0x3e3ab283` (D-1).

## 4. S1, S2, S3 and the node rule on the float scale

- **S1 and S2.** `S = poss − (length as f32) × P` in `f32`, where P is
  the pin's `LONG_SENTENCE_PENALTY`, f32 `0x3e3ab283` =
  0.18232159316539764 (D-1); the precision is ruled below. The ordering
  puts NaN last; otherwise it compares S descending with `partial_cmp`.
  That gives the ruled order: +∞, finite scores descending, −∞, NaN.
  `total_cmp` must not be used: it orders NaNs by sign bit and puts −0
  before +0.
- **Selection key.** (ordered S, batch position), unchanged.
- **Node two-slot rule.** The front loses only when its ordered S is
  strictly worse than the newcomer's. A tie is rejected, including NaN
  against NaN and ∞ against ∞. Front eviction stays.
- **S3.** A stable sort of the raw f32 `poss`, descending, NaN last, with
  ±0 equal. Unchanged.
- **Zhuyin.** The pin's predicate on f32, written directly (`<` is false
  on NaN). The `cost_gt` stand-in is no longer needed.
- **Ruled semantics.** None change. The ruling defines S with ln(1.2);
  263 was its integer form, not a fitted coefficient. Outward moves are
  still possible in three cases: pairs whose advantage lies between
  263/1000 bits (0.182298 nats) and P (0.182322 nats); ties created by
  rounding each step to 1/1000 bit, such as the §12 rows with equal
  native costs (`bo`, 13059 against 13059); and equivalences created by
  saturation. In `f32`, pairs whose exact S differ can also round to
  the same S and tie; the existing tie rules resolve them. These are
  changes of representation. They are attributed to S1/S2 or show up as
  new matches.

**S precision (maintainer ruling, 2026-10-28 UTC): `f32`.**

- S is computed as `S = poss − (length as f32) × P`, with
  `P = f32::from_bits(0x3e3ab283)`, the pin's constant.
- There are two `f32` roundings, as written: the product, then the
  difference. No `mul_add` and no widening.
- Equal S values tie and are resolved by the existing rules: batch
  position in selection, and the node rejects a tying newcomer.
- The ordering is the ruled one: +∞, finite descending, −∞, NaN last.
  Not `total_cmp`.
- PR B tests near-threshold pairs, including pairs whose exact S differ
  but round to the same `f32`.
- `f64` is revisited only if a measurement shows a ranking reason.

## 5. Compatibility-policy disposition — ruled 2026-10-29 UTC

Maintainer approval (2026-10-29 UTC), recorded at 2026-10-29T08:32:25Z (`date -u`):
items 1–8 and the §8.1 frozen changes are approved as proposed. PR B
implements this disposition; PR C removes the remaining rational path.

1. **The FROZEN register entry** ("N-best trellis accumulates gfloat log
   costs"). Mark it superseded in place when PR B lands, citing the
   #1378 maintainer decision of 2026-10-28. Its premise, that fixed
   point is needed for cross-platform bit-exactness, is replaced by the
   math rule and per-host determinism. Keep the history and update the
   classification-table row (row 11).
2. **What stays class (a).** Only the last-bit difference between
   Rust's `f64::ln` and the pin's `log` where the two resolve to
   different libm implementations: macOS, Windows, musl, or a native
   glibc different from the container's. If PR B's shipping object
   calls the same `log@GLIBC_2.29` (D-7), the amd64 reference host has
   no log difference at all.
3. **The arm64 contraction** (D-2) is compiler behaviour, so it falls
   under limb 3 of the decision order, not class (a). Record it with a
   discrepancy record and report `POLICY-ACCEPTED` on arm64-only cases.
   Do not imitate it with `mul_add`. Whether it explains any of the
   #1137 arm64 exclusions is not established.
4. **The six §12 class (a) rows** (`bo`, `quguan`, `kaliantai`,
   `huilianpei`, `yunhoulvlvlunqianaonaoruoqubeiji`, `nou'y`). Re-measure
   them under PR B. A row that matches leaves class (a). A row that still
   differs is investigated before it is classified (Kiro Requirement 4,
   criterion 3). The frozen §12 declaration,
   `fixtures/w4/oracle-sentence-tie-order.txt` and
   `crates/pinyin-oracle/tests/sentence_surface_parity.rs` change only
   with explicit approval.
5. **`scoring-spec.md`** (frozen for W4-T3). Scope its normative cost
   scale to the pre-frequency fallback scorer, with explicit approval.
6. **`sentence-surface.md` §3.** The "fixed-point" bullet and the
   "40-bit floor" bullet close. The first-path-only `pinyin_poss` bullet
   stays.
7. **The 2026-10-28 amendment.** Restate its Cost form
   (`cost_add(cost, 263 · L)`) and its sentinel paragraph in IEEE terms,
   with no change of ruled semantics.
8. **S1/S2 `POLICY-ACCEPTED` records.** Re-run the four BDB pairs in
   `trellis-adjusted-cost-s1-s2.md` and re-record any whose subject
   observation changes.

## 6. PR split and gates

| PR | Content | Gate |
| --- | --- | --- |
| A (this record) | Design record, Appendix A, proposed policy text; the structures record §11 correction | Rule 7 docs case: fmt, `git diff --check` |
| B (behaviour) | **Model:** the language model returns pre-log pieces (the bigram blend `s`; `elem_poss` and `unigram_lambda`) and applies the pin's ε gates, with `lambda_f32` throughout. **Engine:** `poss: f32`; S1, S2, S3, the node rule and zhuyin in float; `phrase.rs` with the pin's `<` and `>`. **Tests:** near-threshold pairs, including pairs whose exact S differ but round to the same `f32`; special values, including λ ∈ {−0.5, 0, 1, 1.5, 2, 1e6, inf, nan}. **Policy:** the ruled form of §5. | fmt, clippy and `cargo test -p` for core, data, engine, runtime and testsupport through `just heavy`; cells from `tools/bisection/cells-for-diff.py`; full differentials on a bdb debug build, including the ignored real-table sentence-surface tests; revert-and-check with a named mutation on bdb; sentence and typing Callgrind campaigns, base / PR / pin in one run; stripped shipping sizes; function-order policies for bdb, kc and tkrzw; `readelf` of the shipping object for the `log` import (D-7) |
| C (removal, after external-I/O stage 1) | Delete the rational path: data `interpolate_ratio`, the rational `Lambda`, `lambda_unit`, `lambda_raw`, `ratio_cost`, `probability_cost`, `zero_total_bigram_cost`; core `reduce_ratio`, `Log2Memo`, `surprisal_with_memo`, `cost_of_probability`, the sentinel costs; the training and testsupport copies; the D1 bridge (§8) | fmt, clippy, unit tests of the touched crates; the 70 `table-conf` lambda contract cases on bdb; training lambda and eval tests; stripped sizes |

Expected cost: each step drops the `u128` products, the ratio reduction
and the logarithm memo, and takes one `ln`. A trellis `Value` shrinks
from 32 to 28 bytes, and each `NbestStepRow` loses its 32-byte memo.
Neither time nor space is expected to worsen; PR B measures both. The
fallback scorer keeps its own `Cost` until spec task 4a; it is outside
#1378.

## 7. Pre-existing differences found while reading

These were derived from source for the original design record, before any
run. PR B fixes items 1 and 2 through its float scoring path; the original
observations are retained below. Items 3 and 4 describe the pre-stage-1
base, as explained in §8. The facade-total ruling and pronunciation audit
remain open.

1. **`phrase.rs` adds sentinel costs with plain `i64` `+`**
   (`phrase_segment`). Debug builds panic on overflow, against
   constitution §4, and differentials build debug. Release builds wrap.
   `Step::best` picks `NAN_COST` as the best value, where the pin keeps
   the first value. Reachable with λ outside [0, 1], or with a merged
   gram whose total is 0.
2. **λ = 1 on the rational path** prices the unigram step `None`
   (skipped), and likewise a zero-count bigram. The pin keeps both steps
   at −∞. The 2026-10-28 amendment names this case.
3. **A λ text with a large reduced denominator** can make
   `lambda_den · bigram_total · unigram_total` overflow `u128`; then
   every blended step is `None`. The pin scores them.
4. **The rational λ is the exact decimal** (312699/10⁶), not the pin's
   f32. Ratios above 1 switch to the f32 within the same sentence.
5. **A facade total that wrapped to 0** makes oxpinyin skip the step;
   the pin divides by 0.0 and continues.
6. **Pronunciation.** Only the first matrix path is used (already
   recorded in `sentence-surface.md` §3), and the sums saturate in `u64`
   where the pin's `guint32` sums wrap. Out of scope for #1378.

## 8. Open questions

1. **Frozen items approved by the maintainer, 2026-10-29 UTC:** the
   `scoring-spec.md` cost scale; the `sentence-surface.md` §12
   declaration, its tie fixture and `sentence_surface_parity.rs`; the
   FROZEN register entry; the amendment's Cost-form wording.

### Resolved: the D1 bridge

"D1 bridge" and "D1 routing" name stage 1's D1 ruling (maintainer,
2026-10-28 UTC). The stage-1 PR records it in `external-io-boundary.md`.
#1404 implemented stage 1 and merged before PR B. Under D1:

- λ comes only from the scanned `f32`.
- The rational routing value derives from that `f32`'s shortest
  round-trippable decimal.
- The rational calculation is kept where its arithmetic fits; on `u128`
  overflow the step takes the raw `f32` path.
- A finite pin score never becomes `UNKNOWN_COST` or a missing step
  because of overflow.

PR C removes the bridge (§6). §7 items 3 and 4 describe the base tree
`0a69da74`, before stage 1.

## Appendix A. Object evidence for D-1 to D-7

Date: 2026-10-28 UTC (`date -u`). Host: Linux amd64. Only disassembly,
object tables and one oxpinyin build; no library was run.

### Objects

| Object | Provenance | SHA-256 against `oracle-pin.txt` |
| --- | --- | --- |
| amd64 `libpinyin.so.15.0.0`, bdb | `ghcr.io/shenghaoc/oxpinyin-oracle:074a2219c90f-debian-trixie-11652421b0ea` (`just compat image-tag` at `0a69da74`); image digest `sha256:668d28139251efe13786be32a722e4b682e94258c132ce61be59d28f3ab48969`; gcc 14.2.0-19, glibc 2.41 | `58f73afe…c34c`, matches `libpinyin_sha256_bdb` |
| amd64 `libzhuyin.so.15.0.0`, bdb | same image | `44c6ca9a…0fb2`, matches `libzhuyin_sha256_bdb` |
| arm64 `libpinyin.so.15.0.0`, bdb | the same tag with `-arm64`; digest `sha256:64325ab255f10ddde6fab8a42ec3e361aced74d00e861b57d476c373b84f6009`; same toolchain | `59bbf820…5e44`, matches `libpinyin_sha256_bdb_linux_arm64` |
| arm64 `libzhuyin.so.15.0.0`, bdb | same image | `7270c446…7f83`, matches `libzhuyin_sha256_bdb_linux_arm64` |
| oxpinyin `libpinyin.so.15.0.0`, bdb | origin/main `0a69da74`, the shipping recipe below; rustc 1.97.1, cargo-c 0.10.24, bdb function-order policy; installed object, not stripped | `c7f0df1e…4a9b` (no pin entry) |

Line references come from the objects' DWARF line tables. Offsets are
virtual addresses in the object.

### D-1. `LONG_SENTENCE_PENALTY`: float `log`, folded to an f32 constant

- amd64 `trellis_value_less_than<2>` at 0x96030: `movss` of `.rodata`
  0xaaca8 at 0x96060, `addss 0xc(%rsi)` at 0x96068, `comiss` at 0x96072
  (`:75-76`); and 0x96080, 0x96088, 0x96097 (`:71-72`).
- `.rodata` 0xaaca8 holds `83 b2 3a 3e`: f32 `0x3e3ab283`,
  0.18232159316539764. That is ln(1.2f) rounded to f32. The neighbours are
  1.0f (0xaaca0) and FLT_EPSILON `0x34000000` (0xaaca4).
- The same constant is used in inlined copies of the comparator: 0x8da90
  and 0x8dab0 (`trellis_value_comp<2>`), 0x970a0
  (`trellis_node<2>::eval_item`), 0x98418 (`search_bigram2`).
- arm64 `trellis_value_less_than<2>` at 0x79d20: `mov #0xb283` and `movk
  #0x3e3a` at 0x79d60/0x79d64, `fadd s` at 0x79d74, `fcmpe s` at 0x79d78;
  again at 0x79d90–0x79da8.

### D-2. Fused multiply-add: none on amd64, fused on arm64

- **amd64, no contraction.** In `PhoneticLookup<2,3>::bigram_gen_next_step`
  (0x97930), `:693`:
  - `mulss (%rbx)` at 0x97a7b computes `bigram_lambda × bigram_poss`;
  - `cvtss2sd 0x4(%rbx)` at 0x97a6c widens `unigram_lambda`;
  - `mulsd` at 0x97a77 multiplies it by `unigram_poss`;
  - `cvtss2sd` at 0x97a84 widens the float product;
  - `addsd` at 0x97a88 adds them; `mulsd` at 0x97a8c multiplies by
    `pinyin_poss`; `call log@plt` at 0x97a90.
- The same instruction sequence appears in:
  - the inlined copy in `search_bigram2` (0x98155, 0x98163, 0x9818c,
    0x98196, 0x9819a);
  - `PhraseLookup::bigram_gen_next_step` (0x744d8 `mulss`, 0x74502
    `mulsd`, 0x7450a `addsd`, 0x7450e `log`; `phrase_lookup.cpp:342`);
  - libzhuyin: 0x931fa–0x9323d, 0x93782–0x937c6, 0x73d58–0x73d8e.
- No `vfmadd`-family, VEX scalar or x87 arithmetic instruction occurs
  anywhere in either amd64 object.
- **arm64, contracted.** In `PhoneticLookup<2,3>::bigram_gen_next_step`
  (0x7b520):
  - `ldp s31, s28, [x19]` at 0x7b60c loads `bigram_lambda` and
    `unigram_lambda`;
  - `fmul s31, s15, s31` at 0x7b620 computes the float product;
  - `fcvt` widens at 0x7b624 and 0x7b63c;
  - `fmadd d31, d28, d14, d31` at 0x7b644 computes `(double)unigram_lambda ×
    unigram_poss + (double)a` with one rounding (`:693`);
  - `fmul d0, d30, d31` at 0x7b648 multiplies by `pinyin_poss`;
    `bl log@plt` at 0x7b64c.
- The other `fmadd` sites, four per object and no other fused
  instruction:
  - libpinyin: 0x7bc38 (`search_bigram2`), 0x59980
    (`PhraseLookup::bigram_gen_next_step`, `phrase_lookup.cpp:342`),
    0x57fd4 (`PinyinLookup2::bigram_gen_next_step`, not on the trellis);
  - libzhuyin: 0x767e4, 0x76d34, 0x591c0, 0x57814.

### D-3. amd64: scalar SSE, one rounding per operation

- **`bigram_poss` division.** `cvtsi2ss`, `cvtsi2ss`, `divss` in
  `SingleGram::search` at 0x5a1fe, 0x5a207, 0x5a20c (`ngram.cpp:171`);
  in `search_bigram2`'s ONESTEP path at 0x985cb, 0x985d7, 0x985dc
  (`:608`); in `PhraseLookup::search_bigram2` at 0x746f6, 0x74704,
  0x74709 (`phrase_lookup.cpp:292`).
- **Step-5 multiply.** `mulss`, as in D-2.
- **`pinyin_poss`.** The per-path quotient is `cvtsi2ss`, `cvtsi2ss`,
  `divss` at 0x613f5, 0x613fa, 0x613ff (`phrase_index.h:162`). The sum
  is `addss 0xc(%rsp),%xmm0` at 0x60e3c, stored back as float by `movss`
  at 0x60e48 (`phonetic_key_matrix.cpp:577`).
- **ε gates.** FLT_EPSILON with `comiss` (0x979b3, 0x97a0a, 0x9735a);
  DBL_EPSILON (`.rodata` 0xa6180, `0x3cb0000000000000`) with `comisd`
  (0x979d9, 0x9732a). The branches let NaN through, as in the source.
- No x87 instruction occurs in either amd64 object.

### D-4. `guint32` → `float` conversion

- **amd64.** The value is zero-extended by a 32-bit move, then converted
  with the 64-bit `cvtsi2ss`: `mov %r8d,%esi` at 0x613e1 then
  `cvtsi2ss %rsi` at 0x613f5; `mov 0xc(%rsp),%eax` at 0x5a203 then
  `cvtsi2ss %rax` at 0x5a207; `mov 0xf0(%rsp),%eax` at 0x985c4 then
  `cvtsi2ss %rax` at 0x985cb.
- **arm64.** `ucvtf` at 0x410c4/0x410c8 (`ngram.cpp:171`), 0x485d0/0x485d4
  (`phrase_index.h:162`), 0x59b18/0x59b1c (`phrase_lookup.cpp:292`) and
  0x7bf34/0x7bf3c (`:608`). Each is followed by `fdiv s` (0x410cc,
  0x485d8, 0x59b20, 0x7bf40).
- **Rounding mode.** Both rounding instructions use the MXCSR (amd64) or
  FPCR (arm64) mode. No `ldmxcsr` or `fldcw` (amd64), no `msr fpcr`
  (arm64), and no `fesetround` or `fesetenv` import occurs in any of the
  four objects. The mode at run time cannot be read from an object.
- **`guint32` → `double`.** `cvtsi2sd` from a zero-extended register
  (0x979b8, 0x979c6) and `ucvtf d` (0x7b5c0, 0x7b5c4); exact.

### D-5. `double` → `float` narrowing of `m_poss`

- **amd64 libpinyin.** `cvtsd2ss` at 0x97ab4 (bigram), 0x973f7
  (unigram), 0x981c7 (inlined in `search_bigram2`), 0x97739 (inlined in
  `search_unigram2`), 0x742fb and 0x74530 (`PhraseLookup`).
- **amd64 libzhuyin.** 0x93265, 0x93bfa, 0x937f3, 0x9400e, 0x73db0,
  0x73b7b.
- **arm64 libpinyin.** `fcvt s15, d15` at 0x7b664; also 0x7bc50 and
  0x599a0.
- In each case the preceding add is `addsd` (or `fadd d`) of the widened
  `m_poss`, for example `cvtss2sd 0xc(%r14)` at 0x97a46 and `addsd` at
  0x97a99.

### D-6. amd64: no reassociation of `elem_poss * pinyin_poss * unigram_lambda`

- **`PhoneticLookup<2,3>::unigram_gen_next_step` (0x97290).**
  - `elem_poss` is spilled at 0x97344 and reloaded at 0x97380;
  - `cvtss2sd` of `pinyin_poss` at 0x97386;
  - `mulsd` by `elem_poss` at 0x973a9;
  - `cvtss2sd 0x4(%rbx)` of `unigram_lambda` at 0x973bb;
  - `mulsd` at 0x973d0, then `call log@plt` at 0x973d4.

  That is `(elem × pinyin) × unigram_lambda`, as written.
- **The same instruction order** appears in `search_unigram2`
  (0x976de, 0x9770c, 0x97710) and in libzhuyin (0x93bab, 0x93bce;
  0x93fb5, 0x93fcd).
- **`PhraseLookup::unigram_gen_next_step` (0x74210)** is one product:
  `cvtss2sd` at 0x742d0, `mulsd` at 0x742d5, `log` at 0x742d9.

### D-7. The `log` each object calls

- **Pin.** `log@GLIBC_2.29`, with version-needed `GLIBC_2.29` from
  `libm.so.6`. It is the only symbol the pin objects bind to `libm`.
  Relocations: `R_X86_64_JUMP_SLOT` at 0xcf128 (libpinyin) and 0xc8120
  (libzhuyin); `R_AARCH64_JUMP_SLOT` at 0xc0128 and 0xc0120. Every step
  site calls it through the PLT: libpinyin 0x973d4, 0x97710, 0x97a90,
  0x9819a, 0x742d9, 0x7450e; libzhuyin 0x9323d, 0x937c6, 0x93bd2,
  0x93fd1, 0x73b59, 0x73d8e. `PinyinLookup2` adds two more sites per
  object.
- **oxpinyin at `0a69da74`.** The only `libm` symbol is
  `log2@GLIBC_2.29`, through `R_X86_64_GLOB_DAT` at 0x25af98. It is
  called through the GOT, for example at 0xf907c
  (`BigramLanguageModel::nbest_step_costs_using`), 0x9d2ca
  (`phrase::step_costs`) and 0x19327e
  (`RuntimeLm::nbest_item_costs_in_row`).
- **Not settled.** The object has no `log` import, so it cannot show how
  `f64::ln` resolves. That is read from PR B's shipping object (§6).

### Reproduction

From the repository root at `0a69da74`, with podman, binutils and the
`llvm-tools` rustup component (or any aarch64-capable objdump):

```sh
OUT=~/.cache/oxpinyin/1378/repro
TAG=$(just compat image-tag)
podman pull --platform linux/arm64 "ghcr.io/shenghaoc/oxpinyin-oracle:$TAG-arm64"
for img in "$TAG" "$TAG-arm64"; do
  id=$(podman create "ghcr.io/shenghaoc/oxpinyin-oracle:$img" /none)
  podman cp "$id:/opt/pinyin-oracle" "$OUT/$img" && podman rm "$id"
  sha256sum "$OUT/$img"/bdb/lib/lib{pinyin,zhuyin}.so.15.0.0
done
```

`podman create` and `podman cp` do not run the image. The amd64 image
is present locally after any `just compat` run, or is pulled the same
way.

```sh
A=$OUT/$TAG/bdb/lib/libpinyin.so.15.0.0
R=$OUT/$TAG-arm64/bdb/lib/libpinyin.so.15.0.0
LO=$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-objdump
objdump -dl -C --no-show-raw-insn --start-address=0x96030 --stop-address=0x960a3 "$A"
objdump -s -j .rodata --start-address=0xaaca0 --stop-address=0xaacb0 "$A"
objdump -dl -C --no-show-raw-insn --start-address=0x97930 --stop-address=0x97bc6 "$A"
objdump -dl -C --no-show-raw-insn --start-address=0x97290 --stop-address=0x97500 "$A"
objdump -d -C --no-show-raw-insn "$A" | grep -E 'call.*<log@plt>|cvtsi2ss|vfmadd'
"$LO" -d -l -C --no-show-raw-insn "$R" | grep -E 'fmadd|ucvtf\s+s'
readelf --dyn-syms -r -V -W "$A" | grep -E 'log@|libm'
```

The oxpinyin object for D-7 is the shipping recipe, with
`OXPINYIN_FUNCTION_ORDER_POLICY` unset so the bdb policy applies:

```sh
just heavy -- just packaging install libpinyin --prefix="$STAGE" \
  -- --locked --release --no-default-features --features bdb,shipped
readelf --dyn-syms -r -V -W "$STAGE/lib/libpinyin.so.15.0.0" | grep -E 'log|libm'
```

## PR B re-measurement (2026-10-29 UTC)

Historical measurements before the record-format rebase; no campaign or size
measurement was rerun for that follow-up. Base: main `9015a178`. Native Ubuntu 26.10, x86_64,
Core i7-9750H, Rust 1.97.1, glibc 2.44, Valgrind 3.27.1. Both arms use
matching provisioned BDB model20 tables. The 3,304-case debug contract
runs have exactly four verdict movements, all MATCH to the ruled zhuyin
ordering differences in [1378-zhuyin-selection.md](1378-zhuyin-selection.md).
The seven pre-existing DIFFERs on that main revision follow the S1/S2
ruling: three longer-normal cases and four GB/negative-lambda cases. #1400
subsequently supplied the latter four registrations and KC/tkrzw entries for
three positive-lambda cases; the three longer-normal registrations remain in
PR B until PR 3 replaces them. The original four BDB S1/S2 pairs are unchanged.

The supplementary fixture's pin data, historical selection costs and input
set stay fixed. Its actual pin differences fall from 163 to 156. Every
remaining surface is identical to current main. All seven changes become
pin-equal; their cause is replacing the rational/integer scoring path and
its per-step millibit grid with the pin's pre-log float sequence and f32
accumulation. No std-math difference is needed to explain these matches.

| Input | Main → PR B (= pin), changed indexed ranks |
| --- | --- |
| `bo` | rank 2 博 → 波 |
| `sao` | rank 2 骚 → 嫂 |
| `lagai` | rank 2 拉盖 → 拉改 |
| `xiehenshuaitong` | ranks 1–2 写很率同 / 写很帅通 → 写很帅通 / 写很率同; the old 47,380 / 47,380 grid tie is separated |
| `huilianpei` | rank 1 会连佩 → 惠连佩 |
| `liekuoouyangzhangzaique` | rank 2 列阔欧阳章在阙 → 列阔欧阳章在确 |
| `yunhoulvlvlunqianaonaoruoqubeiji` | rank 2 云后旅旅伦卡闹闹若区被及 → 云后旅旅伦卡闹闹若去被及 |

The 500-row fixture measures main 496 / 491 / 489 (1-best / distinct-set /
ordered); PR B measures 496 / 492 / 491. Candidate-row agreement moves
489 → 491. The four 1-best misses are unchanged. The current counts and
156 exact ruled selection residuals replace stale integer-era test
expectations; no new PR B non-MATCH is accepted there.

The score-shift assertions now bind these f32 log-possibility bits:
`yaomeichong c1b595f0`, `xiehenshuaitong c201bc36`,
`nuanmanqianzhaofang c225aaba`, `nic c144ff96`.
The fallback scorer keeps its integer costs. PR B removes the row's
logarithm memo and the private pricing helpers that become unused; the
remaining rational/D1 fallback and training path is PR C.

D-7 is resolved by the installed PR B shipping object: `readelf
--dyn-syms -r -V -W` shows an undefined `log@GLIBC_2.29`, an
`R_X86_64_GLOB_DAT` relocation to it, and `libm.so.6` as the version-needed
provider. This is the result of Rust std `f64::ln`; no libc implementation
or instruction sequence is imitated.
