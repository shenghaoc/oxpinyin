//! User phrase libraries, learning state, and profile persistence. [`UserStore`] owns the
//! user indexes, bigram counts, and live system-item changes. The session reads these
//! through [`crate::model::UserModel`]. [`UserStore::is_modified`] reports pending
//! changes and [`crate::facade::ContextCore::save_user`] saves them.
//!
//! [`crate::facade::PhraseExportCursor`] and [`crate::facade::BigramExportWalk`] expose
//! phrase and bigram exports. [`crate::data`] owns the on-disk codecs.

// The USER_FILE sub-indexes and user bigram supply pinyin_save. See
// docs/findings/user-store.md §§2–4,9,11.
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

mod phrase;
mod phrase_table;
mod seed;

mod library_loop;
mod library_table;
mod live_system;
mod lookup;
mod model;
pub(crate) mod persistence;
mod records;
pub(crate) mod replay;
mod store;
pub(crate) mod store_libpinyin;
mod system_items;
#[cfg(test)]
pub(crate) mod test_child;
#[cfg(test)]
mod test_session;
mod user_index;
mod user_library;
mod user_phrase_table;

pub(crate) use crate::data::LibrarySet;
pub use library_loop::LibraryLoopRows;
pub(crate) use library_loop::LibraryLoopStop;
pub(crate) use library_table::LibraryTable;
pub(crate) use live_system::{ItemOrigin, LiveSystemItems, StepLibraries, SystemItem};
pub(crate) use lookup::UserLookup;

pub(crate) use persistence::{MaskLogs, ProfileCheck, SaveBaseline, system_originals};
pub use persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
#[cfg(test)]
pub(crate) use phrase::FIRST_USER_TOKEN;
#[cfg(test)]
pub(crate) use phrase::key_syllable;
pub(crate) use phrase::{
    ADD_PHRASE_UNIGRAM_FACTOR, DEFAULT_PHRASE_COUNT, UserPhrase, UserPronunciation,
};
pub use phrase::{
    KEY_TONE_SHIFT, MAX_KEY_TONE, PinyinKey, is_user_token, key_pinyin_string, toned_key,
};
pub(crate) use replay::JournalFault;
pub use replay::UnigramAdd;
pub use store::{UserStore, UserStoreError, add_system_reading};
#[cfg(test)]
pub(crate) use test_session::UserState;

#[cfg(test)]
#[path = "../../tests/support/native_fixture.rs"]
pub(crate) mod native_fixture;

#[cfg(all(test, feature = "tkrzw"))]
#[path = "../../tests/common/contract_data.rs"]
pub(crate) mod contract_data;

pub(crate) use persistence::{PersistenceError, SystemLibraryRef, check_format_unnamed, fini};
pub use seed::{CEILING_SEED, INITIAL_SEED, unigram_delta};

pub(crate) use phrase_table::key_ucs4 as phrase_key_ucs4;

pub(crate) use persistence::{
    LiveSubIndex, LoadedPinyinIndex, LoadedProfile, MaskStop, SaveRemapFault, SaveReporting,
    SaveSource, SessionProfile, check_format, load_library_logs_live, load_masked_logs_live,
    load_session_in_loop, load_user_bigram_db, load_user_library, read_mask_log,
    read_save_baseline, record_skip, save_reporting,
};

#[cfg(test)]
pub(crate) use persistence::{chewing_rows, encode_live_diff};
pub(crate) use phrase::add_phrase_token;

#[cfg(test)]
pub(crate) use store::tests::seed_bigram;

// Re-exports that only the tests of other modules name.
#[cfg(test)]
pub(crate) use live_system::Lookup;
#[cfg(test)]
pub(crate) use persistence::Loaded;
#[cfg(test)]
pub(crate) use phrase::KEY_SYLLABLE_MASK;
