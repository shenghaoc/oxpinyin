# Evidence for the 074a2219 pin verification (2026-09-06 UTC)

- `v3-probe-driver.c`, `v3-matrix.log` — the fork-per-probe `_check_offset`
  runtime matrix (oracle at both pins vs the port at `87f25055`).
- `determinism-v2.diff`, `data-sha-0c5e80e1-build{1,2}.txt`,
  `data-sha-074a221-build1.txt` — the V2 determinism control.
- `oracle-pin-0c5e80e1-build2-presplit.txt` — the second clean build's
  prefix manifest, **captured before the #358 manifest split** (schema
  `pinyin-oracle-v1`, single `oracle-data.sha256` over all 23 files).
  It is a historical record of what that build produced, not a gate a
  post-split prefix should reproduce.

Hashes: `SHA256SUMS.txt`.
