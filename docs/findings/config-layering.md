# Layered configuration SPEC

Date: 2026-08-09 · Status: **frozen for W4-T0c**

`.kiro/steering/structure.md` states the configuration model in three lines.
This finding turns it into the contract a shell and a test harness both
implement, and pins the one claim that has to be checked by machine rather
than believed: **`Config::default()` equals the captured upstream defaults,
key for key.**

## The default is the parity configuration

Stage 1 is parity with the pinned oracle, and the oracle runs under upstream's
own defaults. If our defaults differed even in one key, every parity
measurement would be measuring two things at once — the decoder and a
configuration delta — with no way to tell them apart afterwards.

So the sane default *is* the parity configuration. There is no separate
"parity mode" to enable. `crates/oxpinyin-engine/tests/upstream_defaults.rs`
re-reads the frozen XML out of `docs/findings/upstream-schema.md` and asserts
the whole key set, every type and every value. It compares against the
document, not against a transcription of it, so the table in `config.rs`
cannot drift.

## Scope: the pinyin schema only

`upstream-schema.md` freezes 114 keys across two schemas. This engine carries
the 69 keys of `com.github.libpinyin.ibus-libpinyin.libpinyin`.

The sibling `com.github.libpinyin.ibus-libpinyin.libbopomofo` schema is
Zhuyin. `parser-spec.md` excludes Zhuyin from Foundation and
`spec-derivation.md` puts Bopomofo in F-F for Stage 2. Carrying its keys now
would mean carrying settings nothing reads.

Many of the 69 are frontend concerns — shortcut strings, lookup-table
orientation, cloud input, OpenCC. They are carried anyway, because
configuration is *data the shell injects* and a shell that reads
`punct-switch` should not need a second channel to get it. The engine reads
the few keys it acts on and passes the rest through untouched.

## Layers

Defaults → system drop-ins → user. Later wins, per key. `merge` is a pure
function of `(base, layers)`: no I/O, no environment, no clock, no global
state, which is what makes constitution item 6 checkable at this level.

Two rules that pull in opposite directions, and why each is right:

- **An unknown key is preserved.** A layer may set a key the base does not
  have. A newer schema on an older engine is an ordinary situation, and
  refusing to start is a worse answer than carrying a setting nobody reads.
- **A known key's type may not change.** A layer that supplies
  `lookup-table-page-size` as text is rejected with
  `ConfigError::TypeMismatch`. Silently accepting it would leave every
  `get_int` on that key returning `None`, which reads exactly like "the key is
  absent" and is undiagnosable from the shell side.

`merge` does not validate ranges. `cloud-request-delay-time` has a
`<range min="200" max="2000"/>` in the schema; enforcing it belongs to whoever
acts on the key, not to a merge that cannot know what a future key means.

## Overlay format

Line-oriented UTF-8, TAB-separated `key=value` fields, the same shape as
`pinyin-capture-v1` and the W4 fixtures. Blank lines and lines starting with
`#` are comments.

```text
key=lookup-table-page-size	type=i	value=9
key=fuzzy-pinyin	type=b	value=true
key=opencc-config	type=s	value=s2t.json
```

`type` is the GSettings letter: `b`, `i`, `x`, `s`. A boolean is `true` or
`false` — not `1`, not `yes`. A string value is literal, with no quoting, so
`value=` is the empty string rather than an absent one. Missing fields,
unknown letters and unparseable values are errors carrying the layer name and
the one-based line number.

This is the file backend the replay harness reads test scenarios from, and the
shape a system drop-in takes on a platform with no GSettings. On Linux a shell
may of course read GSettings directly and build a `ConfigLayer` in memory; the
format exists so it does not have to.

## What is not here

- **No GSettings, dconf, registry or plist.** A backend is a shell concern.
  The engine never opens a settings store, and never reads an environment
  variable to find one.
- **No engine weights and no language-model order.** `structure.md` is
  explicit: those are never user configuration. Customisation is data overlays
  and live preferences.
- **No writes.** Nothing here persists a setting. Producing layers is the
  shell's side of the seam.

## Acceptance

- `Config::default()` equals the captured upstream defaults key for key, type
  for type, value for value — asserted in CI against the frozen document.
- `merge` is fixture-tested over `fixtures/w4/config-system.txt` and
  `config-user.txt`: later layers win, untouched keys keep their default, an
  unknown key survives, and a type change is rejected.
- Every failure carries the layer and line that caused it.
