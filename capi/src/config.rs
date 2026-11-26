//! Context options, input schemes, phrase library loading, and token masking.

use std::os::raw::c_int;

use crate::ffi::borrow_or_return;
use crate::state::context_mut;
use crate::types::{PinyinContext, PinyinOptionT};
use oxpinyin::vocab::{GBK_DICTIONARY, PHRASE_INDEX_LIBRARY_COUNT};

// pinyin.cpp:1299-1306.
/// Set pinyin options on the context.
///
/// # C signature
/// ```c
/// bool pinyin_set_options(pinyin_context_t * context, pinyin_option_t options);
/// ```
///
/// Replace the live context option word. Existing instances read the new options on subsequent
/// operations. Setting options does not reparse input or save a profile. Returns false for a
/// null context and true after setting the word.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_set_options(
    context: *mut PinyinContext,
    options: PinyinOptionT,
) -> bool {
    let ctx = borrow_or_return!(context, context_mut, false);
    // The shared set_options law (stored word and incomplete setting);
    // the zhuyin facade's setter runs the same body. Bits without a flag are kept.
    ctx.core
        .set_options(oxpinyin::OptionBits::from_bits_retain(options));
    true
}

/// Set the full pinyin scheme.
///
/// # C signature
/// ```c
/// bool pinyin_set_full_pinyin_scheme(pinyin_context_t * context,
///                                    FullPinyinScheme scheme);
/// ```
///
/// Select HANYU (1), LUOMA (2), or SECONDARY_ZHUYIN (3) for subsequent parses. Returns false for
/// a null context and true after selection.
///
/// # Aborts
///
/// Any other scheme on a live context aborts the process (`pinyin_parser2.cpp:398`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_set_full_pinyin_scheme(
    context: *mut PinyinContext,
    scheme: c_int,
) -> bool {
    let ctx = borrow_or_return!(context, context_mut, false);
    if !matches!(scheme, 1..=3) {
        // Register row 5d, `pinyin_parser2.cpp:398`: `set_scheme`'s
        // `default:` is an `abort()`.
        std::process::abort();
    }
    ctx.core.live.full_scheme.set(scheme);
    true
}

// pinyin_custom2.h:108-117, pinyin.cpp:1155–1159.
/// Set the double pinyin scheme.
///
/// # C signature
/// ```c
/// bool pinyin_set_double_pinyin_scheme(pinyin_context_t * context,
///                                      DoublePinyinScheme scheme);
/// ```
///
/// Select the double-pinyin scheme on a live context and return true. An unrecognised value
/// other than DOUBLE_PINYIN_CUSTOMIZED keeps the previously selected scheme, clears its fallback
/// table and returns true (`pinyin_parser2.cpp:580,614`). A null context returns false.
///
/// # Aborts
///
/// DOUBLE_PINYIN_CUSTOMIZED (30) aborts on a live context (`pinyin_parser2.cpp:611`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_set_double_pinyin_scheme(
    context: *mut PinyinContext,
    scheme: c_int,
) -> bool {
    let ctx = borrow_or_return!(context, context_mut, false);
    let scheme = match oxpinyin::parser::DoublePinyinScheme::try_from(scheme) {
        // CUSTOMIZED (30): register row 5a, `pinyin_parser2.cpp:611`
        // `abort()`. The `m_fallback_table = NULL` before it (`:580`) is
        // in memory only.
        Ok(oxpinyin::parser::DoublePinyinScheme::Customized) => std::process::abort(),
        // Out-of-enum: upstream's unconditional `m_fallback_table = NULL`
        // (`:580`) fires before the switch falls through to `return false`
        // (`:614`), and the wrapper ignores that `false` and answers `true`
        // (`pinyin.cpp:1155-1159`).  Set the packed fallback-cleared bit.
        Err(_) => {
            let live = &ctx.core.live.double_scheme;
            live.set(live.get() | oxpinyin::facade::FALLBACK_CLEARED_BIT);
            return true;
        }
        Ok(scheme) => scheme,
    };
    // A valid scheme (1-6) never has bit 30, so this store clears the
    // fallback-cleared flag implicitly.
    ctx.core.live.double_scheme.set(i32::from(scheme));
    true
}

// zhuyin_parser2.cpp:295, pinyin.cpp:1189.
/// Set the zhuyin scheme.
///
/// # C signature
/// ```c
/// bool pinyin_set_zhuyin_scheme(pinyin_context_t * context,
///                                ZhuyinScheme scheme);
/// ```
///
/// Select the keyboard for subsequent chewing input. Returns false for a null context and true
/// after selection.
///
/// # Aborts
///
/// On a live context, STANDARD_DVORAK (7) or a value outside the supported keyboard schemes
/// aborts the process.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_set_zhuyin_scheme(
    context: *mut PinyinContext,
    scheme: c_int,
) -> bool {
    let ctx = borrow_or_return!(context, context_mut, false);
    if !matches!(scheme, 1 | 2 | 3 | 4 | 5 | 6 | 8 | 9) {
        // Register rows 5c and 5d: `pinyin.cpp:1189` (`default:`) and, for
        // the `STANDARD_DVORAK` slot, `zhuyin_parser2.cpp:295`; both are
        // `abort()`, with the old parser already deleted in memory.
        std::process::abort();
    }
    ctx.core.live.zhuyin_scheme.set(scheme);
    true
}

/// Load an addon phrase library by index.
///
/// # C signature
/// ```c
/// bool pinyin_load_addon_phrase_library(pinyin_context_t * context,
///                                       guint8 index);
/// ```
///
/// Load the chunk named by the configured addon row and return true. Returns false for a null
/// context, an out-of-range index, an unused (NOT_USED) row, an already loaded library, or a
/// library file that cannot be loaded.
///
/// # Aborts
///
/// On a live context, selecting an addon row whose type is not DICTIONARY (a SYSTEM_FILE or
/// USER_FILE row) aborts the process, as libpinyin asserts (`pinyin.cpp:491`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_load_addon_phrase_library(
    context: *mut PinyinContext,
    index: u8,
) -> bool {
    let ctx = borrow_or_return!(context, context_mut, false);
    let result = ctx.load_addon(index);
    addon_load_answer(result)
}

fn addon_load_answer(result: Result<bool, &'static str>) -> bool {
    match result {
        Ok(loaded) => loaded,
        Err(warning) => {
            crate::ffi::log_warning(warning);
            false
        }
    }
}

// pinyin.cpp:124-131.
/// Unload an addon phrase library.
///
/// # C signature
/// ```c
/// bool pinyin_unload_addon_phrase_library(pinyin_context_t * context,
///                                         guint8 index);
/// ```
///
/// Returns true on a live context even if the library was absent. A null context returns false
/// for an in-range index.
///
/// # Aborts
///
/// An index of 16 or greater aborts, even with a null context (`pinyin.cpp:499`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_unload_addon_phrase_library(
    context: *mut PinyinContext,
    index: u8,
) -> bool {
    if index >= PHRASE_INDEX_LIBRARY_COUNT {
        // `pinyin.cpp:499`: `assert`, before the context is read at `:502`.
        std::process::abort();
    }
    let ctx = borrow_or_return!(context, context_mut, false);
    // In range, the pin answers an unconditional `true`.
    ctx.core.runtime.unload_system_addon(index)
}

// pinyin.cpp:457.
/// Load a default phrase library by index.
///
/// # C signature
/// ```c
/// bool pinyin_load_phrase_library(pinyin_context_t * context,
///                                 guint8 index);
/// ```
///
/// Load the configured library from its system and saved user data. Changes dropped by an unload
/// are lost unless saved beforehand.
///
/// Returns false for a null context, an out-of-range index or an already loaded library. A
/// successful reload returns true. When the file of an unloaded system library cannot be mapped
/// again, the call can return true without making the library visible: as in libpinyin, the
/// result does not report the mapping. A later load can retry.
///
/// # Aborts
///
/// Selecting an in-range row other than SYSTEM_FILE or USER_FILE on a live context aborts.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_load_phrase_library(
    context: *mut PinyinContext,
    index: u8,
) -> bool {
    let ctx = borrow_or_return!(context, crate::state::context_mut, false);
    // `pinyin.cpp:457`: the pin `assert`s that the row of a loaded library
    // is a `SYSTEM_FILE` or `USER_FILE` (an index of 16 or more answers
    // `false` first, `:448`); which rows are is the context's `table.conf`.
    // The facade checks the row before it maps or logs anything.
    match ctx.core.load_phrase_library(u32::from(index)) {
        Ok(loaded) => loaded,
        // Register rows 91 and 108: the reloaded library's saved `.dbin`
        // merge reached a pin assertion (`pinyin.cpp:276`, then
        // `phrase_index_logger.h:202` or `memory_chunk.h:390`).
        Err(oxpinyin::facade::LibraryLoadFault::Journal(_)) => std::process::abort(),
    }
}

// pinyin.cpp:464-472.
/// Unload a default phrase library by index.
///
/// # C signature
/// ```c
/// bool pinyin_unload_phrase_library(pinyin_context_t * context,
///                                   guint8 index);
/// ```
///
/// Only GBK can be unloaded. On a live context an accepted index returns true even if already
/// unloaded. Unloading drops unsaved item changes. A refused index or null context returns
/// false.
///
/// # Aborts
///
/// An index of 16 or greater aborts, even with a null context.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_unload_phrase_library(
    context: *mut PinyinContext,
    index: u8,
) -> bool {
    if index >= PHRASE_INDEX_LIBRARY_COUNT {
        // Register row 21, `pinyin.cpp:466`: `assert`, before the context is
        // read, so a NULL context aborts as well.
        std::process::abort();
    }
    // Only the GBK table can be unloaded (`pinyin.cpp:468-470`), and the
    // refusal precedes the context read at `:473`.
    if index != GBK_DICTIONARY {
        return false;
    }
    let ctx = borrow_or_return!(context, crate::state::context_mut, false);
    // `FacadePhraseIndex::unload` is in memory and the pin answers `true`
    // (policy row 103): the session drops the library and its visibility
    // mask sets, with nothing written and nothing that can fail.
    ctx.core.unload_phrase_library(index)
}

/// Remove entries that match a phrase-token mask.
///
/// # C signature
/// ```c
/// bool pinyin_mask_out(pinyin_context_t * context,
///                      phrase_token_t mask,
///                      phrase_token_t value);
/// ```
///
/// Applying the mask removes bigram entries, unigram deltas, and user
/// phrases whose tokens satisfy `(token & mask) == value`. A matching
/// predecessor removes its whole gram (`pinyin.cpp:1224-1295`,
/// `ngram_kyotodb.cpp:199`, `phrase_index.cpp:689`). In the pin, applying
/// the mask leaves `m_modified` as it was. The pin sets that flag in
/// [`crate::candidates::pinyin_train`] and [`crate::iterators::pinyin_end_add_phrases`].
///
/// Returns `false` for a null context or a store failure.
///
/// If a loaded system library cannot be remapped, the function writes a
/// diagnostic to `stderr` and returns `true` without applying the mask.
/// No matching entries are removed. Unsaved changes remain in memory.
///
/// The user table's `mask_out` walks every `user_pinyin_index.bin` record
/// and dies of SIGABRT at the `switch` whose `default` is `abort()` when a
/// key is past `MAX_PHRASE_LENGTH` syllables
/// (`chewing_large_table2_bdb.cpp:529`), and the user bigram's mask asserts
/// on a key that is not a token (`ngram_bdb.cpp:199`), on a gram too short
/// for its total (`memory_chunk.h:390`) and on a gram it leaves with a
/// residual total (`ngram.cpp:70`); the reload of each loaded system
/// library then asserts on one that is a directory (`memory_chunk.h:493`).
/// The process aborts at each.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_mask_out(
    context: *mut PinyinContext,
    mask: u32,
    value: u32,
) -> bool {
    let ctx = borrow_or_return!(context, context_mut, false);
    let outcome = ctx.core.mask_out(mask, value);
    match outcome {
        oxpinyin::facade::MaskOutOutcome::Done(answer) => answer,
        // Register rows 84 and 94, `chewing_large_table2_bdb.cpp:529` (kc
        // `:499`, tkrzw `:466`): the user pinyin index carries a key past
        // MAX_PHRASE_LENGTH syllables, or one with no whole syllable, and
        // the pin's `mask_out` switch falls to `default: abort()`.
        oxpinyin::facade::MaskOutOutcome::OverlongIndexKey => std::process::abort(),
        // Register row 86, `ngram_bdb.cpp:199` (kc `:173`, tkrzw `:150`):
        // the user bigram carries a key that is not a phrase_token_t and
        // the pin's `get_all_items` assert dies.
        oxpinyin::facade::MaskOutOutcome::NonTokenUserBigramKey => std::process::abort(),
        // Register row 85, `memory_chunk.h:390` through `ngram.cpp:80`:
        // the user bigram's mask loads a gram too short for its total and
        // the pin's `get_total_freq` assert dies.
        oxpinyin::facade::MaskOutOutcome::ShortUserBigramValue => std::process::abort(),
        oxpinyin::facade::MaskOutOutcome::PartialUserBigramTail => {
            // Class (b), register row 98: a partial item tail makes
            // `SingleGram::mask_out` over-read (`ngram.cpp:76-86`). One
            // warning, as before.
            crate::ffi::log_warning(
                "pinyin_mask_out: malformed user bigram value \
                 (upstream asserts on a missing total or reads past a partial item; \
                 memory_chunk.h:390, ngram.cpp:76-86)",
            );
            false
        }
        // Register row 87, `ngram.cpp:70`: masking a user bigram removed
        // every item of a gram whose total its items did not cover, and the
        // pin's `get_length` assert dies (`Bigram::mask_out`,
        // `ngram_bdb.cpp:243`).
        oxpinyin::facade::MaskOutOutcome::ResidualUserBigramGram => std::process::abort(),
        // Register rows 91, 92, 97 and 108: a reloaded system library's
        // `.dbin` reached an assertion in `merge_with_mask`
        // (`phrase_index_logger.h:202`, `phrase_index.cpp:745`,
        // `phrase_index_logger.h:245`/`:257`, `memory_chunk.h:390`), after
        // the user tables were masked; the pin dies there.
        oxpinyin::facade::MaskOutOutcome::Journal(_) => std::process::abort(),
        // Register row 96, `memory_chunk.h:493`: after the user tables, the
        // mask maps each loaded system library again and one is a directory.
        oxpinyin::facade::MaskOutOutcome::ChunkHeaderRead => std::process::abort(),
    }
}

#[cfg(test)]
mod tests;
