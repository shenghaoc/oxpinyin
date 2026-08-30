//! Auxiliary text retrieval.
//!
//! Full pinyin is C++-formatted (space-separated syllable keys with `|` at
//! the cursor). Double pinyin and chewing use the scheme aux walkers.

use oxpinyin_core::OptionBits;
use oxpinyin_core::graph::SegmentGraph;
use oxpinyin_core::phonetic_initial;
use oxpinyin_core::{DoublePinyinParse, ZhuyinParse};

use crate::ffi::{ffi_catch, owned_cstr};
use crate::state::instance_ref;
use crate::types::{GChar, PinyinInstance};

/// Formats the parsed prefix of `raw` the way the pinned C++ backend does:
/// space-separated syllable spellings with `|` at the byte cursor.
///
/// The selected keys come from [`SegmentGraph::fewest_keys`] with incomplete
/// edges admitted, so `nih` renders `ni h` and the initial-only tail stays
/// visible. Apostrophes are never rendered: a cursor on the apostrophe byte
/// or on the following key start both land on that key's `syllable_start`
/// (`ni'hao` cursor 2 and 3 both render `ni |hao `).
/// Aux text for the full-pinyin index schemes: walks the stored parse's
/// raw spans, rendering each key's canonical spelling plus its tone
/// digit (upstream renders `ChewingKey::get_pinyin_string`, which
/// appends `%d` for non-zero tones).
fn full_index_aux_text(
    input: &str,
    parse: &oxpinyin_core::FullPinyinIndexParse,
    cursor: usize,
) -> String {
    let parsed_len = parse.consumed().min(input.len());
    let cursor = cursor.min(parsed_len);
    let mut out = String::new();
    let mut inserted = false;

    for key in parse.keys() {
        let display = if key.tone() == 0 {
            key.canonical().to_owned()
        } else {
            format!("{}{}", key.canonical(), key.tone())
        };
        let start = key.start();
        let end = key.end();

        if !inserted && cursor <= start {
            out.push('|');
            inserted = true;
            out.push_str(&display);
            out.push(' ');
        } else if !inserted && cursor < end {
            let split = (cursor - start).min(display.len());
            let chars: Vec<char> = display.chars().collect();
            let left: String = chars[..split.min(chars.len())].iter().collect();
            let right: String = chars[split.min(chars.len())..].iter().collect();
            out.push_str(&left);
            out.push('|');
            out.push_str(&right);
            out.push(' ');
            inserted = true;
        } else {
            out.push_str(&display);
            out.push(' ');
        }
    }

    if !inserted {
        out.push('|');
    }
    out
}

fn full_aux_text(raw: &str, parsed_len: usize, cursor: usize, options: OptionBits) -> String {
    let parsed = &raw[..parsed_len.min(raw.len())];
    if parsed.is_empty() {
        return String::new();
    }
    let cursor = cursor.min(parsed.len());
    let Ok(graph) = SegmentGraph::build_with_options(parsed.as_bytes(), options) else {
        return String::new();
    };

    let mut out = String::new();
    let mut inserted = false;

    for edge in graph.fewest_keys(true) {
        let start = edge.syllable_start();
        let end = edge.to();
        // `ChewingKey::get_pinyin_string` (chewing_key.cpp:47-58): the
        // canonical table spelling with the tone digit appended for
        // non-zero tones. Carrying the digit keeps raw and canonical
        // lengths equal on HANYU under USE_TONE, which is the invariant
        // that holds the #130 aux over-read closed — the clamp below is
        // the belt, not the excuse.
        let display = if edge.tone() == 0 {
            edge.key().text().to_owned()
        } else {
            format!("{}{}", edge.key().text(), edge.tone())
        };

        if !inserted && cursor <= start {
            out.push('|');
            inserted = true;
            out.push_str(&display);
            out.push(' ');
        } else if !inserted && cursor < end {
            let split = (cursor - start).min(display.len());
            out.push_str(&display[..split]);
            out.push('|');
            out.push_str(&display[split..]);
            out.push(' ');
            inserted = true;
        } else {
            out.push_str(&display);
            out.push(' ');
        }
    }

    if !inserted {
        out.push('|');
    }
    out
}

fn split_pinyin_key(key: &str) -> (String, String) {
    let initial = phonetic_initial(key).unwrap_or("");
    let yunmu = &key[initial.len()..];
    (initial.to_owned(), yunmu.to_owned())
}

fn double_aux_text(
    input: &str,
    parse: &DoublePinyinParse,
    parsed_len: usize,
    cursor: usize,
) -> String {
    let parsed_len = parsed_len.min(input.len());
    let cursor = cursor.min(parsed_len);
    let keys = parse.keys();

    let mut prefix = String::new();
    for item in keys {
        if cursor < item.end() {
            break;
        }
        prefix.push_str(item.key().text());
        prefix.push(' ');
    }

    let mut postfix = String::new();
    for item in keys {
        if cursor > item.start() {
            continue;
        }
        postfix.push_str(item.key().text());
        postfix.push(' ');
    }

    let mut middle = String::new();
    let mut offset = 0;
    for item in keys {
        if cursor == offset {
            middle.push('|');
            break;
        }

        let begin = item.start();
        let end = item.end();
        if begin < cursor && cursor < end {
            let (shengmu, yunmu) = split_pinyin_key(item.key().text());
            match cursor - begin {
                1 => {
                    middle.push_str(&shengmu);
                    middle.push('|');
                    middle.push_str(&yunmu);
                    middle.push(' ');
                }
                2 => {
                    middle.push_str(&shengmu);
                    middle.push_str(&yunmu);
                    middle.push('|');
                    middle.push(' ');
                }
                _ => middle.push('|'),
            }
            break;
        }
        offset = end;
    }
    if middle.is_empty() {
        middle.push('|');
    }

    format!("{prefix}{middle}{postfix}")
}

fn chewing_aux_text(input: &str, parse: &ZhuyinParse, parsed_len: usize, cursor: usize) -> String {
    let parsed_len = parsed_len.min(input.len());
    let cursor = cursor.min(parsed_len);
    let keys = parse.keys();

    let mut prefix = String::new();
    for item in keys {
        if cursor < item.end() {
            break;
        }
        prefix.push_str(&item.display());
        prefix.push(' ');
    }

    let mut postfix = String::new();
    for item in keys {
        if cursor > item.start() {
            continue;
        }
        postfix.push_str(&item.display());
        postfix.push(' ');
    }

    let mut middle = String::new();
    let mut offset = 0;
    for item in keys {
        if cursor == offset {
            middle.push('|');
            break;
        }

        let begin = item.start();
        let end = item.end();
        if begin < cursor && cursor < end {
            let chars: Vec<char> = item.display().chars().collect();
            let split = (cursor - begin).min(chars.len());
            let left: String = chars[..split].iter().collect();
            let right: String = chars[split..].iter().collect();
            middle.push_str(&left);
            middle.push('|');
            middle.push_str(&right);
            middle.push(' ');
            break;
        }
        offset = end;
    }
    if middle.is_empty() {
        middle.push('|');
    }

    format!("{prefix}{middle}{postfix}")
}

/// Get auxiliary text for full pinyin display.
///
/// # C signature
/// ```c
/// bool pinyin_get_full_pinyin_auxiliary_text(pinyin_instance_t * instance,
///                                            size_t cursor,
///                                            gchar ** aux_text);
/// ```
///
/// Out-param `aux_text` is caller-owned (`g_free`). The returned buffer is
/// allocated with libc `malloc`, which `g_free` releases on every platform.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_full_pinyin_auxiliary_text(
    instance: *mut PinyinInstance,
    cursor: usize,
    aux_text: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        // Upstream returns false with an allocated empty string when
        // the matrix is empty — no parse, or a parse that consumed
        // nothing (`pinyin.cpp:3382-3386`).
        if inst.parsed_len == 0 {
            if !aux_text.is_null() {
                let owned = owned_cstr("");
                // SAFETY: Null-checked above.
                unsafe {
                    *aux_text = owned;
                }
            }
            return false;
        }
        let text = inst.full_parse.as_ref().map_or_else(
            || {
                full_aux_text(
                    inst.session.raw_input(),
                    inst.parsed_len,
                    cursor,
                    inst.options(),
                )
            },
            // LUOMA / SECONDARY_ZHUYIN: render the stored index parse —
            // canonical spellings (tone digit appended when a tone was
            // parsed, like `ChewingKey::get_pinyin_string`) over raw
            // spans.
            |parse| full_index_aux_text(&inst.full_input, parse, cursor),
        );
        if !aux_text.is_null() {
            // SAFETY: Null-checked above. `owned_cstr` returns null on an
            // interior NUL or allocation failure; otherwise ownership
            // transfers to the caller, which frees it with `g_free`.
            let owned = owned_cstr(&text);
            // SAFETY: Null-checked above.
            unsafe {
                *aux_text = owned;
            }
            if owned.is_null() {
                return false;
            }
        }
        true
    })
}

/// Get auxiliary text for double pinyin display.
///
/// # C signature
/// ```c
/// bool pinyin_get_double_pinyin_auxiliary_text(pinyin_instance_t * instance,
///                                              size_t cursor,
///                                              gchar ** aux_text);
/// ```
///
/// Out-param `aux_text` is caller-owned (`g_free`).
///
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_double_pinyin_auxiliary_text(
    instance: *mut PinyinInstance,
    cursor: usize,
    aux_text: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let Some(parse) = inst.double_parse.as_ref() else {
            if !aux_text.is_null() {
                // Upstream returns false with an allocated empty string when
                // the matrix is empty (pinyin.cpp:3442-3445).
                let owned = owned_cstr("");
                // SAFETY: Null-checked above.
                unsafe {
                    *aux_text = owned;
                }
            }
            return false;
        };
        if parse.keys().is_empty() {
            if !aux_text.is_null() {
                let owned = owned_cstr("");
                // SAFETY: Null-checked above.
                unsafe {
                    *aux_text = owned;
                }
            }
            return false;
        }
        let text = double_aux_text(&inst.double_input, parse, inst.parsed_len, cursor);
        if !aux_text.is_null() {
            // SAFETY: Null-checked above. `owned_cstr` returns null on an
            // interior NUL or allocation failure; otherwise ownership
            // transfers to the caller, which frees it with `g_free`.
            let owned = owned_cstr(&text);
            // SAFETY: Null-checked above.
            unsafe {
                *aux_text = owned;
            }
            if owned.is_null() {
                return false;
            }
        }
        true
    })
}

/// Get auxiliary text for chewing (bopomofo) display.
///
/// # C signature
/// ```c
/// bool pinyin_get_chewing_auxiliary_text(pinyin_instance_t * instance,
///                                        size_t cursor,
///                                        gchar ** aux_text);
/// ```
///
/// Out-param `aux_text` is caller-owned (`g_free`).
///
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_chewing_auxiliary_text(
    instance: *mut PinyinInstance,
    cursor: usize,
    aux_text: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let Some(parse) = inst.zhuyin_parse.as_ref() else {
            if !aux_text.is_null() {
                let owned = owned_cstr("");
                // SAFETY: Null-checked above.
                unsafe {
                    *aux_text = owned;
                }
            }
            return false;
        };
        if parse.keys().is_empty() {
            if !aux_text.is_null() {
                let owned = owned_cstr("");
                // SAFETY: Null-checked above.
                unsafe {
                    *aux_text = owned;
                }
            }
            return false;
        }
        let text = chewing_aux_text(&inst.zhuyin_input, parse, inst.parsed_len, cursor);
        if !aux_text.is_null() {
            // SAFETY: Null-checked above. `owned_cstr` returns null on an
            // interior NUL or allocation failure; otherwise ownership
            // transfers to the caller, which frees it with `g_free`.
            let owned = owned_cstr(&text);
            // SAFETY: Null-checked above.
            unsafe {
                *aux_text = owned;
            }
            if owned.is_null() {
                return false;
            }
        }
        true
    })
}

#[cfg(test)]
mod tests {
    use super::full_aux_text;
    use oxpinyin_core::{OptionBits, USE_TONE};

    #[test]
    fn full_aux_text_matches_the_oracle_for_simple_keys() {
        // Captured from the pinned C++ libpinyin 2.11.91 oracle with
        // PINYIN_INCOMPLETE set, using tools/bisection's dlopen driver.
        for (cursor, expected) in [
            (0, "|ni hao "),
            (1, "n|i hao "),
            (2, "ni |hao "),
            (3, "ni h|ao "),
            (4, "ni ha|o "),
            (5, "ni hao |"),
            (6, "ni hao |"),
            (99, "ni hao |"),
        ] {
            assert_eq!(
                full_aux_text("nihao", 5, cursor, OptionBits::default()),
                expected,
                "nihao cursor {cursor}"
            );
        }
    }

    #[test]
    fn full_aux_text_carries_the_tone_digit_under_use_tone() {
        // `ChewingKey::get_pinyin_string` (chewing_key.cpp:47-58) renders
        // "%s%d" for non-zero tones and the aux splits that rendered
        // string at the raw-relative cursor (pinyin.cpp:3411-3423) — the
        // cursor can land on the digit itself. Carrying the digit keeps
        // raw and canonical lengths equal, holding the #130 over-read
        // closed on HANYU.
        let options = OptionBits::from_bits(oxpinyin_core::PINYIN_INCOMPLETE | USE_TONE);
        for (cursor, expected) in [
            (0, "|zai4 "),
            (1, "z|ai4 "),
            (2, "za|i4 "),
            (3, "zai|4 "),
            (4, "zai4 |"),
            (99, "zai4 |"),
        ] {
            assert_eq!(
                full_aux_text("zai4", 4, cursor, options),
                expected,
                "zai4 cursor {cursor}"
            );
        }
        assert_eq!(full_aux_text("ni3hao3", 7, 7, options), "ni3 hao3 |");
        assert_eq!(full_aux_text("zai4'an", 7, 7, options), "zai4 an |");
        assert_eq!(full_aux_text("zai4Q", 4, 4, options), "zai4 |");
    }

    #[test]
    fn full_aux_text_drops_the_digit_when_use_tone_is_clear() {
        // The invariant: with the bit clear the toned inputs render
        // exactly as the frozen toneless parser always did.
        for (raw, parsed, cursor, expected) in [
            ("zai4", 3, 0, "|zai "),
            ("zai4", 3, 3, "zai |"),
            ("ni3hao3", 3, 3, "ni |"),
        ] {
            assert_eq!(
                full_aux_text(raw, parsed, cursor, OptionBits::default()),
                expected,
                "{raw} cursor {cursor}"
            );
        }
    }

    #[test]
    fn full_aux_text_matches_the_oracle_for_apostrophes() {
        // The apostrophe is consumed by the following edge and is never
        // rendered; cursors on the apostrophe byte (2) and on the key start
        // (3) are both the boundary between ni and hao.
        for (cursor, expected) in [
            (0, "|ni hao "),
            (2, "ni |hao "),
            (3, "ni |hao "),
            (4, "ni h|ao "),
            (6, "ni hao |"),
        ] {
            assert_eq!(
                full_aux_text("ni'hao", 6, cursor, OptionBits::default()),
                expected,
                "ni'hao cursor {cursor}"
            );
        }
    }

    #[test]
    fn full_aux_text_matches_the_oracle_for_incomplete_tails() {
        // nih parses as ni + incomplete h with PINYIN_INCOMPLETE set.
        for (cursor, expected) in [(0, "|ni h "), (2, "ni |h "), (3, "ni h |"), (4, "ni h |")] {
            assert_eq!(
                full_aux_text("nih", 3, cursor, OptionBits::default()),
                expected,
                "nih cursor {cursor}"
            );
        }

        // A bare incomplete initial renders as the initial itself.
        assert_eq!(full_aux_text("n", 1, 0, OptionBits::default()), "|n ");
        assert_eq!(full_aux_text("n", 1, 1, OptionBits::default()), "n |");
    }

    #[test]
    fn full_pinyin_auxiliary_text_uses_the_fewest_keys_walk() {
        use crate::parse::pinyin_parse_more_full_pinyins;
        use crate::test_support::{TempUserDir, cstr, open};
        use crate::types::GChar;

        let user_dir = TempUserDir::new("full-aux");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        let cases = [
            ("nihao", 5, 2, "ni |hao "),
            ("ni'hao", 6, 3, "ni |hao "),
            ("nih", 3, 3, "ni h |"),
        ];
        for (input, consumed, cursor, expected) in cases {
            let input = cstr(input);
            assert_eq!(
                pinyin_parse_more_full_pinyins(instance, input.as_ptr()),
                consumed
            );
            let mut aux: *mut GChar = std::ptr::null_mut();
            assert!(super::pinyin_get_full_pinyin_auxiliary_text(
                instance, cursor, &mut aux
            ));
            assert!(!aux.is_null());
            assert_eq!(crate::ffi::take_owned_cstr(aux.cast()), expected);
        }

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}
