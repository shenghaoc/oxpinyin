//! Logical session values. The pin keeps phrase items in owned memory;
//! routing each field through an on-memory DBM adds key framing, copies and
//! native B-tree operations to every import. Only standalone stores need
//! these logical tables in a native container. Saved libpinyin files still
//! use the selected backend through `persistence`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ops::{Bound, ControlFlow};
use std::path::Path;

use oxpinyin_store::{Flow, ReadStore, StoreError, WriteStore, WriteTxn};

type Rows = BTreeMap<Vec<u8>, Vec<u8>>;
type Tables = BTreeMap<String, Rows>;
type Changes = BTreeMap<String, BTreeMap<Vec<u8>, Option<Vec<u8>>>>;
type Visitor<'a> = dyn FnMut(&[u8], &[u8]) -> Result<Flow, StoreError> + 'a;

pub(crate) enum SessionValues<S> {
    Native(S),
    Memory(RefCell<Tables>),
}

impl<S> SessionValues<S> {
    #[must_use]
    pub(crate) fn transient() -> Self {
        Self::Memory(RefCell::new(Tables::new()))
    }
}

fn valid_table(table: &str) -> Result<(), StoreError> {
    if table.is_empty() {
        return Err(StoreError::InvalidInput("empty table name"));
    }
    if table.contains('\0') {
        return Err(StoreError::InvalidInput("table name contains NUL"));
    }
    Ok(())
}

fn borrowed() -> StoreError {
    StoreError::Backend("session values already borrowed".into())
}

// BTreeMap rejects contradictory bounds; the byte-store contract instead
// visits no rows. In particular, []..[] is a valid empty range.
fn empty_range(lo: Bound<&[u8]>, hi: Bound<&[u8]>) -> bool {
    match (lo, hi) {
        (Bound::Included(a), Bound::Included(b)) => a > b,
        (Bound::Included(a) | Bound::Excluded(a), Bound::Excluded(b))
        | (Bound::Excluded(a), Bound::Included(b)) => a >= b,
        _ => false,
    }
}

fn scan_rows(
    rows: Option<&Rows>,
    lo: Bound<&[u8]>,
    hi: Bound<&[u8]>,
    mut visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
) -> Result<(), StoreError> {
    if !empty_range(lo, hi)
        && let Some(rows) = rows
    {
        for (key, value) in rows.range::<[u8], _>((lo, hi)) {
            if visit(key, value)?.is_break() {
                break;
            }
        }
    }
    Ok(())
}

impl<S: WriteStore> ReadStore for SessionValues<S> {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        S::open_read_only(path).map(Self::Native)
    }

    // Shared by the many user-store callers, as native point operations are.
    #[inline(never)]
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        match self {
            Self::Native(store) => store.get(table, key),
            Self::Memory(tables) => {
                valid_table(table)?;
                Ok(tables
                    .try_borrow()
                    .map_err(|_| borrowed())?
                    .get(table)
                    .and_then(|rows| rows.get(key))
                    .cloned())
            }
        }
    }

    fn scan(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        match self {
            Self::Native(store) => store.scan(table, lo, hi, visit),
            Self::Memory(tables) => {
                valid_table(table)?;
                let tables = tables.try_borrow().map_err(|_| borrowed())?;
                scan_rows(tables.get(table), lo, hi, visit)
            }
        }
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        match self {
            Self::Native(store) => store.is_empty(table),
            Self::Memory(tables) => {
                valid_table(table)?;
                Ok(tables
                    .try_borrow()
                    .map_err(|_| borrowed())?
                    .get(table)
                    .is_none_or(BTreeMap::is_empty))
            }
        }
    }
}

// Erase only the native transaction's lifetime-bearing type. Native
// transactions retain their own staging and commit/error behavior.
pub(crate) trait NativeTransaction {
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;
    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError>;
    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError>;
    fn is_empty(&self, table: &str) -> Result<bool, StoreError>;
    fn scan(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError>;
}

impl<T: WriteTxn> NativeTransaction for T {
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        WriteTxn::get(self, table, key)
    }
    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        WriteTxn::put(self, table, key, value)
    }
    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        WriteTxn::remove(self, table, key)
    }
    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        WriteTxn::is_empty(self, table)
    }
    fn scan(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        WriteTxn::scan(self, table, lo, hi, visit)
    }
}

pub(crate) enum SessionTxn<'a> {
    Native(&'a mut dyn NativeTransaction),
    Memory {
        tables: &'a Tables,
        changes: Changes,
    },
}

impl WriteTxn for SessionTxn<'_> {
    #[inline(never)]
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        match self {
            Self::Native(txn) => txn.get(table, key),
            Self::Memory { tables, changes } => {
                valid_table(table)?;
                Ok(match changes.get(table).and_then(|rows| rows.get(key)) {
                    Some(value) => value.clone(),
                    None => tables.get(table).and_then(|rows| rows.get(key)).cloned(),
                })
            }
        }
    }

    #[inline(never)]
    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        match self {
            Self::Native(txn) => txn.put(table, key, value),
            Self::Memory { changes, .. } => {
                valid_table(table)?;
                changes
                    .entry(table.to_owned())
                    .or_default()
                    .insert(key.to_vec(), Some(value.to_vec()));
                Ok(())
            }
        }
    }

    #[inline(never)]
    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        match self {
            Self::Native(txn) => txn.remove(table, key),
            Self::Memory { changes, .. } => {
                valid_table(table)?;
                changes
                    .entry(table.to_owned())
                    .or_default()
                    .insert(key.to_vec(), None);
                Ok(())
            }
        }
    }

    fn scan(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        match self {
            Self::Native(txn) => txn.scan(table, lo, hi, &mut visit),
            Self::Memory { tables, changes } => {
                valid_table(table)?;
                if empty_range(lo, hi) {
                    return Ok(());
                }
                let Some(changes) = changes.get(table) else {
                    return scan_rows(tables.get(table), lo, hi, visit);
                };
                let mut pending = changes.range::<[u8], _>((lo, hi)).peekable();
                let mut stopped = false;
                scan_rows(tables.get(table), lo, hi, |key, value| {
                    while let Some((pkey, pvalue)) = pending.peek() {
                        match pkey.as_slice().cmp(key) {
                            std::cmp::Ordering::Greater => break,
                            std::cmp::Ordering::Equal => {
                                let result = match pvalue {
                                    Some(value) => visit(pkey, value)?,
                                    None => ControlFlow::Continue(()),
                                };
                                pending.next();
                                stopped = result.is_break();
                                return Ok(result);
                            }
                            std::cmp::Ordering::Less => {
                                if let Some(value) = pvalue
                                    && visit(pkey, value)?.is_break()
                                {
                                    stopped = true;
                                    return Ok(ControlFlow::Break(()));
                                }
                                pending.next();
                            }
                        }
                    }
                    let result = visit(key, value)?;
                    stopped = result.is_break();
                    Ok(result)
                })?;
                if !stopped {
                    for (key, value) in pending {
                        if let Some(value) = value
                            && visit(key, value)?.is_break()
                        {
                            break;
                        }
                    }
                }
                Ok(())
            }
        }
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        if let Self::Native(txn) = self {
            return txn.is_empty(table);
        }
        let mut empty = true;
        WriteTxn::scan(self, table, Bound::Unbounded, Bound::Unbounded, |_, _| {
            empty = false;
            Ok(ControlFlow::Break(()))
        })?;
        Ok(empty)
    }
}

impl<S: WriteStore> WriteStore for SessionValues<S> {
    type Txn<'a>
        = SessionTxn<'a>
    where
        S: 'a;

    fn create(path: &Path) -> Result<Self, StoreError> {
        S::create(path).map(Self::Native)
    }
    fn create_hash(path: &Path) -> Result<Self, StoreError> {
        S::create_hash(path).map(Self::Native)
    }
    fn create_user(path: &Path) -> Result<Self, StoreError> {
        S::create_user(path).map(Self::Native)
    }
    fn create_user_hash(path: &Path) -> Result<Self, StoreError> {
        S::create_user_hash(path).map(Self::Native)
    }
    fn create_in_memory() -> Result<Self, StoreError> {
        Ok(Self::transient())
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&mut SessionTxn<'_>) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        SessionValues::write(self, f)
    }

    fn compact(&mut self) -> Result<(), StoreError> {
        match self {
            Self::Native(store) => store.compact(),
            Self::Memory(_) => Ok(()),
        }
    }
}

// Keep callers on the concrete transaction lifetime rather than normalizing
// the GAT through a generic native backend.
impl<S: WriteStore> SessionValues<S> {
    pub(crate) fn write<R>(
        &self,
        f: impl FnOnce(&mut SessionTxn<'_>) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        match self {
            Self::Native(store) => store.write(|txn| f(&mut SessionTxn::Native(txn))),
            Self::Memory(tables) => {
                let source = tables.try_borrow().map_err(|_| borrowed())?;
                let mut txn = SessionTxn::Memory {
                    tables: &source,
                    changes: Changes::new(),
                };
                let out = f(&mut txn)?;
                let SessionTxn::Memory { changes, .. } = txn else {
                    return Err(borrowed());
                };
                drop(source);
                let mut tables = tables.try_borrow_mut().map_err(|_| borrowed())?;
                for (table, changes) in changes {
                    let rows = tables.entry(table).or_default();
                    for (key, value) in changes {
                        match value {
                            Some(value) => {
                                rows.insert(key, value);
                            }
                            None => {
                                rows.remove(&key);
                            }
                        }
                    }
                }
                Ok(out)
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use oxpinyin_store::DefaultStore;

    #[test]
    fn transient_transactions_merge_ordered_rows_and_roll_back() {
        let store = SessionValues::<DefaultStore>::transient();
        store
            .write(|txn| {
                for key in [b"a", b"c", b"e"] {
                    WriteTxn::put(txn, "rows", key, key)?;
                }
                Ok(())
            })
            .unwrap();
        store
            .write(|txn| {
                WriteTxn::put(txn, "rows", b"b", b"B")?;
                WriteTxn::put(txn, "rows", b"c", b"C")?;
                WriteTxn::remove(txn, "rows", b"e")?;
                WriteTxn::put(txn, "rows", b"f", b"F")?;
                assert_eq!(WriteTxn::get(txn, "rows", b"c")?, Some(b"C".to_vec()));
                let mut values = Vec::new();
                WriteTxn::scan(
                    txn,
                    "rows",
                    Bound::Included(b"b"),
                    Bound::Included(b"f"),
                    |key, value| {
                        values.push((key.to_vec(), value.to_vec()));
                        Ok(ControlFlow::Continue(()))
                    },
                )?;
                assert_eq!(
                    values,
                    vec![
                        (b"b".to_vec(), b"B".to_vec()),
                        (b"c".to_vec(), b"C".to_vec()),
                        (b"f".to_vec(), b"F".to_vec())
                    ]
                );
                let mut visited = 0;
                WriteTxn::scan(txn, "rows", Bound::Unbounded, Bound::Unbounded, |_, _| {
                    visited += 1;
                    Ok(ControlFlow::Break(()))
                })?;
                assert_eq!(visited, 1);
                Ok(())
            })
            .unwrap();
        let failure: Result<(), _> = store.write(|txn| {
            WriteTxn::remove(txn, "rows", b"c")?;
            WriteTxn::put(txn, "other", b"x", b"y")?;
            Err(StoreError::InvalidInput("rollback"))
        });
        assert!(failure.is_err());
        assert_eq!(store.get("rows", b"c").unwrap(), Some(b"C".to_vec()));
        assert!(store.is_empty("other").unwrap());
        for (lo, hi) in [
            (
                Bound::Included(b"z".as_slice()),
                Bound::Included(b"a".as_slice()),
            ),
            (
                Bound::Excluded(b"".as_slice()),
                Bound::Excluded(b"".as_slice()),
            ),
        ] {
            store
                .scan("rows", lo, hi, |_, _| {
                    Err(StoreError::InvalidInput("must be empty"))
                })
                .unwrap();
        }
        assert!(matches!(
            store.get("", b""),
            Err(StoreError::InvalidInput("empty table name"))
        ));
        assert!(matches!(
            store.get("a\0b", b""),
            Err(StoreError::InvalidInput("table name contains NUL"))
        ));
    }
}
