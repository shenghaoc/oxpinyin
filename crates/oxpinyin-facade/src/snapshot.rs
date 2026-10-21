//! The C facades' candidate snapshot: the rows a `lookup_candidate_t *`
//! addresses, with every row's NUL-terminated text packed into one buffer.
//!
//! The pin keeps one `GArray` of `lookup_candidate_t` and one `g_strdup`ed
//! string per row (`pinyin.cpp:97-108`, `:1335-1355` at 074a2219). The
//! snapshot keeps the rows in one vector and the strings in one byte
//! buffer, so a guess that lists `n` candidates makes two allocations
//! instead of `n + 1`, and freeing the instance frees two blocks instead
//! of `n + 1`. Nothing observable changes: a string pointer handed to C
//! points into the buffer exactly as it pointed into a `CString` before,
//! and both the rows and the buffer are rebuilt — pointers invalidated —
//! at the same calls as before (a guess, a predicted guess, a reset, the
//! instance free).
//!
//! Each row carries its string's pointer as well as its span, because the
//! zhuyin facade's string getter answers a NULL instance exactly as the
//! pin does (`zhuyin.cpp:1865-1870` reads only the candidate) and so
//! cannot reach the buffer through the instance. The snapshot keeps every
//! row's pointer current: a row is pointed at its string when it is
//! pushed, and whenever the buffer moves (a reservation or an interning
//! that outgrows the capacity) every row is pointed again, so no row ever
//! holds a pointer into a freed buffer.

use std::ffi::{CStr, c_char};
use std::ops::{Deref, DerefMut};

/// One interned string's place in the snapshot's text buffer: the byte
/// offset of its first byte and its length without the terminator.
///
/// `u32` offsets size the row to need: a snapshot holds at most a few
/// hundred phrases of at most 16 characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSpan {
    start: u32,
    len: u32,
}

/// A snapshot row that carries one [`TextSpan`] into its snapshot's buffer
/// and the pointer C reads that string through.
pub trait SnapshotRow {
    /// The row's text span.
    fn text_span(&self) -> TextSpan;
    /// Points the row at `span`, whose string now starts at `ptr`.
    fn set_text(&mut self, span: TextSpan, ptr: *const c_char);
}

/// The rows and their packed, NUL-terminated texts.
#[derive(Debug)]
pub struct CandidateSnapshot<R> {
    rows: Vec<R>,
    text: Vec<u8>,
}

/// The pointer to `span`'s string inside `text`, or to an empty string
/// when the span is not of this buffer (which the snapshot's own calls
/// never produce).
fn span_ptr(text: &[u8], span: TextSpan) -> *const c_char {
    let start = span.start as usize;
    text.get(start..=start.saturating_add(span.len as usize))
        .and_then(|bytes| CStr::from_bytes_with_nul(bytes).ok())
        .unwrap_or(c"")
        .as_ptr()
}

impl<R> Default for CandidateSnapshot<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R> CandidateSnapshot<R> {
    /// An empty snapshot; allocates nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            rows: Vec::new(),
            text: Vec::new(),
        }
    }

    /// Drops every row and every string. Capacity stays, as a cleared
    /// `Vec`'s does, so the next rebuild of the same size allocates nothing.
    pub fn clear(&mut self) {
        self.rows.clear();
        self.text.clear();
    }

    /// `span`'s bytes without the terminator. A span that is not of this
    /// buffer (which the type's own calls never produce) reads as empty.
    #[must_use]
    pub fn text_bytes(&self, span: TextSpan) -> &[u8] {
        let start = span.start as usize;
        self.text
            .get(start..start.saturating_add(span.len as usize))
            .unwrap_or(&[])
    }

    /// `span`'s string with its terminator.
    #[must_use]
    pub fn text(&self, span: TextSpan) -> &CStr {
        let start = span.start as usize;
        self.text
            .get(start..=start.saturating_add(span.len as usize))
            .and_then(|bytes| CStr::from_bytes_with_nul(bytes).ok())
            .unwrap_or(c"")
    }

    /// The pointer C reads `span`'s string through: the first byte of the
    /// string inside the buffer, NUL-terminated there.
    #[must_use]
    pub fn text_ptr(&self, span: TextSpan) -> *const c_char {
        span_ptr(&self.text, span)
    }

    /// Takes the snapshot out, leaving an empty one behind.
    #[must_use]
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }
}

impl<R: SnapshotRow> CandidateSnapshot<R> {
    /// Sizes the snapshot for `rows` more rows and `text_bytes` more bytes
    /// of terminated text, so the rebuild that follows allocates at most
    /// once per buffer and exactly to need. Rows already held are pointed
    /// at the buffer again if it moved.
    pub fn reserve(&mut self, rows: usize, text_bytes: usize) {
        self.rows.reserve_exact(rows);
        if text_bytes > self.text.capacity() - self.text.len() {
            self.text.reserve_exact(text_bytes);
            self.repoint();
        }
    }

    /// Appends `bytes` and a terminator to the text buffer.
    ///
    /// `None` when `bytes` carry an interior NUL — the input `CString::new`
    /// refuses, and the row the caller drops as before — or when the buffer
    /// would outgrow the span's offsets. Rows already held are pointed at
    /// the buffer again if it moved.
    pub fn intern(&mut self, bytes: &[u8]) -> Option<TextSpan> {
        if bytes.contains(&0) {
            return None;
        }
        let start = u32::try_from(self.text.len()).ok()?;
        let len = u32::try_from(bytes.len()).ok()?;
        // The terminator must fit the offsets too.
        let needed = start.checked_add(len)?.checked_add(1)? as usize;
        let moved = needed > self.text.capacity();
        self.text.extend_from_slice(bytes);
        self.text.push(0);
        if moved {
            self.repoint();
        }
        Some(TextSpan { start, len })
    }

    /// Appends one row, pointed at its span's string.
    pub fn push(&mut self, mut row: R) {
        let span = row.text_span();
        row.set_text(span, span_ptr(&self.text, span));
        self.rows.push(row);
    }

    /// Appends `other`'s rows after this snapshot's, re-interning each
    /// row's text into this buffer. A row whose text cannot be interned
    /// here is dropped, as a `CString::new` failure dropped it before.
    pub fn append(&mut self, other: Self) {
        self.reserve(other.rows.len(), other.text.len());
        for mut row in other.rows {
            let Some(span) = self.intern(other_text(&other.text, row.text_span())) else {
                continue;
            };
            row.set_text(span, span_ptr(&self.text, span));
            self.rows.push(row);
        }
    }

    /// Points every row at its string in the buffer as it now stands.
    fn repoint(&mut self) {
        let text = &self.text;
        for row in &mut self.rows {
            let span = row.text_span();
            row.set_text(span, span_ptr(text, span));
        }
    }
}

fn other_text(text: &[u8], span: TextSpan) -> &[u8] {
    let start = span.start as usize;
    text.get(start..start.saturating_add(span.len as usize))
        .unwrap_or(&[])
}

impl<R> Deref for CandidateSnapshot<R> {
    type Target = [R];

    fn deref(&self) -> &[R] {
        &self.rows
    }
}

impl<R> DerefMut for CandidateSnapshot<R> {
    fn deref_mut(&mut self) -> &mut [R] {
        &mut self.rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Row(TextSpan, *const c_char);

    impl SnapshotRow for Row {
        fn text_span(&self) -> TextSpan {
            self.0
        }

        fn set_text(&mut self, span: TextSpan, ptr: *const c_char) {
            self.0 = span;
            self.1 = ptr;
        }
    }

    fn row(span: TextSpan) -> Row {
        Row(span, std::ptr::null())
    }

    #[test]
    fn interned_strings_are_terminated_and_addressable() {
        let mut snapshot = CandidateSnapshot::<Row>::new();
        let a = snapshot.intern("你好".as_bytes()).expect("interned");
        let b = snapshot.intern(b"").expect("interned");
        snapshot.push(row(a));
        snapshot.push(row(b));
        assert_eq!(snapshot.len(), 2);
        assert_eq!(
            snapshot[0].1,
            snapshot.text_ptr(a),
            "a pushed row points at its string"
        );
        assert_eq!(snapshot.text(a).to_bytes(), "你好".as_bytes());
        assert_eq!(snapshot.text(b).to_bytes(), b"");
        assert_eq!(snapshot.text_bytes(a), "你好".as_bytes());
        assert!(
            snapshot.intern(b"a\0b").is_none(),
            "an interior NUL is refused"
        );
        assert_eq!(snapshot.text_ptr(a), snapshot.text(a).as_ptr());
        snapshot.clear();
        assert!(snapshot.is_empty());
        assert_eq!(
            snapshot.text(a).to_bytes(),
            b"",
            "a stale span reads as empty"
        );
    }

    #[test]
    fn append_re_interns_the_other_rows() {
        let mut first = CandidateSnapshot::<Row>::new();
        let x = first.intern(b"x").expect("interned");
        first.push(row(x));
        let mut second = CandidateSnapshot::<Row>::new();
        let yy = second.intern(b"yy").expect("interned");
        second.push(row(yy));
        first.append(second);
        assert_eq!(first.len(), 2);
        assert_eq!(first.text(first[0].0).to_bytes(), b"x");
        assert_eq!(first.text(first[1].0).to_bytes(), b"yy");
        assert_eq!(
            first[0].1,
            first.text_ptr(first[0].0),
            "rows follow a moved buffer"
        );
        assert_eq!(first[1].1, first.text_ptr(first[1].0));
    }

    #[test]
    fn growth_repoints_every_row() {
        let mut snapshot = CandidateSnapshot::<Row>::new();
        for _ in 0..64 {
            let span = snapshot.intern(b"abcdefgh").expect("interned");
            snapshot.push(row(span));
        }
        for item in snapshot.iter() {
            assert_eq!(item.1, snapshot.text_ptr(item.0));
        }
    }
}
