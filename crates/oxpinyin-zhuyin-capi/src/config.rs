//! Configuration symbols: options, schemes, phrase library loading.

use std::os::raw::c_int;

use crate::state::context_mut;
use crate::types::{PinyinOptionT, ZhuyinContext};

/// `PHRASE_INDEX_LIBRARY_COUNT` (`phrase_index.h`): sixteen sub-indices.
const PHRASE_INDEX_LIBRARY_COUNT: u8 = 16;

/// `TSI_DICTIONARY` (`novel_types.h`): the one default library that stays.
const TSI_DICTIONARY: u8 = 1;

/// Set the zhuyin scheme.
///
/// # C signature
/// ```c
/// bool zhuyin_set_chewing_scheme(zhuyin_context_t * context,
///                                ZhuyinScheme scheme);
/// ```
///
/// The Rust parameter is `c_int`: callers may pass any `int`.
/// Every implemented Zhuyin keyboard is table-driven. The
/// `ZHUYIN_STANDARD_DVORAK` (7) slot (`zhuyin_parser2.cpp:295`) and every
/// out-of-enum value (`zhuyin.cpp:736`) abort the process, as upstream does
/// (register rows 5c and 5d).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_set_chewing_scheme(context: *mut ZhuyinContext, scheme: c_int) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_mut(context) };
    if !matches!(scheme, 1 | 2 | 3 | 4 | 5 | 6 | 8 | 9) {
        // Register rows 5c and 5d: the dvorak slot (7) falls through to
        // `zhuyin_parser2.cpp:295`, every out-of-enum value to
        // `zhuyin.cpp:736`, both `abort()`, with the old parser already
        // deleted in memory (`:710-711`).
        std::process::abort();
    }
    ctx.core.live.zhuyin_scheme.set(scheme);
    true
}

/// Set the full pinyin scheme.
///
/// # C signature
/// ```c
/// bool zhuyin_set_full_pinyin_scheme(zhuyin_context_t * context,
///                                    FullPinyinScheme scheme);
/// ```
///
/// HANYU (1), LUOMA (2) and `SECONDARY_ZHUYIN` (3) are the schemes; any other
/// value aborts the process, as `set_scheme`'s `default: abort()` does
/// (`pinyin_parser2.cpp:398` via `zhuyin.cpp:704`, register row 5d).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_set_full_pinyin_scheme(
    context: *mut ZhuyinContext,
    scheme: c_int,
) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_mut(context) };
    if !matches!(scheme, 1..=3) {
        // Register row 5d, `pinyin_parser2.cpp:398`: `abort()` in
        // `FullPinyinParser2::set_scheme`, after `zhuyin.cpp:703` stored the
        // scheme in memory.
        std::process::abort();
    }
    ctx.core.live.full_scheme.set(scheme);
    true
}

/// `zhuyin_set_options` — copies the caller's option word.
///
/// # C signature
/// ```c
/// bool zhuyin_set_options(zhuyin_context_t * context,
///                         pinyin_option_t options);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_set_options(context: *mut ZhuyinContext, options: PinyinOptionT) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_mut(context) };
    // The shared set_options law (stored word and incomplete setting);
    // the pinyin facade's setter runs the same body.
    ctx.core.set_options(options);
    true
}

/// `zhuyin_mask_out`.
///
/// # C signature
/// ```c
/// bool zhuyin_mask_out(zhuyin_context_t * context,
///                      phrase_token_t mask,
///                      phrase_token_t value);
/// ```
///
/// The user table's `mask_out` walks every `user_pinyin_index.bin` record
/// and dies of SIGABRT at the `switch` whose `default` is `abort()` when a
/// key is past `MAX_PHRASE_LENGTH` syllables
/// (`chewing_large_table2_bdb.cpp:529`), and the user bigram's mask asserts
/// on a key that is not a token (`ngram_bdb.cpp:199`), on a gram too short
/// for its total (`memory_chunk.h:390`) and on a gram it leaves with a
/// residual total (`ngram.cpp:70`); the reload of each loaded system
/// library then asserts on one that is a directory (`memory_chunk.h:493`).
/// The process aborts at each (register rows 84, 85, 86, 87, 94 and 96).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_mask_out(context: *mut ZhuyinContext, mask: u32, value: u32) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`;
    // the unique borrow lasts only for the mask call.
    let ctx = unsafe { context_mut(context) };
    let outcome = ctx.core.mask_out(mask, value);
    match outcome {
        oxpinyin_facade::MaskOutOutcome::Done(answer) => answer,
        // Register rows 84 and 94, `chewing_large_table2_bdb.cpp:529` (kc
        // `:499`, tkrzw `:466`): the user pinyin index carries a key past
        // MAX_PHRASE_LENGTH syllables, or one with no whole syllable, and
        // the pin's `mask_out` switch falls to `default: abort()`.
        oxpinyin_facade::MaskOutOutcome::OverlongIndexKey => std::process::abort(),
        // Register row 86, `ngram_bdb.cpp:199` (kc `:173`, tkrzw `:150`):
        // the user bigram carries a key that is not a phrase_token_t and
        // the pin's `get_all_items` assert dies.
        oxpinyin_facade::MaskOutOutcome::NonTokenUserBigramKey => std::process::abort(),
        // Register row 85, `memory_chunk.h:390` through `ngram.cpp:80`:
        // the user bigram's mask loads a gram too short for its total and
        // the pin's `get_total_freq` assert dies.
        oxpinyin_facade::MaskOutOutcome::ShortUserBigramValue => std::process::abort(),
        oxpinyin_facade::MaskOutOutcome::PartialUserBigramTail => {
            // Class (b), register row 98: a partial item tail makes
            // `SingleGram::mask_out` over-read (`ngram.cpp:76-86`). One
            // warning, as before.
            crate::ffi::log_warning(
                "zhuyin_mask_out: malformed user bigram value \
                 (upstream asserts on a missing total or reads past a partial item; \
                 memory_chunk.h:390, ngram.cpp:76-86)",
            );
            false
        }
        // Register row 87, `ngram.cpp:70`: masking a user bigram removed
        // every item of a gram whose total its items did not cover, and the
        // pin's `get_length` assert dies (`Bigram::mask_out`,
        // `ngram_bdb.cpp:243`).
        oxpinyin_facade::MaskOutOutcome::ResidualUserBigramGram => std::process::abort(),
        // Register rows 91, 92, 97 and 108: a reloaded system library's
        // `.dbin` reached an assertion in `merge_with_mask`
        // (`phrase_index_logger.h:202`, `phrase_index.cpp:745`,
        // `phrase_index_logger.h:245`/`:257`, `memory_chunk.h:390`), after
        // the user tables were masked; the pin dies there.
        oxpinyin_facade::MaskOutOutcome::Journal(_) => std::process::abort(),
        // Register row 96, `memory_chunk.h:493`: after the user tables, the
        // mask maps each loaded system library again and one is a directory.
        oxpinyin_facade::MaskOutOutcome::ChunkHeaderRead => std::process::abort(),
        // `copy_tkrzwdb` / `copy_bdb` assert on a failed destination write
        // or source walk (`tkrzwdb_utils.h:65`, `:72`; `bdb_utils.h:61`,
        // `:67`). The mask does not copy, so it never reaches here.
        oxpinyin_facade::MaskOutOutcome::FatalCopy => std::process::abort(),
    }
}

/// Load a default phrase library by index.
///
/// # C signature
/// ```c
/// bool zhuyin_load_phrase_library(zhuyin_context_t * context,
///                                 guint8 index);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_load_phrase_library(context: *mut ZhuyinContext, index: u8) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { crate::state::context_mut(context) };
    // `zhuyin.cpp:372`: the pin `assert`s that the row of a loaded library
    // is a `SYSTEM_FILE` or `USER_FILE` (an index of 16 or more answers
    // `false` first, `:362-363`); which rows are is the context's
    // `table.conf`. The facade answers that before it maps or logs
    // anything, and the process aborts (register rows 66 and 79).
    let outcome = ctx.core.load_phrase_library(u32::from(index));
    if outcome
        == Err(oxpinyin_facade::LibraryLoadFault::Row(
            oxpinyin_facade::LibraryRowAssert::NotLoadable,
        ))
    {
        std::process::abort();
    }
    match outcome {
        Ok(loaded) => loaded,
        // Register row 96, `memory_chunk.h:493`: the library being loaded
        // again is a directory the pin maps and asserts on, before anything
        // changes.
        Err(oxpinyin_facade::LibraryLoadFault::Row(
            oxpinyin_facade::LibraryRowAssert::ChunkHeaderRead,
        )) => std::process::abort(),
        // Register rows 91 and 108: the reloaded library's saved `.dbin`
        // merge reached a pin assertion (`zhuyin.cpp:219`, then
        // `phrase_index_logger.h:202` or `memory_chunk.h:390`).
        Err(oxpinyin_facade::LibraryLoadFault::Journal(_)) => std::process::abort(),
        // `NotLoadable` aborted above; the facade answers no other row
        // fault here.
        Err(oxpinyin_facade::LibraryLoadFault::Row(
            oxpinyin_facade::LibraryRowAssert::NotLoadable
            | oxpinyin_facade::LibraryRowAssert::NotDictionary
            | oxpinyin_facade::LibraryRowAssert::NullChunkName,
        )) => false,
    }
}

/// Unload a default phrase library by index.
///
/// # C signature
/// ```c
/// bool zhuyin_unload_phrase_library(zhuyin_context_t * context,
///                                   guint8 index);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_unload_phrase_library(context: *mut ZhuyinContext, index: u8) -> bool {
    if index >= PHRASE_INDEX_LIBRARY_COUNT {
        // Register row 66, `zhuyin.cpp:381`: `assert`, before the context is
        // read, so a NULL context aborts as well.
        std::process::abort();
    }
    // `tsi.bin` can't be unloaded (`zhuyin.cpp:383-385`), and the refusal
    // precedes the context read at `:388`; any other library is unloaded,
    // and the pin ignores what `unload` returns.
    if index == TSI_DICTIONARY {
        return false;
    }
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { crate::state::context_mut(context) };
    // `FacadePhraseIndex::unload` is in memory (policy row 103): the
    // session drops the library and its visibility mask sets, with nothing
    // written and nothing that can fail.
    ctx.core.unload_phrase_library(index)
}
