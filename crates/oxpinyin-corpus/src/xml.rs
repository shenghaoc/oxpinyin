//! Minimal streaming extractor for MediaWiki XML export dumps.
//!
//! Hand-rolled on purpose: the corpus front-end must not grow a
//! dependency for what is a two-tag scan (`<page>` … `</page>`, then
//! `<title>` / `<ns>` / `<text>` per page). Handles the export-0.11
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
            match self.scanner.find(b"<page")? {
                None => return Ok(None),
                Some(_) => {
                    self.scanner.pos += b"<page".len();
                    self.scanner.skip_until(b'>')?;
                    self.page_base = self.scanner.absolute();
                    self.page.clear();
                    self.in_page = true;
                }
            }
        }
        match self.scanner.find_into(b"</page>", &mut self.page)? {
            None => Err(CorpusError::MalformedXml {
                detail: "unterminated <page> at end of input".into(),
            }),
            Some(_) => {
                self.scanner.pos += b"</page>".len();
                self.in_page = false;
                let block = std::mem::take(&mut self.page);
                Ok(Some(parse_page(&block, self.page_base)?))
            }
        }
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
            let open_end = find_bytes(block, b"<title>")
                .map(|start| start + b"<title>".len())
                .unwrap_or(0);
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

/// Byte-slice search (naive; pages are small).
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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
        match named {
            Some(byte) => out.push(byte),
            None => match decode_numeric(name).and_then(char::from_u32) {
                Some(ch) => {
                    let mut buf = [0_u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                None => {
                    out.push(b'&');
                    out.extend_from_slice(name);
                    out.push(b';');
                }
            },
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
    fn absolute(&self) -> usize {
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
    use super::{Page, XmlDump, decode_entities, parse_page};

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
            other => panic!("expected InvalidUtf8, got {other}"),
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
