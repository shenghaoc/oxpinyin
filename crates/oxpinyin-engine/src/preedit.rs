//! Preedit as text plus styled spans.
//!
//! Never markup, never a platform attribute list. A shell maps the three
//! styles onto its own underline and highlight conventions.

/// How a shell should present one preedit span.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum SpanStyle {
    /// Input the user typed that the engine has not converted.
    Raw,
    /// Engine output that the user has not chosen yet.
    Converted,
    /// Text the user has already chosen in this composition.
    Selected,
}

/// One styled byte range of a [`Preedit`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PreeditSpan {
    start: usize,
    end: usize,
    style: SpanStyle,
}

impl PreeditSpan {
    pub(crate) const fn new(start: usize, end: usize, style: SpanStyle) -> Self {
        Self { start, end, style }
    }

    /// Inclusive byte offset where the span begins.
    #[must_use]
    pub const fn start(self) -> usize {
        self.start
    }

    /// Exclusive byte offset where the span ends.
    #[must_use]
    pub const fn end(self) -> usize {
        self.end
    }

    /// Length of the span in bytes.
    #[must_use]
    pub const fn len(self) -> usize {
        self.end - self.start
    }

    /// Whether the span covers no bytes.
    ///
    /// A [`Preedit`] never contains one; the accessor exists so callers can
    /// check ranges they build themselves.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// How the span should be presented.
    #[must_use]
    pub const fn style(self) -> SpanStyle {
        self.style
    }
}

/// What the shell should show in place of the raw keystrokes.
///
/// The spans are non-overlapping, ascending, and together cover `text`
/// exactly. `cursor` is a byte offset into `text` on a character boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Preedit {
    text: String,
    spans: Vec<PreeditSpan>,
    cursor: usize,
}

impl Preedit {
    pub(crate) const fn new(text: String, spans: Vec<PreeditSpan>, cursor: usize) -> Self {
        Self {
            text,
            spans,
            cursor,
        }
    }

    /// The text to display.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The styled spans, ascending and covering [`Preedit::text`] exactly.
    #[must_use]
    pub fn spans(&self) -> &[PreeditSpan] {
        &self.spans
    }

    /// Cursor position as a byte offset into [`Preedit::text`].
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Whether there is nothing to display.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Preedit, PreeditSpan, SpanStyle};

    #[test]
    fn spans_cover_the_text_exactly() {
        let preedit = Preedit::new(
            "\u{4f60}hao".to_owned(),
            vec![
                PreeditSpan::new(0, 3, SpanStyle::Selected),
                PreeditSpan::new(3, 6, SpanStyle::Raw),
            ],
            6,
        );

        let mut covered = 0;
        for span in preedit.spans() {
            assert_eq!(span.start(), covered, "spans must be contiguous");
            assert!(!span.is_empty());
            covered = span.end();
        }
        assert_eq!(covered, preedit.text().len());
        assert_eq!(preedit.cursor(), preedit.text().len());
        assert!(preedit.text().is_char_boundary(preedit.cursor()));
        assert_eq!(preedit.spans()[0].len(), 3);
        assert_eq!(preedit.spans()[1].style(), SpanStyle::Raw);
    }

    #[test]
    fn the_default_preedit_is_empty() {
        let preedit = Preedit::default();
        assert!(preedit.is_empty());
        assert!(preedit.spans().is_empty());
        assert_eq!(preedit.cursor(), 0);
    }
}
