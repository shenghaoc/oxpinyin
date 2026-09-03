//! Parsing symbols: full pinyin, double pinyin, chewing.

use std::os::raw::c_char;
use std::ptr;

use std::sync::atomic::Ordering;

use oxpinyin_core::graph::ExactSegment;
use oxpinyin_core::{
    DoublePinyinKey, DoublePinyinScheme, FullPinyinScheme, SyllableKey, USE_TONE,
    ZHUYIN_INCOMPLETE, ZhuyinKey, ZhuyinScheme,
};

use crate::ffi::{cstr_to_string, ffi_catch};
use crate::state::{instance_mut, instance_ref};
use crate::types::{GChar, PinyinInstance};

/// Shared batch-parse path: reset the instance, type `text`, store the
/// parsed prefix length, and return it.
///
/// The getter must return this snapshot, not a length recomputed from the
/// current session: the fork compares `pinyin_choose_candidate`'s cursor
/// against it (`docs/findings/abi-subset.md` W8 contract).
///
/// The stored length is the session's filtered `fewest_keys` prefix, so
/// `PINYIN_INCOMPLETE` off reports 2 for `nih` rather than the unfiltered
/// graph `consumed()` of 3.
///
/// Resets and clears the candidate snapshot even for empty input, so a
/// prior composition is discarded and the stored length returns to 0.
pub(crate) const fn full_scheme(value: i32) -> Option<FullPinyinScheme> {
    match value {
        1 => Some(FullPinyinScheme::Hanyu),
        2 => Some(FullPinyinScheme::Luoma),
        3 => Some(FullPinyinScheme::SecondaryZhuyin),
        _ => None,
    }
}

fn parse_more(instance: *mut PinyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    inst.begin_parse(text.as_bytes());
    if inst.session.set_options(inst.options()).is_err() {
        return 0;
    }
    if text.is_empty() {
        return 0;
    }

    // LUOMA / SECONDARY_ZHUYIN: parse the raw input through the
    // scheme's pinned index, store the parse for aux rendering, and
    // drive the session with the canonical spellings — the same
    // transformed-spelling seam the double and chewing paths use.
    if let Some(scheme) = full_scheme(inst.full_scheme.load(Ordering::Relaxed))
        && let Some(index) = scheme.index()
    {
        let use_tone = inst.options().contains(USE_TONE);
        let parsed = oxpinyin_core::parse_full_pinyin_index(text.as_bytes(), use_tone, index);
        let full = parsed.full_pinyin();
        if !full.is_empty() && inst.session.replace_raw(&full).is_err() {
            return 0;
        }
        inst.parsed_len = parsed.consumed();
        text.clone_into(&mut inst.full_input);
        inst.full_parse = Some(parsed);
        return inst.parsed_len;
    }

    let consumed = match inst.session.replace_raw(text) {
        Ok(()) => inst.session.full_parsed_len(),
        Err(_) => 0,
    };
    inst.parsed_len = consumed;
    consumed
}

pub(crate) const fn double_scheme(value: i32) -> Option<DoublePinyinScheme> {
    match value {
        1 => Some(DoublePinyinScheme::Zrm),
        2 => Some(DoublePinyinScheme::Ms),
        3 => Some(DoublePinyinScheme::Ziguang),
        4 => Some(DoublePinyinScheme::Abc),
        5 => Some(DoublePinyinScheme::Pyjj),
        6 => Some(DoublePinyinScheme::Xhe),
        _ => None,
    }
}

/// Double-pinyin batch-parse path.
///
/// Parses the original double-pinyin input into full-pinyin `SyllableKey`s,
/// stores the original bytes and key spans for aux/candidate-offset mapping,
/// and drives the existing session decoder with the `'`-joined full-pinyin
/// spelling. The returned length is the original consumed byte count, not the
/// transformed spelling length.
///
/// The caller's full option word crosses the seam (the pin's
/// `options = context->m_options` at `src/pinyin.cpp:1543`), so
/// `FORCE_TONE`'s length-3 gate and `USE_TONE`'s tone carriage are honoured
/// by the batch law — the frozen double-pinyin SPEC's Tone section
/// (`docs/findings/double-pinyin-spec.md`).
fn parse_double_more(instance: *mut PinyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    inst.begin_parse(text.as_bytes());

    let Some(scheme) = double_scheme(inst.double_scheme.load(Ordering::Relaxed)) else {
        return 0;
    };
    let allow_incomplete = inst.incomplete.load(Ordering::Relaxed);
    let parser = oxpinyin_core::DoublePinyinParser::with_scheme(scheme);
    let parsed = parser.parse_with_options(text.as_bytes(), inst.options().bits());

    if text.is_empty() {
        inst.parsed_len = 0;
        return 0;
    }

    if inst
        .session
        .set_incomplete_pinyin(allow_incomplete)
        .is_err()
    {
        return 0;
    }

    let (full, segments) = exact_input(parsed.keys());
    if !full.is_empty() && inst.session.replace_raw_exact(&full, &segments).is_err() {
        return 0;
    }

    inst.parsed_len = parsed.consumed();
    text.clone_into(&mut inst.double_input);
    inst.double_parse = Some(parsed);
    inst.parsed_len
}

pub(crate) const fn zhuyin_scheme(value: i32) -> Option<ZhuyinScheme> {
    match value {
        1 => Some(ZhuyinScheme::Standard),
        2 => Some(ZhuyinScheme::Hsu),
        3 => Some(ZhuyinScheme::Ibm),
        4 => Some(ZhuyinScheme::Ginyieh),
        5 => Some(ZhuyinScheme::Eten),
        6 => Some(ZhuyinScheme::Eten26),
        // Total over the header enum: the parser supports every layout
        // even though `pinyin_set_zhuyin_scheme` refuses 7 today (the
        // `ZHUYIN_STANDARD_DVORAK` upstream abort-slot); should that
        // guard ever open, the parse seam already answers here.
        7 => Some(ZhuyinScheme::StandardDvorak),
        8 => Some(ZhuyinScheme::HsuDvorak),
        9 => Some(ZhuyinScheme::DachenCp26),
        _ => None,
    }
}

/// Zhuyin batch-parse path.
///
/// Parses STANDARD keyboard input into tone-less full-pinyin `SyllableKey`s,
/// stores the original keystrokes and key spans for aux/candidate-offset
/// mapping, and drives the session decoder with the `'`-joined full-pinyin
/// spelling. The returned length is the original consumed byte count.
fn parse_chewing_more(instance: *mut PinyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    inst.begin_parse(text.as_bytes());

    let Some(scheme) = zhuyin_scheme(inst.zhuyin_scheme.load(Ordering::Relaxed)) else {
        return 0;
    };
    let use_tone = inst.use_tone.load(Ordering::Relaxed);
    // Upstream passes the caller's option word through after stripping the
    // parser-owned corrections (`pinyin.cpp:1589`, inside
    // `pinyin_parse_more_chewings`; all cites at pin `0c5e80e1`).
    // Three caller bits reach the parsers:
    // `USE_TONE` and `FORCE_TONE` (`zhuyin_parser2.cpp:171-179` for Simple,
    // `:373`/`:387` for Discrete, `:595-605` for CP26) and
    // `ZHUYIN_INCOMPLETE` (`:49-51`). This seam carries `USE_TONE` and
    // `ZHUYIN_INCOMPLETE`; `FORCE_TONE` does not cross yet — the open item
    // in `docs/findings/upstream-divergences.md` (FORCE_TONE entry,
    // pinyin-facade chewing batch seam). The libzhuyin facade forwards the
    // whole word through `ZhuyinParser::parse_with_options`.
    let allow_incomplete = inst.options().contains(ZHUYIN_INCOMPLETE);
    let parser = oxpinyin_core::ZhuyinParser::with_scheme(scheme);
    let parsed = parser.parse(text.as_bytes(), use_tone, allow_incomplete);

    if text.is_empty() {
        inst.parsed_len = 0;
        return 0;
    }

    let (full, segments) = exact_input(parsed.keys());
    if !full.is_empty() && inst.session.replace_raw_exact(&full, &segments).is_err() {
        return 0;
    }

    inst.parsed_len = parsed.consumed();
    text.clone_into(&mut inst.zhuyin_input);
    inst.zhuyin_parse = Some(parsed);
    inst.parsed_len
}

/// Builds the exact-decoder input for a scheme parse: the `'`-joined
/// full-pinyin text plus one [`ExactSegment`] per key over that text.
///
/// The session's graph then carries exactly the scheme parser's keys —
/// the pinyin inventory never re-segments the joined spelling (upstream's
/// decoder receives the parser's `ChewingKey`s; `docs/findings/
/// bopomofo-spec.md`).
fn exact_input(keys: &[impl ExactKey]) -> (String, Vec<ExactSegment>) {
    let mut text = String::new();
    let mut segments = Vec::with_capacity(keys.len());
    for key in keys {
        if !text.is_empty() {
            text.push('\'');
        }
        let start = text.len();
        text.push_str(key.key().text());
        segments.push(ExactSegment::new(start, text.len(), key.key(), key.tone()));
    }
    (text, segments)
}

/// The per-key view [`exact_input`] needs: the resolved syllable and its
/// tone.
trait ExactKey {
    /// The full-pinyin key this scheme key resolved to.
    fn key(&self) -> SyllableKey;
    /// The tone consumed with the key, `0` when toneless.
    fn tone(&self) -> u8;
}

impl ExactKey for ZhuyinKey {
    fn key(&self) -> SyllableKey {
        ZhuyinKey::key(self)
    }
    fn tone(&self) -> u8 {
        ZhuyinKey::tone(self)
    }
}

impl ExactKey for DoublePinyinKey {
    fn key(&self) -> SyllableKey {
        DoublePinyinKey::key(*self)
    }
    fn tone(&self) -> u8 {
        DoublePinyinKey::tone(*self)
    }
}

fn parse_c_string(instance: *mut PinyinInstance, text: *const c_char) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `text` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(text) };
        parse_more(instance, &text)
    })
}

/// Parse multiple full pinyins.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_full_pinyins(pinyin_instance_t * instance,
///                                       const char * pinyins);
/// ```
///
/// Returns number of bytes consumed, 0 on failure.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_full_pinyins(
    instance: *mut PinyinInstance,
    pinyins: *const c_char,
) -> usize {
    parse_c_string(instance, pinyins)
}

/// Parse multiple double pinyins.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_double_pinyins(pinyin_instance_t * instance,
///                                         const char * pinyins);
/// ```
///
/// Parses through [`oxpinyin_core::DoublePinyinParser`] and drives the
/// session with the apostrophe-joined full-pinyin spelling.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_double_pinyins(
    instance: *mut PinyinInstance,
    pinyins: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `pinyins` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(pinyins) };
        parse_double_more(instance, &text)
    })
}

/// Parse multiple chewing (bopomofo) inputs.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_chewings(pinyin_instance_t * instance,
///                                    const char * chewings);
/// ```
///
/// Parses through [`oxpinyin_core::ZhuyinParser`] (STANDARD) and drives
/// the session with the apostrophe-joined full-pinyin spelling.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_chewings(
    instance: *mut PinyinInstance,
    chewings: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `chewings` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(chewings) };
        parse_chewing_more(instance, &text)
    })
}

/// Get the parsed length of the input.
///
/// # C signature
/// ```c
/// size_t pinyin_get_parsed_input_length(pinyin_instance_t * instance);
/// ```
///
/// Returns the byte count of raw input consumed by the most recent parse
/// call, `0` before any parse and after [`pinyin_reset`](crate::instance::pinyin_reset),
/// matching upstream `pinyin.cpp:1611-1613` and reset `pinyin.cpp:2692`.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_parsed_input_length(instance: *mut PinyinInstance) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        inst.parsed_len
    })
}

/// Check whether an input key is in the current chewing keyboard scheme.
///
/// # C signature
/// ```c
/// bool pinyin_in_chewing_keyboard(pinyin_instance_t * instance,
///                                  const char key,
///                                  gchar *** symbols);
/// ```
///
/// `key` is a plain `char` value (not a pointer).
/// `symbols` receives a NULL-terminated string array; caller frees with
/// `g_strfreev`.
///
/// Returns the Zhuyin symbol(s) for `key` under the current scheme.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_in_chewing_keyboard(
    instance: *mut PinyinInstance,
    key: c_char,
    symbols: *mut *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let Some(scheme) = zhuyin_scheme(inst.zhuyin_scheme.load(Ordering::Relaxed)) else {
            return false;
        };
        let use_tone = inst.use_tone.load(Ordering::Relaxed);
        let parser = oxpinyin_core::ZhuyinParser::with_scheme(scheme);
        let mapped = parser.symbols_for(u8::try_from(key).unwrap_or(0), use_tone);
        if mapped.is_empty() {
            if !symbols.is_null() {
                // SAFETY: Null-checked above.
                unsafe {
                    *symbols = ptr::null_mut();
                }
            }
            return false;
        }

        if !symbols.is_null() {
            let ptr = crate::ffi::owned_cstr_list(&mapped);
            if ptr.is_null() {
                // SAFETY: Null-checked above.
                unsafe {
                    *symbols = ptr::null_mut();
                }
                return false;
            }
            // SAFETY: `owned_cstr_list` is a malloc array of malloc
            // strings; the caller releases both with g_strfreev.
            unsafe {
                *symbols = ptr;
            }
        }
        true
    })
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::{pinyin_get_parsed_input_length, pinyin_parse_more_full_pinyins};
    use crate::candidates::pinyin_get_candidate;
    use crate::instance::pinyin_reset;
    use crate::sentence::pinyin_guess_candidates;
    use crate::test_support::{DEFAULT_SORT, TempUserDir, cstr, open};

    #[test]
    fn parsed_input_length_stores_the_parse_result_and_clears_on_reset() {
        let user_dir = TempUserDir::new("parsed-len");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        assert_eq!(pinyin_get_parsed_input_length(instance), 0);

        let nihao = cstr("nihao");
        assert_eq!(pinyin_parse_more_full_pinyins(instance, nihao.as_ptr()), 5);
        assert_eq!(pinyin_get_parsed_input_length(instance), 5);

        assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
        let mut first = ptr::null_mut();
        assert!(pinyin_get_candidate(instance, 0, &raw mut first));
        assert!(crate::candidates::pinyin_choose_candidate(instance, 0, first) > 0);
        assert_eq!(pinyin_get_parsed_input_length(instance), 5);

        let partial = cstr("nihaoXYZ");
        let consumed = pinyin_parse_more_full_pinyins(instance, partial.as_ptr());
        assert_eq!(pinyin_get_parsed_input_length(instance), consumed);

        assert!(pinyin_reset(instance));
        assert_eq!(pinyin_get_parsed_input_length(instance), 0);

        let empty = cstr("");
        assert_eq!(pinyin_parse_more_full_pinyins(instance, empty.as_ptr()), 0);
        assert_eq!(pinyin_get_parsed_input_length(instance), 0);
        assert_eq!(pinyin_get_parsed_input_length(ptr::null_mut()), 0);

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}
