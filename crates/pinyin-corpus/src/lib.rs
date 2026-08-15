//! Corpus front-end for the W9 training chain.
//!
//! Converts a Chinese-Wikipedia XML dump into the line-oriented raw Han
//! text that [`pinyin_segment`] (the `ngseg` reproduction) consumes:
//!
//! 1. extract `<page>` wikitext (XML layer),
//! 2. strip wiki markup,
//! 3. convert traditional → simplified via the `opencc` binary,
//! 4. split into one sentence per line.
//!
//! This is ordinary data engineering: libpinyin's own corpus preparation
//! was an undocumented private pipeline, so this stage has no libpinyin
//! oracle. It is verified by well-formedness invariants (tier 1) and the
//! end-to-end differential through T1→T2→T3→T4a (tier 2). See
//! `docs/findings/corpus-pipeline.md`. Never ships with the engine.
#![deny(unsafe_code)]
#![warn(missing_docs)]

mod convert;
mod error;
mod markup;
mod split;
mod xml;

use std::io::{BufWriter, Read, Write};

pub use convert::{Converter, DEFAULT_CONFIG};
pub use error::CorpusError;
pub use markup::strip_wikitext;
pub use split::split_sentences;
pub use xml::{Page, XmlDump};

/// Cleaner configuration.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Stop after this many pages (a bounded sample of a large dump).
    pub max_pages: Option<usize>,
    /// Traditional → simplified converter; `None` skips conversion
    /// (`--no-convert`).
    pub converter: Option<Converter>,
}

/// Cleaning statistics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    /// `<page>` blocks read.
    pub pages_read: usize,
    /// Pages skipped: non-article namespace, redirect, disambiguation,
    /// or no text.
    pub pages_skipped: usize,
    /// Sentences emitted.
    pub sentences: usize,
    /// Raw bytes written.
    pub bytes_written: u64,
}

/// Redirect markers a page may start with.
const REDIRECT_PREFIXES: &[&str] = &["#REDIRECT", "#redirect", "#重定向", "#重新導向"];

/// Disambiguation templates that mark a page as non-prose.
const DISAMBIG_MARKERS: &[&str] = &["{{disambig", "{{消歧义", "{{消歧義", "{{dab}}", "{{dab "];

/// Whether a page is prose an n-gram trainer wants.
fn is_trainable(page: &Page) -> bool {
    if page.ns != 0 {
        return false;
    }
    // List articles are tables end to end; the stripper's table handling
    // is not the point of a training corpus.
    if page.title.contains("列表") {
        return false;
    }
    let Some(text) = page.text.as_deref() else {
        return false;
    };
    let trimmed = text.trim_start();
    if REDIRECT_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return false;
    }
    if DISAMBIG_MARKERS.iter().any(|marker| text.contains(marker)) {
        return false;
    }
    true
}

/// Cleans a dump: extracts wikitext, strips markup, converts, and
/// sentence-splits into the writer.
///
/// # Errors
///
/// Returns [`CorpusError`] on malformed XML or a failed conversion.
pub fn clean<R: Read, W: Write>(
    input: R,
    output: W,
    options: &Options,
) -> Result<Stats, CorpusError> {
    let mut writer = BufWriter::new(output);
    let mut stats = Stats::default();
    let mut dump = XmlDump::new(input);

    while let Some(page) = dump.next_page()? {
        stats.pages_read += 1;
        if !is_trainable(&page) {
            stats.pages_skipped += 1;
            continue;
        }
        let text = page.text.unwrap_or_default();
        // Magic page-name templates resolve to the page title so the
        // lead sentence stays complete after template stripping.
        let substituted = text
            .replace("{{PAGENAMEBASE}}", &page.title)
            .replace("{{PAGENAME}}", &page.title);
        let stripped = strip_wikitext(&substituted);
        let converted = match &options.converter {
            Some(converter) => converter.convert(&stripped)?,
            None => stripped,
        };
        for sentence in split_sentences(&converted) {
            writer.write_all(sentence.as_bytes())?;
            writer.write_all(b"\n")?;
            stats.sentences += 1;
            stats.bytes_written += sentence.len() as u64 + 1;
        }
        if options.max_pages.is_some_and(|max| stats.pages_read >= max) {
            break;
        }
    }
    writer.flush()?;
    Ok(stats)
}
