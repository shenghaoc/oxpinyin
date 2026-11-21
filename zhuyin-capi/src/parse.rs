//! Parsing symbols: full pinyin, chewing (bopomofo), and the shared
//! batch-parse shell.

use std::os::raw::c_char;

use crate::ffi::borrow_or_return;
use crate::state::{instance_mut, instance_ref};
use crate::types::{GChar, ZhuyinInstance};

fn parse_chewing_more(inst: &mut crate::state::CapiInstanceMut<'_>, text: &str) -> usize {
    // The pin preserves candidate rows and strings across parsing; only a
    // candidate guess, reset or instance destruction invalidates them.
    inst.core
        .parse_chewing_more(text, oxpinyin::facade::ChewingCorrections::FromOptions)
}

fn parse_full_more(inst: &mut crate::state::CapiInstanceMut<'_>, text: &str) -> usize {
    inst.core.parse_full_more_zhuyin(text)
}

/// Parse multiple full pinyins.
/// # C signature
/// ```c
/// size_t zhuyin_parse_more_full_pinyins(zhuyin_instance_t * instance,
///                                       const char * pinyins);
/// ```
///
/// Parse with the current scheme and replace the instance’s parsed length and key matrix.
/// Returns the number of input bytes consumed, which may cover only a prefix. Returns zero for a
/// null instance or when no bytes were consumed.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `pinyins` is a
/// string. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_parse_more_full_pinyins(
    instance: *mut ZhuyinInstance,
    pinyins: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }

    let text = oxpinyin::marshal::parser_prefix(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(pinyins) },
    );
    // SAFETY: Handles guarantees a live instance; the NULL check precedes
    // conversion. Serial calls and the absence of retained views permit this borrow.
    let mut inst = unsafe { instance_mut(instance) };
    parse_full_more(&mut inst, &text)
}

/// Parse multiple chewing (bopomofo) inputs.
///
/// # C signature
/// ```c
/// size_t zhuyin_parse_more_chewings(zhuyin_instance_t * instance,
///                                   const char * chewings);
/// ```
///
/// Parse with the current scheme and replace the instance’s parsed length and key matrix.
/// Returns the number of input bytes consumed, which may cover only a prefix. Returns zero for a
/// null instance or when no bytes were consumed.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `chewings` is a
/// string. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_parse_more_chewings(
    instance: *mut ZhuyinInstance,
    chewings: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }

    let text = oxpinyin::marshal::parser_prefix(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(chewings) },
    );
    // SAFETY: Handles guarantees a live instance; the NULL check precedes
    // conversion. Serial calls and the absence of retained views permit this borrow.
    let mut inst = unsafe { instance_mut(instance) };
    parse_chewing_more(&mut inst, &text)
}

/// Get the parsed length of the input.
///
/// # C signature
/// ```c
/// size_t zhuyin_get_parsed_input_length(zhuyin_instance_t * instance);
/// ```
///
/// Returns the input byte count from the last batch parse, or zero before parsing, after reset,
/// or for a null instance.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_get_parsed_input_length(instance: *mut ZhuyinInstance) -> usize {
    let inst = borrow_or_return!(instance, instance_ref, 0);
    inst.core.parsed_input_length()
}

/// Check whether an input key is in the current chewing keyboard scheme.
///
/// # C signature
/// ```c
/// bool zhuyin_in_chewing_keyboard(zhuyin_instance_t * instance,
///                                 const char key,
///                                 gchar *** symbols);
/// ```
///
/// Returns whether the selected keyboard maps this key. A null output checks membership only. A
/// hit writes a null-terminated symbol vector that the caller frees with `g_strfreev`. A miss
/// writes null. A null instance returns false without writing.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `symbols` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_in_chewing_keyboard(
    instance: *mut ZhuyinInstance,
    key: std::os::raw::c_char,
    symbols: *mut *mut *mut GChar,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    // `c_char` is `i8` on some targets and `u8` on others (aarch64
    // Linux among them); `as u8` is a lossless reinterpret on both,
    // and the cast is not "unnecessary" on the targets where it is
    // `i8`.
    #[allow(
        clippy::unnecessary_cast,
        clippy::cast_sign_loss,
        reason = "c_char is u8 on aarch64 Linux and i8 elsewhere, so a                   TryFrom conversion would fail to compile on one of them;                   the i8-to-u8 wrap is byte identity, and wrapped negatives                   (>= 0x80) match no keyboard table entry"
    )]
    let mapped = inst.core.in_keyboard(key as u8);
    if symbols.is_null() {
        return !mapped.is_empty();
    }
    // `in_chewing_scheme` (`zhuyin_parser2.cpp:301-332`, `:496-544`,
    // `:798-851`): the symbols NULL, then the `GPtrArray` built with the
    // pin's GLib calls, handed out for `g_strfreev`; NULL with `false`
    // where no symbol matched.
    let vector = crate::ffi::symbol_vector(&mapped);
    // SAFETY: Null-checked above.
    unsafe {
        *symbols = vector;
    }
    !mapped.is_empty()
}
