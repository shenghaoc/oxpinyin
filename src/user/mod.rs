//! User phrase libraries, learning state and profile persistence. [`UserStore`] owns the
//! user indexes, bigram counts and live system-item changes. The session reads these
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
mod persistence;
mod records;
mod replay;
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

pub use library_loop::{LibraryLoopRows, LibraryLoopStop};
pub use library_table::{LibrarySet, LibraryTable};
pub use live_system::{ItemOrigin, LiveSystemItems, Lookup, Reading, ReadingKeys, SystemItem};
pub use lookup::UserLookup;

pub use persistence::{
    MaskLogs, ProfileCheck, SaveBaseline, SystemLibrary, SystemOriginals, UserConfLaw,
    system_originals,
};
#[cfg(any(test, feature = "test-hooks"))]
pub use phrase::FIRST_USER_TOKEN;
#[cfg(test)]
pub use phrase::key_syllable;
pub use phrase::{
    ADD_PHRASE_UNIGRAM_FACTOR, DEFAULT_PHRASE_COUNT, KEY_SYLLABLE_MASK, KEY_TONE_SHIFT,
    MAX_KEY_TONE, PinyinKey, UserPhrase, UserPronunciation, is_user_token, key_pinyin_string,
    key_tone, toned_key,
};
pub use replay::{JournalFault, UnigramAdd};
#[cfg(any(test, feature = "test-hooks"))]
pub use store::ExportedPhrase;
pub use store::{
    MaskCheck, TrainingCheck, TrainingFault, UserStore, UserStoreError, add_system_reading,
};

#[cfg(feature = "tools")]
pub use replay::tool;

#[cfg(test)]
#[path = "../../tests/support/native_fixture.rs"]
pub(crate) mod native_fixture;

#[cfg(all(test, feature = "tkrzw"))]
#[path = "../../tests/common/contract_data.rs"]
pub(crate) mod contract_data;

pub use persistence::{
    Loaded, OriginalLibraries, PersistenceError, SelectedOriginals, SystemLibraryRef, UserState,
    check_format_unnamed, fini, lowered_open_counter,
};
pub use seed::{
    CEILING_SEED, EXPAND_FACTOR, INITIAL_SEED, UNIGRAM_FACTOR, predicted_seed, training_seed,
    unigram_delta,
};

#[cfg(any(test, feature = "test-hooks"))]
pub use phrase_table::key as phrase_key;
pub use phrase_table::{
    PhraseTableEntry, key_ucs4 as phrase_key_ucs4, search_bytes as search_phrase_bytes,
};

pub(crate) use persistence::{
    LiveSubIndex, LoadedPinyinIndex, LoadedProfile, MaskStop, SaveReporting, SaveSource,
    SessionProfile, check_format, journal_item_bytes, load_library_logs_live,
    load_masked_logs_live, load_session_in_loop, load_user_bigram_db, load_user_library,
    read_mask_log, record_skip, save_reporting,
};

#[cfg(test)]
pub(crate) use persistence::{chewing_rows, diff_live_records};
pub(crate) use phrase::add_phrase_token;
