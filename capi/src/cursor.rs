//! Pinyin key access and cursor/offset navigation.
//!
//! The cursor → lookup-offset normalization (D1) and the word-level
//! left/right moves (D2) port the pin's matrix laws
//! (`pinyin.cpp:3008-3095` at the pin) over the engine's positional
//! data — `oxpinyin::engine::lookup_offset_over_spans` and the
//! `*_word_offset_over_spans` pair. Where the pin's `_check_offset` aborts
//! (`pinyin.cpp:2175`), the left and right moves abort too (register row 14);
//! `pinyin_get_pinyin_offset` aborts only over an empty matrix with a parsed
//! length above 0 and a cursor of 1 or more; a cursor of 0, or a parsed length
//! of 0, answers `true` with offset 0. `pinyin_get_right_pinyin_offset` aborts
//! on an empty matrix, as row 66 records.
//!
//! Parse-mode dispatch mirrors `CapiInstance::validate_lookup_offset`:
//! plain full pinyin runs the law over the session's own buffer; LUOMA /
//! `SECONDARY_ZHUYIN` run it over the stored original input with the index
//! parse's key spans (the pinned index parse consumes `'` as the same
//! separator); double pinyin and the zhuyin keyboards hold no zero-key
//! columns, so the law steps their parse's key spans only.

use std::ptr;
use std::sync::atomic::{AtomicU16, Ordering};

use crate::ffi::{backing_ref, borrow_or_return, key_text, strdup, write_if_nonnull};
use crate::state::instance_ref;
use crate::types::{ChewingKey, ChewingKeyRest, GChar, PinyinInstance};

/// The slot `pinyin_get_pinyin_key` hands out: the pin's function-local
/// `static ChewingKey key` (`pinyin.cpp:2936`), one process-wide word that
/// every instance overwrites and that outlives every instance. An atomic
/// word keeps the Rust side free of a data race; the C side reads it
/// through the pointer, as it reads the pin's static.
static KEY_SLOT: AtomicU16 = AtomicU16::new(0);

/// The `ChewingKeyRest` twin of [`KEY_SLOT`] (`static ChewingKeyRest
/// key_rest`, `pinyin.cpp:2960`): `m_raw_begin` then `m_raw_end`.
#[repr(C)]
struct KeyRestSlot {
    begin: AtomicU16,
    end: AtomicU16,
}

const _: () = {
    assert!(size_of::<KeyRestSlot>() == size_of::<ChewingKeyRest>());
    assert!(align_of::<KeyRestSlot>() == align_of::<ChewingKeyRest>());
    assert!(size_of::<AtomicU16>() == size_of::<ChewingKey>());
};

static KEY_REST_SLOT: KeyRestSlot = KeyRestSlot {
    begin: AtomicU16::new(0),
    end: AtomicU16::new(0),
};

/// Applies the empty-matrix failure at the C key-accessor boundary.
fn check_key_matrix(inst: &crate::state::CapiInstanceRead<'_>, offset: usize) {
    if offset != usize::MAX && !inst.core.mode_has_keys() {
        std::process::abort();
    }
}

/// Get the pinyin key rest at an offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_key_rest(pinyin_instance_t * instance,
///                                 size_t offset,
///                                 ChewingKeyRest ** key_rest);
/// ```
///
/// Success writes a borrowed pointer to a static slot. All instances share this getter’s slot.
/// Its contents change on the next successful call, and its storage outlives the instance. The
/// caller does not free it. An ordinary miss writes null. A null instance returns false without
/// writing.
///
/// # Aborts
///
/// On an empty matrix, an offset other than `SIZE_MAX` aborts. `SIZE_MAX` returns false.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `key_rest` is an
/// out-parameter. The serial-calls rule applies. The pointer written to `*key_rest` addresses a
/// process-wide slot, which the next call of this function overwrites.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_pinyin_key_rest(
    instance: *mut PinyinInstance,
    offset: usize,
    key_rest: *mut *mut ChewingKeyRest,
) -> bool {
    if instance.is_null() {
        return false;
    }
    write_if_nonnull!(key_rest, ptr::null_mut());

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`; this call only reads the instance and writes
    // only the process-wide key slot.
    let inst = unsafe { instance_ref(instance) };
    check_key_matrix(&inst, offset);
    let Ok(Some(found)) = inst.core.abi_key_at(offset, false) else {
        return false;
    };
    // `m_raw_begin`/`m_raw_end` are `guint16` (`chewing_key.h:102-103`),
    // and the pin's `int` positions narrow into them mod 2^16: past byte
    // 65535 the positions wrap, as the pin's do (maintainer ruling
    // 2026-11-01 UTC, `docs/findings/upstream-divergences.md#key-positions-past-byte-65535`).
    KEY_REST_SLOT.begin.store(found.1 as u16, Ordering::Relaxed);
    KEY_REST_SLOT.end.store(found.2 as u16, Ordering::Relaxed);
    // The returned pointer refers to the static slot above.
    write_if_nonnull!(
        key_rest,
        ptr::addr_of!(KEY_REST_SLOT)
            .cast_mut()
            .cast::<ChewingKeyRest>()
    );
    true
}

/// Get the begin/end byte positions of a pinyin key rest.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_key_rest_positions(pinyin_instance_t * instance,
///                                           ChewingKeyRest * key_rest,
///                                           guint16 * begin,
///                                           guint16 * end);
/// ```
///
/// Either `begin` or `end` may be NULL to skip. The pin answers `true`
/// and dereferences `key_rest` only to fill a non-NULL `begin` or `end`
/// (`pinyin.cpp:2967-2977`): a NULL `key_rest` answers `true` when both are
/// NULL, and `false` here rather than crashing when either is not.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `key_rest` is a borrowed object and `begin` and
/// `end` are out-parameters. `_instance` is not read. `key_rest` is read only when `begin` or
/// `end` is not NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_pinyin_key_rest_positions(
    _instance: *mut PinyinInstance,
    key_rest: *mut ChewingKeyRest,
    begin: *mut u16,
    end: *mut u16,
) -> bool {
    // The pin never reads `instance`, and reads `key_rest` only to fill a
    // non-NULL `begin` or `end` (`pinyin.cpp:2967-2977`).
    if key_rest.is_null() && (!begin.is_null() || !end.is_null()) {
        return false;
    }

    write_if_nonnull!(begin, {
        // SAFETY: A requested output implies the checked key rest is non-null.
        // The caller supplies the key rest returned by pinyin_get_pinyin_key_rest.
        unsafe { (*key_rest).begin }
    });
    write_if_nonnull!(end, {
        // SAFETY: The same checked key-rest pointer supplies the end field.
        unsafe { (*key_rest).end }
    });
    true
}

/// Get the raw byte length of a pinyin key rest.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_key_rest_length(pinyin_instance_t * instance,
///                                        ChewingKeyRest * key_rest,
///                                        guint16 * length);
/// ```
///
/// The pin's `key_rest->length()` — `m_raw_end - m_raw_begin` in
/// `guint16` (`chewing_key.h:111-113`), which wraps on a reversed rest
/// (begin 5, end 2 answers 65533) — and, like the pin, `true` whenever it can
/// answer. fcitx branches on this being 2 to pick its shuangpin rendering
/// (`eim.cpp:473`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `key_rest` is a borrowed object and `length` is
/// an out-parameter. `_instance` is not read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_pinyin_key_rest_length(
    _instance: *mut PinyinInstance,
    key_rest: *mut ChewingKeyRest,
    length: *mut u16,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2979-2984`).
    let rest = borrow_or_return!(key_rest, backing_ref, false);
    write_if_nonnull!(length, rest.end.wrapping_sub(rest.begin));
    true
}

/// Render a pinyin key as its full spelling.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_string(pinyin_instance_t * instance,
///                               ChewingKey * key,
///                               gchar ** utf8_str);
/// ```
///
/// Returns true for a key with a spelling. Free the returned string with `g_free`. An unset key
/// writes null and returns false. A null key returns false without writing. A null output skips
/// the write.
///
/// # Aborts
///
/// A packed key with an out-of-range component, or an invalid tone reached during rendering,
/// aborts (`chewing_key.cpp:28-42,75-108`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `key` is a borrowed object and `utf8_str` is an
/// out-parameter. `_instance` is not read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_pinyin_string(
    _instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    // `_ChewingKey::get_pinyin_string` (chewing_key.cpp:47-58): the
    // canonical spelling, tone digit appended for a non-zero tone.

    // SAFETY: The entry point guarantees Borrowed objects: a non-NULL key is valid for reads. Out-
    // parameters: a non-NULL output is aligned and writable without conflicting with live
    // borrows.
    unsafe { render_key(key, utf8_str, oxpinyin::ChewingKey::pinyin_text) }
}

// eim.cpp:512-515.
/// Render a pinyin key as its Zhuyin spelling.
///
/// # C signature
/// ```c
/// bool pinyin_get_zhuyin_string(pinyin_instance_t * instance,
///                               ChewingKey * key,
///                               gchar ** utf8_str);
/// ```
///
/// Returns true for a key with a spelling. Free the returned string with `g_free`. An unset key
/// writes null and returns false. A null key returns false without writing. A null output skips
/// the write.
///
/// # Aborts
///
/// A packed key with an out-of-range component, or an invalid tone reached during rendering,
/// aborts (`chewing_key.cpp:28-42,75-108`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `key` is a borrowed object and `utf8_str` is an
/// out-parameter. `_instance` is not read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_zhuyin_string(
    _instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    // SAFETY: The entry point guarantees Borrowed objects: a non-NULL key is valid for reads. Out-
    // parameters: a non-NULL output is aligned and writable without conflicting with live
    // borrows.
    unsafe { render_key(key, utf8_str, oxpinyin::ChewingKey::zhuyin_text) }
}

// eim.cpp:478-479.
/// Render a pinyin key as its shengmu / yunmu pair.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_strings(pinyin_instance_t * instance,
///                                ChewingKey * key,
///                                gchar ** shengmu,
///                                gchar ** yunmu);
/// ```
///
/// Returns false without writing either output for a null or unset key. Either output may be
/// omitted. Free each returned string with `g_free`. A syllable without an initial returns an
/// empty initial.
///
/// # Aborts
///
/// An out-of-range initial, middle or final aborts (`chewing_key.cpp:28-32`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `key` is a borrowed object and `shengmu` and
/// `yunmu` are out-parameters. `_instance` is not read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_pinyin_strings(
    _instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    shengmu: *mut *mut GChar,
    yunmu: *mut *mut GChar,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2751-2763`).
    let slot = borrow_or_return!(key, backing_ref, false);
    let decoded = slot.to_core();
    abort_on_elements_outside_dims(decoded);
    if decoded.table_index() == 0 {
        return false;
    }
    // `chewing_key.cpp:60-72`: one `g_strdup` per requested half.
    write_if_nonnull!(shengmu, strdup(decoded.shengmu_string()));
    write_if_nonnull!(yunmu, strdup(decoded.yunmu_string()));
    true
}

/// `_ChewingKey::get_table_index` asserts the initial, middle and final
/// against the table dimensions (`chewing_key.cpp:28-32`), and the string
/// renderers assert the tone (`:40`, `:75`, `:92`, `:105`): a key packed from
/// caller memory with a field outside them aborts the process, as the pin
/// does (register row 4).
fn abort_on_elements_outside_dims(key: oxpinyin::ChewingKey) {
    if !key.elements_in_dims() {
        std::process::abort();
    }
}

/// The tone half of [`abort_on_elements_outside_dims`], asserted after the
/// `get_table_index` guard by every renderer that prints a tone.
fn abort_on_tone_outside_dims(key: oxpinyin::ChewingKey) {
    if !key.tone_in_dims() {
        std::process::abort();
    }
}

/// The shared body of the single-string renderers: the rendering made with
/// the pin's `g_strdup`/`g_strdup_printf` call (`chewing_key.cpp:47-88`).
///
/// # Safety
///
/// Under the crate-level rules, Borrowed objects: a non-NULL key is valid for
/// reads. Out-parameters: a non-NULL output is aligned and writable without
/// conflicting with live borrows.
unsafe fn render_key(
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
    render: impl Fn(oxpinyin::ChewingKey) -> oxpinyin::KeyText,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2707-2727`).
    if key.is_null() {
        return false;
    }
    write_if_nonnull!(utf8_str, ptr::null_mut());

    // SAFETY: Borrowed objects guarantees a readable key; NULL was checked.
    let slot = unsafe { &*key };
    let decoded = slot.to_core();
    abort_on_elements_outside_dims(decoded);
    if decoded.table_index() == 0 {
        return false;
    }
    abort_on_tone_outside_dims(decoded);
    write_if_nonnull!(utf8_str, key_text(render(decoded)));
    true
}

// pinyin.cpp:3008-3027.
/// Get the lookup offset from a user cursor position.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_offset(pinyin_instance_t * instance,
///                               size_t cursor,
///                               size_t * offset);
/// ```
///
/// Clamp the byte cursor to the parsed length. Search backward from that column, including it,
/// for a nonempty matrix column, and stop at zero. Then move the result backward across zero-key
/// separator columns. The result can equal the parsed length, so it is not always the start of a
/// key. Success writes the lookup offset. Failure leaves the output untouched.
///
/// # Aborts
///
/// A positive clamped cursor on an empty key matrix aborts.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `offset` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_pinyin_offset(
    instance: *mut PinyinInstance,
    cursor: usize,
    offset: *mut usize,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    // The pin clamps the cursor to the parsed length and reads
    // `get_column_size` at each position from there down. A parse that
    // consumed bytes but placed no key (apostrophes only) leaves the matrix
    // empty (`fill_matrix`, `phonetic_key_matrix.cpp:34-38`), and the first
    // read asserts (`phonetic_key_matrix.h:103`; register row 66).
    if inst.core.pin_matrix_columns() == 0 && cursor.min(inst.core.pin_parsed_len()) >= 1 {
        std::process::abort();
    }
    let Ok(normalized) = inst.core.lookup_offset(cursor) else {
        return false;
    };
    write_if_nonnull!(offset, normalized);
    true
}

/// The pin reads matrix column `index` with `get_column_size`, which asserts
/// `index < size` (`phonetic_key_matrix.h:103`). The matrix has
/// `parsed_len + 1` columns, fewer than the input has bytes when the parse
/// stopped early, and none while it placed no key; the process aborts for a
/// column outside it (register row 66).
fn abort_past_matrix(inst: &crate::state::CapiInstanceRead<'_>, index: usize) {
    if index >= inst.core.pin_matrix_columns() {
        std::process::abort();
    }
}

// pinyin.cpp:3029-3059, pinyin.cpp:3035, phonetic_key_matrix.h:103.
/// Get the left offset from a lookup offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_left_pinyin_offset(pinyin_instance_t * instance,
///                                    size_t offset,
///                                    size_t * left);
/// ```
///
/// Find the column of a key that ends at this offset, or use zero if no key ends there. Then
/// move the result backward across zero-key separator columns. Write that lookup position. It is
/// not always the raw start of the preceding key. Returns true on success. An ordinary failure
/// leaves the output untouched.
///
/// # Aborts
///
/// An offset that reaches an invalid matrix column or fails the separator check aborts.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `left` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_left_pinyin_offset(
    instance: *mut PinyinInstance,
    offset: usize,
    left: *mut usize,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    // `_check_offset` reads column `offset - 1` first (`pinyin.cpp:3035`).
    if offset >= 1 {
        abort_past_matrix(&inst.read(), offset - 1);
    }
    let result = match inst.core.left_offset(offset) {
        Ok(result) => result,
        // Register row 14: `assert(_check_offset(...))`,
        // `pinyin.cpp:3035`, `:3057`; an offset past the matrix asserts in
        // its `get_column_size` (`phonetic_key_matrix.h:103`).
        Err(_) => std::process::abort(),
    };
    write_if_nonnull!(left, result);
    true
}

// pinyin.cpp:3061-3094, pinyin.cpp:3087-3088, pinyin.cpp:3067, phonetic_key_matrix.h:103.
/// Get the right offset from a lookup offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_right_pinyin_offset(pinyin_instance_t * instance,
///                                     size_t offset,
///                                     size_t * right);
/// ```
///
/// Write the end of the next key, skipping leading separator columns. Returns false when no key
/// starts there. Returns true on success. An ordinary failure leaves the output untouched.
///
/// # Aborts
///
/// An offset that reaches an invalid matrix column or fails the separator check aborts. On an
/// empty matrix any offset aborts.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `right` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_right_pinyin_offset(
    instance: *mut PinyinInstance,
    offset: usize,
    right: *mut usize,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    // The loop reads column `offset` and `_check_offset` column `offset - 1`
    // (`pinyin.cpp:3067`, `zhuyin.cpp:1556`).
    abort_past_matrix(&inst.read(), offset);
    let result = match inst.core.right_offset(offset) {
        Ok(Some(result)) => result,
        // The pin's own graceful `false`, `pinyin.cpp:3087-3088`. An empty
        // matrix, where the loop's `get_column_size` asserts first, has no
        // column, so `abort_past_matrix` above has already aborted.
        Ok(None) => return false,
        // Register row 14: `assert(_check_offset(...))`,
        // `pinyin.cpp:3067`, `:3092`, and the matrix's own index assert
        // past its end.
        Err(_) => std::process::abort(),
    };
    write_if_nonnull!(right, result);
    true
}

// ── The preedit key family ───────────────────────────────────────────────
//
// fcitx-libpinyin's preedit renderer (`eim.cpp:419-520`) walks the parsed
// input calling `pinyin_get_pinyin_key` at each offset, uses a `false`
// return as its loop terminator, and renders every key through the string
// functions below. Its loop advances with `pinyin_get_right_pinyin_offset`,
// so the offsets it asks about are always syllable-aligned; the
// empty-column `false` path exists for callers that walk byte by byte.
//
// `ChewingKey` and `ChewingKeyRest` are opaque to consumers — the shipped
// `pinyin.h` forward-declares both and `chewing_key.h` is not installed —
// so what must match the pin is the observable output: the boolean, the
// two `guint16`s, and the rendered strings.

/// Get the pinyin key at an offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_key(pinyin_instance_t * instance,
///                            size_t offset,
///                            ChewingKey ** key);
/// ```
///
/// Success writes a borrowed pointer to a static slot. All instances share this getter’s slot.
/// Its contents change on the next successful call, and its storage outlives the instance. The
/// caller does not free it. An ordinary miss writes null. A null instance returns false without
/// writing.
///
/// # Aborts
///
/// On an empty matrix, an offset other than `SIZE_MAX` aborts. `SIZE_MAX` returns false.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `key` is an
/// out-parameter. The serial-calls rule applies. The pointer written to `*key` addresses a
/// process-wide slot, which the next call of this function overwrites.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_pinyin_key(
    instance: *mut PinyinInstance,
    offset: usize,
    key: *mut *mut ChewingKey,
) -> bool {
    if instance.is_null() {
        return false;
    }
    write_if_nonnull!(key, ptr::null_mut());

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`; this call only reads the instance and writes
    // only the process-wide key slot.
    let inst = unsafe { instance_ref(instance) };
    check_key_matrix(&inst, offset);
    let Ok(Some(found)) = inst.core.abi_key_at(offset, false) else {
        return false;
    };
    let packed = found.0;
    KEY_SLOT.store(packed, Ordering::Relaxed);
    // The returned pointer refers to the static slot above.
    write_if_nonnull!(key, KEY_SLOT.as_ptr().cast::<ChewingKey>());
    true
}

#[cfg(test)]
mod tests;
