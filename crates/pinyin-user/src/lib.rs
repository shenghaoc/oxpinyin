//! redb ACID store for user data: learning, frequencies, preferences.
//! The redb major version is pinned. Internal crate — the supported
//! public API is `pinyin-engine`.
//!
//! W6-T1: the integer count [`seed`] arithmetic pinned in
//! `docs/findings/user-store.md` §2, the redb count tables, and the first
//! [`pinyin_core::UserModel`] implementor. W6-T2: the user phrase index and
//! `USER_DICTIONARY` token allocation (§3), as additional tables in the same
//! database. W6-T3 wires the store through [`pinyin_core::UserModel`] (typed
//! with the engine's [`pinyin_core::PhraseToken`]) into the engine session and
//! the C ABI. W6-T4 exposes the stored counts as a
//! [`pinyin_core::UserCountDelta`] so decode can merge them additively with
//! the system model. W6-T5 adds the save cycle behind `pinyin_save`: the §4
//! `m_modified` gate ([`UserStore::is_modified`] / [`UserStore::save`]) over
//! redb's per-commit durability — there is no serialization step, because
//! every training update is already committed atomically to disk. W6-T7 adds
//! the §9 export surface ([`UserStore::export_phrases`] /
//! [`UserStore::export_bigrams`]) that backs the C ABI's export iterators and
//! the W6 differential.
#![warn(missing_docs)]

pub mod phrase;
pub mod seed;

mod model;
mod registry;
mod store;

pub use phrase::{
    ADD_PHRASE_UNIGRAM_FACTOR, DEFAULT_PHRASE_COUNT, FIRST_USER_TOKEN, MAX_PHRASE_LENGTH,
    PHRASE_INDEX_LIBRARY_MASK, PHRASE_MASK, PinyinKey, USER_DICTIONARY, UserPhrase,
    UserPronunciation, is_user_token, phrase_index_library_index, phrase_index_make_token,
};
pub use store::{ExportedPhrase, SENTENCE_START, Token, UserStore, UserStoreError};
#[doc(hidden)]
pub use store::{
    MigrationBigram, MigrationDump, MigrationPhrase, MigrationPronunciation,
};
