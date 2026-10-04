//! Auxiliary text retrieval.
//!
//! Full pinyin is C++-formatted (space-separated syllable keys with `|` at
//! the cursor). Double pinyin and chewing use the scheme aux walkers.

use oxpinyin_core::OptionBits;
use oxpinyin_core::ZhuyinParse;
use oxpinyin_core::graph::SegmentGraph;

use crate::aux_matrix::SharedMatrix;
use crate::ffi::owned_cstr;
use crate::state::instance_ref;
use crate::types::{GChar, PinyinInstance};

/// Renders the instance's shared matrix. A matrix the renderer then refuses
/// is the pin's abort — `pinyin.cpp:3311` (`assert(get_column_size(offset)
/// >= 1)`) or `:3488` (`abort()` for a cut past two bytes) — answered with
/// `None` and one warning; no matrix is the silent `None`.
fn render_shared(
    inst: &crate::state::CapiInstance,
    name: &str,
    render: impl FnOnce(&SharedMatrix) -> Option<String>,
) -> Option<String> {
    let matrix = SharedMatrix::of(inst)?;
    let text = render(&matrix);
    if text.is_none() {
        crate::ffi::log_warning(&format!("{name}: aborted on the matrix walk"));
    }
    text
}

/// The plain full-pinyin renderers walk the session's own keys, not the pin's
/// matrix, so the matrix walk's aborts (`pinyin.cpp:3311`: a leading `'`
/// leaves column 0 empty) are looked for on the shared matrix first. Warns
/// once and answers `true` where the pin aborts.
fn full_walk_aborts(inst: &crate::state::CapiInstance, cursor: usize) -> bool {
    let aborts = SharedMatrix::of(inst).is_some_and(|matrix| matrix.full(cursor).is_none());
    if aborts {
        crate::ffi::log_warning(
            "pinyin_get_full_pinyin_auxiliary_text: aborted on the matrix walk",
        );
    }
    aborts
}

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

/// Writes `text` through the caller's out-param, or the empty string the
/// pin writes with its `false` for an empty matrix (`pinyin.cpp:3382-3388`).
/// Returns the pin's boolean.
fn write_aux(aux_text: *mut *mut GChar, text: Option<String>) -> bool {
    let answered = text.is_some();
    if !aux_text.is_null() {
        // SAFETY: Null-checked above. `owned_cstr` returns null on an
        // interior NUL or allocation failure; otherwise ownership
        // transfers to the caller, which frees it with `g_free`.
        let owned = owned_cstr(&text.unwrap_or_default());
        // SAFETY: Null-checked above.
        unsafe {
            *aux_text = owned;
        }
        if owned.is_null() {
            return false;
        }
    }
    answered
}

/// Whether the parse placed a key: the pin's matrix is empty otherwise
/// (`fill_matrix`, `phonetic_key_matrix.cpp:34-38`) and every auxiliary-text
/// function answers `false` with an empty string.
fn matrix_has_keys(inst: &crate::state::CapiInstance) -> bool {
    inst.core
        .mode_keys()
        .is_ok_and(|(keys, input, _)| !(keys.is_empty() || input.is_empty()))
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

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // Upstream returns false with an allocated empty string when
    // the matrix is empty — no parse, or a parse that placed no key
    // (`pinyin.cpp:3382-3386`).
    if inst.core.parsed_len == 0 || !matrix_has_keys(inst) {
        return write_aux(aux_text, None);
    }
    let text = if inst.core.double_parse.is_some() || inst.core.zhuyin_parse.is_some() {
        // A double or chewing parse filled the matrix; the pin renders it
        // all the same (register row 55).
        render_shared(inst, "pinyin_get_full_pinyin_auxiliary_text", |m| {
            m.full(cursor)
        })
    } else if full_walk_aborts(inst, cursor) {
        None
    } else {
        Some(inst.core.full_parse.as_ref().map_or_else(
            || {
                full_aux_text(
                    inst.core.session.raw_input(),
                    inst.core.parsed_len,
                    cursor,
                    inst.core.options(),
                )
            },
            // LUOMA / SECONDARY_ZHUYIN: render the stored index parse —
            // canonical spellings (tone digit appended when a tone was
            // parsed, like `ChewingKey::get_pinyin_string`) over raw
            // spans.
            |parse| full_index_aux_text(&inst.core.full_input, parse, cursor),
        ))
    };
    write_aux(aux_text, text)
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

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // Upstream returns false with an allocated empty string when
    // the matrix is empty (pinyin.cpp:3442-3445).
    if !matrix_has_keys(inst) {
        return write_aux(aux_text, None);
    }
    // The pin renders the shared matrix whichever parser filled it
    // (register row 55): the keys of a double-pinyin parse carry their
    // tone, which the renderer appends after a cut key.
    let text = render_shared(inst, "pinyin_get_double_pinyin_auxiliary_text", |m| {
        m.double(cursor)
    });
    write_aux(aux_text, text)
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

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    if !matrix_has_keys(inst) {
        return write_aux(aux_text, None);
    }
    let text = match inst.core.zhuyin_parse.as_ref() {
        Some(parse) => Some(chewing_aux_text(
            &inst.core.zhuyin_input,
            parse,
            inst.core.parsed_len,
            cursor,
        )),
        // Another parser filled the matrix; the pin renders it all the
        // same (register row 55).
        None => render_shared(inst, "pinyin_get_chewing_auxiliary_text", |m| {
            m.chewing(cursor)
        }),
    };
    write_aux(aux_text, text)
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
        // The "nih" case needs the incomplete tail the USE_TONE default
        // refuses.
        assert!(crate::config::pinyin_set_options(
            context,
            oxpinyin_core::PINYIN_INCOMPLETE
        ));

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
                instance,
                cursor,
                &raw mut aux
            ));
            assert!(!aux.is_null());
            assert_eq!(crate::ffi::take_owned_cstr(aux.cast()), expected);
        }

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}
