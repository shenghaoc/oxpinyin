//! Pinyin key access and cursor/offset navigation.
//!
//! The cursor → lookup-offset normalization and the word-level left/right
//! moves port the pin's matrix laws over the engine's positional data, using
//! the zhuyin parse's key spans. Where the pin asserts — `_check_offset`
//! (`zhuyin.cpp:1453`) or the matrix's own index check
//! (`phonetic_key_matrix.h:103`) — the process aborts (register row 66).

use std::ptr;
use std::sync::atomic::{AtomicU16, Ordering};

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

/// Register row 66, `phonetic_key_matrix.h:103`: on an empty matrix (a
/// fresh or keyless instance: `fill_matrix` leaves it at size 0 when the
/// parse placed no key, `phonetic_key_matrix.cpp:37`) the pin's first
/// `get_column_size` asserts — the key accessors' after `matrix.size() - 1`
/// wraps (`zhuyin.cpp:1892-1895`, `:1914-1917`) — and the process aborts.
/// The right move's assert in its loop (`:2037-2038`) is
/// [`abort_past_matrix`]'s: an empty matrix has no column.
fn abort_on_empty_matrix(inst: &crate::state::CapiInstanceRead<'_>) {
    if !inst.core.mode_has_keys() {
        std::process::abort();
    }
}

/// [`abort_on_empty_matrix`] for the key accessors, which test
/// `offset >= matrix.size() - 1` first (`zhuyin.cpp:1892-1893`,
/// `:1914-1915`): on an empty matrix that is `offset >= SIZE_MAX`, so
/// `SIZE_MAX` itself answers `false` with the out-param NULLed.
fn abort_on_empty_matrix_below(inst: &crate::state::CapiInstanceRead<'_>, offset: usize) {
    if offset != usize::MAX {
        abort_on_empty_matrix(&inst.read());
    }
}

/// The key accessors' answer for an `abi_key_at` error:
/// `_check_offset`'s assert on a lone zero key before the offset
/// (`zhuyin.cpp:1453` via `:1898`, `:1920`) aborts the process (register row
/// 66); a parse the facade cannot read, which the pin has no counterpart
/// for, answers `false` with one warning.
fn key_lookup_failed(error: &oxpinyin_engine::EngineError, name: &str) -> bool {
    if matches!(
        error,
        oxpinyin_engine::EngineError::ZeroKeyOffsetCheck { .. }
    ) {
        std::process::abort();
    }
    crate::ffi::log_warning(&format!("{name}: the lookup failed"));
    false
}

/// Get the zhuyin key rest at an offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_key_rest(zhuyin_instance_t * instance,
///                                 size_t offset, ChewingKeyRest ** key_rest);
/// ```
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
    if !key_rest.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *key_rest = ptr::null_mut();
        }
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`; this call only reads the instance and writes
    // only the process-wide key slot.
    let inst = unsafe { instance_ref(instance) };
    abort_on_empty_matrix_below(&inst, offset);
    let found = match inst.core.abi_key_at(offset, true) {
        Ok(Some(found)) => found,
        Ok(None) => return false,
        Err(error) => return key_lookup_failed(&error, "zhuyin_get_zhuyin_key_rest"),
    };
    // `m_raw_begin`/`m_raw_end` are `guint16` (`chewing_key.h:102-103`),
    // and the pin's `int` positions narrow into them mod 2^16: past byte
    // 65535 the positions wrap, as the pin's do (maintainer ruling
    // 2026-11-01 UTC, `docs/findings/upstream-divergences.md#key-positions-past-byte-65535`).
    KEY_REST_SLOT.begin.store(found.1 as u16, Ordering::Relaxed);
    KEY_REST_SLOT.end.store(found.2 as u16, Ordering::Relaxed);
    if !key_rest.is_null() {
        // SAFETY: Null-checked above; the slot is a `static`, so the
        // pointer never dangles. `KeyRestSlot` is `repr(C)` over two
        // `AtomicU16`, the layout of `ChewingKeyRest` (asserted above).
        unsafe {
            *key_rest = ptr::addr_of!(KEY_REST_SLOT)
                .cast_mut()
                .cast::<ChewingKeyRest>();
        }
    }
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
/// answers `true` when both are NULL and `false` (row 41) when either is not.
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

    if !begin.is_null() {
        // SAFETY: `begin` is non-null, so `key_rest` is non-null (checked
        // above) and was produced by `zhuyin_get_zhuyin_key_rest`.
        unsafe {
            *begin = (*key_rest).begin;
        }
    }
    if !end.is_null() {
        // SAFETY: As for `begin`.
        unsafe {
            *end = (*key_rest).end;
        }
    }
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
/// The pin's body never reads the instance (`zhuyin.cpp:1942-1946`): a NULL
/// instance is a defined call there, so it is unused here. The key-rest
/// pointer is read and `length` is written unconditionally by the pin, so a
/// NULL `length` is register row 41's class (b) site; the optional write is
/// skipped and the call still answers `true`, as the pinyin twin does
/// (`crates/oxpinyin-capi/src/cursor.rs:201-218`).
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
    if key_rest.is_null() {
        return false;
    }

    // SAFETY: Non-null and produced by `zhuyin_get_zhuyin_key_rest`.
    let rest = unsafe { &*key_rest };
    if !length.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *length = rest.end.wrapping_sub(rest.begin);
        }
    }
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
    if !key.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *key = ptr::null_mut();
        }
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`; this call only reads the instance and writes
    // only the process-wide key slot.
    let inst = unsafe { instance_ref(instance) };
    abort_on_empty_matrix_below(&inst, offset);
    let found = match inst.core.abi_key_at(offset, true) {
        Ok(Some(found)) => found,
        Ok(None) => return false,
        Err(error) => return key_lookup_failed(&error, "zhuyin_get_zhuyin_key"),
    };
    let packed = found.0;
    KEY_SLOT.store(packed, Ordering::Relaxed);
    if !key.is_null() {
        // SAFETY: Null-checked above; the slot is a `static`, so the
        // pointer never dangles. `AtomicU16` has the size and alignment of
        // the two-byte `ChewingKey` (asserted above).
        unsafe {
            *key = KEY_SLOT.as_ptr().cast::<ChewingKey>();
        }
    }
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
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
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
    if !offset.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *offset = normalized;
        }
    }
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

/// Get the left offset from a lookup offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_left_zhuyin_offset(zhuyin_instance_t * instance,
///                                    size_t offset, size_t * left);
/// ```
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
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // `_check_offset` reads column `offset - 1` (`zhuyin.cpp:1449`), which
    // an empty matrix does not have; offset 0 reads no column and answers 0.
    if offset > 0 {
        abort_past_matrix(&inst.read(), offset - 1);
    }
    let result = match inst.core.abi_left_offset(offset, true) {
        Ok(result) => result,
        Err(error) => {
            match oxpinyin_facade::CursorFault::of(&error, oxpinyin_facade::CursorLaw::CheckOffset)
            {
                // Register row 66: `_check_offset`'s assert (`zhuyin.cpp:1453`,
                // via `:1998` and `:2020`), and its `get_column_size` past the
                // matrix (`phonetic_key_matrix.h:103`).
                oxpinyin_facade::CursorFault::PinAssert => std::process::abort(),
                // A parse the facade cannot read, which the pin has no counterpart
                // for.
                oxpinyin_facade::CursorFault::Unreadable => {
                    crate::ffi::log_warning("zhuyin_get_left_zhuyin_offset: the lookup failed");
                    return false;
                }
            }
        }
    };
    if !left.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *left = result;
        }
    }
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
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // The loop reads column `offset` and `_check_offset` column `offset - 1`
    // (`pinyin.cpp:3067`, `zhuyin.cpp:1556`). An empty matrix has no column,
    // so this also aborts there (register row 66).
    abort_past_matrix(&inst.read(), offset);
    let result = match inst.core.right_offset(offset) {
        Ok(Some(result)) => result,
        // The pin's own graceful `false` (`zhuyin.cpp:2050-2051`).
        Ok(None) => return false,
        Err(error) => {
            match oxpinyin_facade::CursorFault::of(&error, oxpinyin_facade::CursorLaw::CheckOffset)
            {
                // Register row 66: `_check_offset`'s assert (`zhuyin.cpp:1453`,
                // via `:2030` and `:2055`), and its `get_column_size` past the
                // matrix (`phonetic_key_matrix.h:103`).
                oxpinyin_facade::CursorFault::PinAssert => std::process::abort(),
                // A parse the facade cannot read, which the pin has no counterpart
                // for.
                oxpinyin_facade::CursorFault::Unreadable => {
                    crate::ffi::log_warning("zhuyin_get_right_zhuyin_offset: the lookup failed");
                    return false;
                }
            }
        }
    };
    if !right.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *right = result;
        }
    }
    true
}
