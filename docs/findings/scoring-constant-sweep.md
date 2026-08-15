# Scoring constant sweep

Date: 2026-08-10 · Status: **measured, values frozen in `ScoringConfig::default`**

The functional form of the scorer is frozen in `docs/findings/scoring-spec.md`.
That SPEC deliberately left the numeric weights provisional. This finding
records the sweep that settles them against the real exported tables and the
frozen candidate fixture `fixtures/w4/oracle-candidates.txt`.

## Method

- Engine: `Session<SystemDictionary, BigramLanguageModel>` with unigrams
  installed (`set_unigrams_from_dict`), interpolated bigram scoring as of
  the parity-climb bigram commit.
- Metric: top-1 against the pin's first candidate (primary); top-5-set and
  absent reported for context.
- Sample: 2,000 inputs drawn as every 5th entry of the 10,190 corpus members
  that have oracle candidates (stratified; the alphabetical head of the
  corpus is short-input-biased and was discarded as a sample).
- Tool: `cargo run -p pinyin-oracle --release --bin parity-sweep`.
- Constraints preserved from the capture inequalities:
  - `phrase_key_bonus > 0`
  - `segmentation_penalty > 0`
  - `incomplete_penalty < phrase_key_bonus`

## First pass — ±50% of the provisional defaults

Provisional defaults were `seg=500`, `inc=1000`, `bonus=2000`.

| Trial | seg | inc | bonus | sample top-1 |
|---|---:|---:|---:|---:|
| baseline | 500 | 1000 | 2000 | 41.9% |
| seg −50% | 250 | 1000 | 2000 | 39.0% |
| seg −25% | 375 | 1000 | 2000 | 40.6% |
| seg +25% | 625 | 1000 | 2000 | 43.5% |
| seg +50% | 750 | 1000 | 2000 | 44.2% |
| inc −50% | 500 | 500 | 2000 | 39.7% |
| inc −25% | 500 | 750 | 2000 | 40.0% |
| inc +25% | 500 | 1250 | 2000 | 45.5% |
| inc +50% | 500 | 1500 | 2000 | **54.6%** |
| bonus −25% | 500 | 1000 | 1500 | **54.8%** |
| bonus +25% | 500 | 1000 | 2500 | 39.9% |
| bonus +50% | 500 | 1000 | 3000 | 39.2% |

Directions: **raise** `segmentation_penalty`, **raise** `incomplete_penalty`
toward `phrase_key_bonus`, **lower** `phrase_key_bonus`. Joint trials that
raised the bonus above 2000 lost.

## Focused grid

Around the first-pass winners:

| seg | inc | bonus | sample top-1 |
|---:|---:|---:|---:|
| 500 | 1000 | 1500 | 54.8% |
| 750 | 1000 | 1500 | 54.9% |
| 750 | 1000 | 1200 | 62.8% |
| 500 | 1000 | 1200 | 62.1% |
| 500 | 1400 | 1500 | 61.6% |
| 750 | 999 | 1000 | **63.8%** |
| 500 | 999 | 1000 | 62.6% |

## Full-corpus confirmation

| Config (seg / inc / bonus) | top-1 | top-5-set | prefix-10 | absent |
|---|---:|---:|---:|---:|
| pre-sweep (500 / 1000 / 2000) | 41% | 69% | 43% | 170 |
| **750 / 999 / 1000 (chosen)** | **63%** | **89%** | **65%** | **177** |
| 750 / 1000 / 1200 | 62% | 91% | 61% | 162 |
| 500 / 1000 / 1200 | 61% | 92% | 60% | 105 |

Chosen values maximise top-1 on the full corpus. The 999 / 1000 pair is
intentional: `incomplete_penalty` must stay strictly below `phrase_key_bonus`
(capture inequality 3), and the measured maximum sat on that boundary.

## Frozen defaults

```text
lm_weight             = 100   (WEIGHT_SCALE; unchanged)
exact_penalty         = 0
segmentation_penalty  = 750
incomplete_penalty    = 999
phrase_key_bonus      = 1000
expansion_limit       = 64    (unchanged)
```

Re-run the sweep with `cargo run -p pinyin-oracle --release --bin parity-sweep`
if the scorer form or the data tables change.

## λ read from config (2026-08-15)

The interpolation weight λ was an authored, deliberately-neutral `1/2`. It is
now **read from the model's `table.conf`** instead of hardcoded. The pinned
value is `0.312699` (recorded in `data-formats.md` §3, verified against the
oracle's installed copy). The decode LM (`pinyin_data::lm`) defaults to
`Lambda::PINNED` (`0.312699`) and overrides it from an install's `table.conf`;
the fetched cache ships none (`table.conf` is a config artifact, not part of
`model20.text.tar.gz`), so the default stands there.

**Representation.** λ is held as the exact decimal rational the file denotes,
reduced to lowest terms (`0.312699` → `312699 / 1_000_000`), so the
interpolation stays in the deterministic u128 ratio (no float in the hot
path — constitution §6). `from_decimal` rejects a value `> 1` (λ is a weight in
`[0, 1]`) and `model_cost` uses checked arithmetic, so a malformed `table.conf`
floors at `UNKNOWN_COST` rather than overflowing.

**Ranking impact is path-specific.** Candidate ordering has two paths
(`Session::refresh`, gated on `LanguageModel::has_real_unigrams`):

- **Real-unigram path** (`set_unigrams_from_interpolation2`, used by
  `real_tables_session_reports_parity`): candidates rank by the three-key order
  (text length, pinyin span, real unigram count), which does *not* read the
  interpolated `model_cost`. This path is **λ-insensitive** — re-running the
  release parity test with λ = `0.312699` reproduces the frozen pins
  bit-identically:

  ```
  compared            10190
  top-1               10136   (unchanged)
  top-5-set           10182   (unchanged)
  prefix-10 overlap   94456 of 98930 (unchanged)
  absent                  1   (unchanged)
  ```

- **Export-ABI path** (`set_unigrams_from_dict`, `real_unigrams == false`):
  candidates sort by `Candidate::cost`, which includes the interpolated
  `model_cost`, so λ **does** reorder them. This path is what the shipping C
  ABI (`CapiContext::new`) and the `parity-sweep` / `parity-worst` tools use.
  No pin covers it — the parity fixture is measured through the real-unigram
  path — so those lists reorder unmeasured with the λ change.

The change is correct regardless: it makes the decoder use the model's actual λ
instead of an arbitrary `1/2`. Its effect surfaces in the full-sentence Viterbi
decode and in the export-ABI candidate order; no real-model differential covers
either on `main` yet (the sentence/segmentation `decode_differential` runs on
the mini-fixture LM). No re-pin was required for the real-unigram pins.
