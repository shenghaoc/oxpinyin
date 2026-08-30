//! The shared chewing-key surface.
//!
//! Upstream builds one engine into two facades: `libzhuyin.la` compiles
//! `$(pinyin_SOURCES) zhuyin.cpp` against its own version script and its
//! own installed header (`src/Makefile.am:108-126`, `configure.ac:140-144`
//! at the pin) — the boundary is facade file + version script + configure
//! flag, never inside the engine. This crate mirrors that cut for the
//! chewing-key machinery a second facade would share: the packed
//! [`ChewingKey`], its display renderers, and the frozen
//! `content_table` / `chewing_key_table` port.
//!
//! Adding a `zhuyin_*` facade later is purely additive: a new facade
//! crate depending on this one (and on the parser surface it needs), plus
//! its own `#[no_mangle]` wrappers. Nothing here is feature-gated and no
//! existing export moves — upstream's `ENABLE_LIBZHUYIN` never strips
//! chewing from `libpinyin.so` (`libpinyin.ver` keeps
//! `pinyin_parse_chewing` and `pinyin_get_zhuyin_string` unconditional),
//! and neither does this workspace.
//!
//! The parse_one_key seams live in `oxpinyin-core` (on the scheme
//! parsers), not here: they consume the frozen parser tables core owns
//! (`zhuyin_map`, the double-pinyin scheme tables, the alias-gated
//! syllable inventory), and relocating them would either duplicate that
//! data or move the whole parser surface, which is not the thin shared
//! machinery this crate cuts.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod chewing_key;
mod chewing_key_data;

pub use chewing_key::{CHEWING_ZERO_TONE, ChewingKey};
