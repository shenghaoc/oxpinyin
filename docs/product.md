# Product

oxpinyin is a GPL-3.0-or-later Rust re-expression of libpinyin 2.11.92.
It ships as a drop-in replacement at the `libpinyin.so.15` ABI.
The Zhuyin facade uses `libzhuyin.so.15`.
Use "re-expression" in project claims.
Do not claim to replace, succeed or outperform libpinyin.
Limit performance claims to scoped measurements.
See the [roadmap](../ROADMAP.md) and [constitution](constitution.md).

**Goal (maintainer, 2026-09-12).**

The shipped object is indistinguishable from libpinyin's shared object except for established exceptions.
It has the same SONAME, exports, data, user files and behaviour, measured against the pin.
Other programs use its C ABI exactly as they use libpinyin.
That ABI is the product.
See the [native Rust policy](findings/compatibility-policy.md#native-rust-implementation).

Keep two kinds of product statement separate:

- [Pinned behaviour](pinned-behaviour.md): what libpinyin does, including defects.
- [oxpinyin improvements](oxpinyin-improvements.md): differences and the rule that permits each one.

## Stages (ruling 2026-10-14 UTC)

**Stage 1: complete.**
The candidate surface agrees bit-identically with the pin-built oracle on every W2 corpus input at depth 10.
The results are top-1 10,190/10,190, top-5-set 10,190, absent 0, order-only 0 and prefix-10 98,930/98,930.
Drop-in measurements covered Fedora rawhide with Kyoto Cabinet, Debian testing with tkrzw, and NixOS.
Each had 1,571/1,571 rows.

**Stage 2: current, not complete.**
The round-2 audit closed on 2026-10-17 (#573).
Remaining work is the init complexity tracker #870, followed by a third bug-for-bug sweep.
The roadmap and [performance index](perf/README.md) record landed performance work.
This includes P1–P6, fat LTO, the key-cost table and store/user hot paths.

**Stage 3: after Stage 2.**

The [source policy](source-policy.md#stages) owns what each stage permits.

## Frontends and references

No frontend drives the ABI end to end in this repository.
The findings use fcitx5-oxpinyin as a reference consumer, not a shipped driver.
The [libpinyin wiki](https://github.com/libpinyin/libpinyin/wiki) describes architecture, data formats and the model.
Use the pinned source as the authority when it differs from the wiki.
