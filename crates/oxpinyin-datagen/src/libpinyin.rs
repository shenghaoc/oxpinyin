//! libpinyin-schema row builders for the KC/Tkrzw drop-in DBMs.
//!
//! The two index DBMs (`pinyin_index.bin`, `phrase_index.bin`) are the
//! byte-level output of upstream's `ChewingLargeTable2::load_text` /
//! `PhraseLargeTable3::load_text` writer paths. The builders now live in
//! `oxpinyin-data` (`table_entries`), where the runtime's user store
//! reaches them for the user tables (`user_pinyin_index.bin`,
//! `user_phrase_index.bin`); this module keeps the datagen paths stable.
//! See the moved module's docs for the record layouts, the prefix-marker
//! closure, and the ordering contracts.
//!
//! Only the KC/Tkrzw producers emit this schema for the *system* tables.
//! redb and LMDB keep the native oxpinyin schema there — no drop-in
//! requirement exists for them (`docs/findings/datagen-compat-2026-09-01.md`);
//! the *user* tables use this schema on every backend (drop-in task 9).

pub use oxpinyin_data::table_entries::{
    Entries, ParsedRow, item2_stride, phrase_index_entries, pinyin_index_entries,
};
