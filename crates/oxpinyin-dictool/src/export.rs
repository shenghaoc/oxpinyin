//! `oxpinyin-dictool export`: user store → classic ibus-libpinyin text.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use oxpinyin_core::PhraseString;
use oxpinyin_facade::{ExportedBigramRow, ExportedRow};
use oxpinyin_user::USER_DICTIONARY;

use crate::context::UserImportContext;

/// A user-vocabulary export failure.
#[derive(Debug)]
pub enum ExportError {
    /// The user-store context could not be opened.
    Context(PathBuf),
    /// The phrase export iterator failed.
    PhraseSnapshot,
    /// The bigram export iterator failed.
    BigramSnapshot,
    /// The destination could not be written.
    Write(io::Error),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Context(path) => {
                write!(f, "cannot open user store under {}", path.display())
            }
            Self::PhraseSnapshot => write!(f, "phrase export iterator failed"),
            Self::BigramSnapshot => write!(
                f,
                "bigram export needs system tables (stored rows could not be rendered)"
            ),
            Self::Write(error) => write!(f, "cannot write export file: {error}"),
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Write(error) => Some(error),
            Self::Context(_) | Self::PhraseSnapshot | Self::BigramSnapshot => None,
        }
    }
}

/// Write one classic row: 2 fields when count is zero (`dictool-format.md`
/// §4), otherwise 3 fields. The phrase is written as the bytes the pin's
/// export hands out, which are UTF-8 unless a stored word is no scalar value;
/// a row with a NULL phrase has none to write ([`run`] refuses the export
/// before it writes one).
fn write_row<W: Write>(
    out: &mut W,
    phrase: &PhraseString,
    pinyin: &str,
    count: i64,
) -> io::Result<()> {
    out.write_all(phrase.as_bytes().unwrap_or_default())?;
    if count == 0 {
        writeln!(out, " {pinyin}")
    } else {
        writeln!(out, " {pinyin} {count}")
    }
}

/// Write the classic file body: phrase rows then bigram rows.
fn write_rows<W: Write>(
    out: &mut W,
    phrases: &[ExportedRow],
    bigrams: &[ExportedBigramRow],
) -> io::Result<()> {
    for ExportedRow {
        string,
        pinyin,
        count,
    } in phrases
    {
        write_row(
            out,
            string,
            pinyin,
            i64::try_from(*count).unwrap_or(i64::MAX),
        )?;
    }
    for ExportedBigramRow {
        phrase,
        pinyin,
        count,
    } in bigrams
    {
        write_row(out, phrase, pinyin, *count)?;
    }
    Ok(())
}

/// Export `user_dir`'s store in the classic format.
///
/// `output` names the destination file, created/truncated; `None` writes to
/// stdout. Ordering is the frontend's: every phrase row first (token order,
/// then pronunciation order), then every rendered bigram row.
pub fn run(user_dir: &Path, output: Option<&Path>) -> Result<(), ExportError> {
    let context = UserImportContext::open(user_dir)
        .ok_or_else(|| ExportError::Context(user_dir.to_path_buf()))?;
    let phrases = context
        .core()
        .export_phrases(u32::from(USER_DICTIONARY))
        .ok_or(ExportError::PhraseSnapshot)?;
    // The classic file has no text for a NULL phrase: the export of this
    // sub-index does not complete.
    if phrases.iter().any(|row| row.string.is_null()) {
        return Err(ExportError::PhraseSnapshot);
    }
    // The §9 bigram snapshot refuses rather than skip rows whose rendering
    // needs system tables this user-store-only context does not carry.
    if !context.core().can_render_export_bigrams() {
        return Err(ExportError::BigramSnapshot);
    }
    let bigrams = context
        .core()
        .export_bigram_rows()
        .ok_or(ExportError::BigramSnapshot)?;
    if bigrams.iter().any(|row| row.phrase.is_null()) {
        return Err(ExportError::BigramSnapshot);
    }

    if let Some(path) = output {
        let mut file = fs::File::create(path).map_err(ExportError::Write)?;
        write_rows(&mut file, &phrases, &bigrams).map_err(ExportError::Write)?;
    } else {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        write_rows(&mut out, &phrases, &bigrams).map_err(ExportError::Write)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_count_rows_are_two_fields_like_the_frontend() {
        let mut out = Vec::new();
        write_rows(
            &mut out,
            &[ExportedRow {
                string: "词".into(),
                pinyin: "ci".to_owned(),
                count: 0,
            }],
            &[ExportedBigramRow {
                phrase: "你好".into(),
                pinyin: "ni'hao".to_owned(),
                count: 138,
            }],
        )
        .expect("write rows");
        assert_eq!(
            out,
            b"\xe8\xaf\x8d ci\n\xe4\xbd\xa0\xe5\xa5\xbd ni'hao 138\n"
        );
    }
}
