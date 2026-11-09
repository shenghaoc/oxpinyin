# #1378 — zhuyin selection order

- **Status:** accepted under the maintainer's 2026-10-29 UTC ruling
  “No ported external-library internals”; recorded 2026-10-29T08:41:25Z (`date -u`).
- **Disposition:** POLICY-ACCEPTED, never MATCH.
- **Policy limb:** internal language/library artefact. The choice of equal
  elements made by libstdc++'s heap schedule is not a product ranking key,
  a transcendental precision difference, a memory-safety refusal or an abort.
- **Pin:** libpinyin 2.11.92 at `074a2219`; read from the pinned git tree,
  `phonetic_lookup.h` blob `c092e7610cb0`. Facade: zhuyin. Each difference
  below was verified on BDB, KC and sound-tkrzw.
- **Pin derivation:** `src/lookup/phonetic_lookup.h:66-91,141-172,331-335`
  compares equal-length raw possibilities without an identity tie key,
  then runs the standard heap algorithms. `src/zhuyin.cpp:50` chooses
  the one-store, one-result shape. User reading insertion order is the
  tone/key order of `src/storage/chewing_large_table2.h:208-231` with
  `pinyin_phrase3.h:33-63`; the fixture adds tokens in the opposite order.
- **Rust counterpart:** `nbest.rs`, `selection_loses_to`: length ascending,
  raw f32 possibility descending, NaN last, signed zeros equal, then earlier
  batch position. Single-slot admission still uses the pin's raw `<`.
- **Forward trace:** `zhuyin_begin_add_phrases` / iterator → stored user
  readings → toneless `dada` parse → `zhuyin_guess_sentence` → beam and
  final selection → `zhuyin_get_sentence`. No sort or cleanup of stored
  records occurs. The selected sentence is the observable.
- **Backward trace:** every zhuyin beam and final-tail selection uses this
  same comparator. Pinyin uses adjusted S and its existing batch tie key;
  phrase segmentation uses raw `<` / `>` scans. Candidate-frequency and
  predicted sorting, storage writes, and the C ownership paths are unchanged.
- **Input partition:** ordinary distinct scores keep the length/raw-score
  keys; equal scores use batch position. NaNs rank last at equal length;
  signed zeros tie. The case adds equally counted `龘龘` (tone 3) then
  `靐靐` (tone 2), parses toneless `dada`, and reads the one sentence.
- **Difference:** pin `龘龘`; subject `靐靐`. Both parse 4 bytes, guess
  successfully, retain tokens `0x7000001` and `0x7000002`, and emit the
  same fresh-user diagnostic. Tone-2 `靐靐` is earlier in the stored reading
  walk and therefore the candidate batch. No arithmetic difference is
  needed to explain this equal-count, equal-length tie.
- **Contract preserved:** C ABI/layout, real GLib ownership, exported symbols,
  persistent bytes/formats, data ownership, diagnostics and process effects
  are unchanged. Only the ruled sentence tie differs.
- **Tests:** `nbest-user-table-order-zhuyin` is POLICY-ACCEPTED. `oxpinyin-engine`'s
  `nbest::tests::selection_uses_batch_ties_and_zhuyin_length_first_nan_last` and
  `nbest::tests::the_zhuyin_shape_keeps_one_tail_with_batch_ties` hold it. The pinyin
  counterpart retains its existing S2 record. `glib-sort-tails-zhuyin` and
  `table-conf-lambda-one-zhuyin` are controls. Unit tests cover NaN, signed
  zero and earlier batch position; the full BDB contract run covers the
  other call routes. No new driver or dependency.
- **Prior rulings:** the 2026-10-29 ruling supplies these exact keys. It
  preserves single-slot admission and pinyin front eviction.
- **Reproduction:** provision with `tools/cloud/setup.sh`; build debug
  artifacts with `just cell-artifacts`; run `
  the recorded runner contract-diff bdb "$PINYIN_ORACLE_PREFIX" "$OXPINYIN_CAPI_SO"
  "$OXPINYIN_ZHUYIN_SO" -- --cases nbest-user-table-order-zhuyin,glib-sort-tails-zhuyin,table-conf-lambda-one-zhuyin`.
  Verification output stays local and disposable.

The same key completes ordering when λ exceeds one. The unigram coefficient
is negative (`phonetic_lookup.h:651-663,712-713`), so unigram paths take
`log(negative)` and carry NaN; blended paths can remain finite
(`:679-693`). The pin's `<` predicate leaves NaN unordered. The ruled
selection key puts NaN last while preserving the single-slot admission
rule. The three additional cases are POLICY-ACCEPTED:

| Case | Rank-0 `ni3hao3`, pin → subject | Rank-0 `yi4shu4`, pin → subject |
| --- | --- | --- |
| `table-conf-lambda-one-and-a-half-zhuyin` | 儞䝥 → 你好 | 苅襩 → 艺术 |
| `table-conf-lambda-two-zhuyin` | 儞䝥 → 你好 | 苅襩 → 一书 |
| `table-conf-lambda-million-zhuyin` | 儞䝥 → 你好 | 苅襩 → 亦舒 |

Candidate-prefix/count changes follow the chosen NBEST sentence and the
pin's existing sentence-over-phrase dedup; candidate-frequency sorting
itself is unchanged. The four cases above are POLICY-ACCEPTED on BDB, KC and
sound-tkrzw.

## Candidate-sort amendment (#1297 item A)

The selection/sentence results above stay as recorded. The maintainer
decision dated 2026-10-30 preserves the wrapping product comparison,
allowing defined fallback only for cyclic lists. On BDB, KC and sound-tkrzw,
λ=1.5 and λ=2 restore exactly main's previous complete subject observations;
their accepted differences remain the independent selection ruling above.
Only λ=million retains a new candidate-view difference: cyclic groups use
stable numeric fallback while consistent groups retain wrapping order.
Its guard, scope and source proof are in
[1297-candidate-numeric-sort](1297-candidate-numeric-sort.md). All three
cases stay POLICY-ACCEPTED on each cell. The pin's result does not change;
only oxpinyin's candidate view does. All four
`row58-signed-{score,duplicate}-{off,on}` cases MATCH on every cell; this
record never registers them as differences.

## Conversion amendment (#1416)

#1416 stores the candidate frequency with Rust's `as u32` instead of the
amd64 `cvttss2si` imitation (decision-order limb 3 ([source policy](../source-policy.md#internal-language-artefacts))). For λ > 1 every
positive-unigram candidate score is negative and now stores 0, so the
length-1 rows keep their collection order instead of the wrapped-key
order the amd64 pin produces; the arm64 pin stores 0 too. The three
λ > 1 cases stay POLICY-ACCEPTED on bdb, kc and sound tkrzw with that
oxpinyin candidate view (2026-10-30 UTC); the pin's result does not
change. The difference is class (b) in
`upstream-divergences.md` ("Zhuyin candidate frequency: negative
`float`-to-`guint32` store at λ > 1").
