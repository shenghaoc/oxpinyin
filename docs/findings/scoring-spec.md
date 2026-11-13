# Scoring

Status: **reference**

This page defines the scoring quantities that oxpinyin's lookup and decoding use.
The candidate ranking key is in [candidate-construction.md §8.2](candidate-construction.md#82-the-gate-and-the-frequency-key).
Sentence scoring and its float rules are in the [compatibility policy](compatibility-policy.md#algorithm-precondition-violations) and [sentence-surface.md](sentence-surface.md).

## The interpolated bigram

The language-model term is the interpolation the pin uses:

```text
P(w_n | w_n-1) = λ · P_bigram(w_n | w_n-1) + (1 − λ) · P_unigram(w_n)
```

- **λ** comes from the model's `table.conf`. `TableConf::load` scans it with the pin's `fscanf("%f")` and keeps that `f32`, whatever its value; the pinned model gives `0.312699` ([data-formats.md §3](data-formats.md)).
- **`P_unigram`** is the phrase item's stored unigram field over the facade total, as every upstream reader computes it (`PinyinLookup2`, `_compute_frequency_of_items`, `train_result3`). The stored field is the corpus count plus the 1 that `gen_unigram` adds to every item, so an item that the corpus never saw has probability `1/Σ`. On the pinned model, `Σ item == Σ \1-gram count + 138,096`.
- `crates/oxpinyin-data/src/lm/mod.rs` implements the term through the `LanguageModel` seam ([core-trait-seam.md](core-trait-seam.md)).

## Sentence scores

Sentence scoring accumulates `f32` log probabilities (`crates/oxpinyin-engine/src/nbest.rs`), as the pin's `gfloat m_poss` does (`src/lookup/phonetic_lookup.h:41-52,663,692-693` at `074a2219`).
Candidate order is the list order; candidates carry no separate integer score.

## Incomplete keys

An incomplete key stands for every complete key with the same phonetic initial (`ChewingKey.m_initial`), in inventory order.
String-prefix expansion is wrong: `n` must not reach zero-initial `ng`, and `z`, `c` and `s` must not reach `zh`, `ch` and `sh`.
Example: `nih` offers `你好`, `霓虹`, `拟合` and other two-key phrases whose second syllable starts with `h`.

The runtime dictionaries answer a partial-key query in one probe (`Dictionary::handles_partial_keys`).
For a dictionary that does not, `oxpinyin_core::scoring::expand_keys` expands the keys as a Cartesian product.
Above the caller's limit (`SCAN_EXPANSION_LIMIT`, 4,096, in `crates/oxpinyin-engine/src/session/mod.rs`), the expansion yields nothing rather than a truncated subset, so a caller cannot mistake a truncation for the whole answer.
