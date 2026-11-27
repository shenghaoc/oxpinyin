//! Zhuyin key access and cursor offsets.
//!
//! Read parsed keys and their raw input spans with [`zhuyin_get_zhuyin_key`]
//! and [`zhuyin_get_zhuyin_key_rest`]. [`zhuyin_get_zhuyin_offset`] maps an
//! input cursor to a lookup offset. [`zhuyin_get_left_zhuyin_offset`] and
//! [`zhuyin_get_right_zhuyin_offset`] move to the preceding key start or the next key end.
//!
//! Where the pin asserts in `_check_offset` (`zhuyin.cpp:1453`) or the
//! matrix index check (`phonetic_key_matrix.h:103`), the process aborts.

use std::ptr;
use std::sync::atomic::{AtomicU16, Ordering};

use crate::ffi::{backing_ref, borrow_or_return, write_if_nonnull};
use crate::state::instance_ref;
use crate::types::{ChewingKey, ChewingKeyRest, ZhuyinInstance};

/// The slot `zhuyin_get_zhuyin_key` hands out: the pin's function-local
/// `static ChewingKey key` (`zhuyin.cpp:1898`), one process-wide word that
/// every instance overwrites and that outlives every instance. An atomic
/// word keeps the Rust side free of a data race; the C side reads it
/// through the pointer, as it reads the pin's static.
static KEY_SLOT: AtomicU16 = AtomicU16::new(0);

/// The `ChewingKeyRest` twin of [`KEY_SLOT`] (`static ChewingKeyRest
/// key_rest`, `zhuyin.cpp:1921`): `m_raw_begin` then `m_raw_end`.
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

/// Get the zhuyin key rest at an offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_key_rest(zhuyin_instance_t * instance,
///                                 size_t offset, ChewingKeyRest ** key_rest);
/// ```
///
/// Success writes a borrowed pointer to a static slot. All instances share this getter's slot.
/// Its contents change on the next successful call, and its storage outlives the instance. The
/// caller does not free it. An ordinary miss writes null. A null instance returns false without
/// writing.
///
/// # Aborts
///
/// On an empty matrix, an offset other than `SIZE_MAX` aborts. `SIZE_MAX` returns false.
/// A separator offset that fails the offset check also aborts (`zhuyin.cpp:1453`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `key_rest` is an
/// out-parameter. The serial-calls rule applies. The pointer written to `*key_rest` addresses a
/// process-wide slot, which the next call of this function overwrites.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_get_zhuyin_key_rest(
    instance: *mut ZhuyinInstance,
    offset: usize,
    key_rest: *mut *mut ChewingKeyRest,
) -> bool {
    if instance.is_null() {
        return false;
    }
    write_if_nonnull!(key_rest, ptr::null_mut());

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`; this call only reads the instance and writes
    // only the process-wide key slot.
    let inst = unsafe { instance_ref(instance) };
    check_key_matrix(&inst, offset);
    let found = match inst.core.abi_key_at(offset, oxpinyin::engine::Law::Zhuyin) {
        Ok(Some(found)) => found,
        Ok(None) => return false,
        Err(_) => std::process::abort(),
    };
    // `m_raw_begin`/`m_raw_end` are `guint16` (`chewing_key.h:102-103`),
    // and the pin's `int` positions narrow into them mod 2^16: past byte
    // 65535 the positions wrap, as the pin's do (`docs/findings/upstream-divergences.md#key-positions-past-byte-65535`).
    KEY_REST_SLOT
        .begin
        .store(found.begin as u16, Ordering::Relaxed);
    KEY_REST_SLOT.end.store(found.end as u16, Ordering::Relaxed);
    // The returned pointer refers to the static slot above.
    write_if_nonnull!(
        key_rest,
        ptr::addr_of!(KEY_REST_SLOT)
            .cast_mut()
            .cast::<ChewingKeyRest>()
    );
    true
}

/// Get the begin/end byte positions of a zhuyin key rest.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_key_rest_positions(zhuyin_instance_t * instance,
///                                           ChewingKeyRest * key_rest,
///                                           guint16 * begin, guint16 * end);
/// ```
///
/// The pin's body never reads the instance (`zhuyin.cpp:1930-1941`): a NULL
/// instance is a defined call there, so it is unused here. The key-rest
/// pointer is read only to fill a non-NULL `begin` or `end`, so a NULL one
/// answers `true` when both are NULL and `false` when either is not.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `key_rest` is a borrowed object and `begin` and
/// `end` are out-parameters. `_instance` is not read. `key_rest` is read only when `begin` or
/// `end` is not NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_get_zhuyin_key_rest_positions(
    _instance: *mut ZhuyinInstance,
    key_rest: *mut ChewingKeyRest,
    begin: *mut u16,
    end: *mut u16,
) -> bool {
    // The pin reads `key_rest` only to fill a non-NULL `begin` or `end`
    // (`zhuyin.cpp:1930-1940`).
    if key_rest.is_null() && (!begin.is_null() || !end.is_null()) {
        return false;
    }

    write_if_nonnull!(begin, {
        // SAFETY: A requested output implies the checked key rest is non-null.
        // The caller supplies the key rest returned by zhuyin_get_zhuyin_key_rest.
        unsafe { (*key_rest).begin }
    });
    write_if_nonnull!(end, {
        // SAFETY: The same checked key-rest pointer supplies the end field.
        unsafe { (*key_rest).end }
    });
    true
}

/// Get the raw byte length of a zhuyin key rest.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_key_rest_length(zhuyin_instance_t * instance,
///                                        ChewingKeyRest * key_rest,
///                                        guint16 * length);
/// ```
///
/// Returns false without writing for a null key rest. Otherwise, writes the unsigned
/// 16-bit difference between end and begin if requested and returns true. The instance
/// is unused (`zhuyin.cpp:1942-1946`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `key_rest` is a borrowed object and `length` is
/// an out-parameter. `_instance` is not read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_get_zhuyin_key_rest_length(
    _instance: *mut ZhuyinInstance,
    key_rest: *mut ChewingKeyRest,
    length: *mut u16,
) -> bool {
    let rest = borrow_or_return!(key_rest, backing_ref, false);
    write_if_nonnull!(length, rest.end.wrapping_sub(rest.begin));
    true
}

/// Get the zhuyin key at an offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_key(zhuyin_instance_t * instance,
///                            size_t offset, ChewingKey ** key);
/// ```
///
/// Success writes a borrowed pointer to a static slot. All instances share this getter's slot.
/// Its contents change on the next successful call, and its storage outlives the instance. The
/// caller does not free it. An ordinary miss writes null. A null instance returns false without
/// writing.
///
/// # Aborts
///
/// On an empty matrix, an offset other than `SIZE_MAX` aborts. `SIZE_MAX` returns false.
/// A separator offset that fails the offset check also aborts (`zhuyin.cpp:1453`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `key` is an
/// out-parameter. The serial-calls rule applies. The pointer written to `*key` addresses a
/// process-wide slot, which the next call of this function overwrites.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_get_zhuyin_key(
    instance: *mut ZhuyinInstance,
    offset: usize,
    key: *mut *mut ChewingKey,
) -> bool {
    if instance.is_null() {
        return false;
    }
    write_if_nonnull!(key, ptr::null_mut());

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`; this call only reads the instance and writes
    // only the process-wide key slot.
    let inst = unsafe { instance_ref(instance) };
    check_key_matrix(&inst, offset);
    let found = match inst.core.abi_key_at(offset, oxpinyin::engine::Law::Zhuyin) {
        Ok(Some(found)) => found,
        Ok(None) => return false,
        Err(_) => std::process::abort(),
    };
    let packed = found.packed;
    KEY_SLOT.store(packed, Ordering::Relaxed);
    // The returned pointer refers to the static slot above.
    write_if_nonnull!(key, KEY_SLOT.as_ptr().cast::<ChewingKey>());
    true
}

/// Get the lookup offset from a user cursor position.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_offset(zhuyin_instance_t * instance,
///                               size_t cursor, size_t * offset);
/// ```
///
/// Clamp the byte cursor to the parsed length. Search backward from that column, including it,
/// for a nonempty matrix column, and stop at zero. Then move the result backward across zero-key
/// separator columns. The result can be the start of the key at the cursor, or the parsed
/// length. It is not always the start of the preceding syllable. Success writes the lookup
/// offset. Failure leaves the output untouched.
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
pub unsafe extern "C" fn zhuyin_get_zhuyin_offset(
    instance: *mut ZhuyinInstance,
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
/// column outside it.
fn abort_past_matrix(inst: &crate::state::CapiInstanceRead<'_>, index: usize) {
    if index >= inst.core.pin_matrix_columns() {
        std::process::abort();
    }
}

/// Get the left offset from a lookup offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_left_zhuyin_offset(zhuyin_instance_t * instance,
///                                    size_t offset, size_t * left);
/// ```
///
/// Write the start of the preceding key, or zero if none ends at this offset. Returns true on
/// success. An ordinary failure leaves the output untouched.
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
pub unsafe extern "C" fn zhuyin_get_left_zhuyin_offset(
    instance: *mut ZhuyinInstance,
    offset: usize,
    left: *mut usize,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    // `_check_offset` reads column `offset - 1` (`zhuyin.cpp:1449`), which
    // an empty matrix does not have; offset 0 reads no column and answers 0.
    if offset > 0 {
        abort_past_matrix(&inst.read(), offset - 1);
    }
    let result = match inst
        .core
        .abi_left_offset(offset, oxpinyin::engine::Law::Zhuyin)
    {
        Ok(result) => result,
        // Register row 66: `_check_offset`'s assert (`zhuyin.cpp:1453`,
        // via `:1998` and `:2020`), and its `get_column_size` past the
        // matrix (`phonetic_key_matrix.h:103`).
        Err(_) => std::process::abort(),
    };
    write_if_nonnull!(left, result);
    true
}

/// Get the right offset from a lookup offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_right_zhuyin_offset(zhuyin_instance_t * instance,
///                                     size_t offset, size_t * right);
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
pub unsafe extern "C" fn zhuyin_get_right_zhuyin_offset(
    instance: *mut ZhuyinInstance,
    offset: usize,
    right: *mut usize,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false);
    // The loop reads column `offset` and `_check_offset` column `offset - 1`
    // (`pinyin.cpp:3067`, `zhuyin.cpp:1556`). An empty matrix has no column,
    // so this also aborts there (register row 66).
    abort_past_matrix(&inst.read(), offset);
    let result = match inst.core.right_offset(offset) {
        Ok(Some(result)) => result,
        // The pin's own graceful `false` (`zhuyin.cpp:2050-2051`).
        Ok(None) => return false,
        // Register row 66: `_check_offset`'s assert (`zhuyin.cpp:1453`,
        // via `:2030` and `:2055`), and its `get_column_size` past the
        // matrix (`phonetic_key_matrix.h:103`).
        Err(_) => std::process::abort(),
    };
    write_if_nonnull!(right, result);
    true
}
