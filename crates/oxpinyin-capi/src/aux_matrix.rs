//! Auxiliary text rendered from the shared parse matrix.
//!
//! The pin keeps one `PhoneticKeyMatrix` per instance, whichever parser
//! filled it, and all three auxiliary-text functions read that matrix:
//! `pinyin_get_full_pinyin_auxiliary_text`, `_double_pinyin_` and
//! `_chewing_auxiliary_text` differ only in how they cut a key that the
//! cursor splits and in whether the keys are spelled in pinyin or zhuyin
//! (`pinyin.cpp:3295-3576`). So after a full-pinyin parse the double and
//! chewing functions answer `true` with the same keys, and after a double
//! or chewing parse so does the full one.
//!
//! [`SharedMatrix`] rebuilds the part of that matrix those functions read —
//! one column per input byte, a key at the column where it begins, a zero
//! key at each `'` and at the last column (`fill_matrix`,
//! `phonetic_key_matrix.cpp:27-80`) — from whichever parse the instance
//! holds, and the three renderers are ports of the pin's loops.
//!
//! Where the pin asserts (`pinyin.cpp:3311`, `:3353`, `:3410`, `:3467`,
//! `:3545`: a column without a key at an offset the loop reads) or aborts
//! (`:3488`: a double-pinyin cursor three or more bytes into a key) the
//! renderers return `None` and the entry points answer `false` with an
//! empty string, the no-abort policy. Those sites are logged by the later
//! class (c) change, not here.

use oxpinyin_core::ChewingKey;
use oxpinyin_facade::KeyAt;

use crate::state::CapiInstance;

/// Which spelling the prefix and postfix keys take.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spelling {
    /// `ChewingKey::get_pinyin_string`, tone digit appended.
    Pinyin,
    /// `ChewingKey::get_zhuyin_string`, tone mark appended.
    Zhuyin,
}

/// An auxiliary text as the pin's final `g_strconcat(prefix, middle,
/// postfix, NULL)` receives it (`pinyin.cpp:3431`, `:3509`, `:3569`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AuxText {
    /// The keys before the cursor, each followed by a space.
    pub(crate) prefix: String,
    /// The cursor mark, or the key it splits.
    pub(crate) middle: String,
    /// The keys after the cursor, each followed by a space.
    pub(crate) postfix: String,
}

impl std::fmt::Display for AuxText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}{}", self.prefix, self.middle, self.postfix)
    }
}

/// One key of the matrix: the key itself and its byte span in the input.
struct MatrixKey {
    key: ChewingKey,
    begin: usize,
    end: usize,
}

/// What a column of the matrix holds (`get_column_size`, `get_item(.., 0)`).
#[derive(Clone, Copy)]
enum Column {
    /// Size 0: inside a key, or before the first one.
    Empty,
    /// The zero key: a `'`, or the reserved last column.
    Zero,
    /// A key, by index.
    Key(usize),
}

/// The matrix the auxiliary-text functions read.
pub(crate) struct SharedMatrix {
    keys: Vec<MatrixKey>,
    columns: Vec<Column>,
    parsed_len: usize,
}

impl SharedMatrix {
    /// The matrix of the instance's current parse, whatever mode made it;
    /// `None` where the pin's matrix is empty (`fill_matrix` clears it for
    /// a parse without keys, `phonetic_key_matrix.cpp:34-38`) or a key has
    /// no spelling.
    pub(crate) fn of(inst: &CapiInstance) -> Option<Self> {
        let keys = if inst.core.double_parse.is_some() {
            Vec::new()
        } else {
            inst.core.mode_key_list().ok()?
        };
        Self::of_keys(inst, &keys)
    }

    /// [`Self::of`] over the key list
    /// [`oxpinyin_facade::InstanceCore::mode_key_list`] gave for the
    /// current parse. Double pinyin reads its parse's keys instead.
    pub(crate) fn of_keys(inst: &CapiInstance, keys: &[KeyAt]) -> Option<Self> {
        let core = &inst.core;
        let parsed_len = core.parsed_len;
        let spans: Vec<(&'static str, u8, usize, usize)> =
            if let Some(parse) = core.double_parse.as_ref() {
                // `mode_keys` leaves the double-pinyin tone out; the key
                // carries it (`ChewingKey::m_tone`).
                parse
                    .keys()
                    .iter()
                    .map(|k| (k.key().text(), k.tone(), k.start(), k.end()))
                    .collect()
            } else {
                keys.iter()
                    .map(|k| (k.text, k.tone, k.begin, k.end))
                    .collect()
            };
        let mut keys = Vec::with_capacity(spans.len());
        for (text, tone, begin, end) in spans {
            if end > parsed_len || begin >= end {
                continue;
            }
            keys.push(MatrixKey {
                key: ChewingKey::from_pinyin(text)?.with_tone(tone),
                begin,
                end,
            });
        }
        if keys.is_empty() {
            return None;
        }
        let mut columns = vec![Column::Empty; parsed_len + 1];
        // Several keys can begin at one byte (the divided, resplit and fuzzy
        // alternatives); the pin's renderers read the column's first item
        // (`get_item(column, 0)`), so the first key placed there stays.
        for (index, key) in keys.iter().enumerate() {
            if let Some(slot) = columns.get_mut(key.begin)
                && matches!(slot, Column::Empty)
            {
                *slot = Column::Key(index);
            }
        }
        // The zero keys: one per `'` between two keys, and every byte from
        // the last key to the reserved last column.
        let mut previous_end = None;
        for key in &keys {
            if let Some(range) = previous_end.and_then(|end| columns.get_mut(end..key.begin)) {
                range.fill(Column::Zero);
            }
            previous_end = Some(key.end);
        }
        if let Some(range) = previous_end.and_then(|end| columns.get_mut(end..=parsed_len)) {
            range.fill(Column::Zero);
        }
        Some(Self {
            keys,
            columns,
            parsed_len,
        })
    }

    /// Whether a key is an initial alone carrying a tone — the shape
    /// `contains_incomplete_pinyin` asserts on (`pinyin_phrase3.h:152`) in
    /// every phrase-table search that holds it.
    pub(crate) fn has_toned_initial(&self) -> bool {
        self.keys
            .iter()
            .any(|k| k.key.middle == 0 && k.key.final_ == 0 && k.key.tone != 0)
    }

    fn size(&self) -> usize {
        self.columns.len()
    }

    /// `_compute_pinyin_start` (`pinyin.cpp:2903-2919`): skips the zero
    /// keys at the start of `offset`, never past the last column.
    fn pinyin_start(&self, offset: usize) -> usize {
        let mut start = offset;
        while start + 1 < self.size() && matches!(self.columns[start], Column::Zero) {
            start += 1;
        }
        start
    }

    /// The key a column starts, `None` where the column has none.
    fn key_at(&self, offset: usize) -> Option<&MatrixKey> {
        match self.columns.get(offset)? {
            Column::Key(index) => Some(&self.keys[*index]),
            Column::Empty | Column::Zero => None,
        }
    }

    fn spell(key: &MatrixKey, spelling: Spelling) -> String {
        match spelling {
            Spelling::Pinyin => key.key.pinyin_string(),
            Spelling::Zhuyin => key.key.zhuyin_string(),
        }
    }

    /// `_get_aux_text_prefix` (`pinyin.cpp:3295-3335`): the keys that end
    /// at or before the cursor.
    fn prefix(&self, cursor: usize, spelling: Spelling) -> Option<String> {
        let mut out = String::new();
        let mut offset = 0;
        while offset < self.size() {
            offset = self.pinyin_start(offset);
            if self.size() - 1 == offset {
                break;
            }
            let key = self.key_at(offset)?;
            if cursor < key.end {
                break;
            }
            out.push_str(&Self::spell(key, spelling));
            out.push(' ');
            offset = key.end;
        }
        Some(out)
    }

    /// `_get_aux_text_postfix` (`pinyin.cpp:3337-3379`): the keys that
    /// begin at or after the cursor.
    fn postfix(&self, cursor: usize, spelling: Spelling) -> Option<String> {
        let mut out = String::new();
        let mut offset = 0;
        while offset < self.size() {
            offset = self.pinyin_start(offset);
            if self.size() - 1 == offset {
                break;
            }
            let key = self.key_at(offset)?;
            if cursor > key.begin {
                offset = key.end;
                continue;
            }
            out.push_str(&Self::spell(key, spelling));
            out.push(' ');
            offset = key.end;
        }
        Some(out)
    }

    /// `pinyin_get_full_pinyin_auxiliary_text`'s body
    /// (`pinyin.cpp:3381-3438`).
    pub(crate) fn full(&self, cursor: usize) -> Option<AuxText> {
        let cursor = cursor.min(self.parsed_len);
        let prefix = self.prefix(cursor, Spelling::Pinyin)?;
        let postfix = self.postfix(cursor, Spelling::Pinyin)?;
        let mut middle = None;
        let mut offset = 0;
        while offset < self.size() {
            let newoffset = self.pinyin_start(offset);
            // At the pinyin boundary of the user input.
            if offset <= cursor && cursor <= newoffset {
                middle = Some("|".to_owned());
                break;
            }
            offset = newoffset;
            let key = self.key_at(offset)?;
            // In the middle of a key.
            if key.begin < cursor && cursor < key.end {
                let pinyin = key.key.pinyin_string();
                let len = cursor - key.begin;
                // `g_strndup(pinyin, len)` and `pinyin + len`.
                let left: String = pinyin.chars().take(len).collect();
                let right: String = pinyin.chars().skip(len).collect();
                middle = Some(format!("{left}|{right} "));
                break;
            }
            offset = key.end;
        }
        // The pin's final `g_strconcat(prefix, middle, postfix, NULL)` stops
        // at a NULL middle and drops the postfix. `middle` is always set
        // before the walk can end: every step leaves the cursor inside the
        // current span or advances a span that ends at or before it, so the
        // walk meets the boundary test or a containing key first. A matrix
        // that breaks that invariant is the pin's
        // `assert(get_column_size(offset) >= 1)`, not a NULL concat.
        let middle = middle?;
        Some(AuxText {
            prefix,
            middle,
            postfix,
        })
    }

    /// `pinyin_get_double_pinyin_auxiliary_text`'s body
    /// (`pinyin.cpp:3440-3516`): a key the cursor splits is cut into its
    /// shengmu and yunmu, one or two bytes in; further in, the pin aborts.
    pub(crate) fn double(&self, cursor: usize) -> Option<AuxText> {
        let cursor = cursor.min(self.parsed_len);
        let prefix = self.prefix(cursor, Spelling::Pinyin)?;
        let postfix = self.postfix(cursor, Spelling::Pinyin)?;
        let mut middle = None;
        let mut offset = 0;
        while offset < self.size() {
            // At the boundary.
            if cursor == offset {
                middle = Some("|".to_owned());
                break;
            }
            let (begin, end, key) = self.span_at(offset)?;
            let Some(key) = key.filter(|_| begin < cursor && cursor < end) else {
                offset = end;
                continue;
            };
            let shengmu = key.key.shengmu_string();
            let yunmu = key.key.yunmu_string();
            let mut text = match cursor - begin {
                1 => format!("{shengmu}|{yunmu}"),
                2 => format!("{shengmu}{yunmu}|"),
                _ => return None,
            };
            if let Some(digit) = tone_digit(key.key) {
                text.push(digit);
            }
            text.push(' ');
            middle = Some(text);
            break;
        }
        // `middle` is reached exactly as in `full`; the pin's NULL-stop is
        // the same unreachable concat, not a rendering branch.
        let middle = middle?;
        Some(AuxText {
            prefix,
            middle,
            postfix,
        })
    }

    /// `pinyin_get_chewing_auxiliary_text`'s body (`pinyin.cpp:3518-3576`):
    /// the zhuyin spelling cut by scalar, `g_utf8_substring(zhuyin, len,
    /// end)` for the right half. The cut point is the raw byte delta used
    /// as a scalar index; see [`split_zhuyin`].
    pub(crate) fn chewing(&self, cursor: usize) -> Option<AuxText> {
        let cursor = cursor.min(self.parsed_len);
        let prefix = self.prefix(cursor, Spelling::Zhuyin)?;
        let postfix = self.postfix(cursor, Spelling::Zhuyin)?;
        let mut middle = None;
        let mut offset = 0;
        while offset < self.size() {
            if cursor == offset {
                middle = Some("|".to_owned());
                break;
            }
            let (begin, end, key) = self.span_at(offset)?;
            let Some(key) = key.filter(|_| begin < cursor && cursor < end) else {
                offset = end;
                continue;
            };
            let zhuyin = key.key.zhuyin_string();
            let (left, right) = split_zhuyin(&zhuyin, cursor - begin);
            middle = Some(format!("{left}|{right} "));
            break;
        }
        let middle = middle?;
        Some(AuxText {
            prefix,
            middle,
            postfix,
        })
    }

    /// The span a column starts, with its key; a zero key spans one byte.
    fn span_at(&self, offset: usize) -> Option<(usize, usize, Option<&MatrixKey>)> {
        match self.columns.get(offset)? {
            Column::Key(index) => {
                let key = &self.keys[*index];
                Some((key.begin, key.end, Some(key)))
            }
            Column::Zero => Some((offset, offset + 1, None)),
            Column::Empty => None,
        }
    }
}

/// The pin's `g_utf8_substring(zhuyin, 0, split)` and
/// `g_utf8_substring(zhuyin, split, end)` pair: a scalar-index cut of a
/// zhuyin spelling. `split` is the raw-input byte delta, which the pin
/// passes as a character offset. At or past the spelling's last scalar the
/// right half is empty — the visible result of the second substring
/// starting at the terminator. Past that scalar the pin's
/// `g_utf8_offset_to_pointer` walks off the allocation (class (b)); this
/// cut stays at the string's end instead.
pub(crate) fn split_zhuyin(spelling: &str, split: usize) -> (&str, &str) {
    let end = spelling
        .char_indices()
        .nth(split)
        .map_or(spelling.len(), |(offset, _)| offset);
    spelling.split_at(end)
}

/// The tone digit `%d` appends, for tones 1 to 5.
fn tone_digit(key: ChewingKey) -> Option<char> {
    match key.tone {
        tone @ 1..=5 => char::from_digit(u32::from(tone), 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::parse::{pinyin_parse_more_double_pinyins, pinyin_parse_more_full_pinyins};
    use crate::test_support::{TempUserDir, cstr, open};
    use crate::text::{
        pinyin_get_chewing_auxiliary_text, pinyin_get_double_pinyin_auxiliary_text,
        pinyin_get_full_pinyin_auxiliary_text,
    };
    use crate::types::{GChar, PinyinInstance};

    /// `(true, text)` for an answer, `(false, "")` for the empty matrix.
    fn aux(
        render: extern "C" fn(*mut PinyinInstance, usize, *mut *mut GChar) -> bool,
        instance: *mut PinyinInstance,
        cursor: usize,
    ) -> (bool, String) {
        let mut text: *mut GChar = std::ptr::null_mut();
        let answered = render(instance, cursor, &raw mut text);
        (answered, crate::ffi::take_owned_cstr(text))
    }

    /// The pin renders one shared matrix, so the three functions answer
    /// after any parse: a full parse read as double and chewing, a double
    /// parse read as full and chewing (measured on bdb at 074a2219).
    #[test]
    fn the_three_renderers_read_the_matrix_whichever_parser_filled_it() {
        let user_dir = TempUserDir::new("aux-shared-matrix");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()),
            5
        );
        assert_eq!(
            aux(pinyin_get_double_pinyin_auxiliary_text, instance, 3),
            (true, "ni h|ao ".to_owned())
        );
        assert_eq!(
            aux(pinyin_get_chewing_auxiliary_text, instance, 2),
            (true, "ㄋㄧ |ㄏㄠ ".to_owned())
        );

        assert_eq!(
            pinyin_parse_more_double_pinyins(instance, cstr("nihk").as_ptr()),
            4
        );
        assert_eq!(
            aux(pinyin_get_full_pinyin_auxiliary_text, instance, 2),
            (true, "ni |hao ".to_owned())
        );
        assert_eq!(
            aux(pinyin_get_chewing_auxiliary_text, instance, 4),
            (true, "ㄋㄧ ㄏㄠ |".to_owned())
        );

        // No key placed: the pin's matrix is empty.
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, cstr("'").as_ptr()),
            1
        );
        for render in [
            pinyin_get_full_pinyin_auxiliary_text,
            pinyin_get_double_pinyin_auxiliary_text,
            pinyin_get_chewing_auxiliary_text,
        ] {
            assert_eq!(aux(render, instance, 0), (false, String::new()));
        }

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }

    /// The chewing split is the pin's `g_utf8_substring` cut: the raw byte
    /// delta is used as a scalar index into the zhuyin spelling. At the last
    /// scalar the right half is empty; past it the pin's pointer is already
    /// off the allocation (class (b)), so oxpinyin keeps the right half
    /// empty as well.
    #[test]
    fn chewing_cuts_the_zhuyin_spelling_by_scalar() {
        let user_dir = TempUserDir::new("aux-chewing-scalar");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()),
            5
        );
        // "hao" spans bytes 2..5 and spells two scalars: cursor 3 cuts after
        // the first, cursor 4 after the second (the terminator).
        assert_eq!(
            aux(pinyin_get_chewing_auxiliary_text, instance, 3),
            (true, "ㄋㄧ ㄏ|ㄠ ".to_owned())
        );
        assert_eq!(
            aux(pinyin_get_chewing_auxiliary_text, instance, 4),
            (true, "ㄋㄧ ㄏㄠ| ".to_owned())
        );
        // An overrun cursor clamps to the parsed length, the end boundary.
        assert_eq!(
            aux(pinyin_get_chewing_auxiliary_text, instance, 99),
            (true, "ㄋㄧ ㄏㄠ |".to_owned())
        );

        // "zhong" spells three scalars in five raw bytes, so cursor 4 is a
        // scalar past the spelling.
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, cstr("zhong").as_ptr()),
            5
        );
        assert_eq!(
            aux(pinyin_get_chewing_auxiliary_text, instance, 4),
            (true, "ㄓㄨㄥ| ".to_owned())
        );

        // A tone-1 key keeps its digit in the raw span but not in the
        // zhuyin spelling: cursor 2 lands exactly on the terminator.
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, cstr("ni1").as_ptr()),
            3
        );
        assert_eq!(
            aux(pinyin_get_chewing_auxiliary_text, instance, 2),
            (true, "ㄋㄧ| ".to_owned())
        );

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}
