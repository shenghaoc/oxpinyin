//! The user data of one libpinyin session: the user phrase arenas (the
//! pin's `USER_FILE` sub-indexes), the user pinyin index and phrase table,
//! the pin's own user bigram container, the live system items, and the
//! user-file persistence behind `pinyin_save`. The user files are read and
//! written with the compiled-in `DefaultStore` backend (tkrzw, Kyoto
//! Cabinet or Berkeley DB — one per binary, selected by feature; see
//! `oxpinyin-store`). Internal crate — the supported external interfaces
//! are the libpinyin-compatible and libzhuyin-compatible C APIs and ABIs.
//!
//! The integer count [`seed`] arithmetic is pinned in
//! `docs/findings/user-store.md` §2, and `USER_DICTIONARY` token allocation
//! in §3. The store is the [`oxpinyin_core::UserModel`] implementor (typed
//! with the engine's [`oxpinyin_core::PhraseToken`]) the engine session and
//! the C ABI reach; it exposes the counts as a
//! [`oxpinyin_core::UserCountDelta`] so decode can merge them additively
//! with the system model, the §4 `m_modified` gate
//! ([`UserStore::is_modified`] / [`UserStore::save_reporting`]) behind `pinyin_save`,
//! and the §9 export surface ([`UserStore::export_phrases_in`] /
//! [`UserStore::bigram_predecessors`]) behind the C ABI's export iterators.
//! Nothing is durable between saves: `save` writes the pin's own user file
//! set (`.tmp` + rename), the pin's own shape (`docs/findings/user-store.md`
//! §11). The user bigram is held once, as the pin's own `SingleGram`
//! values, and every bigram operation runs the pin's `SingleGram` code over
//! them (`oxpinyin_core::single_gram`; the `store` module documentation).
#![forbid(unsafe_code)]
#![warn(missing_docs)]
// Constitution §4: keep these non-test denials. A permitted site needs the
// narrowest justified #[expect], citing the pin's abort site or an invariant
// making failure unreachable; see docs/rust-conventions.md.
// An expectation proves lint coverage, not correspondence with the pin.
// Inline #[cfg(test)] modules are exempt; tests/, benches/ and examples/
// are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![cfg_attr(not(test), deny(clippy::unreachable))]

pub mod phrase;
pub mod phrase_table;
pub mod seed;

mod library_loop;
mod library_table;
mod live_system;
mod lookup;
mod model;
pub mod persistence;
mod records;
mod replay;
mod store;
pub(crate) mod store_libpinyin;
mod system_items;
#[cfg(test)]
mod test_session;
mod user_index;
mod user_library;
mod user_phrase_table;

pub use library_loop::{LibraryLoopRows, LibraryLoopStop};
pub use library_table::{LibrarySet, LibraryTable};
pub use live_system::{ItemOrigin, LiveSystemItems, Lookup, Reading, ReadingKeys, SystemItem};
pub use lookup::UserLookup;
pub use oxpinyin_data::c_stderr;
pub use oxpinyin_data::user_files::SystemVersions;
pub use persistence::{
    ChunkHeaderField, MaskLogs, ProfileCheck, SaveBaseline, SystemLibrary, SystemOriginals,
    UserConfLaw, system_originals,
};
pub use phrase::{
    ADD_PHRASE_UNIGRAM_FACTOR, ADDON_DICTIONARY, DEFAULT_PHRASE_COUNT, FIRST_NETWORK_TOKEN,
    FIRST_USER_TOKEN, KEY_SYLLABLE_MASK, KEY_TONE_SHIFT, MAX_KEY_TONE, MAX_PHRASE_LENGTH,
    NETWORK_DICTIONARY, PHRASE_INDEX_LIBRARY_MASK, PHRASE_MASK, PinyinKey, USER_DICTIONARY,
    UserPhrase, UserPronunciation, is_user_file_library, is_user_file_token, is_user_token,
    key_pinyin_string, key_syllable, key_tone, phrase_index_library_index, phrase_index_make_token,
    toned_key,
};
pub use replay::{JournalFault, UnigramAdd};
pub use store::{
    ExportedPhrase, SENTENCE_START, Token, TrainingCheck, TrainingFault, UserStore, UserStoreError,
    add_system_reading,
};

#[cfg(feature = "tools")]
pub use replay::tool;
