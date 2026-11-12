//! The record copy of the pin's `load_db` and `save_db` on tkrzw and
//! Berkeley DB: `copy_tkrzwdb` (`tkrzwdb_utils.h:48-74`) and `copy_bdb`
//! (`bdb_utils.h:43-74`) at 074a2219.
//!
//! Both copies `assert` on a failed destination write (`tkrzwdb_utils.h:65`,
//! `bdb_utils.h:61`) and on a source walk that ends in anything but its
//! normal end (`tkrzwdb_utils.h:72`, `bdb_utils.h:67`). The reference build
//! keeps its asserts, so the pin stops there, before the caller renames a
//! staged file or keeps a partly filled container. [`copy`] stops there
//! with [`StoreError::FatalCopy`], which the callers propagate to the C
//! adapters, which abort (constitution §4). The soft failures before the
//! copy (a file that does not open, a missing iterator or cursor, a failed
//! unlink) stay with the callers. Kyoto Cabinet's `load_snapshot` and
//! `dump_snapshot` assert nothing and do not come here.

use crate::StoreError;

/// Copies the records that `walk` hands its visitor into `set`, in the walk's
/// order. A failed `set` stops the copy at that record.
///
/// # Errors
///
/// [`StoreError::FatalCopy`] when a `set` fails or the walk ends in an
/// error: the points where the pin's copy asserts.
pub(crate) fn copy(
    walk: impl FnOnce(&mut crate::RecordSink<'_>) -> Result<(), StoreError>,
    mut set: impl FnMut(&[u8], &[u8]) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    walk(&mut |key, value| {
        #[cfg(feature = "test-hooks")]
        faults::source_read()?;
        #[cfg(feature = "test-hooks")]
        let written = faults::destination_write().and_then(|()| set(key, value));
        #[cfg(not(feature = "test-hooks"))]
        let written = set(key, value);
        written.map_err(|_| StoreError::FatalCopy)
    })
    .map_err(|_| StoreError::FatalCopy)
}

/// Fault points in [`copy`] for other crates' death tests: no input makes an
/// in-memory container fail a read or a write, and a file's own writes fail
/// at a page flush or a growth, not at a chosen record. Test-only: the
/// module exists only with the `test-hooks` feature, which only a
/// `[dev-dependencies]` entry enables.
#[cfg(feature = "test-hooks")]
pub mod faults {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::StoreError;

    /// The source reads left before each later read fails; `usize::MAX`
    /// never fails.
    static SOURCE_READS: AtomicUsize = AtomicUsize::new(usize::MAX);
    /// The destination writes left before each later write fails.
    static DESTINATION_WRITES: AtomicUsize = AtomicUsize::new(usize::MAX);
    /// The iterator creations left before each later one fails.
    static ITERATORS: AtomicUsize = AtomicUsize::new(usize::MAX);

    /// Lets the next `reads` source records of every later copy in this
    /// process through, then fails each source read after them, as a native
    /// walk that reports an error does.
    pub fn fail_source_read_after(reads: usize) {
        SOURCE_READS.store(reads, Ordering::SeqCst);
    }

    /// Lets the next `writes` destination writes of every later copy in
    /// this process through, then fails each write after them, as a native
    /// `Set` or `put` that reports an error does.
    pub fn fail_destination_write_after(writes: usize) {
        DESTINATION_WRITES.store(writes, Ordering::SeqCst);
    }

    /// Lets the next `iterators` source iterators of the copies in this
    /// process be made, then fails each later one, as a `MakeIterator` or
    /// `DB->cursor` that gives no iterator does. The copy has not begun, so
    /// this failure is soft.
    pub fn fail_iterator_creation_after(iterators: usize) {
        ITERATORS.store(iterators, Ordering::SeqCst);
    }

    fn take(left: &AtomicUsize) -> Result<(), StoreError> {
        match left.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| match n {
            0 => None,
            usize::MAX => Some(usize::MAX),
            n => Some(n - 1),
        }) {
            Ok(_) => Ok(()),
            Err(_) => Err(StoreError::Io(std::io::Error::other("injected copy fault"))),
        }
    }

    pub(super) fn source_read() -> Result<(), StoreError> {
        take(&SOURCE_READS)
    }

    pub(super) fn destination_write() -> Result<(), StoreError> {
        take(&DESTINATION_WRITES)
    }

    /// The fault point before a copy's source iterator is made.
    pub(crate) fn make_iterator() -> Result<(), StoreError> {
        take(&ITERATORS)
    }
}
