//! Safe input conversions shared by the two exposed C boundaries.
//!
//! Pointer borrowing stays in entry points and helpers with unsafe contracts.
//! These functions consume a nullable borrowed C string and return native
//! Rust values. Scalar-only
//! conversion is appropriate for parser spellings and test helpers; a pin
//! `g_utf8_to_ucs4` site instead needs [`utf8_to_ucs4`].

use std::ffi::CStr;
use std::path::PathBuf;

use oxpinyin_core::PhraseText;

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
    PhraseText::from_words(crate::ucs4::decode(input.to_bytes()).ok()?)
}

/// Read a scalar spelling, rejecting NULL and invalid Unicode UTF-8.
///
/// This is not the pin's UCS-4 conversion: use [`utf8_to_ucs4`] for text
/// that must retain non-scalar words.
#[must_use]
pub fn scalar_text(input: Option<&CStr>) -> Option<String> {
    input?.to_str().ok().map(str::to_owned)
}

/// Preserve the prefix a raw-byte batch keyboard parser can consume.
///
/// Full/double pinyin and keyboard parsers stop before a byte outside their
/// alphabets. Do not use this for direct zhuyin import: that parser rejects
/// a complete space/apostrophe-delimited segment, not an arbitrary suffix.
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

/// Read a native filename using the shared boundary's byte-path conversion.
#[must_use]
pub fn filename(input: Option<&CStr>) -> Option<PathBuf> {
    Some(oxpinyin_input::path::path_from_bytes(input?.to_bytes()))
}

/// Define the nullable C-string borrow at a C ABI boundary.
#[macro_export]
macro_rules! borrow_c_string_helpers {
    () => {
        /// Borrow a nullable C string for one C call.
        ///
        /// # Safety
        ///
        /// Under the crate-level Strings rule, NULL is accepted. A non-NULL
        /// pointer must address a live NUL-terminated string, readable through
        /// its terminator in one allocation within `isize::MAX` bytes. It must
        /// stay valid and unchanged for all of `'a`. The borrow must not
        /// outlive the C call.
        pub(crate) unsafe fn borrow_c_string<'a>(
            pointer: *const ::std::os::raw::c_char,
        ) -> Option<&'a ::std::ffi::CStr> {
            if pointer.is_null() {
                None
            } else {
                // SAFETY: The Strings contract guarantees readable, unchanged
                // NUL-terminated storage for the returned borrow's lifetime.
                Some(unsafe { ::std::ffi::CStr::from_ptr(pointer) })
            }
        }
    };
}

/// Keep complete direct-zhuyin segments before the first rejected segment.
/// The pin splits on ASCII space/apostrophe before checking each spelling
/// (074a2219 storage/zhuyin_parser2.cpp:914-959).
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

#[cfg(all(test, windows))]
mod tests;
