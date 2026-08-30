# Findings — the oxpinyin-chewing crate seam (D6)

Date: 2026-08-30 · Status: design rationale for the crate cut; nothing
here is a divergence entry.

## What upstream draws

At the pin (`0c5e80e1`, read via `git show` from the local clone at
`~/Documents/repos/libpinyin`, HEAD `95e3af7` = `2.11.91-3-g95e3af7`):

- `git show 0c5e80e1:src/Makefile.am | sed -n '30,45p'` — the installed
  headers are `pinyin.h` plus `zhuyin.h` under `ENABLE_LIBZHUYIN`;
  `storage/chewing_key.h` is not installed.
- `git show 0c5e80e1:src/Makefile.am | sed -n '105,126p'` —
  `libzhuyin_la_SOURCES = $(pinyin_SOURCES) zhuyin.cpp` with its own
  version script (`libzhuyin.ver`). Same engine, second facade.
- `git show 0c5e80e1:configure.ac | sed -n '140,148p'` — the
  `ENABLE_LIBZHUYIN` flag (default off).
- `git show 0c5e80e1:src/libzhuyin.ver` — 52 `zhuyin_*` symbols;
  `git show 0c5e80e1:src/libpinyin.ver` — 79 `pinyin_*` symbols.

The boundary upstream draws is **facade file + version script + configure
flag**, never inside the engine: `ENABLE_LIBZHUYIN` does not strip chewing
from `libpinyin.so` — `pinyin_SOURCES` always compiles
`storage/zhuyin_parser2.cpp`, and `libpinyin.ver` always exports
`pinyin_parse_chewing`, `pinyin_get_zhuyin_string`, and the rest.

## What oxpinyin mirrors

`oxpinyin-chewing` holds the shared chewing-key machinery: the packed
`ChewingKey` (upstream's two-byte bitfield word, `#[repr(C)]`, size and
alignment asserted — the D1' contract), its six display renderers, and
the frozen `content_table` / `chewing_key_table` port. `oxpinyin-core`
depends on it and re-exports the key type. The capi facade stays thin:
`keys.rs` holds `#[no_mangle]` wrappers and failure-shape handling only;
the renderers and table logic live in the crate.

Deliberate scope line: the three `parse_one_key` seams stay methods on
`oxpinyin-core`'s scheme parsers, because they consume the frozen parser
tables core owns (`zhuyin_map`, the double-pinyin scheme tables, the
alias-gated syllable inventory). Relocating them would either duplicate
that data or move the whole parser surface, which is not the thin shared
machinery this crate cuts.

## Why the seam exists (upstream context, both open)

- **libpinyin/ibus-libpinyin #530, "a concern about ibus-libpinyin"**
  (opened 2025-09-16, OPEN): a user request to split ibus-libpinyin into
  `ibus-libpinyin` and `ibus-libbopomofo` so pinyin and bopomofo users
  each install only their own input method.
- **libpinyin/ibus-libpinyin #565, "Please clarify between libbopomofo
  input method and ibus-libzhuyin project"** (opened 2026-06-08 by
  Boyuan Yang, OPEN): upstream asked to clarify the difference between
  the `libbopomofo` input method in this repo and the `libzhuyin`
  project, so downstream distros can triage bugs and decide follow-up
  reporting.

Both are open and unanswered. The packaging and packaging-clarity
pressure they represent is why the chewing surface is excisable here: if
a bopomofo-only consumer materializes, a `zhuyin_*` facade is purely
additive — a new facade crate depending on this one plus its own
wrappers — and no existing export moves, because no symbol is
feature-gated (mirroring upstream's unconditional chewing exports in
`libpinyin.so`).
