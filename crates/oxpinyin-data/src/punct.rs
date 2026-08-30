//! System punctuation table: phrase token → predicted punctuation strings.
//!
//! Committed under `fixtures/w3/` (frozen; no longer regenerated in-tree)
//! per `docs/findings/prediction-punct.md`.  Keys are `phrase_token_t` little
//! endian; values are NUL-terminated UTF-8 punctuation strings in the
//! table-file (decreasing-frequency) order.

use std::collections::BTreeMap;
use std::path::Path;

use crate::dict::DictError;
use crate::table;

/// Predicted punctuation lookup, keyed by the preceding phrase token.
pub struct PunctTable {
    by_token: BTreeMap<u32, Vec<String>>,
}

impl PunctTable {
    /// An empty table: every lookup returns no punctuation.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            by_token: BTreeMap::new(),
        }
    }

    /// Opens an Option A `punct.redb`.
    pub fn open(path: &Path) -> Result<Self, DictError> {
        let mut by_token = BTreeMap::new();
        table::for_each_row(path, |key, value| {
            if key.len() != 4 {
                return Err(DictError::Parse(format!(
                    "punct key length {} is not 4",
                    key.len()
                )));
            }
            let token = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
            let puncts = decode_puncts(value)?;
            if !puncts.is_empty() {
                by_token.insert(token, puncts);
            }
            Ok::<(), DictError>(())
        })?;
        Ok(Self { by_token })
    }

    /// Opens `path` when it is a readable Option A table; otherwise empty.
    ///
    /// Missing files and the raw HashDBM convert of `punct.bin` both become
    /// empty. Upstream `pinyin_init` ignores a failed `PunctTable::attach`.
    #[must_use]
    pub fn open_optional(path: &Path) -> Self {
        if !path.is_file() {
            return Self::empty();
        }
        Self::open(path).unwrap_or_else(|_| Self::empty())
    }

    /// Builds a table from decoded `(token, puncts)` rows, in table
    /// order; rows with no punctuation are dropped, like [`Self::open`]
    /// skips them.
    pub(crate) fn from_rows(rows: Vec<(u32, Vec<String>)>) -> Self {
        let mut by_token = BTreeMap::new();
        for (token, puncts) in rows {
            if !puncts.is_empty() {
                by_token.insert(token, puncts);
            }
        }
        Self { by_token }
    }

    /// Punctuation strings stored for `token`, in table order.
    #[must_use]
    pub fn punctuations(&self, token: u32) -> &[String] {
        self.by_token.get(&token).map_or(&[], Vec::as_slice)
    }

    /// Number of tokens that have at least one punctuation.
    #[must_use]
    pub fn token_count(&self) -> usize {
        self.by_token.len()
    }

    /// `true` when no token has punctuation.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_token.is_empty()
    }
}

fn decode_puncts(value: &[u8]) -> Result<Vec<String>, DictError> {
    if value.is_empty() || value.last() != Some(&0) {
        return Err(DictError::Parse(
            "punct value is not NUL-terminated".to_owned(),
        ));
    }

    let mut out = Vec::new();
    for chunk in value[..value.len() - 1].split(|byte| *byte == 0) {
        if chunk.is_empty() {
            return Err(DictError::Parse(
                "punct value contains an empty field".to_owned(),
            ));
        }
        let text = std::str::from_utf8(chunk)
            .map_err(|_| DictError::Parse("punct value is not UTF-8".to_owned()))?;
        out.push(text.to_owned());
    }
    Ok(out)
}

/// Decodes one compat `punct.bin` value — a pin `PunctTableEntry`
/// chunk, exactly as `punct_table_kyotodb.cpp`'s `store_entry` writes
/// it: NUL-terminated UCS4 strings concatenated to the end of the
/// blob. The pin's "escape" is the UTF-8→UCS4 conversion itself
/// (`punct_table.cpp` `escape()`: `g_utf8_to_ucs4` plus one NUL
/// `ucs4_t`; `unescape()` walks back the other way), so there are no
/// escape characters to undo — each word is a raw `ucs4_t`
/// (`guint32`), native-endian in the pin and read little-endian here
/// like every other u32 the compat loader reads (every target libpinyin
/// ships on is little-endian). An empty blob is a token with no
/// punctuations.
pub(crate) fn decode_compat_puncts(value: &[u8]) -> Result<Vec<String>, DictError> {
    if !value.len().is_multiple_of(4) {
        return Err(DictError::Parse(
            "punct chunk is not a whole number of ucs4_t words".to_owned(),
        ));
    }

    let mut out = Vec::new();
    let mut current = String::new();
    for word in value.chunks_exact(4) {
        let cp = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        if cp == 0 {
            if current.is_empty() {
                return Err(DictError::Parse(
                    "punct chunk contains an empty field".to_owned(),
                ));
            }
            out.push(std::mem::take(&mut current));
        } else {
            let ch = char::from_u32(cp).ok_or_else(|| {
                DictError::Parse(format!("punct chunk holds invalid code point {cp:#x}"))
            })?;
            current.push(ch);
        }
    }
    if !current.is_empty() {
        return Err(DictError::Parse(
            "punct chunk ends with an unterminated string".to_owned(),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_rejects_empty_unterminated_and_empty_fields() {
        assert!(decode_puncts(b"").is_err());
        assert!(decode_puncts("，".as_bytes()).is_err());
        assert!(decode_puncts(b"\0").is_err());
        assert!(decode_puncts(b"\xef\xbc\x8c\x00\x00\xe3\x80\x82\x00").is_err());
        assert_eq!(
            decode_puncts(b"\xef\xbc\x8c\x00\xe3\x80\x82\x00").unwrap(),
            ["，", "。"]
        );
    }

    /// The ucs4_t words of `text` plus the terminating NUL, the way
    /// `punct_table.cpp`'s escape() lays one string into the chunk.
    fn ucs4_string(text: &str) -> Vec<u8> {
        let mut out: Vec<u8> = text
            .chars()
            .flat_map(|c| (c as u32).to_le_bytes())
            .collect();
        out.extend_from_slice(&0_u32.to_le_bytes());
        out
    }

    #[test]
    fn compat_decode_reads_ucs4_chunks() {
        // Empty chunk: the token exists but has no punctuations.
        assert_eq!(decode_compat_puncts(&[]).unwrap(), Vec::<String>::new());
        // One string; two strings concatenated.
        assert_eq!(decode_compat_puncts(&ucs4_string("，")).unwrap(), ["，"]);
        let mut two = ucs4_string("。");
        two.extend(ucs4_string("，"));
        assert_eq!(
            decode_compat_puncts(&two).unwrap(),
            ["。".to_owned(), "，".to_owned()]
        );
        // A four-byte-ASCII punctuation survives too.
        assert_eq!(decode_compat_puncts(&ucs4_string("!")).unwrap(), ["!"]);
    }

    #[test]
    fn compat_decode_rejects_malformed_chunks() {
        // Not a whole number of ucs4_t words.
        assert!(decode_compat_puncts(&[0x0C]).is_err());
        assert!(decode_compat_puncts(&[0x0C, 0xFF, 0x00, 0x00, 0x00]).is_err());
        // An empty field: a NUL word with no string before it.
        assert!(decode_compat_puncts(&0_u32.to_le_bytes()).is_err());
        // Unterminated final string.
        let mut cut = ucs4_string("，");
        cut.truncate(cut.len() - 4);
        assert!(decode_compat_puncts(&cut).is_err());
        // A surrogate is not a code point.
        let lone_surrogate: Vec<u8> = [0xD800_u32, 0]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        assert!(decode_compat_puncts(&lone_surrogate).is_err());
    }

    #[test]
    fn from_rows_skips_empty_rows_and_keeps_order() {
        let table = PunctTable::from_rows(vec![
            (7, vec!["，".to_owned()]),
            (9, Vec::new()),
            (42, vec!["。".to_owned(), "!".to_owned()]),
        ]);
        assert_eq!(table.token_count(), 2);
        assert_eq!(table.punctuations(7), ["，"]);
        assert_eq!(table.punctuations(9), Vec::<String>::new());
        assert_eq!(table.punctuations(42), ["。", "!"]);
    }
}
