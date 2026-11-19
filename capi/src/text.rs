//! Auxiliary text retrieval.
//!
//! Full pinyin is C++-formatted (space-separated syllable keys with `|` at
//! the cursor). Double pinyin and chewing use the scheme aux walkers.

use oxpinyin::OptionBits;
use oxpinyin::ZhuyinParse;
use oxpinyin::graph::SegmentGraph;

use crate::aux_matrix::{AuxText, SharedMatrix, split_zhuyin};
use crate::ffi::{strconcat_aux, strdup};
use crate::state::instance_ref;
use crate::types::{GChar, PinyinInstance};

/// Renders the instance's shared matrix. A matrix the renderer then refuses
/// is the pin's abort — `pinyin.cpp:3311` (`assert(get_column_size(offset)
/// >= 1)`) or `:3488` (`abort()` for a cut past two bytes) — and the process
/// aborts there (register row 63); no matrix is the silent `None`.
fn render_shared(
    inst: &crate::state::CapiInstanceRead<'_>,
    render: impl FnOnce(&SharedMatrix) -> Option<AuxText>,
) -> Option<AuxText> {
    let matrix = SharedMatrix::of(&inst.read())?;
    let Some(text) = render(&matrix) else {
        std::process::abort();
    };
    Some(text)
}

/// The index-based full-pinyin renderer walks the stored parser keys.
/// Check the corresponding parse-time matrix for the pin's aborts first
/// (`pinyin.cpp:3311`, register row 63).
fn abort_on_full_walk(inst: &crate::state::CapiInstanceRead<'_>, cursor: usize) {
    if SharedMatrix::of(&inst.read()).is_some_and(|matrix| matrix.full(cursor).is_none()) {
        std::process::abort();
    }
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
    parse: &oxpinyin::FullPinyinIndexParse,
    cursor: usize,
) -> AuxText {
    let parsed_len = parse.consumed().min(input.len());
    let cursor = cursor.min(parsed_len);
    let mut aux = AuxText::default();
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
            aux.middle.push('|');
            inserted = true;
            aux.postfix.push_str(&display);
            aux.postfix.push(' ');
        } else if !inserted && cursor < end {
            let split = (cursor - start).min(display.len());
            let chars: Vec<char> = display.chars().collect();
            let left: String = chars[..split.min(chars.len())].iter().collect();
            let right: String = chars[split.min(chars.len())..].iter().collect();
            aux.middle.push_str(&left);
            aux.middle.push('|');
            aux.middle.push_str(&right);
            aux.middle.push(' ');
            inserted = true;
        } else {
            let part = if inserted {
                &mut aux.postfix
            } else {
                &mut aux.prefix
            };
            part.push_str(&display);
            part.push(' ');
        }
    }

    if !inserted {
        aux.middle.push('|');
    }
    aux
}

fn full_aux_text(
    raw: &str,
    parsed_len: usize,
    cursor: usize,
    options: OptionBits,
) -> Option<AuxText> {
    let parsed = &raw[..parsed_len.min(raw.len())];
    let cursor = cursor.min(parsed.len());
    let graph = SegmentGraph::build_with_options(parsed.as_bytes(), options).ok()?;
    let keys = graph.fewest_keys(options.contains(OptionBits::PINYIN_INCOMPLETE));
    if keys.first()?.syllable_start() != 0 {
        // A leading separator leaves the first column empty. The pin's
        // prefix walk asserts there (074a2219 pinyin.cpp:3307-3311).
        std::process::abort();
    }

    let mut aux = AuxText::default();
    let mut inserted = false;

    for edge in keys {
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
            aux.middle.push('|');
            inserted = true;
            aux.postfix.push_str(&display);
            aux.postfix.push(' ');
        } else if !inserted && cursor < end {
            let split = (cursor - start).min(display.len());
            aux.middle.push_str(&display[..split]);
            aux.middle.push('|');
            aux.middle.push_str(&display[split..]);
            aux.middle.push(' ');
            inserted = true;
        } else {
            let part = if inserted {
                &mut aux.postfix
            } else {
                &mut aux.prefix
            };
            part.push_str(&display);
            part.push(' ');
        }
    }

    if !inserted {
        aux.middle.push('|');
    }
    Some(aux)
}

fn chewing_aux_text(input: &str, parse: &ZhuyinParse, parsed_len: usize, cursor: usize) -> AuxText {
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
            let display = item.display();
            let (left, right) = split_zhuyin(&display, cursor - begin);
            middle.push_str(left);
            middle.push('|');
            middle.push_str(right);
            middle.push(' ');
            break;
        }
        offset = end;
    }
    if middle.is_empty() {
        middle.push('|');
    }

    AuxText {
        prefix,
        middle,
        postfix,
    }
}

/// Writes the auxiliary text through the caller's out-param with the pin's
/// final `g_strconcat(prefix, middle, postfix, NULL)` (`pinyin.cpp:3431`,
/// `:3509`, `:3569`), or, for an empty matrix, the `g_strdup("")` the pin
/// writes with its `false` (`:3386`, `:3445`, `:3523`). GLib allocates, so
/// a failed allocation aborts as the pin's does. Returns the pin's boolean.
///
/// # Safety
///
/// Under the crate-level rules, Out-parameters: a non-NULL output is aligned
/// and writable without conflicting with live borrows. Its contents need not be
/// initialized.
unsafe fn write_aux(aux_text: *mut *mut GChar, text: Option<AuxText>) -> bool {
    let answered = text.is_some();
    if !aux_text.is_null() {
        let owned = text.map_or_else(
            || strdup(""),
            |text| strconcat_aux(&text.prefix, &text.middle, &text.postfix),
        );
        // SAFETY: Out-parameters guarantees aligned writable storage; NULL was checked.
        // Ownership transfers to the caller,
        // which frees it with `g_free`.
        unsafe {
            *aux_text = owned;
        }
    }
    answered
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
/// Success writes the rendered text. An empty key matrix returns false and writes an allocated
/// empty string. Free either string with `g_free`. A null instance returns false without
/// writing.
///
/// # Aborts
///
/// A cursor walk that requires a key in an empty column aborts.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `aux_text` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_full_pinyin_auxiliary_text(
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
    if inst.core.parsed_len == 0 {
        // SAFETY: The entry point guarantees Out-parameters: a non-NULL output is aligned and writable
        // without conflicting with live borrows. Its contents need not be initialized.
        return unsafe { write_aux(aux_text, None) };
    }
    let text = if let Some(parse) = inst.core.full_parse.as_ref() {
        if !inst.core.mode_has_keys() {
            // SAFETY: The entry point guarantees Out-parameters: a non-NULL output is aligned and writable
            // without conflicting with live borrows. Its contents need not be initialized.
            return unsafe { write_aux(aux_text, None) };
        }
        abort_on_full_walk(&inst.read(), cursor);
        Some(full_index_aux_text(&inst.core.full_input, parse, cursor))
    } else if inst.core.double_parse.is_some() || inst.core.zhuyin_parse.is_some() {
        // A double or chewing parse filled the matrix; the pin renders it
        // all the same (register row 55).
        render_shared(&inst.read(), |m| m.full(cursor))
    } else {
        // The pin uses the matrix made by the last parse. A context
        // option write or candidate refresh does not replace those keys
        // (pinyin.cpp:1299-1306, 1497-1524, 3381-3438).
        full_aux_text(
            inst.core.session.raw_input(),
            inst.core.parsed_len,
            cursor,
            inst.state.full_aux_options,
        )
    };

    // SAFETY: The entry point guarantees Out-parameters: a non-NULL output is aligned and writable
    // without conflicting with live borrows. Its contents need not be initialized.
    unsafe { write_aux(aux_text, text) }
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
/// Success writes the rendered text. An empty key matrix returns false and writes an allocated
/// empty string. Free either string with `g_free`. A null instance returns false without
/// writing.
///
/// # Aborts
///
/// A cursor walk that requires a key in an empty column aborts. A split beyond the two bytes of
/// a double-pinyin syllable also aborts.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `aux_text` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_double_pinyin_auxiliary_text(
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
    if !inst.core.mode_has_keys() {
        // SAFETY: The entry point guarantees Out-parameters: a non-NULL output is aligned and writable
        // without conflicting with live borrows. Its contents need not be initialized.
        return unsafe { write_aux(aux_text, None) };
    }
    // The pin renders the shared matrix whichever parser filled it
    // (register row 55): the keys of a double-pinyin parse carry their
    // tone, which the renderer appends after a cut key.
    let text = render_shared(&inst.read(), |m| m.double(cursor));

    // SAFETY: The entry point guarantees Out-parameters: a non-NULL output is aligned and writable
    // without conflicting with live borrows. Its contents need not be initialized.
    unsafe { write_aux(aux_text, text) }
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
/// Success writes the rendered text. An empty key matrix returns false and writes an allocated
/// empty string. Free either string with `g_free`. A null instance returns false without
/// writing.
///
/// # Aborts
///
/// A cursor walk that requires a key in an empty column aborts.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `aux_text` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_chewing_auxiliary_text(
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
    if !inst.core.mode_has_keys() {
        // SAFETY: The entry point guarantees Out-parameters: a non-NULL output is aligned and writable
        // without conflicting with live borrows. Its contents need not be initialized.
        return unsafe { write_aux(aux_text, None) };
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
        None => render_shared(&inst.read(), |m| m.chewing(cursor)),
    };

    // SAFETY: The entry point guarantees Out-parameters: a non-NULL output is aligned and writable
    // without conflicting with live borrows. Its contents need not be initialized.
    unsafe { write_aux(aux_text, text) }
}

#[cfg(test)]
mod tests;
