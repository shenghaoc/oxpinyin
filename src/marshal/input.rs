//! Safe input conversions shared by the two exposed C boundaries.
//!
//! Pointer borrowing stays in entry points and helpers with unsafe contracts.
//! These functions consume a nullable borrowed C string and return native
//! Rust values. Scalar-only
//! conversion is appropriate for parser spellings and test helpers; a pin
//! `g_utf8_to_ucs4` site instead needs [`utf8_to_ucs4`].

use std::ffi::CStr;

use crate::PhraseText;

/// Decode the canonical historical encodings accepted by `g_utf8_to_ucs4`.
///
/// NULL or rejected bytes produce `None`. Empty input produces an empty
/// phrase; callers decide whether that is a refusal or a no-input operation.
#[must_use]
pub fn utf8_to_ucs4(input: Option<&CStr>) -> Option<PhraseText> {
    let input = input?;
    if let Ok(text) = input.to_str() {
        return Some(PhraseText::from(text));
    }
    PhraseText::from_words(crate::marshal::ucs4::decode(input.to_bytes()).ok()?)
}

/// Read a scalar spelling, rejecting NULL and invalid Unicode UTF-8.
///
/// This is not the pin's UCS-4 conversion: use [`utf8_to_ucs4`] for text
/// that must retain non-scalar words.
#[must_use]
pub fn scalar_text(input: Option<&CStr>) -> Option<String> {
    input?.to_str().ok().map(str::to_owned)
}

/// Returns the valid UTF-8 prefix of a nullable C string. A keyboard parser can reject
/// bytes within that prefix. Use [`direct_zhuyin_reading`] for segmented Zhuyin text.
#[must_use]
pub fn parser_prefix(input: Option<&CStr>) -> String {
    let Some(input) = input else {
        return String::new();
    };
    let bytes = input.to_bytes();
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(error) => std::str::from_utf8(&bytes[..error.valid_up_to()])
            .unwrap_or_default()
            .to_owned(),
    }
}

// ZhuyinDirectParser2::parse, storage/zhuyin_parser2.cpp:914–959.
/// Prepares input for [`crate::parse_zhuyin_direct`]. Valid UTF-8 passes through in full.
/// On malformed UTF-8, the result ends just after the last ASCII space or apostrophe
/// before the bad byte, or is empty when there is no such separator.
#[must_use]
pub fn direct_zhuyin_reading(input: Option<&CStr>) -> String {
    let Some(input) = input else {
        return String::new();
    };
    let bytes = input.to_bytes();
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(error) => {
            let prefix = &bytes[..error.valid_up_to()];
            let end = prefix
                .iter()
                .rposition(|byte| matches!(byte, b' ' | b'\''))
                .map_or(0, |index| index + 1);
            std::str::from_utf8(&prefix[..end])
                .unwrap_or_default()
                .to_owned()
        }
    }
}
