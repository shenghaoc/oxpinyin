---
inclusion: always
---
# Product

A GPL-3.0-or-later Rust re-expression of libpinyin 2.11.91 targeting a
drop-in replacement at the `libpinyin.so.15` ABI. Claims: re-expression is
fine; never replace / succeed / outperform. Scoped measurements only. See
`ROADMAP.md` and `AGENTS.md`.

**Stage 1 — oracle parity.** The candidate surface agrees with the pinned,
source-built libpinyin 2.11.91 oracle bit-identically on every W2 corpus
input at depth 10: top-1 10,190/10,190, top-5-set 10,190, absent 0,
order-only 0, prefix-10 98,930/98,930. The sentence surface carries one
measured residual from platform-dependent `gfloat`+log accumulation,
**frozen as a permanent Stage-1 divergence** (maintainer ruling
2026-09-02, re-frozen 2026-09-04 at 491/396/390 of 496 comparable
inputs — 1-best / n-best distinct-set / ordered;
`docs/findings/sentence-surface.md` §12). §3 constraint machinery is closed
(`pinyin_clear_constraint` exported); `DYNAMIC_ADJUST` runs at the pin's
three gates; the seven-symbol preedit key family is exposed.

**Drop-in.** The cdylib carries SONAME `libpinyin.so.15` and all 79
`pinyin_*` exports live (closed 2026-08-30, `docs/findings/abi-subset.md`
§6); the measured consumer union of 58 symbols is the differential-probe
scope, not the export boundary. Since P6 (2026-09-02) there is no compat
layer: the runtime reads an unmodified libpinyin install's `data/`
through the same readers it uses for its own output (Kyoto Cabinet and
tkrzw carry libpinyin's file names). Measured drop-in on Fedora rawhide
(Kyoto Cabinet), Debian testing (tkrzw) and NixOS — 1,571/1,571 rows
each, sets byte-identical, order-only, the whole divergence attributed
to R1's defined-order rule (`docs/findings/upstream-divergences.md`).
Still open in the drop-in spec: task 9, the write path for learned user
data in libpinyin's own user-file format. The BerkeleyDB compat path is
SHELVED (`docs/findings/berkeleydb-compat-phase1.md`).

**Storage.** Four backends, compile-time selected, exactly one per
binary: tkrzw (default since 2026-09-05), Kyoto Cabinet, LMDB (Linux C
deps), redb (pure-Rust portability fallback for macOS/Windows). Select a
peer with `--no-default-features --features {kyotocabinet|lmdb|redb}`;
the feature forwards down the crate chain to store. There is no
no-backend fallback — `oxpinyin-store` refuses a build with zero or more
than one backend feature at compile time.

**Stage 2** — measured init-time, RAM and binary-size upgrades — is in
progress (`ROADMAP.md` "Stage 2", `docs/perf/README.md`): the P1–P6
data-layer inversion, the fat-LTO release profile and the store/user-crate
hot-path work have landed, each measured against the pin in the same
container. Named next targets: the per-instance key-cost table at
`pinyin_alloc_instance` and steady-state candidate lookup. **Python:**
`oxpinyin-python` serves the engine session API over PyO3 (not the C
ABI), written for free-threaded CPython 3.14t with the GIL released
around engine calls; the spec carries three open items (interpreter
floor, non-Linux wheels, GIL-build claim —
`.kiro/specs/python-binding/tasks.md`). **Frontends:**
no frontend drives the ABI end-to-end yet; fcitx5-oxpinyin appears in the
findings as a reference consumer, not a shipped driver.

**Reference:** [libpinyin wiki](https://github.com/libpinyin/libpinyin/wiki) — architecture, data formats and model description; the authoritative upstream source while the project catches up.
