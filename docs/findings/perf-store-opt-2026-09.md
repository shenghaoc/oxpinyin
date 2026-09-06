# Performance Store Optimizations — 2026-09-06

## Executive summary

PRs #340 (`opt/redb-alloc-reduction`) and #341 (`opt/lmdb-backend`)
landed six store-layer changes on main. One of them has a clean, reliable
measurement; this document records that one and says why the other
five do not.

- **S5 — `export_phrases` single-walk pronunciation collection
  (#341)**: 2.2× faster at every size measured, 64–1024 phrases. The
  saving is 0.36–0.44 µs per phrase (0.36 at 64, 0.44 at 1024 — 450 µs
  per call at 1024 phrases) and it is the removal of one LMDB read transaction plus one cursor
  open per phrase, replaced by a single ordered walk of the
  pronunciation table. Scaling stays linear on both sides.
- The redb allocation trims (F1/F3), the redb `is_empty` header probe
  (F4), the LMDB `bulk_load_raw` path (S4), and `MDB_WRITEMAP` (S3)
  are **not measured** here; the two fsync-dominated benches are filed
  for redesign as #342 and #343.

These are x86_64 numbers from one host, branch-vs-branch on the same
build; no libpinyin cell exists for these backends and none is
claimed.

## Experimental design

### Commits

| Role | Commit | Content |
|---|---|---|
| Baseline | `ab56dc79` | merge-base of #340 and #341, before either landed |
| After | `a81570d6` | `origin/main` with #340 and #341 merged |

The bench sources were identical at both commits (cherry-picked onto
the baseline); only the crates under test differ.

### Harness

- Host: x86_64 Linux 6.12, 12 cores, this checkout's
  `rust-toolchain.toml`, `cargo bench` release profile.
- criterion 0.8, 100 samples per ID, `--save-baseline` at `ab56dc79`
  then `--baseline` at `a81570d6`; criterion's own change verdict is
  the one reported.
- Backend: `--no-default-features --features lmdb` for S5 (the walk is
  backend-agnostic in `oxpinyin-user`; LMDB is the backend #341
  targets).
- Data: deterministic synthetic phrases, one pronunciation each, keys
  in the renderable syllable range, no model fixture. The store is
  seeded once per N outside the criterion loop; only the
  `export_phrases` call is timed.

The bench that ships with this document —
`crates/oxpinyin-user/benches/export_phrases.rs`, group
`user_export_phrases`, IDs `phrases/{64,256,1024}` — is the same shape
with longer phrase texts (`phrase_{i}`, 8–11 keys) than the
measurement run used (2 CJK characters, 2 keys), so its absolute
numbers on main run ~1.5–1.8× above the table below; the ratio is what
the bench is for.

## Results — S5

| Bench ID | Baseline `ab56dc79` | After `a81570d6` | Δ | criterion verdict | stddev / mean |
|---|---:|---:|---:|---|---|
| `user_export_phrases/phrases/64` | 40.83 µs | 17.77 µs | −56.5 % | improved (p < 0.05) | 1.3 % / 1.6 % |
| `user_export_phrases/phrases/256` | 188.8 µs | 91.1 µs | −51.7 % | improved (p < 0.05) | 1.0 % / 1.3 % |
| `user_export_phrases/phrases/1024` | 798.9 µs | 348.7 µs | −56.3 % | improved (p < 0.05) | 0.5 % / 1.0 % |

Throughput at 1024 phrases: 1.28 → 2.94 Melem/s.

### Interpretation

- **2.2× at every N.** The ratio is flat from 64 to 1024 phrases
  (2.30×, 2.07×, 2.29×), so the change is per-phrase, not fixed-cost.
- **0.36–0.44 µs saved per phrase**: (798.9 − 348.7) / 1024 = 0.44 µs at
  the largest N, (40.83 − 17.77) / 64 = 0.36 µs at the smallest; the
  per-phrase saving is the one number a caller can budget with.
- **Mechanism.** Before, `export_phrases_in` ran one
  `collect_pronunciations_from_store` per phrase — one backend read
  transaction and one cursor per token. After, one
  `collect_pronunciations_for_tokens` walk over the whole
  pronunciation table groups rows by token in a single transaction and
  cursor. At N = 1024 that is 1024 read transactions and 1024 cursor
  opens replaced by one of each.
- **Linear on both sides.** Baseline per-phrase cost is 0.64 / 0.74 /
  0.78 µs at 64 / 256 / 1024; after it is 0.28 / 0.36 / 0.34 µs. Neither
  side bends, so the walk's filtering of non-matching rows is not a
  visible cost at these sizes.

## What is NOT measured

- **F1 / F3 (redb — fixed-width `pronunciation_range` bounds,
  in-place UTF-8 decode of the phrase text, #340)**: below criterion's
  resolution. `UserStore::phrase` sits at ~2.4 µs on redb either way,
  and three runs of the *same* main binary spread 2.40–2.55 µs (up to
  +7 %), wider than the removed allocations can be. The
  `phrase_read` bench ships as a regression canary only, not as an
  improvement signal.
- **F4 (redb `is_empty` header probe, #340)**: the draft bench went
  through `WriteStore::write`, which commits, so it timed a ~3 ms redb
  fsync with the probe as a sub-µs rider (stddev 40–103 % of mean).
  Redesign filed as #342.
- **S4 (LMDB `bulk_load_raw`, APPEND + NOSYNC, #341)**: fsync-dominated
  on every arm (stddev 50–60 % of mean); the point estimates ordered as
  expected (14.2 → 11.9 → 9.4 ms) but the intervals overlap. Redesign
  filed as #343.
- **S3 (`MDB_WRITEMAP`, #341)**: skipped by design — the effect is a
  kernel mmap interaction that criterion wall-clock does not resolve.

## Regression protection

`cargo bench -p oxpinyin-user --no-default-features --features lmdb
--bench export_phrases` reproduces the S5 rows; a return to per-token
range scans would show as a ~2× regression at every N.
`cargo bench -p oxpinyin-user --no-default-features --features redb
--bench phrase_read` is the F1/F3 canary; treat a change outside
±10 % as signal, anything inside as run-to-run drift.
