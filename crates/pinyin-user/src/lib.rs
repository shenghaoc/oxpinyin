//! redb ACID store for user data: learning, frequencies, preferences.
//! The redb major version is pinned. Internal crate — the supported
//! public API is `pinyin-engine`.
//!
//! W6-T1: the integer count [`seed`] arithmetic pinned in
//! `docs/findings/user-store.md` §2, the redb count tables, and the first
//! [`pinyin_core::UserModel`] implementor. W6-T2: the user phrase index and
//! `USER_DICTIONARY` token allocation (§3), as additional tables in the same
//! database. Persistence semantics (T5), Session/capi training wiring (T3)
//! and the decode-time additive merge (T4) are out of scope.
#![warn(missing_docs)]

pub mod phrase;
pub mod seed;

mod model;
mod store;

pub use phrase::{
    ADD_PHRASE_UNIGRAM_FACTOR, DEFAULT_PHRASE_COUNT, FIRST_USER_TOKEN, MAX_PHRASE_LENGTH,
    PHRASE_INDEX_LIBRARY_MASK, PHRASE_MASK, PinyinKey, USER_DICTIONARY, UserPhrase,
    UserPronunciation, is_user_token, phrase_index_library_index, phrase_index_make_token,
};
pub use store::{SENTENCE_START, Token, UserStore, UserStoreError};
