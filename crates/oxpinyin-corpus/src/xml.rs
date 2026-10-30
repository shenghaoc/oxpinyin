//! Minimal streaming extractor for `MediaWiki` XML export dumps.
//!
//! Hand-rolled on purpose: the corpus front-end must not grow an XML
//! parser dependency for what is a two-tag scan (`<page>` … `</page>`, then
//! `<title>` / `<ns>` / `<text>` per page); only the byte-substring search
//! under it is `memchr::memmem`. Handles the export-0.11
//! shape produced by `Special:Export` and `dumps.wikimedia.org` alike.
//! Entity decoding covers the five XML built-ins plus numeric references.

use std::io::{BufReader, Read};

use crate::error::CorpusError;

/// One `<page>` block: title, namespace, and the raw (entity-decoded)
/// wikitext of the latest revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Page {
    /// Page title, entity-decoded.
    pub title: String,
    /// Namespace key; articles are `0`.
    pub ns: u32,
    /// Entity-decoded wikitext, or `None` when the page has no text
    /// (deleted/empty revisions).
    pub text: Option<String>,
}

/// Streaming iterator over the `<page>` blocks of an XML dump.
pub struct XmlDump<R: Read> {
    scanner: Scanner<R>,
    /// Absolute byte offset of the first content byte of the page
    /// currently being accumulated (for error offsets).
    page_base: usize,
    /// Accumulated `<page>` content bytes.
    page: Vec<u8>,
    /// Whether a page is currently open.
    in_page: bool,
}

impl<R: Read> XmlDump<R> {
    /// Wraps a dump reader. Chunked internally; memory is bounded by the
    /// largest single page plus one chunk.
    pub fn new(reader: R) -> Self {
        Self {
            scanner: Scanner::new(reader),
            page_base: 0,
            page: Vec::new(),
            in_page: false,
        }
    }

    /// The next page block, or `None` at end of input.
    ///
    /// # Errors
    ///
    /// Returns [`CorpusError::MalformedXml`] when a `<page>` open tag has
    /// no `>` or a page has no `</page>` before EOF.
    pub fn next_page(&mut self) -> Result<Option<Page>, CorpusError> {
        if !self.in_page {
            if self.scanner.find(b"<page")?.is_none() {
                return Ok(None);
            }
            self.scanner.pos += b"<page".len();
            self.scanner.skip_until(b'>')?;
            self.page_base = self.scanner.absolute();
            self.page.clear();
            self.in_page = true;
        }
        if self
            .scanner
            .find_into(b"</page>", &mut self.page)?
            .is_none()
        {
            return Err(CorpusError::MalformedXml {
                detail: "unterminated <page> at end of input".into(),
            });
        }
        self.scanner.pos += b"</page>".len();
        self.in_page = false;
        let block = std::mem::take(&mut self.page);
        Ok(Some(parse_page(&block, self.page_base)?))
    }
}

/// Parses one `<page>` block into title / namespace / text.
///
/// # Errors
///
/// [`CorpusError::InvalidUtf8`] when structural or content bytes are not
/// UTF-8; [`CorpusError::MalformedXml`] when `<text` has no closing tag.
pub fn parse_page(block: &[u8], base: usize) -> Result<Page, CorpusError> {
    let title_bytes = tag_content(block, b"<title>", b"</title>");
    let ns = tag_content(block, b"<ns>", b"</ns>")
        .and_then(|slice| std::str::from_utf8(slice).ok())
        .map(str::trim)
        .and_then(|text| text.parse::<u32>().ok())
        .unwrap_or(0);

    let text = match find_bytes(block, b"<text") {
        None => None,
        Some(start) => {
            let mut cursor = start + b"<text".len();
            // Skip the open tag: attributes are rare (`xml:space`) but a
            // quoted `>` must not terminate the scan.
            let mut quote = None;
            let open_end = loop {
                match block.get(cursor) {
                    None => {
                        return Err(CorpusError::MalformedXml {
                            detail: "unterminated <text> open tag".into(),
                        });
                    }
                    Some(b'"') | Some(b'\'') if quote.is_none() => quote = Some(block[cursor]),
                    Some(&byte) if quote == Some(byte) => quote = None,
                    Some(b'>') if quote.is_none() => break cursor + 1,
                    Some(_) => {}
                }
                cursor += 1;
            };
            if cursor >= 2 && block[cursor - 1] == b'/' {
                // Self-closing `<text … />`: no content.
                None
            } else {
                let end = match find_bytes(&block[open_end..], b"</text>") {
                    None => {
                        return Err(CorpusError::MalformedXml {
                            detail: "unterminated <text> in page".into(),
                        });
                    }
                    Some(relative) => open_end + relative,
                };
                let content = &block[open_end..end];
                let decoded = decode_entities(content);
                let text =
                    String::from_utf8(decoded).map_err(|error| CorpusError::InvalidUtf8 {
                        offset: base + open_end + error.utf8_error().valid_up_to(),
                    })?;
                Some(text)
            }
        }
    };

    let title = match title_bytes {
        Some(bytes) => {
            let decoded = decode_entities(bytes);
            // The decoded slice starts just after `<title>` inside the
            // page block; the absolute offset must include it, matching
            // the `<text>` branch above.
            let open_end =
                find_bytes(block, b"<title>").map_or(0, |start| start + b"<title>".len());
            String::from_utf8(decoded).map_err(|error| CorpusError::InvalidUtf8 {
                offset: base + open_end + error.utf8_error().valid_up_to(),
            })?
        }
        None => String::new(),
    };

    Ok(Page { title, ns, text })
}

/// Returns the byte slice between the first `open` and the `close` that
/// follows it, or `None` when `open` is absent.
fn tag_content<'a>(haystack: &'a [u8], open: &[u8], close: &[u8]) -> Option<&'a [u8]> {
    let start = find_bytes(haystack, open)? + open.len();
    let end = find_bytes(&haystack[start..], close)? + start;
    Some(&haystack[start..end])
}

/// Offset of the first occurrence of `needle` in `haystack`; an empty
/// needle matches at 0.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    #[cfg(test)]
    if tests::NAIVE.with(std::cell::Cell::get) {
        return tests::naive_find_bytes(haystack, needle);
    }
    memchr::memmem::find(haystack, needle)
}

/// Decodes XML entities in a byte slice, returning owned bytes. Unknown
/// entities stay verbatim.
pub fn decode_entities(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.iter().position(|&byte| byte == b'&') {
        out.extend_from_slice(&rest[..amp]);
        rest = &rest[amp + 1..];
        let Some(semi) = rest
            .iter()
            .position(|&byte| byte == b';')
            .filter(|&pos| pos <= 32)
        else {
            out.push(b'&');
            continue;
        };
        let name = &rest[..semi];
        let named = match name {
            b"lt" => Some(b'<'),
            b"gt" => Some(b'>'),
            b"amp" => Some(b'&'),
            b"quot" => Some(b'"'),
            b"apos" => Some(b'\''),
            _ => None,
        };
        if let Some(byte) = named {
            out.push(byte);
        } else if let Some(ch) = decode_numeric(name).and_then(char::from_u32) {
            let mut buf = [0_u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        } else {
            out.push(b'&');
            out.extend_from_slice(name);
            out.push(b';');
        }
        rest = &rest[semi + 1..];
    }
    out.extend_from_slice(rest);
    out
}

/// `#123` → `Some(123)`, `#x1F` → `Some(31)`; otherwise `None`.
fn decode_numeric(name: &[u8]) -> Option<u32> {
    let digits = name.strip_prefix(b"#")?;
    if let Some(hex) = digits
        .strip_prefix(b"x")
        .or_else(|| digits.strip_prefix(b"X"))
    {
        let text = std::str::from_utf8(hex).ok()?;
        u32::from_str_radix(text, 16).ok()
    } else {
        let text = std::str::from_utf8(digits).ok()?;
        text.parse().ok()
    }
}

/// Buffered byte scanner with needle search across chunk boundaries.
struct Scanner<R: Read> {
    inner: BufReader<R>,
    buf: Vec<u8>,
    /// Next unread byte in `buf`.
    pos: usize,
    /// Absolute stream offset of `buf[0]`.
    base: usize,
    eof: bool,
}

impl<R: Read> Scanner<R> {
    const CHUNK: usize = 64 * 1024;

    fn new(reader: R) -> Self {
        Self {
            inner: BufReader::with_capacity(Self::CHUNK, reader),
            buf: Vec::new(),
            pos: 0,
            base: 0,
            eof: false,
        }
    }

    /// The absolute stream offset of the next unread byte.
    const fn absolute(&self) -> usize {
        self.base + self.pos
    }

    /// Compacts consumed bytes and tops the buffer up from the reader.
    fn fill(&mut self) -> Result<usize, CorpusError> {
        if self.pos > 0 {
            self.buf.drain(..self.pos);
            self.base += self.pos;
            self.pos = 0;
        }
        if self.eof {
            return Ok(0);
        }
        let mut chunk = vec![0_u8; Self::CHUNK];
        let read = self.inner.read(&mut chunk).map_err(CorpusError::Io)?;
        if read == 0 {
            self.eof = true;
            return Ok(0);
        }
        chunk.truncate(read);
        self.buf.extend_from_slice(&chunk);
        Ok(read)
    }

    /// Advances to the start of the next occurrence of `needle`, returning
    /// its absolute offset, or `None` at EOF. Bytes skipped while
    /// searching are appended to `sink`.
    fn find_into(
        &mut self,
        needle: &[u8],
        sink: &mut Vec<u8>,
    ) -> Result<Option<usize>, CorpusError> {
        loop {
            if let Some(relative) = find_bytes(&self.buf[self.pos..], needle) {
                // Everything before the match belongs to the sink.
                sink.extend_from_slice(&self.buf[self.pos..self.pos + relative]);
                self.pos += relative;
                return Ok(Some(self.base + self.pos));
            }
            // Keep a needle-length tail so a match spanning the boundary
            // is not missed; the rest is sink-bound.
            let keep = needle.len().saturating_sub(1);
            let drop = self.buf.len().saturating_sub(keep);
            if self.pos <= drop {
                sink.extend_from_slice(&self.buf[self.pos..drop]);
            }
            self.buf.drain(..drop);
            self.base += drop;
            self.pos = self.pos.saturating_sub(drop);
            if self.eof {
                return Ok(None);
            }
            if self.fill()? == 0 && self.eof {
                return Ok(None);
            }
        }
    }

    /// Advances to the start of the next occurrence of `needle`, returning
    /// its absolute offset, or `None` at EOF. Skipped bytes are dropped.
    fn find(&mut self, needle: &[u8]) -> Result<Option<usize>, CorpusError> {
        let mut sink = Vec::new();
        self.find_into(needle, &mut sink)
    }

    /// Skips bytes until the next `delimiter` (consumed as well).
    fn skip_until(&mut self, delimiter: u8) -> Result<(), CorpusError> {
        loop {
            if let Some(relative) = self.buf[self.pos..]
                .iter()
                .position(|&byte| byte == delimiter)
            {
                self.pos += relative + 1;
                return Ok(());
            }
            // The whole buffer is discarded; `absolute()` is base + pos,
            // so base must advance by the discarded bytes, exactly like
            // `fill` / `find_into` do.
            self.base += self.buf.len();
            self.buf.clear();
            self.pos = 0;
            if self.eof || self.fill()? == 0 {
                return Err(CorpusError::MalformedXml {
                    detail: "open tag not terminated by '>'".into(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io::Read;

    use proptest::prelude::*;

    use super::{Page, Scanner, XmlDump, decode_entities, find_bytes, parse_page};

    thread_local! {
        /// Routes `find_bytes` to [`naive_find_bytes`] on this test thread.
        pub(super) static NAIVE: Cell<bool> = const { Cell::new(false) };
    }

    /// The pre-memchr search, kept verbatim as the equivalence reference.
    pub(super) fn naive_find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return Some(0);
        }
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn with_naive<T>(naive: bool, body: impl FnOnce() -> T) -> T {
        NAIVE.with(|flag| flag.set(naive));
        let out = body();
        NAIVE.with(|flag| flag.set(false));
        out
    }

    const CHUNK: usize = Scanner::<&[u8]>::CHUNK;

    /// Hands out at most `step` bytes per `read`, so the scanner meets a
    /// read boundary at every multiple of `step` instead of every `CHUNK`.
    struct Trickle<'a> {
        data: &'a [u8],
        step: usize,
    }

    impl Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let take = self.step.min(buf.len()).min(self.data.len());
            buf[..take].copy_from_slice(&self.data[..take]);
            self.data = &self.data[take..];
            Ok(take)
        }
    }

    /// Every page a dump yields, then its first error as Debug text.
    type Drained = Vec<Result<Page, String>>;

    /// Drains `reader` through [`XmlDump`] into a [`Drained`] sequence.
    fn drain_dump(reader: impl Read) -> Drained {
        let mut dump = XmlDump::new(reader);
        let mut out = Vec::new();
        loop {
            match dump.next_page() {
                Ok(Some(page)) => out.push(Ok(page)),
                Ok(None) => return out,
                Err(error) => {
                    out.push(Err(format!("{error:?}")));
                    return out;
                }
            }
        }
    }

    /// Asserts the memchr search and the naive reference yield the same
    /// page sequence for `input`, read in `CHUNK`s and at every step in
    /// `steps`; returns that sequence.
    fn assert_dump_equivalent(input: &[u8], steps: &[usize]) -> Drained {
        let reference = with_naive(true, || drain_dump(input));
        assert_eq!(drain_dump(input), reference, "memchr vs naive, CHUNK reads");
        for &step in steps {
            let trickle = || Trickle { data: input, step };
            let naive = with_naive(true, || drain_dump(trickle()));
            assert_eq!(naive, reference, "naive, step {step} vs CHUNK reads");
            assert_eq!(drain_dump(trickle()), reference, "memchr, step {step}");
        }
        reference
    }

    #[test]
    fn find_bytes_matches_hand_computed_offsets() {
        let cases: &[(&[u8], &[u8], Option<usize>)] = &[
            (b"", b"", Some(0)),
            (b"abc", b"", Some(0)),
            (b"", b"a", None),
            (b"a", b"a", Some(0)),
            (b"xyz<", b"<", Some(3)),
            (b"xyz", b"<", None),
            (b"ab", b"abc", None),
            (b"<page>rest", b"<page", Some(0)),
            (b"body</page>", b"</page>", Some(4)),
            (b"body</page", b"</page>", None),
            (b"aaaaab", b"aaab", Some(2)),
            (b"abababab", b"abab", Some(0)),
            (b"</pag</page</page>", b"</page>", Some(11)),
            (b"\0\0</ns>\0", b"</ns>", Some(2)),
            (b"\xff\xfe<text\xc3", b"<text", Some(2)),
            (b"<title>a</title><title>b</title>", b"</title>", Some(8)),
        ];
        for &(haystack, needle, expected) in cases {
            assert_eq!(
                naive_find_bytes(haystack, needle),
                expected,
                "naive {haystack:?} / {needle:?}"
            );
            assert_eq!(
                find_bytes(haystack, needle),
                expected,
                "memchr {haystack:?} / {needle:?}"
            );
        }
    }

    #[test]
    fn find_bytes_on_long_repetitive_input() {
        // 1 MiB of `</page` near-misses (ending in a partial `</pa`), the
        // naive scan's worst shape for this needle.
        let mut haystack: Vec<u8> = b"</page".iter().copied().cycle().take(1 << 20).collect();
        assert_eq!(naive_find_bytes(&haystack, b"</page>"), None);
        assert_eq!(find_bytes(&haystack, b"</page>"), None);
        haystack.extend_from_slice(b"</page>");
        assert_eq!(naive_find_bytes(&haystack, b"</page>"), Some(1 << 20));
        assert_eq!(find_bytes(&haystack, b"</page>"), Some(1 << 20));
    }

    /// Tag-soup fragments: real needles, their prefixes, quotes, entities,
    /// NUL, invalid UTF-8 and multi-byte text.
    const FRAGMENTS: &[&[u8]] = &[
        b"<page>",
        b"<page id=\"1\">",
        b"</page>",
        b"<title>",
        b"</title>",
        b"<ns>",
        b"0",
        b"14",
        b"</ns>",
        b"<text>",
        b"<text xml:space=\"preserve\">",
        b"<text a='>'>",
        b"<text/>",
        b"</text>",
        b"</pag",
        b"<",
        b"/",
        b">",
        b"\"",
        b"'",
        b"&amp;",
        b"&#x4E2D;",
        b"\0",
        b"\xff",
        b"a",
        "中文".as_bytes(),
    ];

    proptest! {
        #[test]
        fn find_bytes_agrees_with_naive_on_arbitrary_bytes(
            haystack in proptest::collection::vec(any::<u8>(), 0..300),
            needle in proptest::collection::vec(any::<u8>(), 0..10),
        ) {
            prop_assert_eq!(find_bytes(&haystack, &needle), naive_find_bytes(&haystack, &needle));
        }

        #[test]
        fn find_bytes_agrees_with_naive_on_a_small_alphabet(
            haystack in proptest::collection::vec(prop::sample::select(b"<a/>\0".to_vec()), 0..300),
            needle in proptest::collection::vec(prop::sample::select(b"<a/>\0".to_vec()), 0..8),
        ) {
            prop_assert_eq!(find_bytes(&haystack, &needle), naive_find_bytes(&haystack, &needle));
        }

        #[test]
        fn find_bytes_agrees_with_naive_on_planted_needles(
            haystack in proptest::collection::vec(any::<u8>(), 0..2000),
            needle in proptest::collection::vec(any::<u8>(), 1..10),
            at in any::<prop::sample::Index>(),
        ) {
            let mut planted = haystack;
            let pos = at.index(planted.len() + 1);
            planted.splice(pos..pos, needle.iter().copied());
            let found = find_bytes(&planted, &needle);
            prop_assert_eq!(found, naive_find_bytes(&planted, &needle));
            prop_assert!(found.is_some_and(|offset| offset <= pos));
        }

        #[test]
        fn dump_agrees_with_naive_on_tag_soup(
            fragments in proptest::collection::vec(prop::sample::select(FRAGMENTS.to_vec()), 0..60),
            step in 1_usize..40,
        ) {
            assert_dump_equivalent(&fragments.concat(), &[step]);
        }
    }

    #[test]
    fn mediawiki_fixture_is_identical_at_every_read_step() {
        let fixture = include_bytes!("../../../fixtures/w9/corpus-xml.xml");
        let pages = assert_dump_equivalent(fixture, &[1, 7, 4096, CHUNK - 1]);
        let titles: Vec<&str> = pages
            .iter()
            .map(|page| page.as_ref().expect("valid fixture").title.as_str())
            .collect();
        assert_eq!(titles, ["裸茎金腰", "昌国街道", "苇河镇"]);
    }

    #[test]
    fn needles_straddling_read_boundaries_are_found() {
        let head = b"<mediawiki><page><title>t</title><ns>0</ns><text>";
        // `split` bytes of `</page>` land before the first CHUNK boundary,
        // the rest after it; split 0 puts the whole tag after it.
        for split in 0..=7 {
            let filler = "x".repeat(CHUNK - split - head.len() - b"</text>".len());
            let mut input = head.to_vec();
            input.extend_from_slice(filler.as_bytes());
            input.extend_from_slice(b"</text></page>");
            // The next `<page` straddles the second boundary the same way.
            input.resize(2 * CHUNK - split.min(5), b' ');
            input.extend_from_slice(b"<page><title>u</title><ns>0</ns><text>y</text></page>");
            let pages = assert_dump_equivalent(&input, &[997, CHUNK - 1]);
            assert_eq!(
                pages,
                [
                    Ok(Page {
                        title: "t".into(),
                        ns: 0,
                        text: Some(filler),
                    }),
                    Ok(Page {
                        title: "u".into(),
                        ns: 0,
                        text: Some("y".into()),
                    }),
                ],
                "split {split}"
            );
        }
    }

    #[test]
    fn page_spanning_several_reads_is_identical() {
        let body = "中文&amp;\0".repeat(3 * CHUNK / 8);
        let input = format!("<page><title>long</title><ns>0</ns><text>{body}</text></page>");
        let pages = assert_dump_equivalent(input.as_bytes(), &[997, CHUNK - 1]);
        let expected = body.replace("&amp;", "&");
        assert_eq!(pages.len(), 1);
        assert_eq!(
            pages[0].as_ref().expect("valid page").text.as_deref(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn invalid_utf8_offsets_are_absolute_across_reads() {
        // The bad byte sits past the first CHUNK boundary; both searches
        // must report its absolute stream offset.
        let mut input = b"<page><title>t</title><ns>0</ns><text>".to_vec();
        input.resize(CHUNK + 100, b'x');
        let bad = input.len();
        input.extend_from_slice(b"\xff</text></page>");
        let pages = assert_dump_equivalent(&input, &[997]);
        assert_eq!(pages, [Err(format!("InvalidUtf8 {{ offset: {bad} }}"))]);
    }

    #[test]
    fn malformed_and_truncated_dumps_fail_identically() {
        let malformed = |detail: &str| Err(format!("MalformedXml {{ detail: {detail:?} }}"));
        let ok = |title: &str, text: Option<&str>| {
            Ok(Page {
                title: title.into(),
                ns: 0,
                text: text.map(Into::into),
            })
        };
        let cases: Vec<(&[u8], Drained)> = vec![
            (b"", vec![]),
            (b"<mediawiki></mediawiki>", vec![]),
            (b"<page", vec![malformed("open tag not terminated by '>'")]),
            (
                b"<page><title>x</title>",
                vec![malformed("unterminated <page> at end of input")],
            ),
            (
                b"<page><text a='>'</page>",
                vec![malformed("unterminated <text> open tag")],
            ),
            (
                b"<page><text>abc</page>",
                vec![malformed("unterminated <text> in page")],
            ),
            (b"<page><title>e</title><text/></page>", vec![ok("e", None)]),
            (
                b"<page><title>n\0</title><text>\0z\0</text></page>",
                vec![ok("n\0", Some("\0z\0"))],
            ),
            (
                b"<page><title>a</title><text>b</text></page><page><title>",
                vec![
                    ok("a", Some("b")),
                    malformed("unterminated <page> at end of input"),
                ],
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                assert_dump_equivalent(input, &[1, 3]),
                expected,
                "{input:?}"
            );
        }
    }

    #[test]
    fn entity_decoding_covers_xml_builtins_and_numeric() {
        assert_eq!(
            decode_entities(b"a&lt;b&gt;&amp;&quot;&apos;&#65;&#x42;"),
            b"a<b>&\"'AB".to_vec()
        );
        assert_eq!(decode_entities(b"no entities"), b"no entities".to_vec());
        assert_eq!(decode_entities(b"&unknown;"), b"&unknown;".to_vec());
        assert_eq!(decode_entities(b"&nbsp;"), b"&nbsp;".to_vec());
    }

    #[test]
    fn page_parsing_extracts_title_ns_and_text() {
        let block = b"  <title>Example &amp; more</title>\n<ns>0</ns>\n\
                     <revision><text xml:space=\"preserve\">#REDIRECT [[x]]\nBody &lt;ref&gt;.</text></revision>\n";
        let page = parse_page(block, 0).expect("valid page");
        assert_eq!(page.title, "Example & more");
        assert_eq!(page.ns, 0);
        assert_eq!(page.text.as_deref(), Some("#REDIRECT [[x]]\nBody <ref>."));
    }

    #[test]
    fn self_closing_text_yields_no_text() {
        let block = b"<title>t</title><ns>2</ns><text bytes=\"0\" />";
        let page = parse_page(block, 0).expect("valid page");
        assert_eq!(page.text, None);
    }

    #[test]
    fn dump_iteration_handles_multiple_pages() {
        let dump: &[u8] = b"<mediawiki><page><title>diyi</title><ns>0</ns>\
             <text>jia</text></page><page><title>dier</title><ns>0</ns>\
             <text>yi&amp;bing</text></page></mediawiki>";
        let mut dump_reader = XmlDump::new(dump);
        let pages: Vec<Page> = std::iter::from_fn(|| dump_reader.next_page().transpose())
            .collect::<Result<_, _>>()
            .expect("valid dump");
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].title, "diyi");
        assert_eq!(pages[0].text.as_deref(), Some("jia"));
        assert_eq!(pages[1].text.as_deref(), Some("yi&bing"));
    }

    #[test]
    fn truncated_page_is_an_error() {
        let mut dump_reader = XmlDump::new(b"<page><title>x</title>".as_slice());
        assert!(dump_reader.next_page().is_err());
    }

    #[test]
    fn invalid_utf8_title_offset_includes_the_open_tag() {
        // `<title>` content starts at block offset 11; with base 1000 the
        // error offset must be 1011, not 1000.
        let block = b"junk<title>\xff</title><ns>0</ns><text>x</text>";
        let error = parse_page(block, 1000).expect_err("invalid title utf-8");
        match error {
            crate::error::CorpusError::InvalidUtf8 { offset } => assert_eq!(offset, 1011),
            other @ (crate::error::CorpusError::Io(_)
            | crate::error::CorpusError::MalformedXml { .. }
            | crate::error::CorpusError::Convert { .. }) => {
                panic!("expected InvalidUtf8, got {other}")
            }
        }
    }

    #[test]
    fn skip_until_advances_base_across_full_chunks() {
        // No `>` in the first two 64 KiB chunks: the clear branch must
        // advance `base` by the discarded bytes, or `absolute()` — and
        // every error offset derived from it — undercounts.
        let mut input = vec![b'a'; 2 * super::Scanner::<&[u8]>::CHUNK + 10];
        input.push(b'>');
        let mut scanner = super::Scanner::new(input.as_slice());
        scanner.skip_until(b'>').expect("delimiter exists");
        assert_eq!(
            scanner.absolute(),
            2 * super::Scanner::<&[u8]>::CHUNK + 11,
            "absolute() must count the discarded chunk bytes"
        );
    }

    #[test]
    fn non_article_pages_still_parse() {
        let block = b"<title>Category:X</title><ns>14</ns><text>c</text>";
        let page = parse_page(block, 0).expect("valid page");
        assert_eq!(page.ns, 14);
    }
}
