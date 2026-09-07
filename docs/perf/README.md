# Perf

Performance measurement snapshots for Stage 2, dated in the file name.
`perf-stage2-harness-2026-08.md` describes the harness; the container
recipe and the runbook are in `../runbooks/benches.md`. Snapshots that
belong to a finding (the backend matrix, store optimisations, the
key-cost walk) are filed under `../findings/perf-*.md` and indexed there.

| Document | Subject |
| --- | --- |
| [`perf-alloc-2026-08`](perf-alloc-2026-08.md) | Stage-2 allocation pass — W8 candidate-cycle (2026-08-19) |
| [`perf-baseline-2026-08`](perf-baseline-2026-08.md) | W8 performance baseline — oracle vs installed oxpinyin (2026-08) |
| [`perf-baseline-kc-2026-09`](perf-baseline-kc-2026-09.md) | Stage-2 Performance Baseline — KC Backend (2026-09) |
| [`perf-candidate-cap-2026-08`](perf-candidate-cap-2026-08.md) | Perf attribution — candidate-cap removal (2026-08) |
| [`perf-exploration`](perf-exploration.md) | Window-scan performance exploration |
| [`perf-fill-lookup-2026-08`](perf-fill-lookup-2026-08.md) | Stage-2 leftover: `fill_lookup` (2026-08-20) |
| [`perf-init-text-slurp-2026-08`](perf-init-text-slurp-2026-08.md) | Stage-2 init cut: interpolation2.text parse + redb slurp (2026-08-21) |
| [`perf-init-typed-map-2026-08`](perf-init-typed-map-2026-08.md) | Stage-2 leftover: init typed-map insert (2026-08-21) |
| [`perf-python-shared-engine-2026-08`](perf-python-shared-engine-2026-08.md) | Python binding: shared `Engine` vs one `Engine` per thread (2026-08) |
| [`perf-so-size-2026-09`](perf-so-size-2026-09.md) | `.so` size — fat LTO + single codegen unit (2026-09) |
| [`perf-stage2-harness-2026-08`](perf-stage2-harness-2026-08.md) | Stage-2 measurement harness (continues the W8 baseline) |
