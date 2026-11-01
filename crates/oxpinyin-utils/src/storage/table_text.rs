//! `src/storage/table_text_parser.h` at libpinyin 074a2219: the four-field
//! `.table` and `punct.table` records.
//!
//! `fgets` reads into a 4096-byte array, so a longer line is split at the
//! buffer boundary and each part is parsed as its own record.
//! `table_text_split_line` skips `" \t"`, ends a field at `" \t\r\n"`, and
//! requires four fields; a short or blank line is skipped, not an error.
//! `g_strlcpy` keeps at most 255 bytes and
//! `g_ascii_strtoull(field, &end, 10)` must consume the whole field, set no
//! errno, and not exceed the target's maximum. A failure skips the line.

use std::ffi::CString;

use oxpinyin_input::tool::storage::{TableFile, ascii_strtoull, strlcpy_256};

/// One parsed `.table` record.
pub(crate) struct Record {
    /// The pinyin spelling column.
    pub pinyin: CString,
    /// The phrase text column.
    pub phrase: CString,
    /// The token column.
    pub token: u32,
    /// The frequency column.
    pub freq: usize,
}

/// One parsed `punct.table` record.
pub(crate) struct PunctRecord {
    /// The token column.
    pub token: u32,
    /// The punctuation column.
    pub punctuation: CString,
}

/// `table_text_split_line` (`:34-52`): four fields of `line` (which ends in
/// its NUL), or `None`.
fn split_line(line: &[u8]) -> Option<[&[u8]; 4]> {
    let mut fields: [&[u8]; 4] = [&[]; 4];
    let mut cur = 0_usize;
    for field in &mut fields {
        while line
            .get(cur)
            .is_some_and(|byte| matches!(*byte, b' ' | b'\t'))
        {
            cur += 1;
        }
        match line.get(cur) {
            Some(&b'\0' | &b'\r' | &b'\n') | None => return None,
            Some(_) => {}
        }
        let begin = cur;
        while let Some(byte) = line.get(cur) {
            if matches!(*byte, b' ' | b'\t' | b'\r' | b'\n' | b'\0') {
                break;
            }
            cur += 1;
        }
        *field = line.get(begin..cur)?;
        if line.get(cur).copied().unwrap_or(0) != 0 {
            cur += 1;
        }
    }
    Some(fields)
}

/// `table_text_copy_field` (`:71-73`): `Some` when `g_strlcpy` kept the
/// field whole.
fn copy_field(field: &[u8]) -> Option<CString> {
    let field = CString::new(field).ok()?;
    strlcpy_256(&field)
}

/// `table_text_parse_integer<Integer>` (`:54-69`) for `phrase_token_t`.
fn parse_token(field: &[u8]) -> Option<u32> {
    let field = CString::new(field).ok()?;
    let value = ascii_strtoull(&field)?;
    u32::try_from(value).ok()
}

/// `table_text_parse_integer<size_t>` (`:54-69`) for the count column.
fn parse_freq(field: &[u8]) -> Option<usize> {
    let field = CString::new(field).ok()?;
    let value = ascii_strtoull(&field)?;
    usize::try_from(value).ok()
}

/// `table_text_read_pinyin_record` (`:75-97`).
pub(crate) fn read_pinyin_record(file: &mut TableFile) -> Option<Record> {
    while let Some(line) = file.read_line() {
        let mut line = line;
        line.push(0);
        let Some(fields) = split_line(&line) else {
            continue;
        };
        let (Some(pinyin), Some(phrase), Some(token), Some(freq)) = (
            copy_field(fields[0]),
            copy_field(fields[1]),
            parse_token(fields[2]),
            parse_freq(fields[3]),
        ) else {
            continue;
        };
        return Some(Record {
            pinyin,
            phrase,
            token,
            freq,
        });
    }
    None
}

/// `table_text_read_punct_record` (`:99-121`).
pub(crate) fn read_punct_record(file: &mut TableFile) -> Option<PunctRecord> {
    while let Some(line) = file.read_line() {
        let mut line = line;
        line.push(0);
        let Some(fields) = split_line(&line) else {
            continue;
        };
        let (Some(token), Some(_phrase), Some(punctuation), Some(_freq)) = (
            parse_token(fields[0]),
            copy_field(fields[1]),
            copy_field(fields[2]),
            parse_freq(fields[3]),
        ) else {
            continue;
        };
        return Some(PunctRecord { token, punctuation });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::split_line;

    fn fields(line: &str) -> Option<Vec<String>> {
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(0);
        split_line(&bytes).map(|fields| {
            fields
                .iter()
                .map(|field| String::from_utf8(field.to_vec()).unwrap())
                .collect()
        })
    }

    #[test]
    fn four_fields_split_on_space_and_tab() {
        assert_eq!(
            fields("a\t\u{5440} 16777218 104\n"),
            Some(vec![
                "a".into(),
                "\u{5440}".into(),
                "16777218".into(),
                "104".into()
            ])
        );
    }

    #[test]
    fn short_blank_and_leading_separator_lines_are_skipped() {
        assert_eq!(fields(""), None);
        assert_eq!(fields("a b 1"), None);
        assert_eq!(fields(" \t \n"), None);
        assert_eq!(fields("\r\n"), None);
    }

    #[test]
    fn extra_fields_are_ignored() {
        assert_eq!(fields("a b 1 2 trailing fields here\n").unwrap().len(), 4);
    }
}
