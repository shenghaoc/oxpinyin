//! System attachments whose failed native objects survive initialization.
//!
//! The pin ignores attach's result and later calls the same object
//! (pinyin.cpp:357,369,396,421,429,440 at 074a2219).

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
        #[cfg(feature = "bdb")]
        let result = match crate::bdb::open_pin(path, hash, true)? {
            crate::BdbOpen::Open(store) => Ok(store),
            crate::BdbOpen::Unopened(db) => Err(db),
        };
        #[cfg(feature = "kyotocabinet")]
        let result = crate::kyotocabinet::open_system(path, hash)?;
        #[cfg(feature = "tkrzw")]
        let result = crate::tkrzw::open_system(path, hash)?;
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
    /// Makes the pin's failed point read: BDB get, KC check, or tkrzw
    /// `Get(key, &value)`.
    pub fn read(&self, key: &[u8]) {
        let db = self
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(db) = db.as_ref() {
            #[cfg(feature = "bdb")]
            let _ = db.get(key);
            #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
            db.read(key);
        }
    }

    /// Makes the failed suggestion's first call, with no following cursor
    /// operation: BDB cursor, KC check, tkrzw `Get(key, nullptr)`
    /// (chewing_large_table2_bdb.cpp:567; chewing_large_table2_kyotodb.cpp:541;
    /// chewing_large_table2_tkrzwdb.cpp:511 at 074a2219).
    pub fn suggest(&self, key: &[u8]) {
        let db = self
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(db) = db.as_ref() {
            #[cfg(feature = "bdb")]
            {
                let _ = key;
                db.cursor();
            }
            #[cfg(feature = "kyotocabinet")]
            db.read(key);
            #[cfg(feature = "tkrzw")]
            db.suggest(key);
        }
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

    /// Synchronizes, closes and destroys the failed object once. Repeated
    /// finalization is inert; surviving safe facade instances cannot call
    /// a freed native object.
    pub fn finish(&self) {
        self.db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}
