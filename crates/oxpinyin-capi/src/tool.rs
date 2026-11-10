//! Safe ownership adapters for the pin's nonshipping C-API test drivers.
//! Every operation calls the existing C facade, including its failure paths.

use std::ffi::CStr;
use std::ptr;

use crate::{GChar, PinyinContext, PinyinInstance};

/// A GLib-owned string returned by the C API, including its null result.
pub struct Text(*mut GChar);

impl Text {
    /// Borrow the C API's NUL-terminated result until this owner is dropped.
    #[must_use]
    pub fn as_c_str(&self) -> Option<&CStr> {
        if self.0.is_null() {
            return None;
        }
        // SAFETY: only the C API's string-out functions construct Text; their
        // nonnull results are allocated NUL-terminated strings owned by us.
        Some(unsafe { CStr::from_ptr(self.0) })
    }
}

impl Drop for Text {
    fn drop(&mut self) {
        // SAFETY: the private pointer came from a GLib-owning C API result,
        // and is freed once. g_free accepts null.
        unsafe { glib_sys::g_free(self.0.cast()) };
    }
}

/// The context and instance lifetime used by tests/test_{pinyin,phrase,chewing}.
pub struct Driver {
    context: *mut PinyinContext,
    instance: *mut PinyinInstance,
}

impl Driver {
    /// Initialize exactly the two supplied directories, set optional options,
    /// then allocate the instance (test_pinyin.cpp:35-43 and its peers).
    #[must_use]
    pub fn new(system: &CStr, user: &CStr, options: Option<u32>) -> Self {
        let context = crate::pinyin_init(system.as_ptr(), user.as_ptr());
        if let Some(options) = options {
            crate::config::pinyin_set_options(context, options);
        }
        let instance = crate::instance::pinyin_alloc_instance(context);
        Self { context, instance }
    }

    /// Parse the supplied full-pinyin C string.
    pub fn parse_full(&mut self, input: &CStr) -> usize {
        crate::parse::pinyin_parse_more_full_pinyins(self.instance, input.as_ptr())
    }

    /// Parse the supplied chewing C string.
    pub fn parse_chewing(&mut self, input: &CStr) -> usize {
        crate::parse::pinyin_parse_more_chewings(self.instance, input.as_ptr())
    }

    /// Guess a sentence, with the optional prefix used by test_pinyin.
    pub fn guess(&mut self, prefix: Option<&CStr>) {
        if let Some(prefix) = prefix {
            crate::sentence::pinyin_guess_sentence_with_prefix(self.instance, prefix.as_ptr());
        } else {
            crate::sentence::pinyin_guess_sentence(self.instance);
        }
    }

    /// Build the candidate list with the caller's offset and sorting word.
    pub fn guess_candidates(&mut self, offset: usize, sort: u32) {
        crate::sentence::pinyin_guess_candidates(self.instance, offset, sort);
    }

    /// The allocated full-pinyin auxiliary text at a byte cursor.
    pub fn full_auxiliary(&mut self, cursor: usize) -> Text {
        let mut text = ptr::null_mut();
        crate::text::pinyin_get_full_pinyin_auxiliary_text(self.instance, cursor, &mut text);
        Text(text)
    }

    /// The allocated chewing auxiliary text at a byte cursor.
    pub fn chewing_auxiliary(&mut self, cursor: usize) -> Text {
        let mut text = ptr::null_mut();
        crate::text::pinyin_get_chewing_auxiliary_text(self.instance, cursor, &mut text);
        Text(text)
    }

    /// The allocated first sentence.
    pub fn sentence(&mut self) -> Text {
        let mut text = ptr::null_mut();
        crate::sentence::pinyin_get_sentence(self.instance, 0, &mut text);
        Text(text)
    }

    /// The candidate count, initialized to zero as in test_pinyin.cpp:77.
    pub fn candidate_count(&mut self) -> u32 {
        let mut count = 0;
        crate::candidates::pinyin_get_n_candidate(self.instance, &mut count);
        count
    }

    /// Borrow one candidate's string until the next mutable driver operation.
    pub fn candidate(&mut self, index: u32) -> Option<&CStr> {
        let mut candidate = ptr::null_mut();
        let mut text = ptr::null();
        crate::candidates::pinyin_get_candidate(self.instance, index, &mut candidate);
        crate::candidates::pinyin_get_candidate_string(self.instance, candidate, &mut text);
        if text.is_null() {
            return None;
        }
        // SAFETY: the C facade owns this NUL-terminated candidate string.
        // The returned borrow prevents changing or dropping this instance.
        Some(unsafe { CStr::from_ptr(text) })
    }

    /// Segment a phrase and answer the C API's phrase-result length.
    pub fn segment(&mut self, input: &CStr) -> u32 {
        crate::phrase::pinyin_phrase_segment(self.instance, input.as_ptr());
        let mut count = 0;
        crate::phrase::pinyin_get_n_phrase(self.instance, &mut count);
        count
    }

    /// The token at a phrase-result index; null-token initialization is pinned.
    pub fn phrase_token(&mut self, index: u32) -> u32 {
        let mut token = 0;
        crate::phrase::pinyin_get_phrase_token(self.instance, index, &mut token);
        token
    }

    /// Allocate the text for a phrase token, without requesting its length.
    pub fn phrase(&mut self, token: u32) -> Text {
        let mut text = ptr::null_mut();
        crate::dict::pinyin_token_get_phrase(self.instance, token, ptr::null_mut(), &mut text);
        Text(text)
    }

    /// Train sentence zero and reset, in the drivers' order.
    pub fn train_reset(&mut self) {
        crate::candidates::pinyin_train(self.instance, 0);
        crate::instance::pinyin_reset(self.instance);
    }

    /// Save the same context while its instance remains live.
    pub fn save(&mut self) {
        crate::pinyin_save(self.context);
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        // test_pinyin.cpp:95-99; test_phrase.cpp:66-70;
        // test_chewing.cpp:66-70. These calls preserve the pin's order.
        crate::instance::pinyin_free_instance(self.instance);
        crate::config::pinyin_mask_out(self.context, 0, 0);
        crate::pinyin_save(self.context);
        crate::pinyin_fini(self.context);
    }
}
