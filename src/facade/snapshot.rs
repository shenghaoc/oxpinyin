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

#[cfg(test)]
use std::ffi::CStr;
use std::ffi::c_char;
use std::ops::{Deref, DerefMut};

use crate::PhraseText;

/// A byte offset and byte length identifying a string in its owning snapshot, excluding
/// the terminator. Use the span with the snapshot that interned it.
/// [`CandidateSnapshot::take`] transfers the buffer and its spans to the returned
/// snapshot. [`CandidateSnapshot::clear`] invalidates them.
///
/// A span survives buffer growth. Growth repoints rows held in the snapshot. A separately
/// copied raw string pointer may become invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSpan {
    start: u32,
    len: u32,
}

impl TextSpan {
    /// A missing string, exposed as a null C pointer. An interned empty byte string has a
    /// terminator and a non-null pointer.
    pub const NULL: Self = Self {
        start: u32::MAX,
        len: 0,
    };
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
///
/// A side table, keyed by row position and usually empty, holds what the
/// packed text cannot: the exact text of a row whose phrase is stored words
/// ([`Self::keep_stored_text`]).
#[derive(Debug)]
pub struct CandidateSnapshot<R> {
    rows: Vec<R>,
    text: Vec<u8>,
    stored_texts: Vec<(u32, PhraseText)>,
}

/// The pointer to `span`'s string inside `text`, or to an empty string
/// when the span is not of this buffer (which the snapshot's own calls
/// never produce).
///
/// The bounds and the terminator are checked, so any span points inside
/// the buffer at a terminated string. The bytes before the terminator are
/// not scanned again: [`CandidateSnapshot::intern`] refused an interior NUL
/// when it wrote them. A span is valid only for the build that interned it
/// ([`TextSpan`]); these checks do not detect a span from an earlier build,
/// which can land on a shorter string.
fn span_ptr(text: &[u8], span: TextSpan) -> *const c_char {
    if span == TextSpan::NULL {
        return std::ptr::null();
    }
    let start = span.start as usize;
    let end = start.saturating_add(span.len as usize);
    match text.get(start..=end) {
        Some(bytes) if bytes.last() == Some(&0) => bytes.as_ptr().cast(),
        _ => c"".as_ptr(),
    }
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
            stored_texts: Vec::new(),
        }
    }

    /// Drops every row and every string. Capacity stays, as a cleared
    /// `Vec`'s does, so the next rebuild of the same size allocates nothing.
    pub fn clear(&mut self) {
        self.rows.clear();
        self.text.clear();
        self.stored_texts.clear();
    }

    /// Keeps `text` as the exact text of the row about to be pushed.
    ///
    /// The packed copy of a stored-words phrase is lossy: it stops at the
    /// first zero word and drops the stored length
    /// (`crate::marshal::phrase_text_bytes`), while the selection
    /// record and the constraint store keep every stored word
    /// ([`crate::engine::SelectionTarget::text`]). Call it only for such
    /// a text; the packed bytes of every other text are exact.
    pub fn keep_stored_text(&mut self, text: PhraseText) {
        let row = u32::try_from(self.rows.len()).unwrap_or(u32::MAX);
        self.stored_texts.push((row, text));
    }

    /// `span`'s bytes without the terminator. A span that is not of this
    /// buffer (which the type's own calls never produce) reads as empty.
    #[cfg(test)]
    #[must_use]
    pub fn text_bytes(&self, span: TextSpan) -> &[u8] {
        let start = span.start as usize;
        self.text
            .get(start..start.saturating_add(span.len as usize))
            .unwrap_or(&[])
    }

    /// `span`'s string with its terminator.
    #[cfg(test)]
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
    #[cfg(test)]
    #[must_use]
    pub fn text_ptr(&self, span: TextSpan) -> *const c_char {
        span_ptr(&self.text, span)
    }

    /// Moves the rows, strings and their spans into the returned snapshot and leaves an empty
    /// snapshot behind.
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

    /// Appends rows from `other` in order, copying their text into this snapshot. A missing
    /// string remains null. A row whose text cannot be interned is omitted together with its
    /// associated stored text.
    pub fn append(&mut self, other: Self) {
        self.reserve(other.rows.len(), other.text.len());
        let Self {
            rows,
            text,
            mut stored_texts,
        } = other;
        for (index, mut row) in rows.into_iter().enumerate() {
            let old = u32::try_from(index).unwrap_or(u32::MAX);
            let new = self.rows.len();
            if row.text_span() == TextSpan::NULL {
                self.push(row);
            } else {
                let Some(span) = self.intern(other_text(&text, row.text_span())) else {
                    continue;
                };
                row.set_text(span, span_ptr(&self.text, span));
                self.rows.push(row);
            }
            if let Some(slot) = stored_texts.iter().position(|(kept, _)| *kept == old) {
                let (_, kept) = stored_texts.swap_remove(slot);
                self.stored_texts
                    .push((u32::try_from(new).unwrap_or(u32::MAX), kept));
            }
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
mod tests;
