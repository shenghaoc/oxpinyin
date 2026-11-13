//! Per-library tables: one slot for each phrase library.
//!
//! The pin holds its per-library state in arrays of
//! `PHRASE_INDEX_LIBRARY_COUNT` slots (`m_sub_phrase_indices`,
//! `phrase_index.h:441`; `novel_types.h:43` at 074a2219). [`LibraryTable`]
//! is that array: a library is present or absent in its slot, and a walk
//! visits the present ones in ascending library order.
//!
//! A library index is the pin's `PHRASE_INDEX_LIBRARY_INDEX(token)`
//! (`novel_types.h:44`), which is below 16. An index of 16 or more names no
//! slot: a read of one finds nothing and a write of one stores nothing.

use oxpinyin_data::table_info::PHRASE_INDEX_LIBRARY_COUNT;

/// The library indexes that have a slot, so that a walk can lend each one.
static LIBRARIES: [u8; PHRASE_INDEX_LIBRARY_COUNT] =
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// One optional `T` per phrase library, walked in ascending library order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryTable<T> {
    slots: [Option<T>; PHRASE_INDEX_LIBRARY_COUNT],
}

impl<T> Default for LibraryTable<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> LibraryTable<T> {
    /// A table with every library absent.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; PHRASE_INDEX_LIBRARY_COUNT],
        }
    }

    /// The value of `library`, when it is present.
    #[must_use]
    pub fn get(&self, library: &u8) -> Option<&T> {
        self.slots.get(usize::from(*library))?.as_ref()
    }

    /// The value of `library` for a write, when it is present.
    pub fn get_mut(&mut self, library: &u8) -> Option<&mut T> {
        self.slots.get_mut(usize::from(*library))?.as_mut()
    }

    /// Whether `library` is present.
    #[must_use]
    pub fn contains_key(&self, library: &u8) -> bool {
        self.get(library).is_some()
    }

    /// Stores `value` for `library` and answers the value it replaced.
    pub fn insert(&mut self, library: u8, value: T) -> Option<T> {
        self.slots.get_mut(usize::from(library))?.replace(value)
    }

    /// Takes the value of `library` out of the table.
    pub fn remove(&mut self, library: &u8) -> Option<T> {
        self.slots.get_mut(usize::from(*library))?.take()
    }

    /// The value of `library`, stored from `make` first when it is absent.
    /// `None` for an index that names no slot.
    pub fn get_or_insert_with(&mut self, library: u8, make: impl FnOnce() -> T) -> Option<&mut T> {
        Some(
            self.slots
                .get_mut(usize::from(library))?
                .get_or_insert_with(make),
        )
    }

    /// [`Self::get_or_insert_with`] storing `T::default()`.
    pub fn get_or_default(&mut self, library: u8) -> Option<&mut T>
    where
        T: Default,
    {
        self.get_or_insert_with(library, T::default)
    }

    /// Makes every library absent.
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Whether every library is absent.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }

    /// How many libraries are present.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// The present libraries with their values, in ascending library order.
    pub fn iter(&self) -> Iter<'_, T> {
        Iter(LIBRARIES.iter().zip(&self.slots))
    }

    /// [`Self::iter`] for a write to each value.
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut(LIBRARIES.iter().zip(&mut self.slots))
    }

    /// The present libraries, in ascending order.
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &u8> + use<'_, T> {
        self.iter().map(|(library, _)| library)
    }

    /// The present values, in ascending library order.
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &T> {
        self.iter().map(|(_, value)| value)
    }

    /// [`Self::values`] for a write to each value.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.iter_mut().map(|(_, value)| value)
    }

    /// The present values, owned, in ascending library order.
    pub fn into_values(self) -> impl Iterator<Item = T> {
        self.into_iter().map(|(_, value)| value)
    }

    /// Keeps the libraries `keep` answers `true` for.
    pub fn retain(&mut self, mut keep: impl FnMut(&u8, &mut T) -> bool) {
        for (library, slot) in LIBRARIES.iter().zip(&mut self.slots) {
            if slot.as_mut().is_some_and(|value| !keep(library, value)) {
                *slot = None;
            }
        }
    }
}

impl<T> FromIterator<(u8, T)> for LibraryTable<T> {
    /// A later value of one library replaces an earlier one.
    fn from_iter<I: IntoIterator<Item = (u8, T)>>(rows: I) -> Self {
        let mut table = Self::new();
        table.extend(rows);
        table
    }
}

impl<T, const N: usize> From<[(u8, T); N]> for LibraryTable<T> {
    fn from(rows: [(u8, T); N]) -> Self {
        rows.into_iter().collect()
    }
}

impl<T> Extend<(u8, T)> for LibraryTable<T> {
    fn extend<I: IntoIterator<Item = (u8, T)>>(&mut self, rows: I) {
        for (library, value) in rows {
            self.insert(library, value);
        }
    }
}

/// A test reads a library it has stored.
#[cfg(test)]
impl<T> std::ops::Index<&u8> for LibraryTable<T> {
    type Output = T;

    fn index(&self, library: &u8) -> &T {
        self.get(library).expect("the library is present")
    }
}

/// The borrowed walk of a [`LibraryTable`], in ascending library order.
pub struct Iter<'a, T>(
    std::iter::Zip<std::slice::Iter<'static, u8>, std::slice::Iter<'a, Option<T>>>,
);

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = (&'a u8, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .by_ref()
            .find_map(|(library, slot)| Some((library, slot.as_ref()?)))
    }
}

impl<T> DoubleEndedIterator for Iter<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0
            .by_ref()
            .rev()
            .find_map(|(library, slot)| Some((library, slot.as_ref()?)))
    }
}

impl<'a, T> IntoIterator for &'a LibraryTable<T> {
    type Item = (&'a u8, &'a T);
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Iter<'a, T> {
        self.iter()
    }
}

/// The walk of a [`LibraryTable`] for a write to each value.
pub struct IterMut<'a, T>(
    std::iter::Zip<std::slice::Iter<'static, u8>, std::slice::IterMut<'a, Option<T>>>,
);

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = (&'a u8, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .by_ref()
            .find_map(|(library, slot)| Some((library, slot.as_mut()?)))
    }
}

impl<'a, T> IntoIterator for &'a mut LibraryTable<T> {
    type Item = (&'a u8, &'a mut T);
    type IntoIter = IterMut<'a, T>;

    fn into_iter(self) -> IterMut<'a, T> {
        self.iter_mut()
    }
}

/// The owned walk of a [`LibraryTable`], in ascending library order.
pub struct IntoIter<T>(
    std::iter::Zip<
        std::array::IntoIter<u8, PHRASE_INDEX_LIBRARY_COUNT>,
        std::array::IntoIter<Option<T>, PHRASE_INDEX_LIBRARY_COUNT>,
    >,
);

impl<T> Iterator for IntoIter<T> {
    type Item = (u8, T);

    fn next(&mut self) -> Option<(u8, T)> {
        self.0
            .by_ref()
            .find_map(|(library, slot)| Some((library, slot?)))
    }
}

impl<T> IntoIterator for LibraryTable<T> {
    type Item = (u8, T);
    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> IntoIter<T> {
        IntoIter(LIBRARIES.into_iter().zip(self.slots))
    }
}

/// A set of phrase libraries, walked in ascending order: one bit per
/// library. An index of 16 or more is never in the set.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LibrarySet(u16);

impl LibrarySet {
    /// The set with no library.
    #[must_use]
    pub const fn new() -> Self {
        Self(0)
    }

    const fn bit(library: u8) -> u16 {
        if (library as usize) < PHRASE_INDEX_LIBRARY_COUNT {
            1 << library
        } else {
            0
        }
    }

    /// Whether `library` is in the set.
    #[must_use]
    pub const fn contains(&self, library: &u8) -> bool {
        self.0 & Self::bit(*library) != 0
    }

    /// Adds `library`; `true` when it was not in the set.
    pub const fn insert(&mut self, library: u8) -> bool {
        let added = Self::bit(library) != 0 && !self.contains(&library);
        self.0 |= Self::bit(library);
        added
    }

    /// Removes `library`; `true` when it was in the set.
    pub const fn remove(&mut self, library: &u8) -> bool {
        let held = self.contains(library);
        self.0 &= !Self::bit(*library);
        held
    }

    /// Whether the set has no library.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// The libraries of the set, in ascending order.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = u8> + use<'_> {
        LIBRARIES
            .into_iter()
            .filter(|library| self.contains(library))
    }
}

impl Extend<u8> for LibrarySet {
    fn extend<I: IntoIterator<Item = u8>>(&mut self, libraries: I) {
        for library in libraries {
            self.insert(library);
        }
    }
}

impl FromIterator<u8> for LibrarySet {
    fn from_iter<I: IntoIterator<Item = u8>>(libraries: I) -> Self {
        let mut set = Self::new();
        set.extend(libraries);
        set
    }
}

#[cfg(test)]
mod tests {
    use super::{LibrarySet, LibraryTable};

    #[test]
    fn a_table_walks_its_present_libraries_in_ascending_order() {
        let mut table: LibraryTable<&str> =
            [(7, "user"), (1, "gb"), (15, "last")].into_iter().collect();
        assert_eq!(table.insert(1, "gb again"), Some("gb"));
        assert_eq!(table.len(), 3);
        assert_eq!(
            table.iter().collect::<Vec<_>>(),
            [(&1, &"gb again"), (&7, &"user"), (&15, &"last")]
        );
        assert_eq!(table.keys().copied().collect::<Vec<_>>(), [1, 7, 15]);
        assert_eq!(table.keys().next_back(), Some(&15));
        assert_eq!(table.remove(&7), Some("user"));
        assert!(!table.contains_key(&7));
        table.retain(|library, _| *library != 15);
        assert_eq!(table.into_iter().collect::<Vec<_>>(), [(1, "gb again")]);
    }

    #[test]
    fn an_index_past_the_last_library_names_no_slot() {
        let mut table = LibraryTable::new();
        assert_eq!(table.insert(16, 1_u32), None);
        assert_eq!(table.get(&16), None);
        assert_eq!(table.remove(&0x81), None);
        assert!(table.get_or_default(16).is_none());
        assert!(table.get_or_insert_with(0x81, || 2).is_none());
        assert!(table.is_empty());
        *table.get_or_default(15).expect("the last slot") += 3;
        assert_eq!(table.get(&15), Some(&3));

        let mut set = LibrarySet::new();
        assert!(!set.insert(16) && !set.contains(&16));
        assert!(set.insert(3) && !set.insert(3));
        assert_eq!(set.iter().collect::<Vec<_>>(), [3]);
        assert!(set.remove(&3) && set.is_empty());
    }
}
