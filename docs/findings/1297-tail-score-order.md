# 1297-tail-score-order — Stable ordering of final tails by score

- **Status:** accepted · 2026-10-26 UTC · issue #1297.
- **Disposition:** POLICY-ACCEPTED (never MATCH).
- **Policy limb:** language artefact, limb 3 under #1303 / merged #1304. A language-neutral ranking requirement does not name tail comparator conversion. This is neither a transcendental precision difference (a), a memory-safety substitution (b), nor an abort refusal (c).
- **Pin:** libpinyin 2.11.92 at 074a2219; tree read: a checkout at 074a2219 whose cited blobs were hashed against the pin. Reference GLib 2.84.4; both facades; backend-independent logic (bdb gate).
- **Pin derivation:** `src/lookup/phonetic_lookup.h:174-178,329-339`. Existing binary evidence for the conversion instructions is retained in [921-927-glib-sorts.md](921-927-glib-sorts.md).
- **Rust counterpart:** `crates/oxpinyin-engine/src/nbest.rs`; affected C entry points: guess_sentence, guess_sentence_with_prefix, phrase_segment.
- **Forward trace:** C facade parse / validation -> engine session matrix and dictionary/model lookups -> trellis or candidate construction -> ranking / dedup -> sentence and candidate getters. Predicted variants collect prefix/bigram rows, use the same score and ordering, then prepend punctuation where requested.
- **Backward trace:** ordinary and offset candidate windows, the before-cursor zhuyin path, prefix-seeded and constrained sentence guesses, addon and LONGER candidates, and plain / punctuation prediction are included as applicable. Both facades delegate decoding and window ranking to the engine. Backend adapters provide counts and items; they do not choose these orders. The fallback model without real unigrams uses its existing cost path. Punctuation itself is prepended rather than frequency-sorted.
- **Input partition / affected inputs:** Valid unequal finite scores can reorder; exact Cost ties keep the selected order. NaN Costs are placed last; the infinities retain their integer score order. The trellis predicate, special-cost arithmetic and extraction counts do not change.
- **Difference:** The pin returns a float difference as gint. Scores less than one natural-log unit apart tie through truncation; the amd64 invalid conversion gives INT_MIN for NaN and out-of-range differences. Rust stably orders the fixed-point Cost itself, ascending (best first), placing NAN_COST last, without reconstructing a float possibility or truncating a difference.
- **Why accepted:** the replaced operation is a language/library artefact classified by the pinned #1306 Phase 2 plan. Rust expresses the specified product ranking directly. No product comparator constant, limit, option meaning or interpolation rule is replaced.
- **Contract preserved:** C ABI/layout, exported symbols, real GArray allocation and g_free ownership/lifetime are unchanged. System and user file formats and data ownership are unchanged. Process-wide side effects and diagnostics are unchanged. The documented list/sentence order and text-dedup winner are the affected observables; selecting a changed row naturally selects its existing token path.
- **Complexity:** Stable slice sorting has O(k log k) time and O(k) scratch; k is at most three. The old merge had the same asymptotic bounds.
- **Tests:** existing contract-diff driver, issues #921/#927/#594/#1275, table-conf-lambda, prediction and punctuation cases; n-best choose/train differentials. Changed cases receive exact POLICY-ACCEPTED observations, retained controls remain MATCH. A named mutation verifies the selection. No new generic driver is required.
- **Prior rulings:** none reversed. LC_NUMERIC is unchanged.
- **Evidence:** BDB setup uses `tools/cloud/setup.sh --cells bdb --cache-dir <private-cache>`; reproduction methods are below; retained results and limitations are recorded in PR #1314. Captures stay local and disposable.
- **Conclusion:** accepted under the maintainer's explicit Phase 2 instruction and language-artefact policy, 2026-10-26 UTC.

## Retained corpus attribution method

Before the scanner rebase, the authorized tail fallback ran once because
the selection baseline retained no per-tail costs. The existing diagnostic
`sentence-tail` driver decodes with production selection held fixed and
changes only the final tail sort. It runs both ranking shapes over the same
nine pinyin corpus strata, not a separate zhuyin C facade or Bopomofo corpus.
The earlier one-store shape is therefore distinct from the actual-facade
candidate-window coverage in [1297-ranking-score](1297-ranking-score.md).
Independent tournament and tail-sort counts may overlap; their intersection
and the combined net result are not inferred. The retained counts are in
[PR #1314](https://github.com/shenghaoc/oxpinyin/pull/1314).

These are reproduction commands for that retained run; no rerun is authorized
while #1314 is parked.

Reproduce the tail attribution after provisioning with `tools/cloud/setup.sh --cells 'bdb kc tkrzw'` and sourcing its environment:

```sh
just heavy -- cargo build --locked -p pinyin-oracle --features diagnostic-ranking --bin sentence-tail
PINYIN_EXPORT_DIR="$OXPINYIN_ORACLE_DATA_BDB" just heavy -- target/debug/sentence-tail --tail-sort-attribution
```

## Exact source-derived tail witness

`ranking-close-tail-scores` uses lambda zero, only user phrase items, and
the divided-table parse of xian. Importing 龘 (xian, count 100) and 靐靐
(xi'an, count 110) gives unigrams 300 and 330, total 630. The natural-log
score gap is log(1.1), below both log(1.2) and one unit. The fixed-point
gap is 137, below 263. Both selectors therefore prefer the shorter 龘
before extracting both tails. The pin's final comparator truncates their
score difference to zero and preserves that order; Rust's final Cost sort
places the better-scoring 靐靐 first. No selection tie or frequency-key
conversion is involved. The fresh-profile diagnostic is unchanged.

For the existing `table-conf-lambda-negative-two` and
`table-conf-lambda-one-and-a-half` pinyin cases, placing NAN_COST last
changes the `yishu` head from 医术 to 艺术 and exchanges the first two
candidate texts. The complete observations remain in
[1297-selection-structure.md](1297-selection-structure.md), because these
inputs also exercise directed selection and score-key differences. Their
counts, save/train/add results, file summaries and diagnostics are unchanged.
This is the sentinel-order consequence of removing the comparator's invalid
float-to-integer conversion, rather than a change to `cost_add` or `cost_gt`.

```contract-diff-policy
{
  "schema": 1,
  "record": "1297-tail-score-order",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "ranking-close-tail-scores": {
      "pin": {
        "exit": 0,
        "result": {
          "parsed": 4,
          "guessed": true,
          "sentences": [
            "龘",
            "靐靐"
          ],
          "logs": []
        },
        "stderr": "open SCRATCH/user-X/user.conf failed."
      },
      "subject": {
        "exit": 0,
        "result": {
          "parsed": 4,
          "guessed": true,
          "sentences": [
            "靐靐",
            "龘"
          ],
          "logs": []
        },
        "stderr": "open SCRATCH/user-X/user.conf failed."
      }
    }
  }
}
```
