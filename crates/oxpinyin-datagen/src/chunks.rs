//! Per-library phrase-index chunk files (`MemoryChunk` + `SubPhraseIndex`)
//! — the `gb_char.bin` / `gbk_char.bin` / `opengram.bin` / `merged.bin`
//! and addon `*.bin` files a libpinyin installation ships.
//!
//! The builder now lives in `oxpinyin-data` (`chunk_write`), where the
//! runtime's user store reaches it for the USER_FILE sub-indexes
//! (`user.bin`, `addon.bin`, `network.bin`); this module keeps the
//! datagen paths stable and maps the error. See the moved module's docs
//! for the byte layout and its provenance.

use crate::DatagenError;

pub use oxpinyin_data::chunk_write::{ChunkItem, PHRASE_MASK, build_chunk};

impl From<oxpinyin_data::chunk_write::ChunkWriteError> for DatagenError {
    fn from(error: oxpinyin_data::chunk_write::ChunkWriteError) -> Self {
        DatagenError::Consistency(error.to_string())
    }
}
