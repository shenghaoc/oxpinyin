---
inclusion: always
---
# Product

A GPL-3.0-or-later Rust re-expression of libpinyin 2.11.92 shipped as a
drop-in replacement at the `libpinyin.so.15` ABI (and `libzhuyin.so.15`
for the zhuyin facade). Claims: re-expression is fine; never replace /
succeed / outperform. Scoped measurements only. See `ROADMAP.md` and
`/AGENTS.md`.

**Goal (maintainer, 2026-09-12).** The shipped object is a libpinyin
shared object indistinguishable from libpinyin's own except for the
established exceptions: the same SONAME and exports, the same data and
user files, the same behaviour, measured against the pin. Other programs
use oxpinyin through the C ABI exactly as they use libpinyin; that ABI is
the product.

**Two halves, always kept apart.** Every statement about the product
belongs to one of them:

- `pinned-behaviour.md` — what the pinned libpinyin does, defects
  included. The target.
- `oxpinyin-improvements.md` — what oxpinyin does that the pin does not,
  and the rule that admits each difference.

## Stages (ruling 2026-10-14 UTC)

**Stage 1 — oracle parity (complete).** The candidate surface agrees
with the pin-built oracle bit-identically on every W2 corpus input at
depth 10: top-1 10,190/10,190, top-5-set 10,190, absent 0, order-only 0,
prefix-10 98,930/98,930. Drop-in measured on Fedora rawhide (Kyoto
Cabinet), Debian testing (tkrzw) and NixOS — 1,571/1,571 rows each.

**Stage 2 — bug-for-bug compatibility (current).** Every exported
behaviour matches the pin, defects included, across the whole ABI and
the user-store surfaces — the round-2 audit issues, then a third
bug-for-bug sweep, are this stage's work. Behaviour is derived by
reading the pin's source, then its disassembly, never by running
experiments (`/AGENTS.md` "Stage 2 — bug-for-bug";
`.kiro/specs/bug-for-bug/`). Complexity counts: oxpinyin must not be
worse than the pin on both time and space (axis L, #757); optimising
beyond the pin waits for Stage 3.

**Stage 3 — performance (after Stage 2).** Smaller binary, faster
execution, much lower RAM than libpinyin. The default build stays the
bug-for-bug binary; any Stage 3 change that is not bug-for-bug sits
behind an opt-in compile flag, off by default
(`.kiro/specs/stage3-performance/`). Some of it landed early (P1–P6,
fat LTO, the key-cost table, store/user hot paths — `ROADMAP.md`,
`docs/perf/README.md`). Until the 2026-10-14 relabel this stage was
called Stage 2; git history and issue titles still say so.

**Frontends:** no frontend drives the ABI end-to-end in this repository;
fcitx5-oxpinyin appears in the findings as a reference consumer, not a
shipped driver.

**Reference:** [libpinyin wiki](https://github.com/libpinyin/libpinyin/wiki)
— architecture, data formats and model description; the pinned source is
the authority where the two differ.
