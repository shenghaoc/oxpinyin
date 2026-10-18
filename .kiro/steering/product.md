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

**Stage 2 — bug-for-bug compatibility and internal improvement
(current, not complete).** The target is that every exported behaviour
matches the pin, defects included. Performance, memory, safety and
architecture work is allowed when output, files on disk and the ABI do
not change. oxpinyin may be faster or smaller than the pin by any
amount. This applies to the whole ABI and the user-store surfaces.
The round-2 audit closed on 2026-10-17 (#573). The open work is the
init complexity tracker (#870), then a third bug-for-bug sweep. Behaviour
is derived by reading the pin's source, then its disassembly, never by
running experiments (`/AGENTS.md`, "Stage 2 — bug-for-bug compatibility
and internal improvement"; `.kiro/specs/bug-for-bug/`). The performance floor: oxpinyin must not
be worse than the pin on both time and space (axis L, #757). There is no
ceiling. Landed performance work (P1–P6, fat LTO, the key-cost table,
store/user hot paths) is recorded in `ROADMAP.md` and
`docs/perf/README.md`.

**Stage 3 — opt-in departures (after Stage 2).** Intentional
departures from the pin: for example a new language model, different
candidate behaviour or an incompatible data format. Each sits behind an
opt-in cargo feature, off by default. The default build stays
bug-for-bug (`.kiro/specs/stage3-departures/`).

**Frontends:** no frontend drives the ABI end-to-end in this repository;
fcitx5-oxpinyin appears in the findings as a reference consumer, not a
shipped driver.

**Reference:** [libpinyin wiki](https://github.com/libpinyin/libpinyin/wiki)
— architecture, data formats and model description; the pinned source is
the authority where the two differ.
