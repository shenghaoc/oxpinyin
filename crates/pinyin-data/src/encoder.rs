//! SyllableKey → TableKey encoder (blocked).

use core::fmt;

use pinyin_core::SyllableKey;

use crate::types::TableKey;

/// Why a syllable could not be encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncoderError {
    /// Encoding is blocked pending oracle FFI.
    Blocked {
        /// The syllable that was requested.
        syllable: SyllableKey,
    },
}

impl fmt::Display for EncoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blocked { syllable } => write!(
                f,
                "blocked:syllable-encoder: SyllableKey {} (id {}) \
                 has no TableKey mapping without oracle FFI",
                syllable.text(),
                syllable.index()
            ),
        }
    }
}

impl std::error::Error for EncoderError {}

/// Translate a decoder syllable id to the 6-byte binary key the redb tables use.
pub fn encode(syllable: SyllableKey) -> Result<TableKey, EncoderError> {
    Err(EncoderError::Blocked { syllable })
}
