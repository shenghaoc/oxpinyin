# Design — Stage 3: performance

## Direction (maintainer decision, 2026-09)

Storage stays the three backends, one per build, each handled natively
(`.kiro/steering/pinned-behaviour.md`, "Storage"). An item that keeps
every output and every file on disk identical lands in the default
build; one that does not — moving the system bigram out of the backend's
`bigram.db`, for example — lands behind an opt-in compile flag, off by
default (Requirement 1.2).

- **Compile the text model at data-prep time** — the primary target:
  attacks init time, RAM and disk at once, and converges with upstream's
  own architecture.
- **System bigram out of the KV store** into a compiled blob.
- **Dictionary cursor walk** for lattice building.
- **Incremental per-keystroke lattice.**
- **Per-session arena.**
- **Ordered atomics off the steady cycle.**
- **Host-independent cost derivation.**

## Known leads from the measurements so far

- Steady-cycle allocation churn — the IR differential's target
  (`docs/findings/perf-cycle-ir-differential-2026-09-08.md`).
- arm64 runs ~10% behind the pin where amd64 is at parity
  (`docs/findings/perf-steady-cycle-cross-host-2026-09-07.md`); open
  leads are memory-ordering cost and the scan-matrix fan-out — 4.11 vs
  1.32 paths per window (`docs/findings/scan-matrix-fanout-2026-09-10.md`).
- The RSS gap traces to 3.07× dictionary probe counts faulting Kyoto
  Cabinet leaf pages (`docs/findings/rss-attribution-2026-09-09.md`).

## Landed early (before the restaging)

The measurement harness, the P1–P6 data-layer inversion, the fat-LTO
single-codegen-unit release profile, the elimination of the
per-instance key-cost table, and the store/user hot-path work —
`ROADMAP.md` "Stage 3", `docs/perf/README.md`.
