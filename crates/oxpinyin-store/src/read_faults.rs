//! A fault point in the point reads of the read-only stores, for other
//! crates' tests: no input makes an attached system file fail one chosen
//! read after an earlier read of the same row succeeded. Test-only: the
//! module exists only with the `test-hooks` feature, which only a
//! `[dev-dependencies]` entry enables.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::StoreError;

/// The one key whose reads fail, armed once in a process.
static KEY: OnceLock<Vec<u8>> = OnceLock::new();
/// The reads of [`KEY`] left before each later one fails.
static READS: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Lets the next `reads` point reads of `key` in the read-only stores of
/// this process through, then fails each later read of it, as a native
/// `Get`, `get` or `check` that reports an I/O error does. The in-memory
/// containers are not affected. A process arms one key; a second call
/// changes only the count.
pub fn fail_point_read_of(key: &[u8], reads: usize) {
    let _ = KEY.set(key.to_vec());
    READS.store(reads, Ordering::SeqCst);
}

/// The fault point before a read-only store's point read of `key`.
pub(crate) fn point_read(key: &[u8]) -> Result<(), StoreError> {
    if KEY.get().is_none_or(|armed| armed != key) {
        return Ok(());
    }
    match READS.try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
        left.checked_sub(1)
    }) {
        Ok(_) => Ok(()),
        Err(_) => Err(StoreError::Io(std::io::Error::other(
            "injected point-read fault",
        ))),
    }
}
