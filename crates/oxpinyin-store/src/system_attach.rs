//! System attachments whose failed native objects survive initialization.
//!
//! The pin ignores attach's result and later calls the same object
//! (pinyin.cpp:357,369,396,421,429,440 at 074a2219). The utilities'
//! `Bigram::attach` keeps its failed object the same way
//! ([`SystemAttachment::attach_bigram`]).

use std::path::Path;
use std::sync::Mutex;

use crate::{DefaultStore, StoreError};

#[cfg(feature = "bdb")]
type FailedHandle = crate::BdbUnopenedHandle;
#[cfg(feature = "kyotocabinet")]
type FailedHandle = crate::kyotocabinet::KcUnopenedHandle;
#[cfg(feature = "tkrzw")]
type FailedHandle = crate::tkrzw::TkrzwUnopenedHandle;

/// The result of one system-file attach, including a retained failed object.
pub enum SystemAttachment {
    /// The native file opened successfully.
    Open(DefaultStore),
    /// The native object exists, but its file did not open.
    Unopened(UnopenedSystemDbm),
}

impl SystemAttachment {
    /// Attaches one tree or hash system file read-only, without retrying it.
    ///
    /// # Errors
    /// Returns failures before the native open, such as object allocation.
    pub fn open(path: &Path, hash: bool) -> Result<Self, StoreError> {
        Self::attach(path, hash, true)
    }

    /// The pin's `Bigram::attach` of a hash file, `ATTACH_READONLY` or
    /// `ATTACH_READWRITE | ATTACH_CREATE`: one native open, with no CREATE
    /// retry and no signature check (`ngram_bdb.cpp:110-124`,
    /// `ngram_kyotodb.cpp:110-120`, `ngram_tkrzwdb.cpp:86-99` at
    /// 074a2219). Kyoto Cabinet and tkrzw keep the object whose open
    /// failed, and the owner's `reset()` tears it down (`ngram_kyotodb.cpp:
    /// 43-50`: synchronize, close, delete; `ngram_tkrzwdb.cpp:40-46`:
    /// close, delete). Berkeley DB closes the failed handle at once (row
    /// 113 of the compatibility register).
    ///
    /// # Errors
    /// Returns failures before the native open, such as a path that
    /// contains NUL.
    pub fn attach_bigram(path: &Path, read_only: bool) -> Result<Self, StoreError> {
        Self::attach(path, true, read_only)
    }

    fn attach(path: &Path, hash: bool, read_only: bool) -> Result<Self, StoreError> {
        #[cfg(feature = "bdb")]
        let result = match crate::bdb::open_pin(path, hash, read_only)? {
            crate::BdbOpen::Open(store) => Ok(store),
            crate::BdbOpen::Unopened(db) => Err(db),
        };
        #[cfg(feature = "kyotocabinet")]
        let result = crate::kyotocabinet::open_system(path, hash, read_only)?;
        #[cfg(feature = "tkrzw")]
        let result = crate::tkrzw::open_system(path, hash, read_only)?;
        Ok(match result {
            Ok(store) => Self::Open(store),
            Err(db) => Self::Unopened(UnopenedSystemDbm {
                db: Mutex::new(Some(db)),
            }),
        })
    }
}

/// The actual failed BDB, KC or tkrzw object, closed once at facade
/// finalization.
pub struct UnopenedSystemDbm {
    db: Mutex<Option<FailedHandle>>,
}

impl UnopenedSystemDbm {
    /// Makes the pin's failed point read: KC check or tkrzw
    /// `Get(key, &value)`. On Berkeley DB the failed handle is already
    /// closed (`BdbUnopenedHandle`), so the pin's `DB->get` has
    /// no counterpart.
    pub fn read(&self, key: &[u8]) {
        let db = self
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(db) = db.as_ref() {
            #[cfg(feature = "bdb")]
            let _ = (db, key);
            #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
            db.read(key);
        }
    }

    /// Makes the failed suggestion's first call, with no following cursor
    /// operation: KC check, tkrzw `Get(key, nullptr)`
    /// (chewing_large_table2_kyotodb.cpp:541;
    /// chewing_large_table2_tkrzwdb.cpp:511 at 074a2219). The pin's BDB
    /// cursor (chewing_large_table2_bdb.cpp:567) has no counterpart, as in
    /// [`Self::read`].
    pub fn suggest(&self, key: &[u8]) {
        let db = self
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(db) = db.as_ref() {
            #[cfg(feature = "bdb")]
            let _ = (db, key);
            #[cfg(feature = "kyotocabinet")]
            db.read(key);
            #[cfg(feature = "tkrzw")]
            db.suggest(key);
        }
    }

    /// Makes `Bigram::store`'s native write and returns its answer, which
    /// is always a failure: the failed object refuses KC `set` and tkrzw
    /// `Set(key, value)` (`ngram_kyotodb.cpp:145-153`, `ngram_tkrzwdb.cpp:
    /// 124-133` at 074a2219). On Berkeley DB the pin's `DB->put` on its
    /// failed handle answers `EINVAL` (`ngram_bdb.cpp:148-163`); the handle
    /// is closed, so that answer comes with no call, as in [`Self::read`].
    /// After [`Self::finish`] the pin's `m_db` is null and `store` answers
    /// false too.
    ///
    /// # Errors
    ///
    /// Always: the native refusal, or the Berkeley DB and finished answers.
    pub fn write(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        let db = self
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(db) = db.as_ref() else {
            return Err(StoreError::Backend(
                "the failed attachment is finished".into(),
            ));
        };
        db.write(key, value)
    }

    /// Makes `Bigram::get_all_items`' native walk, which collects no key,
    /// and returns the pin's answer. Kyoto Cabinet ignores its refused
    /// `iterate` and answers true (`ngram_kyotodb.cpp:186-196`); tkrzw
    /// returns its refused `ProcessEach` status (`ngram_tkrzwdb.cpp:
    /// 160-172`); Berkeley DB's `DB->cursor` on the failed handle leaves no
    /// cursor and answers false (`ngram_bdb.cpp:178-191` at 074a2219), with
    /// no call here, as in [`Self::read`]. After [`Self::finish`] the pin's
    /// `m_db` is null and the walk answers false.
    ///
    /// # Errors
    ///
    /// The pin's false answer: the tkrzw refusal, Berkeley DB, or a finished
    /// attachment.
    pub fn walk(&self) -> Result<(), StoreError> {
        let db = self
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(db) = db.as_ref() else {
            return Err(StoreError::Backend(
                "the failed attachment is finished".into(),
            ));
        };
        db.walk()
    }

    /// Whether [`Self::finish`] has run: the native object is gone and
    /// every later call is inert. For the facades' finalization tests.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none()
    }

    /// Synchronizes, closes and destroys the failed object once (on
    /// Berkeley DB the handle is already closed). Repeated
    /// finalization is inert; surviving safe facade instances cannot call
    /// a freed native object.
    pub fn finish(&self) {
        self.db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}
