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
measured residual — 488/385/379 of 496 comparable inputs, from
platform-dependent `gfloat`+log accumulation — recorded as a Stage-1
divergence and recommended to freeze as permanent; the freeze is the
maintainer's call and is still pending
(`docs/findings/sentence-surface.md`). §3 constraint machinery is closed
(`pinyin_clear_constraint` exported); `DYNAMIC_ADJUST` runs at the pin's
three gates; the seven-symbol preedit key family is exposed.

**Drop-in.** The cdylib carries SONAME `libpinyin.so.15`; the consumer
union is 58 symbols (#206, 58/58). The compat read path consumes installed
libpinyin data directly — Kyoto Cabinet on Fedora rawhide, tkrzw on Debian
testing, Kyoto Cabinet on NixOS — 1,571/1,571 rows each, sets
byte-identical, order-only, the whole divergence attributed to R1's
defined-order rule (`docs/findings/upstream-divergences.md`). The
BerkeleyDB compat path is SHELVED
(`docs/findings/berkeleydb-compat-phase1.md`).

**Storage.** Four backends, compile-time selected, one per binary: Kyoto
Cabinet (default), Tkrzw, LMDB (Linux C deps), redb (pure-Rust portability
fallback for macOS/Windows via `--no-default-features`).

**Stage 2** — measured init-time, RAM and binary-size upgrades — has not
started. **Python:** `oxpinyin-python` serves the engine session API over
PyO3 (not the C ABI), free-threaded CPython, GIL released. **Frontends:**
no frontend drives the ABI end-to-end yet; fcitx5-oxpinyin appears in the
findings as a reference consumer, not a shipped driver.
