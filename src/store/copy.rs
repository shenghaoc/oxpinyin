//! The record copy of the pin's `load_db` and `save_db` on tkrzw and
//! Berkeley DB: `copy_tkrzwdb` (`tkrzwdb_utils.h:48-74`) and `copy_bdb`
//! (`bdb_utils.h:43-74`) at 074a2219.
//!
//! Both copies `assert` on a failed destination write (`tkrzwdb_utils.h:65`,
//! `bdb_utils.h:61`) and on a source walk that ends in anything but its
//! normal end (`tkrzwdb_utils.h:72`, `bdb_utils.h:67`). The reference build
//! keeps its asserts, so the pin stops there, before the caller renames a
//! staged file or keeps a partly filled container. [`copy`] aborts at the
//! failed record operation. The soft failures before the
//! copy (a file that does not open, a missing iterator or cursor, a failed
//! unlink) stay with the callers. Kyoto Cabinet's `load_snapshot` and
//! `dump_snapshot` assert nothing and do not come here.

use crate::store::StoreError;

/// Copies the records that `walk` hands its visitor into `set`, in the walk's
/// order. A failed `set` stops the copy at that record.
///
/// # Aborts
/// A failed destination write or source walk aborts at the pin's assertion.
pub(crate) fn copy(
    walk: impl FnOnce(&mut crate::store::RecordSink<'_>) -> Result<(), StoreError>,
    mut set: impl FnMut(&[u8], &[u8]) -> Result<(), StoreError>,
) {
    walk(&mut |key, value| {
        #[cfg(any(test, feature = "test-hooks"))]
        faults::source_read().unwrap_or_else(|_| std::process::abort());
        #[cfg(any(test, feature = "test-hooks"))]
        let written = faults::destination_write().and_then(|()| set(key, value));
        #[cfg(not(any(test, feature = "test-hooks")))]
        let written = set(key, value);
        written.unwrap_or_else(|_| std::process::abort());
        Ok(())
    })
    .unwrap_or_else(|_| std::process::abort());
}

/// Fault injection for the store copy operations used by cross-crate death tests. These
/// controls select a record at which the read or write fails.
#[cfg(any(test, feature = "test-hooks"))]
pub(super) mod faults {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::store::StoreError;

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
    #[cfg(test)]
    pub(crate) fn fail_source_read_after(reads: usize) {
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
    #[cfg(test)]
    pub(crate) fn fail_iterator_creation_after(iterators: usize) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn two_records() {
        copy(
            |sink| {
                sink(b"one", b"1")?;
                sink(b"two", b"2")
            },
            |_, _| Ok(()),
        )
    }

    #[test]
    fn source_walk_failure_aborts() {
        crate::store::death_test::assert_aborts(
            "store::copy::tests::source_walk_failure_aborts",
            || {
                faults::fail_source_read_after(1);
                two_records();
            },
        );
    }

    #[test]
    fn destination_write_failure_aborts() {
        crate::store::death_test::assert_aborts(
            "store::copy::tests::destination_write_failure_aborts",
            || {
                faults::fail_destination_write_after(1);
                two_records();
            },
        );
    }
}
