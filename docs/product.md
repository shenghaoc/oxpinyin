# Product

oxpinyin is a GPL-3.0-or-later idiomatic Rust rewrite of libpinyin 2.11.92.
On Linux, it ships as a drop-in replacement at the `libpinyin.so.15` ABI.
The Zhuyin facade uses `libzhuyin.so.15`.
Project claims call it an "idiomatic Rust rewrite".
They do not claim that it succeeds libpinyin. Performance comparisons with libpinyin are stated only as scoped measurements ([controlling rule](source-policy.md#controlling-rule)).
See the [roadmap](../ROADMAP.md) and [constitution](constitution.md).

**Goal.**

On Linux, the shipped object is indistinguishable from libpinyin's shared object except for established exceptions.
It has the same SONAME, exports, data, user files and behaviour, measured against the pin.
Other programs use its C ABI exactly as they use libpinyin.
That ABI is the product.
See the [native Rust policy](findings/compatibility-policy.md#native-rust-implementation).

Two kinds of product statement stay separate:

- [Pinned behaviour](pinned-behaviour.md): what libpinyin does, including defects.
- [oxpinyin improvements](oxpinyin-improvements.md): differences and the rule that permits each one.

## Stages

The candidate surface agrees bit-identically with the pin-built oracle on the 750 candidate-surface inputs; the [frozen parity numbers](runbooks/goldens-and-pins.md#2-the-frozen-parity-numbers) state the counts.
Drop-in measurements covered Fedora rawhide with Kyoto Cabinet, Debian testing with tkrzw, and NixOS.
Each had 1,571/1,571 rows.

The [roadmap](../ROADMAP.md) records current status and exit gates.

The [source policy](source-policy.md#stages) states what each stage permits.

## Frontends and references

No frontend drives the ABI end to end in this repository.
The findings use fcitx5-oxpinyin as a reference consumer, not a shipped driver.
The [libpinyin wiki](https://github.com/libpinyin/libpinyin/wiki) describes architecture, data formats and the model.
The pinned source is the authority when it differs from the wiki.
The [ibus-libpinyin wiki](https://github.com/libpinyin/ibus-libpinyin/wiki) documents the consumer's features and installation.
