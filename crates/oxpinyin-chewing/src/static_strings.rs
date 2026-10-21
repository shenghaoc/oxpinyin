//! Shared views over generated, relocation-free string tables.

/// A UTF-8 string's byte offset and length in a generated string blob.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StringSpan {
    offset: u16,
    length: u16,
}

impl StringSpan {
    /// Constructs a span for generated table storage.
    #[must_use]
    pub const fn new(offset: u16, length: u16) -> Self {
        Self { offset, length }
    }

    const fn range(self) -> core::ops::Range<usize> {
        let start = self.offset as usize;
        start..start + self.length as usize
    }
}

/// A temporary view of a string table whose static rows contain only integers.
///
/// Table accessors construct this view on demand. The pointers in the view are
/// not repeated in each stored row. Invalid spans resolve to an empty string;
/// generated tables validate all spans and preserve their original contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StringTable<const N: usize> {
    blob: &'static str,
    rows: &'static [[StringSpan; N]],
}

impl<const N: usize> StringTable<N> {
    /// Constructs a view over generated storage.
    #[must_use]
    pub const fn new(blob: &'static str, rows: &'static [[StringSpan; N]]) -> Self {
        Self { blob, rows }
    }

    /// Number of rows in this table.
    #[must_use]
    pub const fn len(self) -> usize {
        self.rows.len()
    }

    /// Whether the table has no rows.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.rows.is_empty()
    }

    fn resolve(self, span: StringSpan) -> &'static str {
        self.blob.get(span.range()).unwrap_or_default()
    }

    /// Resolves one field without decoding the other strings in its row.
    #[must_use]
    pub fn field(self, row: usize, column: usize) -> Option<&'static str> {
        Some(self.resolve(*self.rows.get(row)?.get(column)?))
    }

    /// Resolves a row, preserving the original column order.
    #[must_use]
    pub fn get(self, row: usize) -> Option<[&'static str; N]> {
        Some(self.rows.get(row)?.map(|span| self.resolve(span)))
    }

    /// Resolves all rows in their stored order.
    #[must_use]
    pub fn entries(
        self,
    ) -> impl ExactSizeIterator<Item = [&'static str; N]> + DoubleEndedIterator + Clone {
        self.rows
            .iter()
            .map(move |row| row.map(|span| self.resolve(span)))
    }

    /// Binary-searches the first column of a table sorted by that column.
    ///
    /// The probes compare byte slices: UTF-8 byte order is the `str` order,
    /// and a byte slice needs no char-boundary check per probe.
    #[must_use]
    pub fn position(self, spelling: &str) -> Option<usize> {
        let blob = self.blob.as_bytes();
        let key = spelling.as_bytes();
        let position = self.rows.partition_point(|row| {
            row.first()
                .and_then(|span| blob.get(span.range()))
                .unwrap_or_default()
                < key
        });
        (self.field(position, 0)? == spelling).then_some(position)
    }
}
