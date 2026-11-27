//! A set of phrase libraries: one bit per library.
//!
//! A library index is the pin's `PHRASE_INDEX_LIBRARY_INDEX(token)`
//! (`novel_types.h:44` at 074a2219), which is below 16. The data layer and
//! the user store both keep library sets, so the type lives here, below
//! both.

use crate::vocab::PHRASE_INDEX_LIBRARY_COUNT;

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
        if library < PHRASE_INDEX_LIBRARY_COUNT {
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

    /// The libraries in either set.
    #[must_use]
    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The set less every library whose bit `mask` sets: the library mask's
    /// bit `n` is library `n`, and a bit past the sixteen names none.
    #[must_use]
    pub(crate) const fn without_mask(self, mask: u32) -> Self {
        Self(self.0 & !((mask & 0xffff) as u16))
    }

    /// Whether the set has no library.
    #[cfg(test)]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// The libraries of the set, in ascending order.
    #[cfg(test)]
    #[must_use]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = u8> + use<'_> {
        (0..PHRASE_INDEX_LIBRARY_COUNT).filter(|library| self.contains(library))
    }
}

impl Extend<u8> for LibrarySet {
    fn extend<I: IntoIterator<Item = u8>>(&mut self, libraries: I) {
        for library in libraries {
            self.insert(library);
        }
    }
}
