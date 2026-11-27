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

use crate::vocab::PHRASE_INDEX_LIBRARY_COUNT;

/// The library indexes that have a slot, so that a walk can lend each one.
static LIBRARIES: [u8; PHRASE_INDEX_LIBRARY_COUNT as usize] =
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// One optional `T` per phrase library, walked in ascending library order.
///
/// The 16 slots are one heap allocation, so a table moves as one pointer:
/// the session's tables pass by value through the context open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryTable<T> {
    slots: Box<[Option<T>; PHRASE_INDEX_LIBRARY_COUNT as usize]>,
}

impl<T> Default for LibraryTable<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> LibraryTable<T> {
    /// A table with every library absent.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            slots: Box::new([const { None }; PHRASE_INDEX_LIBRARY_COUNT as usize]),
        }
    }

    /// The value of `library`, when it is present.
    #[must_use]
    pub(crate) fn get(&self, library: &u8) -> Option<&T> {
        self.slots.get(usize::from(*library))?.as_ref()
    }

    /// The value of `library` for a write, when it is present.
    pub(crate) fn get_mut(&mut self, library: &u8) -> Option<&mut T> {
        self.slots.get_mut(usize::from(*library))?.as_mut()
    }

    /// Whether `library` is present.
    #[must_use]
    pub(crate) fn contains_key(&self, library: &u8) -> bool {
        self.get(library).is_some()
    }

    /// Stores `value` and returns the value previously at `library`. A first insertion and an
    /// index outside the 16 slots both return [`None`]. Only an in-range insertion changes
    /// the table.
    pub(crate) fn insert(&mut self, library: u8, value: T) -> Option<T> {
        self.slots.get_mut(usize::from(library))?.replace(value)
    }

    /// Takes the value of `library` out of the table.
    pub(crate) fn remove(&mut self, library: &u8) -> Option<T> {
        self.slots.get_mut(usize::from(*library))?.take()
    }

    /// The value of `library`, stored from `make` first when it is absent.
    /// `None` for an index that names no slot.
    pub(crate) fn get_or_insert_with(
        &mut self,
        library: u8,
        make: impl FnOnce() -> T,
    ) -> Option<&mut T> {
        Some(
            self.slots
                .get_mut(usize::from(library))?
                .get_or_insert_with(make),
        )
    }

    /// Gets or initializes the slot for `library` using [`Default`]. Returns [`None`] for an
    /// index outside `0..16`.
    pub(crate) fn get_or_default(&mut self, library: u8) -> Option<&mut T>
    where
        T: Default,
    {
        self.get_or_insert_with(library, T::default)
    }

    /// Makes every library absent.
    pub(crate) fn clear(&mut self) {
        self.slots.fill_with(|| None);
    }

    /// Whether every library is absent.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }

    /// How many libraries are present.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// The present libraries with their values, in ascending library order.
    #[must_use]
    pub(crate) fn iter(&self) -> Iter<'_, T> {
        Iter(LIBRARIES.iter().zip(self.slots.iter()))
    }

    /// [`Self::iter`] for a write to each value.
    pub(crate) fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut(LIBRARIES.iter().zip(self.slots.iter_mut()))
    }

    /// The present libraries, in ascending order.
    #[must_use]
    pub(crate) fn keys(&self) -> impl DoubleEndedIterator<Item = &u8> + use<'_, T> {
        self.iter().map(|(library, _)| library)
    }

    /// The present values, in ascending library order.
    #[must_use]
    pub(crate) fn values(&self) -> impl DoubleEndedIterator<Item = &T> {
        self.iter().map(|(_, value)| value)
    }

    /// [`Self::values`] for a write to each value.
    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.iter_mut().map(|(_, value)| value)
    }

    /// Keeps the libraries `keep` answers `true` for.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&u8, &mut T) -> bool) {
        for (library, slot) in LIBRARIES.iter().zip(self.slots.iter_mut()) {
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
        std::array::IntoIter<u8, { PHRASE_INDEX_LIBRARY_COUNT as usize }>,
        std::array::IntoIter<Option<T>, { PHRASE_INDEX_LIBRARY_COUNT as usize }>,
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
        IntoIter(LIBRARIES.into_iter().zip(*self.slots))
    }
}

#[cfg(test)]
mod tests;
