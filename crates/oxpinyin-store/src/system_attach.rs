//! System attachments whose failed native objects survive initialization.
//!
//! The pin ignores attach's result and later calls the same object
//! (pinyin.cpp:357,369,396,421,429,440 at 074a2219). Tkrzw is deliberately
//! excluded until its separate concrete-class binding is available.

use std::path::Path;
use std::sync::Mutex;

use crate::{DefaultStore, StoreError};

#[cfg(feature = "bdb")]
type FailedHandle = crate::BdbUnopenedHandle;
#[cfg(feature = "kyotocabinet")]
type FailedHandle = crate::kyotocabinet::KcUnopenedHandle;

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
        Ok(match result {
            Ok(store) => Self::Open(store),
            Err(db) => Self::Unopened(UnopenedSystemDbm {
                db: Mutex::new(Some(db)),
            }),
        })
    }
}

/// The actual failed BDB or KC object, closed once at facade finalization.
pub struct UnopenedSystemDbm {
    db: Mutex<Option<FailedHandle>>,
}

impl UnopenedSystemDbm {
    /// Makes the pin's failed point read: BDB get, or KC check.
    pub fn read(&self, key: &[u8]) {
        let db = self
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(db) = db.as_ref() {
            #[cfg(feature = "bdb")]
            let _ = db.get(key);
            #[cfg(feature = "kyotocabinet")]
            db.read(key);
        }
    }

    /// Makes the failed suggestion's first call, with no following cursor
    /// operation: BDB cursor, KC check (chewing_large_table2_bdb.cpp:567;
    /// chewing_large_table2_kyotodb.cpp:541 at 074a2219).
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
        }
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
