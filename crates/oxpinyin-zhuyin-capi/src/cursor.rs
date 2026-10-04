//! Pinyin key access and cursor/offset navigation.
//!
//! The cursor → lookup-offset normalization and the word-level left/right
//! moves port the pin's matrix laws over the engine's positional data, using
//! the zhuyin parse's key spans. Where the pin's `_check_offset` aborts, these
//! answer `false` per the no-abort policy (divergence class (c)).

use std::ptr;
use std::sync::atomic::{AtomicU16, Ordering};

use crate::state::{CapiInstance, instance_mut, instance_ref};
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

/// Whether the parse placed a key. The pin's matrix is empty (size 0) for a
/// fresh instance or a keyless parse (`phonetic_key_matrix.cpp:34-38`); an
/// unreadable parse is left to the callers' own refusals.
pub(crate) fn matrix_has_keys(inst: &CapiInstance) -> bool {
    let Ok((keys, input, _)) = inst.core.mode_keys() else {
        return true;
    };
    !(input.is_empty() || keys.is_empty())
}

/// Class (c), `phonetic_key_matrix.h:103`: on an empty matrix the pin's
/// `matrix.size() - 1` wraps, the offset test passes, and `get_column_size`
/// asserts. A matrix with keys is the graceful `false`, silent.
fn warn_on_empty_matrix(inst: &CapiInstance, name: &str) {
    if !matrix_has_keys(inst) {
        crate::ffi::log_warning(&format!(
            "{name}: assertion 'index < m_table_content->len' failed"
        ));
    }
}

/// Get the zhuyin key rest at an offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_key_rest(zhuyin_instance_t * instance,
///                                 size_t offset, ChewingKeyRest ** key_rest);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_zhuyin_key_rest(
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
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    let Some(found) = inst.core.key_at(offset) else {
        warn_on_empty_matrix(inst, "zhuyin_get_zhuyin_key_rest");
        return false;
    };
    KEY_REST_SLOT.begin.store(
        u16::try_from(found.begin).unwrap_or(u16::MAX),
        Ordering::Relaxed,
    );
    KEY_REST_SLOT.end.store(
        u16::try_from(found.end).unwrap_or(u16::MAX),
        Ordering::Relaxed,
    );
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
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_zhuyin_key_rest_positions(
    instance: *mut ZhuyinInstance,
    key_rest: *mut ChewingKeyRest,
    begin: *mut u16,
    end: *mut u16,
) -> bool {
    if instance.is_null() || key_rest.is_null() {
        return false;
    }

    // SAFETY: Non-null and produced by `zhuyin_get_zhuyin_key_rest`.
    let rest = unsafe { &*key_rest };
    if !begin.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *begin = rest.begin;
        }
    }
    if !end.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *end = rest.end;
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
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_zhuyin_key_rest_length(
    instance: *mut ZhuyinInstance,
    key_rest: *mut ChewingKeyRest,
    length: *mut u16,
) -> bool {
    if instance.is_null() || key_rest.is_null() {
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
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_zhuyin_key(
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
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    let Some(found) = inst.core.key_at(offset) else {
        warn_on_empty_matrix(inst, "zhuyin_get_zhuyin_key");
        return false;
    };
    // `found.text` comes from `mode_keys`, which reads the parsed keys /
    // the session matrix — always a syllable present in the content table —
    // so `from_spelling` cannot fail in practice. Keep the fetch-failure
    // `unwrap_or(ChewingKey::ZERO)` fallback (matching oxpinyin-capi,
    // cursor.rs) rather than propagating lookup failure: a stale matrix
    // key is not a reachable state, and the fallback keeps the ABI's
    // boolean success semantics identical to the pin.
    let packed = ChewingKey::from_spelling(found.text, found.tone)
        .unwrap_or(ChewingKey::ZERO)
        .packed;
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
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_zhuyin_offset(
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

/// Get the left offset from a lookup offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_left_zhuyin_offset(zhuyin_instance_t * instance,
///                                    size_t offset, size_t * left);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_left_zhuyin_offset(
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
    let Ok(result) = inst.core.left_offset(offset) else {
        // Class (c): the pin's `get_column_size` asserts past the matrix.
        crate::ffi::log_warning(
            "zhuyin_get_left_zhuyin_offset: assertion 'index < m_table_content->len' failed",
        );
        return false;
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
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_right_zhuyin_offset(
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
    let result = match inst.core.right_offset(offset) {
        Ok(Some(result)) => result,
        // The pin's own graceful `false` (`zhuyin.cpp:2049`), except on an
        // empty matrix, where the loop's `get_column_size` asserts first.
        Ok(None) => {
            warn_on_empty_matrix(inst, "zhuyin_get_right_zhuyin_offset");
            return false;
        }
        Err(_) => {
            crate::ffi::log_warning(
                "zhuyin_get_right_zhuyin_offset: assertion 'index < m_table_content->len' failed",
            );
            return false;
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
