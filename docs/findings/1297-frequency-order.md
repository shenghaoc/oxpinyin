# 1297-frequency-order — Compare frequency keys without unsigned subtraction

- **Status:** accepted · 2026-10-26 UTC · issue #1297.
- **Disposition:** POLICY-ACCEPTED (never MATCH).
- **Policy limb:** language artefact, limb 3 under #1303 / merged #1304. A language-neutral ranking requirement does not name frequency comparator subtraction. This is neither a transcendental precision difference (a), a memory-safety substitution (b), nor an abort refusal (c).
- **Pin:** libpinyin 2.11.92 at 074a2219; tree read: a checkout at 074a2219 whose cited blobs were hashed against the pin. Reference GLib 2.84.4; both facades; backend-independent logic (bdb gate).
- **Pin derivation:** `pinyin.cpp:1678-1709,2440-2445`; `zhuyin.cpp:1129-1144`. Existing binary evidence for the conversion instructions is retained in [921-927-glib-sorts.md](921-927-glib-sorts.md).
- **Rust counterpart:** `crates/oxpinyin-engine/src/session/mod.rs` and `crates/oxpinyin-capi/src/predict.rs`; affected C entry points: guess_candidates, guess_candidates_before_cursor, guess_candidates_after_cursor, guess_predicted_candidates, guess_predicted_candidates_with_punctuations.
- **Forward trace:** C facade parse / validation -> engine session matrix and dictionary/model lookups -> trellis or candidate construction -> ranking / dedup -> sentence and candidate getters. Predicted variants collect prefix/bigram rows, use the same score and ordering, then prepend punctuation where requested.
- **Backward trace:** ordinary and offset candidate windows, the before-cursor zhuyin path, prefix-seeded and constrained sentence guesses, addon and LONGER candidates, and plain / punctuation prediction are included as applicable. Both facades delegate decoding and window ranking to the engine. Backend adapters provide counts and items; they do not choose these orders. The fallback model without real unigrams uses its existing cost path. Punctuation itself is prepended rather than frequency-sorted.
- **Input partition / affected inputs:** For lambda in [0,1] the pin's amplified keys are below 2^24, so this subtraction alone does not change their order. Outside that range the wrap can change order independently of the float-to-integer score conversion. Disabled fields compare equally and ties preserve collection order.
- **Difference:** The pin subtracts guint32 frequencies and returns the negated difference as gint. Gaps at least 2^31 can reverse comparisons and create cycles. Rust compares the enabled fields lexicographically with Ord, descending, using stable sort_by. No subtraction or library-specific merge sequence participates.
- **Why accepted:** the replaced operation is a language/library artefact classified by the pinned #1306 Phase 2 plan. Rust expresses the specified product ranking directly. No product comparator constant, limit, option meaning or interpolation rule is replaced.
- **Contract preserved:** C ABI/layout, exported symbols, real GArray allocation and g_free ownership/lifetime are unchanged. System and user file formats and data ownership are unchanged. Process-wide side effects and diagnostics are unchanged. The documented list/sentence order and text-dedup winner are the affected observables; selecting a changed row naturally selects its existing token path.
- **Complexity:** Both sorts are stable O(n log n) with O(n) scratch. Rust's stable sort replaces the recursive GLib port and predicted index permutation.
- **Tests:** existing contract-diff driver, issues #921/#927/#594/#1275, table-conf-lambda, prediction and punctuation cases; n-best choose/train differentials. Changed cases receive exact POLICY-ACCEPTED observations, retained controls remain MATCH. A named mutation verifies the selection. No new generic driver is required.
- **Prior rulings:** none reversed. LC_NUMERIC is unchanged.
- **Evidence:** BDB setup uses `tools/cloud/setup.sh --cells bdb --cache-dir <private-cache>`; the completion command and final results are recorded with the PR. Captures stay local and disposable.
- **Conclusion:** accepted under the maintainer's explicit Phase 2 instruction and language-artefact policy, 2026-10-26 UTC.

## Exact source-derived witness

The case imports 龘, 龘甲 and 龘乙 with counts 1/1/10,000,000. Their unigrams are 3/3/30,000,000. With lambda -500, the high amplified score is between 2^31 and 2^32, while the low key is below 512. The high key fits guint32, so integer conversion does not reverse its numeric order. The comparator's unsigned subtraction returned as signed reverses the two-element comparison: the pin keeps 甲 then 乙; Rust orders the score as 乙 then 甲. No GLib merge comparisons with other candidates intervene: the model20 text tables contain no other 龘-prefixed phrases.

```contract-diff-policy
{
  "schema": 1,
  "record": "1297-frequency-order",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "ranking-frequency-gap": {
      "pin": {
        "exit": 0,
        "result": {
          "ret": true,
          "rows": [
            [
              5,
              "甲"
            ],
            [
              5,
              "乙"
            ]
          ],
          "logs": []
        },
        "stderr": "open SCRATCH/user-X/user.conf failed."
      },
      "subject": {
        "exit": 0,
        "result": {
          "ret": true,
          "rows": [
            [
              5,
              "乙"
            ],
            [
              5,
              "甲"
            ]
          ],
          "logs": []
        },
        "stderr": "open SCRATCH/user-X/user.conf failed."
      }
    }
  }
}
```
