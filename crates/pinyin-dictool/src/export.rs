//! `pinyin-dictool export`: user redb → classic ibus-libpinyin text.
//!
//! Mirrors `LibPinyinBackEnd::exportPinyinDictionary`
//! (`PYLibPinyin.cc:336-353`) with both Export-button flags on: phrase rows
//! first (from `pinyin_begin_get_phrases`), then the rendered bigram rows
//! (from `pinyin_begin_get_bigram_phrases`). The row grammar is the same
//! space-separated classic interchange as import.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use pinyin_capi::{
    ExportedBigramRow, ExportedPhrase, PinyinContext, close_user_import_context,
    open_user_import_context, user_bigram_rows, user_phrase_rows,
};

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
            Self::BigramSnapshot => write!(f, "bigram export iterator failed"),
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

/// Write one classic row: 2 fields when count is zero (upstream's
/// `count == -1` output means "no count recorded"), otherwise 3 fields.
fn write_row<W: Write>(out: &mut W, phrase: &str, pinyin: &str, count: i64) -> io::Result<()> {
    if count == 0 {
        writeln!(out, "{phrase} {pinyin}")
    } else {
        writeln!(out, "{phrase} {pinyin} {count}")
    }
}

/// Write the classic file body: phrase rows then bigram rows.
fn write_rows<W: Write>(
    out: &mut W,
    phrases: &[ExportedPhrase],
    bigrams: &[ExportedBigramRow],
) -> io::Result<()> {
    for ExportedPhrase {
        text,
        pinyin,
        count,
    } in phrases
    {
        write_row(out, text, pinyin, i64::try_from(*count).unwrap_or(i64::MAX))?;
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
    let context: *mut PinyinContext = open_user_import_context(user_dir);
    if context.is_null() {
        return Err(ExportError::Context(user_dir.to_path_buf()));
    }
    let phrases = user_phrase_rows(context).ok_or(ExportError::PhraseSnapshot)?;
    let bigrams = user_bigram_rows(context).ok_or(ExportError::BigramSnapshot)?;
    close_user_import_context(context);

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
            &[ExportedPhrase {
                text: "词".to_owned(),
                pinyin: "ci".to_owned(),
                count: 0,
            }],
            &[ExportedBigramRow {
                phrase: "你好".to_owned(),
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
