# λ-estimator port — deleted-interpolation EM (W9-T3)

This documents the Rust `pinyin-lambda` crate, a value-level reproduction of
libpinyin's held-out counting (`gen_deleted_ngram`) and its
deleted-interpolation λ estimator (`estimate_interpolation`). It stacks on
W9-T2 (`pinyin-counter`, the `gen_ngram` reproduction), consuming that crate's
integer [`Counts`] as the system model and adding the held-out counts and the
EM the T0 characterization (`training-algorithm.md` §5, §4.3) specified.

Reproduction targets, read from the pinned libpinyin `2.11.91` source
(authorized: they are the trainer, not the decoder):

- `utils/training/gen_deleted_ngram.cpp` — held-out `DELETED_BIGRAM` counting.
- `utils/training/estimate_interpolation.cpp` — the EM (`:30-88`) and the
  cross-context averaging (`:119-139`).

Everything below is **SHOWN** (read directly from the cited source) unless
tagged otherwise.

---

## 1. `gen_deleted_ngram` → `held_out.rs`

`gen_deleted_ngram` is `gen_ngram` with the unigram increment removed. A full
`diff gen_ngram.cpp gen_deleted_ngram.cpp` shows the *only* substantive
differences (the rest is the option name, the exit codes, `input` vs `stdin`,
and comment style):

```diff
< static const gchar * bigram_filename = SYSTEM_BIGRAM;      # gen_ngram
> static const gchar * bigram_filename = DELETED_BIGRAM;     # gen_deleted_ngram
---
<         /* training uni-gram */                            # gen_ngram only
<         phrase_index.add_unigram_frequency(cur_token, 1);
---
<     if (!save_phrase_index(phrase_files, &phrase_index))   # gen_ngram only
<         exit(ENOENT);
```

So `gen_deleted_ngram` (a) never touches the unigram phrase index, (b) writes
`DELETED_BIGRAM` (`deleted_bigram.db`, `pinyin_internal.h:59`) instead of
`SYSTEM_BIGRAM`, and (c) never saves the phrase index. The **bigram counting
loop itself is byte-identical** to `gen_ngram`'s (`gen_ngram.cpp:88-124` vs
`gen_deleted_ngram.cpp:88-121`).

`count_deleted_tokens` (`held_out.rs`) reimplements that loop directly, citing
the deleted source:

| `gen_deleted_ngram.cpp` | `held_out.rs` |
|---|---|
| `last_token = cur_token; cur_token = token;` (`:88-89`) | `let last_token = mem::replace(&mut cur_token, token)` |
| `if (null_token == cur_token) continue;` (`:92-93`) | skip a null second word (**no** unigram increment) |
| `if (null_token == last_token) { if (!train_pi_gram) continue; last_token = sentence_start; }` (`:96-100`) | substitute `SENTENCE_START`; drop the boundary bigram when pi-gram is off |
| `get_freq → set_freq(+1)` else `insert_freq(1)`; `set_total_freq(+1)` (`:111-117`) | `bigrams[(last, cur)] += 1` |

Because the loop is byte-identical to `gen_ngram`'s, the held-out bigram
**values** equal what `pinyin_counter::Counter` produces for its bigram half
over the same input. A unit test (`matches_counter_bigrams`) asserts exactly
that, pinning the SHOWN claim mechanically against T2's validated counter.

Output type [`DeletedCounts`] carries `(prev, cur) → count` only — no unigram
section, no freq-1 floor. Integers throughout; no float appears in
`held_out.rs`.

---

## 2. `estimate_interpolation` → `interpolation.rs`

### 2.1 The per-context EM (`estimate_interpolation.cpp:30-88`)

`compute_interpolation(deleted, unigram, bigram)` estimates one λ per held-out
context `prev` by deleted-interpolation EM. Seed and loop (`:34-37`):

```
lambda = 0; next_lambda = 0.6; epsilon = 0.001
while (fabs(lambda - next_lambda) > epsilon) { lambda = next_lambda; next_lambda = 0; ... }
```

For each held-out pair `(prev → cur)` with `deleted_count`, the update is
(exact numerator/denominator, `:53-79`):

| term | libpinyin | `interpolation.rs` |
|---|---|---|
| bigram P(cur\|prev) | `elem_poss = freq / (parameter_t)total_freq` where `freq = bigram.get_freq(cur)`, `total_freq = bigram.get_total_freq()` (`:56-60`) | `freq as f64 / ctx.total as f64` |
| numerator | `numerator = lambda * elem_poss` (`:62`) | `lambda * elem_poss_bigram` |
| unigram P(cur) | `elem_poss = unigram_freq / get_phrase_index_total_freq()` (`:69-71`) | `freq as f64 / uni_total as f64` |
| denominator part | `part_of_denominator = (1 - lambda) * elem_poss` (`:73`) | `(1.0 - lambda) * elem_poss_unigram` |
| skip | `if (0 == numerator + part_of_denominator) continue;` (`:76-77`) | `if sum == 0.0 { continue; }` |
| accumulate | `next_lambda += deleted_count * (numerator / (numerator + part_of_denominator))` (`:79`) | `next_lambda += deleted_count as f64 * (numerator / sum)` |
| normalize | `next_lambda /= table_num` where `table_num = deleted.get_total_freq()` (`:81-82`) | `next_lambda /= table_num as f64` (`table_num = Σ deleted_count`) |

Three idioms reproduced faithfully rather than "fixed":

- **`get_phrase_item` `ERROR_OK == 0`** (`:68`): libpinyin reads
  `if (!unigram->get_phrase_item(token, item))`. `get_phrase_item` returns
  `ERROR_OK == 0` on success (`phrase_index.cpp:179-197`,
  `novel_types.h:78`), so `!result` is `true` **on success** — the block runs
  when the token exists. Reproduced as `unigrams.get(&cur)` (`Some` ⇒
  present). A port must not invert it.
- **`bigram == NULL` short-circuit** (`:56`): when `prev` has no system entry
  the bigram term is 0. Reproduced as `system_ctx: Option<&SystemContext>`;
  `None` ⇒ `elem_poss = 0`. The `assert(0 != total_freq)` (`:59`) is only
  reached on a hit, where the context total is ≥ the item freq > 0.
- **`table_num` counts skipped items** (`:81`): `table_num` is the deleted
  context's *stored* total, so a held-out pair with neither bigram nor
  unigram support (skipped by the `continue`) still enlarges the divisor,
  pulling λ down. `table_num = deleted_ctx.values().sum()` matches, since
  `gen_deleted_ngram` keeps the SingleGram total equal to the sum of counts.

`parameter_t` is `double` (`novel_types.h:130`), so the EM computes in `f64`.
(The `gfloat` normalization in `retrieve_all`, `ngram.cpp:146`, is unused
here — the EM reads raw `m_count`, not the normalized `m_freq`.)

### 2.2 The cross-context mean (`estimate_interpolation.cpp:116-139`)

`main` calls `compute_interpolation` **once per distinct `prev`**
(`Bigram::get_all_items`, `:114`/`:119-122`) and averages the per-context λ
arithmetically: `printf("average lambda:%f", lambda_sum / lambda_count)`
(`:139`). The shipped λ is that mean, *not* a single global EM.
`estimate_lambda` reproduces this: one `compute_interpolation` per
`deleted.contexts()`, then `lambda_sum / lambda_count`. Empty held-out set ⇒
`LambdaError::EmptyDeleted` (libpinyin prints `-nan`; a port must not panic —
constitution §4).

---

## 3. Float boundary — integer where integer, float only for λ

Counts are integers end to end: T2's [`Counts`] (`u64`) and this crate's
[`DeletedCounts`] (`u64`). The **only** floats are inside
`compute_interpolation`: the probability ratios and the `fabs`/ε comparison,
matching libpinyin's `parameter_t = double`. No float ever enters a count
representation; `estimate_lambda` reads integer counts and returns the `f64`
λ. The boundary is one function wide.

**Cross-implementation float determinism.** The estimate is a deterministic
function of the integer counts, decomposed as:

- **Inner EM** — the item loop iterates `retrieve_all` order, which is
  **ascending `cur`** (items are kept token-sorted by `insert_freq`'s
  `lower_bound`, `ngram.cpp:185`). A `BTreeMap` iterates identically. Same
  integer inputs + same order + IEEE-754 `f64` ⇒ **bit-identical per-context
  λ**.
- **Outer mean** — the pin sums per-context λ in `Bigram::get_all_items`
  order (tkrzw `ProcessEach`, hash order — `ngram_tkrzwdb.cpp:161`), which is
  *not* ascending. `estimate_lambda` sums ascending by `prev`. Reordering a
  sum of `n` doubles perturbs the result by at most ≈ `n · ulp` (~3e-14 for
  n = 153). On the differential fixture the perturbation is empirically
  **0** — the 153 per-context λ sum to the bit-identical `f64` in both
  orders — so the average is bit-exact here too.

Consequence for the tolerance (§4): the *underlying* agreement is at the
IEEE-754 floor (bit-exact per-context; ≤ ~3e-14 on the mean in general, 0
here). What is *observable* against the pin is capped by its output: it prints
λ via `%f` (six decimals). So the live gate asserts byte-identity at those six
decimals and bounds the average by `%f`'s rounding floor.

---

## 4. Differential parity

### 4.1 Configuration

The held-out split is **not** defined inside `gen_deleted_ngram` (which has no
split logic — it counts whatever stdin it is given); libpinyin's trainer feeds
it a held-out corpus slice, and `evaluate.py` consumes a *pre-existing*
`deleted_bigram.db` whose corpus is undocumented (`generate.py`/`Makefile.data`
carry no `gen_deleted_ngram` recipe — the shipped `bigram.db` is imported from
`interpolation2.text`, not counted). Per the frozen T0 §9, parity is therefore
**algorithmic — identical output on identical input**, and the split is a
fixture-level choice. What T3 reproduces is the counting and the EM; the
differential feeds the identical partition to both implementations, so the
split choice cannot affect parity.

Two extremes are degenerate on this synthetic corpus and were rejected as
non-discriminating (both verified to reproduce, but they saturate λ):

- **held-out = full corpus** (SYSTEM = DELETED): every pair has full bigram
  support ⇒ every per-context λ = 1.0, average 1.000000.
- **disjoint train/held-out** (SYSTEM = A, DELETED = B, A ∩ B = ∅): the
  synthetic corpus barely repeats bigrams across a split, so every held-out
  pair misses ⇒ every λ = 0.0, average 0.000000.

**Configuration X** (committed): SYSTEM_BIGRAM + floored unigram trained on
**fold A** — the first ⌊N/2⌋ = 116 lines of `segmenter-ngseg.txt` — and
DELETED_BIGRAM over the **full** corpus. Held-out pairs seen in A hit the
bigram path (λ → 1), pairs only in the tail miss it (λ → 0), and contexts
spanning both yield intermediate per-context λ. That spread — 64 contexts at
1.0, 81 at 0.0, 8 strictly between (e.g. 0.333266, 0.499989, 0.857125) — makes
the parity check bite on the real interpolation arithmetic, not a constant.

### 4.2 Result (pinned model20, fold A / full held-out)

Fed the same segmented input chain (T1's `segmenter-ngseg.txt` → T2's counter
over fold A → this crate's held-out counter over the full corpus → the EM),
compared against the live pin pipeline (`gen_binary_files` → `gen_unigram` →
`gen_ngram < A` → `gen_deleted_ngram < full` → `estimate_interpolation`):

- **DELETED_BIGRAM: 199 bigrams, integer bit-exact.** `export_interpolation`
  can only dump `SYSTEM_BIGRAM`, but the `gen_deleted_ngram` loop is
  byte-identical to `gen_ngram`'s (§1), so a fresh `gen_ngram` over the same
  full corpus is the DELETED oracle; every `(prev, cur) → count` matches.
- **Per-context λ: 153 contexts, byte-identical to `estimate_interpolation`
  at the six decimals it prints.** Keyed by `prev`, so the comparison is
  independent of the pin's tkrzw iteration order.
- **Average λ = 0.445689** (full precision `0.44568852293312161`,
  `average_bits = 3fdc8629278cd1c5`). The pin prints `0.445689`;
  `|λ_rust − λ_pin| = 4.771e-7`, which is exactly the `%f` rounding residual
  of the full-precision value.

**Tolerance and significant figures.** λ matches `estimate_interpolation` to
**all six significant figures the tool emits** — every per-context value and
the average, byte-for-byte. The live gate asserts `|Δaverage| < 1e-6`; the
justification is the tool's own output precision: `%f` rounds to six decimals,
a 5e-7 floor, and 1e-6 sits just above it — not a fudge but the tightest bound
the pinned binary's stdout permits. The residual *below* six decimals is at
the IEEE-754 float floor (§3: bit-exact per-context inputs and order; outer
reorder empirically 0 here, ≤ ~3e-14 in general), well under the task's
suggested 1e-9 — but that tighter agreement is not observable from the tool
and so is reasoned, not asserted against the live binary. The committed
manifest instead pins the average by its exact `f64` bit pattern (a
deterministic function of integer counts), asserted bit-exact within Rust.

### 4.3 Gates (env-guarded, mirroring T1 / T2)

`crates/pinyin-lambda/tests/differential.rs`:

- `rust_lambda_matches_committed_manifest` — recomputes Config X from the
  migrate export + fixture and checks context count, system/held-out bigram
  counts, unigram total, the average's exact bits, and the per-context 6dp
  dump checksum against `fixtures/w9/lambda-estimate.manifest`. Skips without
  the export.
- `rust_lambda_matches_live_estimate_interpolation` — runs the pin pipeline
  when `PINYIN_GEN_BINARY_FILES`, `PINYIN_GEN_UNIGRAM`, `PINYIN_GEN_NGRAM`,
  `PINYIN_GEN_DELETED_NGRAM`, `PINYIN_ESTIMATE_INTERPOLATION`,
  `PINYIN_EXPORT_INTERPOLATION`, and `PINYIN_GEN_NGRAM_DATA` are set;
  asserts the DELETED integer bit-exactness, per-context byte-identity, and
  the average bound above.

---

## 5. The hardcoded λ in the decode path (stated, not changed)

The decode language model hardcodes λ as an **authored rational constant**:

- `crates/pinyin-data/src/lm.rs:68-70` — `LAMBDA_NUMERATOR = 1`,
  `LAMBDA_DENOMINATOR = 2` (λ = 1/2), applied at `lm.rs:239-241` as
  `λ·b/bt + (1−λ)·u/ut` over a common `u128` denominator.
- `crates/pinyin-core/src/fixture.rs:34-36` — the same 1/2 in the fixture LM.

This is deliberately integer/rational: the decode score stays in exact `u128`
arithmetic (no float in the hot path), and the constant is documented as
"authored and deliberately neutral". T3's output is the *derivation* of that
constant — `estimate_interpolation`'s λ is exactly what libpinyin wrote into
`table.conf` as `lambda parameter:0.312699` (the value `pinyin-segment` reads
as `PINNED_LAMBDA`). **T3 does not touch the decode path**; it makes λ
derivable.

**A future integration PR** would need to:

1. Decide the *representation*: the EM yields an `f64` (~0.31 on the real
   corpus), but the decode LM interpolates in exact `u128` rationals. Wiring
   the derived λ in means either approximating it as a rational
   `num/den` (keeping the integer hot path) or moving the LM interpolation to
   float — an interface decision, not a mechanical swap.
2. Re-verify the reimplementation's scoring pins: the parity test is
   release + export-gated with a baseline top-1 of 63% (see
   `parity-climb-residual.md`); changing λ from 1/2 to the derived value
   shifts every bigram-vs-unigram tie and would move that number, so the
   sweep (`scoring-constant-sweep.md`) must be re-run and the pins re-frozen
   or accepted to shift.
3. Note the two λ are distinct today: the **segmenter** already reads
   `table.conf`'s 0.312699 (`pinyin-segment` `PINNED_LAMBDA`), while the
   **decode LM** uses 1/2. A derived-λ integration should reconcile which
   consumers move.

`Lambda::table_conf_value()` emits λ in the `table.conf` `%f` form
(`0.xxxxxx`) — the shape `evaluate.py` feeds to `make modify
LAMBDA_PARAMETER=` — as the usable hand-off, without touching any consumer.

---

## 6. Source index

| Claim | Source | Lines | Tag |
|---|---|---|---|
| `gen_deleted_ngram` = `gen_ngram` − unigram, → `DELETED_BIGRAM`, no save | `utils/training/gen_deleted_ngram.cpp` vs `gen_ngram.cpp` | 34, 96-98, 128-131 | SHOWN |
| `DELETED_BIGRAM = "deleted_bigram.db"` | `src/pinyin_internal.h` | 59 | SHOWN |
| EM seed 0.6, ε 0.001, `while fabs>ε` | `utils/training/estimate_interpolation.cpp` | 34-37 | SHOWN |
| bigram/unigram terms, skip, accumulate, `/= table_num` | `estimate_interpolation.cpp` | 53-82 | SHOWN |
| `get_phrase_item` `ERROR_OK == 0` idiom | `src/storage/phrase_index.cpp`, `novel_types.h` | 179-197, 78 | SHOWN |
| per-context then arithmetic mean | `estimate_interpolation.cpp` | 116-139 | SHOWN |
| `parameter_t = double` | `src/include/novel_types.h` | 130 | SHOWN |
| `retrieve_all` ascending token order (sorted `insert_freq`) | `src/storage/ngram.cpp` | 133-151, 178-205 | SHOWN |
| `get_all_items` tkrzw hash order | `src/storage/ngram_tkrzwdb.cpp` | 161-175 | SHOWN |
| `get_phrase_index_total_freq` = Σ unigram freqs | `src/storage/phrase_index.h` | 615-616 | SHOWN |
| shipped `bigram.db` imported from `interpolation2.text` (not `gen_ngram`) | `data/Makefile.am` | 58-62 | SHOWN |
| `evaluate.py` consumes pre-existing `deleted_bigram.db`, runs default `estimate_interpolation` | trainer `evaluate.py` | 21-23, 59 | SHOWN |
| decode λ authored constant 1/2 | `crates/pinyin-data/src/lm.rs`, `crates/pinyin-core/src/fixture.rs` | 68-70/239-241, 34-36/360-362 | SHOWN |
