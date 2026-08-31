# KMM arithmetic audit — line-by-line vs the pin

Scope: verify the `oxpinyin-kmm` arithmetic against the six upstream KMM
sources it re-expresses, field by field and operation by operation, and
classify every intentional divergence against
`docs/findings/compatibility-policy.md`'s four exception classes. This is the
W9 completion-criteria deliverable "verify KMM arithmetic line-by-line"; it
complements `trainer-parity-audit.md` §6 (which maps the call graph) with the
arithmetic proof.

Sources (pin `libpinyin/utils/training/`, read in full):

| upstream | Rust | operation |
| --- | --- | --- |
| `k_mixture_model.h:40-95` | `model.rs:111-164` | α, γ, B, Pr_G_3 |
| `gen_k_mixture_model.cpp:63-412` | `generate.rs` | per-document counting |
| `estimate_k_mixture_model.cpp:36-155` | `estimate.rs` | candidate score (deleted-interpolation EM) |
| `merge_k_mixture_model.cpp:38-238` | `merge.rs` | model merge |
| `prune_k_mixture_model.cpp:45-191` | `prune.rs` | CDF prune |
| `export_k_mixture_model.cpp` / `k_mixture_model_to_interpolation.cpp:59-217` | `text.rs` | export/import/convert |

Verdict: **the arithmetic is reproduced exactly.** Every integer field
transformation and every floating-point expression matches the pin term for
term and in evaluation order. Four items are intentional and argued below;
none changes the observable output for any input the real pipeline produces.

## 1. The model math (`k_mixture_model.h` → `model.rs`)

`parameter_t` is `double`; counts are `guint32`. `Parameter = f64`, counts
`u32`.

| upstream expression | Rust | match |
| --- | --- | --- |
| `alpha = 1 - n_0/(parameter_t)N` | `1.0 - f64::from(n_0)/f64::from(n)` | ✓ |
| `gamma = 1 - n_1/(parameter_t)(N - n_0)` | `1.0 - f64::from(n_1)/f64::from(n.wrapping_sub(n_0))` | ✓ (D1) |
| `B` guard `0==T-n_1 && 0==N-n_0-n_1 → 2` | same, on `wrapping_sub` | ✓ (D1) |
| `B = (T-n_1)/(parameter_t)(N-n_0-n_1)` | `f64::from(t_minus_n1)/f64::from(n_minus_n0_n1)` | ✓ (D1) |
| `Pr_G_3` `k==0 → 1-alpha` | same | ✓ |
| `k==1 → alpha*(1-gamma)` | same | ✓ |
| `k>1 → (alpha*gamma/(B-1)) * pow((1 - 1/(B-1)), k-2)` | `(alpha*gamma/(b-1.0)) * (1.0 - 1.0/(b-1.0)).powf(f64::from(k-2))` | ✓ |

The `k>1` branch preserves the exact parenthesisation: the prefactor
`alpha*gamma/(B-1)` is computed first (left-to-right: `alpha*gamma`, then
`/(B-1)`), then multiplied by `pow(base, k-2)`. `powf` is Rust's binding of
C `pow` (both call the platform libm `pow`). `base = 1 - 1/(B-1)` is
basic-ops (not class (a)); `pow` of it is the only transcendental, and it is
applied to bit-identical `double` operands, so the result matches term for
term. A partition test (`Σ_k Pr_G_3(k) → 1`) pins the whole family.

## 2. `gen` counting (`gen_k_mixture_model.cpp` → `generate.rs`)

`read_document` boundary logic (`:63-142`): `cur/last` carry across lines,
`null_token` in the second word is skipped, a `null_token` predecessor with
pi-gram training on becomes `sentence_start`, else the pair is skipped. Rust
`read_document` is line-for-line identical.

`train_word_pair` (`:144-217`), the arithmetic core:

| upstream | Rust (`train_word_pair`) | match |
| --- | --- | --- |
| exists: `cap = max((guint32)g_maximum_occurs, (guint32)ceil(m_Mr*rate))` | `max_occurs.max(ceil_mul(mr, rate))` | ✓ (D2) |
| `if count > cap` → subtract unigram, return | same | ✓ |
| `m_WC += count` | `wc = wc.wrapping_add(count)` | ✓ (D1) |
| `m_N_n_0 ++` | `n_n_0 = n_n_0.wrapping_add(1)` | ✓ (D1) |
| `if 1==count m_n_1++` | `if count==1 { n_1 = n_1.wrapping_add(1) }` | ✓ |
| `m_Mr = max(m_Mr, count)` | `mr = mr.max(count)` | ✓ |
| new item: `count > g_maximum_occurs` → subtract, return | `count > max_occurs` → subtract, return | ✓ |
| new: `m_WC=count; m_N_n_0=1; m_n_1=(1==count); m_Mr=count` | same | ✓ |
| header `m_WC += count` (only when not skipped) | `header_wc = header_wc.wrapping_add(count)` after the match, unreached on the early return | ✓ |
| unigram subtract `freq -= count; >0 keep / ==0 steal / else abort` | `wrapping_sub`; `==0` remove, else store | ✓ (D1) |

`ceil(m_Mr * g_maximum_increase_rates)`: `ceil_mul(mr, rate) = (f64::from(mr)
* rate).ceil() as u32` — `m_Mr` widened to `double`, multiplied, `ceil`, cast
to `guint32`. Pinned by `ceil_mul_matches_ceil_of_product` (0→0, 1·3→3,
7·3→21, 2·3.5→7).

`train_single_gram` delta (`:219-250`): `delta = header.m_WC - saved`; a
zero delta means every pair was over-cap, and the freshly-created empty row
is dropped. `magic.m_WC += delta` is overflow-guarded (`:280-284`). Rust:
`wrapping_sub` for the delta, `checked_add` for the magic WC (skip on
overflow — the upstream guard returns `false`, i.e. does not add).
`post_processing_unigram` (`:290-318`): `total_freq += freq` per unigram,
`magic.m_total_freq += total` overflow-guarded, and `header.m_freq += freq`
**via `set_array_header`**. That last step is backend-dependent, and the
oracle is pinned to **Tkrzw**: `flexible_ngram_tkrzwdb.h:411-413`'s
`set_array_header` does `m_db->Get(key)` and returns false when the key is
absent, so it only ever *updates* an existing single_gram — a token that
never appears as W1 (no bigram row) gets **no** array header. Its freq counts
toward `magic.m_total_freq` only. Rust matches: `total` always accumulates,
`header_freq` updates a row **only when it exists** (`self.grams.get_mut`,
not `entry().or_default()`) — oracle-verified against pin gen+export
(`tests/differential.rs`). `m_N++` per document: `wrapping_add(1)` (upstream
has no `m_N` overflow guard).

> **Oracle-discovered (2026-08-31).** The Kyoto backend's `set_array_header`
> *creates* the row, so a Kyoto-built pin would store W2-only headers; the
> Tkrzw-built pin (the oracle) does not. The live `gen`+`export` differential
> exposed the divergence (the native was creating W2-only headers), and the
> fix above brings it to Tkrzw parity. A consequence: a small-corpus model
> with W2-only tokens has `Σ header_freq < magic.m_total_freq`, so `validate`
> rejects it — the pin's own `validate` rejects the same model identically
> (exit 61); and merge≠combined on such tokens (the combined single-model run
> stores a later document's freq against a row an earlier document created,
> which the per-candidate merge cannot). At real corpus scale every token is a
> W1, so none of this is observable in the shipped model.

## 3. `estimate` score (`estimate_k_mixture_model.cpp` → `estimate.rs`)

`compute_interpolation` (`:36-96`), the per-context EM:

| upstream | Rust | match |
| --- | --- | --- |
| `lambda=0, next_lambda=0.6, epsilon=0.001` | `SEED_LAMBDA=0.6`, `EPSILON=0.001` | ✓ |
| `while fabs(lambda-next_lambda) > epsilon` | same (`+` iteration cap, D3) | ✓ |
| bigram term `elem = item.m_WC/(parameter_t)header.m_WC` if pair & `header.m_WC!=0` | `f64::from(item.wc)/f64::from(gram.header_wc)` under the same guards | ✓ |
| `numerator = lambda * elem` | same | ✓ |
| unigram term `elem = header.m_freq/(parameter_t)magic.m_total_freq` | `f64::from(gram.header_freq)/total_freq` | ✓ |
| `part = (1-lambda)*elem` | `(1.0-lambda)*unigram_poss` | ✓ |
| `if 0==(num+part) continue` | `if denominator==0.0 continue` | ✓ |
| `next_lambda += deleted_count*(num/(num+part))` | same | ✓ |
| `next_lambda /= header.m_WC` (deleted row) | `/= f64::from(deleted_gram.header_wc)` | ✓ |

Driver (`:98-155`): iterate `deleted` token1 rows with `header.m_WC != 0`,
score each, `lambda_sum += lambda; lambda_count++`, `average =
lambda_sum/lambda_count`. Rust matches; the `average lambda:%f` line
`estimate.py` sorts on is exactly `Estimate::average`. The upstream
`assert(0 != magic.m_total_freq)` (`:45`) becomes a class-(c) `Err` (D4a).

## 4. `merge` (`merge_k_mixture_model.cpp` → `merge.rs`)

`merge_two_phrase_array` equal-token merge (`:59-74`): `m_WC`, `m_N_n_0`,
`m_n_1` **add**, `m_Mr` **max**. Distinct tokens are appended in token
order (a sorted merge-join). Rust merges the two `token2 → item` maps with
`wrapping_add` on `wc`/`n_n_0`/`n_1` and `max` on `mr`; the BTreeMap keeps
the join token-ascending. Headers (`:159-162`) and magic (`:120-125`) add
field-wise; `m_N` adds. Magic `m_WC`/`m_total_freq` carry the upstream
overflow guard `a+b < max(a,b)` → `checked_add`/`EOVERFLOW` (D4b); item,
row-header, and `m_N` sums are unguarded `guint32` → `wrapping_add` (D1).

Merge **order independence**: item and header sums are commutative and
associative, `max` is too, and distinct rows/pairs are unioned — so the
merged model is a pure function of the multiset of inputs, independent of
the candidate-merge order (the sorted-index order only decides the
overflow-check short-circuit, unreachable for well-formed models). This is
what lets `merge_equals_single_run_over_both_documents` hold and the
top-N merge be deterministic.

## 5. `prune` CDF (`prune_k_mixture_model.cpp` → `prune.rs`)

Per pair: `remained_poss = 1 - Σ_{k=0}^{K-1} Pr_G_3_with_count(k, N,
m_WC, N - m_N_n_0, m_n_1)`; each `one_poss` range-checked to `[0,1]`;
`fabs(remained) < DBL_EPSILON → 0`; `errors || remained∉[0,1]` → `exit(EDOM)`;
`if remained < g_prune_poss` prune. Rust `survival` matches exactly
(`n_0 = n.wrapping_sub(m_N_n_0)`, `f64::EPSILON` snap, `KmmError::Domain`
for the `EDOM` case — D4c). On prune: `header.m_WC -= wc`, `magic.m_WC -= wc`,
`magic.m_total_freq -= wc`, and post-pass `W2.m_freq -= wc` (`:159-169`),
then drop rows with `m_WC==0 && m_freq==0` (`:179-186`). Rust uses
`wrapping_sub` throughout (D1) and `retain` for the cleanup.

Prune **decide-then-apply** (D5): upstream interleaves the survival test and
the WC mutations. The survival math reads only `magic.m_N` — constant during
a prune — and each pair's own counts, never a count another pair's removal
mutated. So computing all decisions first (pass 1, read-only) then applying
them (pass 2) yields the identical set of removals and the identical final
magic totals (the totals are order-independent sums). This is a restructuring
with identical output, not a divergence; documented in `prune.rs`.

## 6. export / import / to-interpolation (`text.rs`)

`export` grammar is byte-identical to `export_k_mixture_model`: the `\data`
header, `\1-gram` `\item TOKEN WORD count HEADER_WC freq HEADER_FREQ`,
`\2-gram` `\item T1 W1 T2 W2 count WC T WC N_n_0 N_n_0 n_1 n_1 Mr Mr`
(note `T ≡ m_WC`), `\end`. The native walks token-ascending (BTreeMap); the
pin walks in DBM `get_all_items`/`retrieve_all` order, which for the **Tkrzw**
backend is **hash-iteration order, not token-ascending** (oracle-observed —
an earlier draft of this audit wrongly claimed they coincide). The KMM `.db`
is an unordered DBM, so record *order* carries no meaning: the native
canonicalises to token-ascending, and the live differential compares the
sorted item *set*, not bytes. A record whose token has no phrase text is
skipped (`taglib_token_to_string → NULL`). `import` is the exact inverse. `to-interpolation` (`k_mixture_model_to_interpolation.cpp`) is the
streaming transform: header → `\data model interpolation`; `\1-gram` emits
`\item TOKEN WORD count FREQ` **from the KMM `freq` field**, dropping
`sentence_start` and `freq==0`; `\2-gram` emits `\item T1 W1 T2 W2 count WC`.
`text.rs::kmm_text_to_interpolation` matches field-index for field-index, and
because it streams the token-ascending export it preserves the output
ordering. Pinned by `export_grammar_matches_upstream` and
`to_interpolation_drops_start_and_zero_freq` byte goldens.

## 7. Intentional divergences (compatibility-policy classes)

| id | site | upstream | Rust | class | observable-compat argument |
| --- | --- | --- | --- | --- | --- |
| D1 | γ, B, gen/merge/prune integer updates | `guint32` `+`/`-` (silent wrap) | `wrapping_add`/`wrapping_sub` | mechanism (bit-identical) | `wrapping_*` reproduces the two's-complement `guint32` result bit-for-bit, and for the model invariants (`n_0 ≤ N`, `n_0+n_1 ≤ N`, `count ≤ freq`) no wrap occurs. It never panics, so a malformed model wraps identically instead of triggering Rust's debug overflow panic. **Not a behaviour change.** |
| D2 | `ceil_mul` cast | `(guint32)ceil(...)` | `.ceil() as u32` | mechanism (unreachable) | Differs only when `ceil(m_Mr·rate) > u32::MAX`, i.e. `m_Mr > ~1.43e9` — impossible for a per-document pair count. There, C's `(guint32)` of an out-of-range `double` is itself UB (no defined upstream value to match); Rust saturates. Unreachable in practice. |
| D3 | estimate EM loop | unbounded `while` | `&& iterations < 100_000` | availability (inert) | The EM is a contraction converging in a few dozen steps for real data; the cap is never reached where upstream terminates, so the returned λ is identical. Where upstream would not terminate it produces **no** output (hangs); a bounded library call is strictly better and cannot diverge from an absent value. |
| D4a | estimate `magic.m_total_freq==0` | `assert` (abort) | `Err` + point | (c) availability | Textbook class (c): upstream aborts on caller input, oxpinyin returns `Err` and names the site. |
| D4b | merge magic `m_WC`/`m_total_freq` overflow | `fprintf` + return `false` → `exit(EOVERFLOW)` | `Err` | (c) availability | Same shape: upstream fails the process, oxpinyin returns `Err`. |
| D4c | prune `remained ∉ [0,1]` | `exit(EDOM)` | `Err(Domain)` | (c) availability | Same shape. |
| D5 | prune interleave | decide+mutate interleaved | decide-then-apply two-pass | equivalence | Proven identical output (survival reads only constant `m_N` + each pair's own counts). Not a divergence. |
| D6 | export/serialisation order | Tkrzw `get_all_items` hash order (unordered) | BTreeMap token-ascending (canonical) | canonicalisation | The KMM `.db` is an unordered DBM, so record order is not semantic. The native emits a deterministic token-ascending order; the live differential compares the sorted item *set* (oracle-verified). Not a byte-parity target — matching Tkrzw's hash order is neither possible nor meaningful. |
| D7 | W2-only unigram header | Tkrzw `set_array_header` no-ops on absent key → no header | native stored all headers → **fixed** to match | defect fixed | Was a real divergence (native created W2-only headers a Kyoto pin would, but the Tkrzw-pinned oracle does not). Now token2-only tokens get no header (`generate.rs`), oracle-verified. |

The one behaviour the audit changed to stay inside the policy: the candidate
score's empty-deleted-model case. Upstream computes `lambda_sum/lambda_count`
with `lambda_count==0`, i.e. `0.0/0 = NaN`, and prints `average lambda:nan`
(not an abort — so returning `Err` there would be a divergence *outside* the
four classes). `estimate.rs` now reproduces the NaN (`0.0 / 0.0` is NaN in
IEEE-754), keeping the `Result` contract without diverging. Pinned by
`empty_deleted_model_scores_nan_like_upstream`. The case never arises in the
real pipeline (the deleted model always carries scorable contexts).

## 8. Determinism (constitution item 6)

Every KMM stage is a pure function of its inputs: the counting fold is
order-independent within a document (each `(t1,t2)` occurs once; the
over-cap unigram subtraction is commutative), the merge and prune
aggregations are order-independent across inputs (§4, §5), and every walk is
token-ascending via the ordered maps. So the whole `oxpinyin-kmm` chain
—generate → estimate → merge → prune → export → convert— is deterministic
and independent of the DBM/hash iteration order upstream happens to use,
which is the property that makes the byte-level differential (§6) and the
semantic-parity harness (`crates/oxpinyin-kmm/tests/`) reproducible.
