//! Pinyin key access and cursor/offset navigation.
//!
//! The cursor → lookup-offset normalization (D1) and the word-level
//! left/right moves (D2) port the pin's matrix laws
//! (`pinyin.cpp:3008-3095` at the pin) over the engine's positional
//! data — `oxpinyin_engine::lookup_offset_for_cursor` and the
//! `*_word_offset` pair. Where the pin's `_check_offset` aborts
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

use crate::ffi::{key_text, strdup};
use crate::state::{instance_mut, instance_ref};
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

/// Get the pinyin key rest at an offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_key_rest(pinyin_instance_t * instance,
///                                 size_t offset,
///                                 ChewingKeyRest ** key_rest);
/// ```
///
/// Out-param `key_rest` borrows the process-wide slot [`KEY_REST_SLOT`],
/// as the pin's function-local `static` does: every instance shares it,
/// the next call from any instance overwrites it, and it outlives the
/// instance. Answers at exactly the offsets [`pinyin_get_pinyin_key`]
/// does.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_key_rest(
    instance: *mut PinyinInstance,
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
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    let found = match inst.core.abi_key_at(offset, false) {
        Ok(Some(found)) => found,
        Ok(None) => {
            abort_on_empty_matrix_below(&inst.read(), offset);
            return false;
        }
        Err(_) => {
            crate::ffi::log_warning("pinyin_get_pinyin_key_rest: zero-column assertion");
            return false;
        }
    };
    // `m_raw_begin`/`m_raw_end` are `guint16` (`chewing_key.h:102-103`),
    // and the pin's `int` positions narrow into them mod 2^16: past byte
    // 65535 the positions wrap, as the pin's do (maintainer ruling
    // 2026-11-01 UTC, `docs/findings/1501-guint16-key-positions.md`).
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

/// Register rows 63 and 66, `phonetic_key_matrix.h:103`: with no matrix at
/// all (a fresh or keyless instance: `fill_matrix` leaves it at size 0 when
/// the parse placed no key, `phonetic_key_matrix.cpp:37`) the pin's
/// `matrix.size() - 1` wraps, the offset test passes, and `get_column_size`
/// asserts, so the process aborts. A matrix with keys is the graceful
/// `false`.
fn abort_on_empty_matrix(inst: &crate::state::CapiInstanceRead<'_>) {
    if !crate::sentence::matrix_has_keys(&inst.read()) {
        std::process::abort();
    }
}

/// [`abort_on_empty_matrix`] for the key getters, which test
/// `offset >= matrix.size() - 1` first (`pinyin.cpp:2927-2928`, `:2950-2951`):
/// on an empty matrix that is `offset >= SIZE_MAX`, so `SIZE_MAX` itself
/// answers `false` with the out-param NULLed, and only a smaller offset
/// reaches the assert.
fn abort_on_empty_matrix_below(inst: &crate::state::CapiInstanceRead<'_>, offset: usize) {
    if offset != usize::MAX {
        abort_on_empty_matrix(&inst.read());
    }
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
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_key_rest_positions(
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

    if !begin.is_null() {
        // SAFETY: `begin` is non-null, so `key_rest` is non-null (checked
        // above) and was produced by `pinyin_get_pinyin_key_rest`.
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
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_key_rest_length(
    _instance: *mut PinyinInstance,
    key_rest: *mut ChewingKeyRest,
    length: *mut u16,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2979-2984`).
    if key_rest.is_null() {
        return false;
    }

    // SAFETY: Non-null and produced by `pinyin_get_pinyin_key_rest`.
    let rest = unsafe { &*key_rest };
    if !length.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *length = rest.end.wrapping_sub(rest.begin);
        }
    }
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
/// Out-param `utf8_str` is caller-owned (`g_free`). The pin answers
/// `false` with a NULL out-param for a key whose table index is 0 — the
/// unset key — which an unpopulated slot reproduces.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_string(
    _instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    // `_ChewingKey::get_pinyin_string` (chewing_key.cpp:47-58): the
    // canonical spelling, tone digit appended for a non-zero tone.
    render_key(key, utf8_str, oxpinyin_core::ChewingKey::pinyin_text)
}

/// Render a pinyin key as its Zhuyin spelling.
///
/// # C signature
/// ```c
/// bool pinyin_get_zhuyin_string(pinyin_instance_t * instance,
///                               ChewingKey * key,
///                               gchar ** utf8_str);
/// ```
///
/// Out-param `utf8_str` is caller-owned (`g_free`). fcitx treats a NULL
/// out-param as its "something like xi'" break condition
/// (`eim.cpp:512-515`), which the `false` return preserves.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_zhuyin_string(
    _instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    render_key(key, utf8_str, oxpinyin_core::ChewingKey::zhuyin_text)
}

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
/// Either out-param may be NULL to skip; both are caller-owned
/// (`g_free`). A syllable with no initial answers an empty shengmu, which
/// is the case fcitx substitutes an apostrophe for (`eim.cpp:478-479`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_strings(
    _instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    shengmu: *mut *mut GChar,
    yunmu: *mut *mut GChar,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2751-2763`).
    if key.is_null() {
        return false;
    }

    // SAFETY: Non-null and produced by `pinyin_get_pinyin_key`.
    let slot = unsafe { &*key };
    let decoded = slot.to_core();
    abort_on_elements_outside_dims(decoded);
    if decoded.table_index() == 0 {
        return false;
    }
    // `chewing_key.cpp:60-72`: one `g_strdup` per requested half.
    if !shengmu.is_null() {
        // SAFETY: Null-checked above.
        unsafe { *shengmu = strdup(decoded.shengmu_string()) };
    }
    if !yunmu.is_null() {
        // SAFETY: Null-checked above.
        unsafe { *yunmu = strdup(decoded.yunmu_string()) };
    }
    true
}

/// `_ChewingKey::get_table_index` asserts the initial, middle and final
/// against the table dimensions (`chewing_key.cpp:28-32`), and the string
/// renderers assert the tone (`:40`, `:75`, `:92`, `:105`): a key packed from
/// caller memory with a field outside them aborts the process, as the pin
/// does (register row 4).
fn abort_on_elements_outside_dims(key: oxpinyin_core::ChewingKey) {
    if !key.elements_in_dims() {
        std::process::abort();
    }
}

/// The tone half of [`abort_on_elements_outside_dims`], asserted after the
/// `get_table_index` guard by every renderer that prints a tone.
fn abort_on_tone_outside_dims(key: oxpinyin_core::ChewingKey) {
    if !key.tone_in_dims() {
        std::process::abort();
    }
}

/// The shared body of the single-string renderers: the rendering made with
/// the pin's `g_strdup`/`g_strdup_printf` call (`chewing_key.cpp:47-88`).
fn render_key(
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
    render: impl Fn(oxpinyin_core::ChewingKey) -> oxpinyin_core::KeyText,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2707-2727`).
    if key.is_null() {
        return false;
    }
    if !utf8_str.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *utf8_str = ptr::null_mut();
        }
    }

    // SAFETY: Non-null and produced by `pinyin_get_pinyin_key`.
    let slot = unsafe { &*key };
    let decoded = slot.to_core();
    abort_on_elements_outside_dims(decoded);
    if decoded.table_index() == 0 {
        return false;
    }
    abort_on_tone_outside_dims(decoded);
    if !utf8_str.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *utf8_str = key_text(render(decoded));
        }
    }
    true
}

/// Get the lookup offset from a user cursor position.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_offset(pinyin_instance_t * instance,
///                               size_t cursor,
///                               size_t * offset);
/// ```
///
/// The pin clamps the cursor to the parsed length, walks back to the
/// nearest non-empty matrix column, extends back over the zero-key run
/// before it, and validates (`pinyin.cpp:3008-3027`). A mid-syllable
/// cursor normalizes to the syllable start; the engine's validation error is
/// answered as `false`.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_offset(
    instance: *mut PinyinInstance,
    cursor: usize,
    offset: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // The pin clamps the cursor to the parsed length and reads
    // `get_column_size` at each position from there down. A parse that
    // consumed bytes but placed no key (apostrophes only) leaves the matrix
    // empty (`fill_matrix`, `phonetic_key_matrix.cpp:34-38`), and the first
    // read asserts (`phonetic_key_matrix.h:103`; register row 66).
    if inst
        .core
        .pin_matrix_columns()
        .is_ok_and(|columns| columns == 0)
        && cursor.min(inst.core.pin_parsed_len().unwrap_or(0)) >= 1
    {
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
    if inst
        .core
        .pin_matrix_columns()
        .is_ok_and(|columns| index >= columns)
    {
        std::process::abort();
    }
}

/// Get the left offset from a lookup offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_left_pinyin_offset(pinyin_instance_t * instance,
///                                    size_t offset,
///                                    size_t * left);
/// ```
///
/// Steps syllable-to-syllable: the start of the key ending at `offset`
/// (0 when no key ends there), zero-run-normalized and validated
/// (`pinyin.cpp:3029-3059`). Where the pin's validation asserts
/// (`pinyin.cpp:3035`, `:3057`, or `phonetic_key_matrix.h:103` past the
/// matrix) the process aborts (register row 14).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_left_pinyin_offset(
    instance: *mut PinyinInstance,
    offset: usize,
    left: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // `_check_offset` reads column `offset - 1` first (`pinyin.cpp:3035`).
    if offset >= 1 {
        abort_past_matrix(&inst.read(), offset - 1);
    }
    let result = match inst.core.left_offset(offset) {
        Ok(result) => result,
        // Register row 14: `assert(_check_offset(...))`, `pinyin.cpp:3035`,
        // `:3057`; an offset past the matrix asserts in its
        // `get_column_size` (`phonetic_key_matrix.h:103`).
        Err(
            oxpinyin_engine::EngineError::ZeroKeyOffsetCheck { .. }
            | oxpinyin_engine::EngineError::LookupOffsetOutOfRange { .. },
        ) => std::process::abort(),
        // A graph failure the pin has no counterpart for.
        Err(
            oxpinyin_engine::EngineError::CandidateIndexOutOfRange { .. }
            | oxpinyin_engine::EngineError::LookupOffsetPastSeparator { .. }
            | oxpinyin_engine::EngineError::LookupOffsetInsideCharacter { .. }
            | oxpinyin_engine::EngineError::SelectionAnchorBeforeComposition { .. }
            | oxpinyin_engine::EngineError::MatrixColumnAssert { .. }
            | oxpinyin_engine::EngineError::StaleTrainingConstraint { .. }
            | oxpinyin_engine::EngineError::UnsafeTrainingPredecessor { .. }
            | oxpinyin_engine::EngineError::ShortTrainingPredecessor { .. }
            | oxpinyin_engine::EngineError::ShortLookupPredecessor { .. }
            | oxpinyin_engine::EngineError::UnsafeLookupPredecessor { .. }
            | oxpinyin_engine::EngineError::ReadAfterCheck { .. }
            | oxpinyin_engine::EngineError::TrainingSetFreqMiss { .. }
            | oxpinyin_engine::EngineError::FailedUserBigramStore { .. }
            | oxpinyin_engine::EngineError::StaleTrainingSpan { .. }
            | oxpinyin_engine::EngineError::OverlongUserIndexKey { .. }
            | oxpinyin_engine::EngineError::TonedIncompleteKey
            | oxpinyin_engine::EngineError::ReduceTokensCount { .. }
            | oxpinyin_engine::EngineError::UserModel(_)
            | oxpinyin_engine::EngineError::Graph(_)
            | oxpinyin_engine::EngineError::Scoring(_),
        ) => {
            crate::ffi::log_warning("pinyin_get_left_pinyin_offset: the lookup failed");
            return false;
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
/// bool pinyin_get_right_pinyin_offset(pinyin_instance_t * instance,
///                                     size_t offset,
///                                     size_t * right);
/// ```
///
/// Steps syllable-to-syllable: the raw end of the first key starting at
/// `offset`, skipping a leading lone-zero-key run first
/// (`pinyin.cpp:3061-3094`). Answers `false` when no key starts there
/// (the pin's own graceful false, `pinyin.cpp:3085-3086`). Where the pin
/// asserts — its validation (`pinyin.cpp:3067`, `:3092`), a column past the
/// matrix, or any offset on an empty matrix (`phonetic_key_matrix.h:103`)
/// — the process aborts (register rows 14 and 66).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_right_pinyin_offset(
    instance: *mut PinyinInstance,
    offset: usize,
    right: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // The loop reads column `offset` and `_check_offset` column `offset - 1`
    // (`pinyin.cpp:3067`, `zhuyin.cpp:1556`).
    abort_past_matrix(&inst.read(), offset);
    let result = match inst.core.right_offset(offset) {
        Ok(Some(result)) => result,
        // The pin's own graceful `false`, `pinyin.cpp:3085-3086`, except
        // on an empty matrix, where the loop's `get_column_size` asserts
        // first.
        Ok(None) => {
            abort_on_empty_matrix(&inst.read());
            return false;
        }
        // Register row 14: `assert(_check_offset(...))`, `pinyin.cpp:3067`,
        // `:3092`, and the matrix's own index assert past its end.
        Err(
            oxpinyin_engine::EngineError::ZeroKeyOffsetCheck { .. }
            | oxpinyin_engine::EngineError::LookupOffsetOutOfRange { .. },
        ) => std::process::abort(),
        // A graph failure the pin has no counterpart for.
        Err(
            oxpinyin_engine::EngineError::CandidateIndexOutOfRange { .. }
            | oxpinyin_engine::EngineError::LookupOffsetPastSeparator { .. }
            | oxpinyin_engine::EngineError::LookupOffsetInsideCharacter { .. }
            | oxpinyin_engine::EngineError::SelectionAnchorBeforeComposition { .. }
            | oxpinyin_engine::EngineError::MatrixColumnAssert { .. }
            | oxpinyin_engine::EngineError::StaleTrainingConstraint { .. }
            | oxpinyin_engine::EngineError::UnsafeTrainingPredecessor { .. }
            | oxpinyin_engine::EngineError::ShortTrainingPredecessor { .. }
            | oxpinyin_engine::EngineError::ShortLookupPredecessor { .. }
            | oxpinyin_engine::EngineError::UnsafeLookupPredecessor { .. }
            | oxpinyin_engine::EngineError::ReadAfterCheck { .. }
            | oxpinyin_engine::EngineError::TrainingSetFreqMiss { .. }
            | oxpinyin_engine::EngineError::FailedUserBigramStore { .. }
            | oxpinyin_engine::EngineError::StaleTrainingSpan { .. }
            | oxpinyin_engine::EngineError::OverlongUserIndexKey { .. }
            | oxpinyin_engine::EngineError::TonedIncompleteKey
            | oxpinyin_engine::EngineError::ReduceTokensCount { .. }
            | oxpinyin_engine::EngineError::UserModel(_)
            | oxpinyin_engine::EngineError::Graph(_)
            | oxpinyin_engine::EngineError::Scoring(_),
        ) => {
            crate::ffi::log_warning("pinyin_get_right_pinyin_offset: the lookup failed");
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
/// Out-param `key` borrows the process-wide slot [`KEY_SLOT`], as the
/// pin's function-local `static` does: every instance shares it, the next
/// call from any instance overwrites it, and it outlives the instance.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_key(
    instance: *mut PinyinInstance,
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
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    let found = match inst.core.abi_key_at(offset, false) {
        Ok(Some(found)) => found,
        Ok(None) => {
            abort_on_empty_matrix_below(&inst.read(), offset);
            return false;
        }
        Err(_) => {
            crate::ffi::log_warning("pinyin_get_pinyin_key: zero-column assertion");
            return false;
        }
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

#[cfg(test)]
mod preedit_key_tests {
    use std::ptr;

    use crate::config::pinyin_set_options;
    use crate::parse::pinyin_parse_more_full_pinyins;
    use crate::test_support::{TempUserDir, cstr, key_slot_guard, open};
    use crate::types::{ChewingKey, ChewingKeyRest};

    const PARITY: u32 = 0x18a;

    /// The renderers allocate through GLib; `take_owned_cstr` frees with
    /// `g_free`, so a leak or a mismatched free would surface here rather
    /// than in a consumer.
    fn take(p: *mut crate::types::GChar) -> String {
        assert!(!p.is_null(), "renderer answered true with a NULL out-param");
        crate::ffi::take_owned_cstr(p)
    }

    /// S7: the key at offset 0 of a `nihao` parse under full-pinyin scheme 2 (luoma) and
    /// scheme 3 (secondary zhuyin), both under 0x18a after a sentence guess. The key is `ni`,
    /// packed 43, and both renderers spell it from its table index (`pinyin.cpp:2729-2749` at
    /// 074a2219, `storage/chewing_key.cpp:91-116`). Pin capture of 2026-11-07, runs 1 and 2
    /// identical, observations `scheme-key/luoma/nihao` and `scheme-key/secondary-zhuyin/nihao`.
    #[test]
    fn a_scheme_parse_keys_render_as_luoma_and_secondary_zhuyin() {
        let _slots = key_slot_guard();
        for scheme in [2, 3] {
            let user_dir = TempUserDir::new("scheme-key");
            let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
            assert!(pinyin_set_options(context, PARITY));
            assert!(crate::config::pinyin_set_full_pinyin_scheme(
                context, scheme
            ));
            let input = cstr("nihao");
            assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 5);
            assert!(crate::sentence::pinyin_guess_sentence(instance));
            let mut key: *mut ChewingKey = ptr::null_mut();
            assert!(super::pinyin_get_pinyin_key(instance, 0, &raw mut key));
            // SAFETY: a true answer points `key` at the instance's key slot.
            assert_eq!(unsafe { (*key).packed }, 43, "scheme {scheme}");
            let mut rendered: *mut crate::types::GChar = ptr::null_mut();
            assert!(crate::keys::pinyin_get_luoma_pinyin_string(
                instance,
                key,
                &raw mut rendered
            ));
            assert_eq!(take(rendered), "ni", "scheme {scheme}");
            assert!(crate::keys::pinyin_get_secondary_zhuyin_string(
                instance,
                key,
                &raw mut rendered
            ));
            assert_eq!(take(rendered), "ni", "scheme {scheme}");
            crate::instance::pinyin_free_instance(instance);
            crate::context::pinyin_fini(context);
        }
    }

    /// The expectation table established from the pin: `nihao` parses as
    /// `ni|hao`, so keys start at byte 0 and byte 2 and every other offset
    /// answers `false` — offsets 1/3/4 are empty mid-syllable columns and 5
    /// is the reserved slot (`offset >= matrix.size() - 1`).
    #[test]
    fn nihao_preedit_family_matches_the_pin_expectation_table() {
        let _slots = key_slot_guard();
        let user_dir = TempUserDir::new("preedit-nihao");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        assert!(pinyin_set_options(context, PARITY));
        let input = cstr("nihao");
        assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 5);

        let expected: [(usize, &str, u16, u16, &str, &str, &str); 2] = [
            (0, "ni", 0, 2, "n", "i", "ㄋㄧ"),
            (2, "hao", 2, 5, "h", "ao", "ㄏㄠ"),
        ];

        for (offset, pinyin, begin, end, shengmu, yunmu, zhuyin) in expected {
            let mut key: *mut ChewingKey = ptr::null_mut();
            assert!(
                super::pinyin_get_pinyin_key(instance, offset, &raw mut key),
                "offset {offset} starts a key"
            );
            assert!(!key.is_null());

            let mut rest: *mut ChewingKeyRest = ptr::null_mut();
            assert!(super::pinyin_get_pinyin_key_rest(
                instance,
                offset,
                &raw mut rest
            ));
            let (mut b, mut e) = (0_u16, 0_u16);
            assert!(super::pinyin_get_pinyin_key_rest_positions(
                instance, rest, &raw mut b, &raw mut e
            ));
            assert_eq!((b, e), (begin, end), "raw span at offset {offset}");

            let mut len = 0_u16;
            assert!(super::pinyin_get_pinyin_key_rest_length(
                instance,
                rest,
                &raw mut len
            ));
            assert_eq!(len, end - begin, "rest length at offset {offset}");

            let mut s: *mut crate::types::GChar = ptr::null_mut();
            assert!(super::pinyin_get_pinyin_string(instance, key, &raw mut s));
            assert_eq!(take(s), pinyin);

            let (mut sm, mut ym): (*mut crate::types::GChar, *mut crate::types::GChar) =
                (ptr::null_mut(), ptr::null_mut());
            assert!(super::pinyin_get_pinyin_strings(
                instance,
                key,
                &raw mut sm,
                &raw mut ym
            ));
            assert_eq!((take(sm), take(ym)), (shengmu.to_owned(), yunmu.to_owned()));

            let mut z: *mut crate::types::GChar = ptr::null_mut();
            assert!(super::pinyin_get_zhuyin_string(instance, key, &raw mut z));
            assert_eq!(take(z), zhuyin);
        }

        for offset in [1_usize, 3, 4, 5] {
            let mut key: *mut ChewingKey = ptr::null_mut();
            assert!(
                !super::pinyin_get_pinyin_key(instance, offset, &raw mut key),
                "offset {offset} starts no key"
            );
            assert!(key.is_null(), "a false answer nulls the out-param");
            let mut rest: *mut ChewingKeyRest = ptr::null_mut();
            assert!(!super::pinyin_get_pinyin_key_rest(
                instance,
                offset,
                &raw mut rest
            ));
        }

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }

    /// The pin's key and key-rest slots are function-local `static`s
    /// (`pinyin.cpp:2936`, `:2960`): one slot for the whole process, so a
    /// second instance's call overwrites what the first was handed, and the
    /// pointer survives the instance. The key-rest length is a `guint16`
    /// difference, which wraps on a reversed rest.
    #[test]
    fn the_key_slots_are_process_wide_and_the_rest_length_wraps() {
        let _slots = key_slot_guard();
        let user_dir = TempUserDir::new("preedit-static-slots");
        let (context, first) = open(user_dir.path.to_str().expect("UTF-8 path"));
        let second = crate::instance::pinyin_alloc_instance(context);
        assert_eq!(
            pinyin_parse_more_full_pinyins(first, cstr("ni").as_ptr()),
            2
        );
        assert_eq!(
            pinyin_parse_more_full_pinyins(second, cstr("hao").as_ptr()),
            3
        );

        let (mut a, mut b): (*mut ChewingKey, *mut ChewingKey) = (ptr::null_mut(), ptr::null_mut());
        assert!(super::pinyin_get_pinyin_key(first, 0, &raw mut a));
        // SAFETY: `a` is the slot the call above handed out.
        let ni = unsafe { *a };
        assert!(super::pinyin_get_pinyin_key(second, 0, &raw mut b));
        assert_eq!(a, b, "one slot for every instance");
        // SAFETY: as above.
        assert_ne!(unsafe { *a }, ni, "the second call overwrote the first");

        let (mut ra, mut rb): (*mut ChewingKeyRest, *mut ChewingKeyRest) =
            (ptr::null_mut(), ptr::null_mut());
        assert!(super::pinyin_get_pinyin_key_rest(first, 0, &raw mut ra));
        assert!(super::pinyin_get_pinyin_key_rest(second, 0, &raw mut rb));
        assert_eq!(ra, rb);

        let mut length = 0_u16;
        let mut reversed = ChewingKeyRest { begin: 5, end: 2 };
        assert!(super::pinyin_get_pinyin_key_rest_length(
            first,
            &raw mut reversed,
            &raw mut length
        ));
        assert_eq!(length, 65533, "end - begin in guint16");

        crate::instance::pinyin_free_instance(first);
        crate::instance::pinyin_free_instance(second);
        // SAFETY: the slot is a `static`: the pointer outlives the instances.
        assert_ne!(unsafe { *a }, ni);
        crate::context::pinyin_fini(context);
    }

    /// Past byte 65535 the key-rest positions narrow into the pin's
    /// `guint16` fields mod 2^16 (maintainer ruling 2026-11-01 UTC,
    /// `docs/findings/1501-guint16-key-positions.md`). `ni3` then `ni`
    /// repeated puts a key at 65535..65537, which crosses byte 65536, and
    /// one at 65537..65539, wholly past it; both lengths stay 2.
    #[test]
    fn key_rest_positions_past_byte_65535_wrap() {
        use crate::config::pinyin_set_double_pinyin_scheme;
        use crate::parse::pinyin_parse_more_double_pinyins;

        let _slots = key_slot_guard();
        let user_dir = TempUserDir::new("key-rest-wrap-double");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        assert!(pinyin_set_options(context, 1 << 5));
        assert!(pinyin_set_double_pinyin_scheme(context, 2));
        let input = format!("ni3{}", "ni".repeat(32_769));
        assert_eq!(
            pinyin_parse_more_double_pinyins(instance, cstr(&input).as_ptr()),
            65_541
        );

        for (offset, begin, end) in [(65_535_usize, 65_535_u16, 1_u16), (65_537, 1, 3)] {
            let mut rest: *mut ChewingKeyRest = ptr::null_mut();
            assert!(super::pinyin_get_pinyin_key_rest(
                instance,
                offset,
                &raw mut rest
            ));
            let (mut b, mut e) = (0_u16, 0_u16);
            assert!(super::pinyin_get_pinyin_key_rest_positions(
                instance, rest, &raw mut b, &raw mut e
            ));
            assert_eq!((b, e), (begin, end), "offset {offset}");
            let mut length = 0_u16;
            assert!(super::pinyin_get_pinyin_key_rest_length(
                instance,
                rest,
                &raw mut length
            ));
            assert_eq!(length, 2, "offset {offset}");
        }

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}
