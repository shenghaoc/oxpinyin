//! System punctuation table: phrase token → predicted punctuation strings.
//!
//! Produced by `oxpinyin-migrate export-punct` from `punct.table`
//! (`docs/findings/prediction-punct.md`). Keys are `phrase_token_t` little
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures")
            .join("w3")
    }

    #[test]
    fn fixture_has_hao_and_de_in_table_order() {
        let table = PunctTable::open(&fixtures_dir().join("punct.redb")).unwrap();
        assert!(
            table.token_count() >= 3,
            "Option A export keeps the 好/中/国 tokens used by the punct differential"
        );
        assert_eq!(
            table.punctuations(16_779_429),
            ["，".to_owned(), "。".to_owned()]
        );
        assert_eq!(
            table.punctuations(16_778_715),
            [
                "，".to_owned(),
                "。".to_owned(),
                "“".to_owned(),
                "、".to_owned(),
                "；".to_owned()
            ]
        );
        assert!(table.punctuations(0).is_empty());
    }

    #[test]
    fn missing_file_is_empty() {
        let table = PunctTable::open_optional(Path::new("/no/such/punct.redb"));
        assert!(table.is_empty());
    }

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
}
