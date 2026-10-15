//! The §9 export materialization: user-store rows rendered the way the
//! C ABI's export iterators yield them.
//!
//! Moved from `oxpinyin-capi`'s state layer so the standalone migration
//! tool (`oxpinyin-dictool`) drives the exact code the C iterators drive,
//! instead of a Rust-only re-implementation growing beside it. Pure Rust
//! over the context's user store and optional runtime — no C types cross
//! this boundary.

use oxpinyin_core::{ChewingKey, SyllableKey};
use oxpinyin_runtime::{PhraseItemView, RuntimeDict};
use oxpinyin_user::{
    ADDON_DICTIONARY, ExportedPhrase, MAX_KEY_TONE, NETWORK_DICTIONARY, PinyinKey, SENTENCE_START,
    USER_DICTIONARY, UserPhrase, UserPronunciation, UserStore, UserStoreError, is_user_file_token,
    key_syllable, key_tone,
};

use std::collections::{HashMap, VecDeque};

use crate::ContextCore;

/// The first `SYSTEM_FILE` library (`novel_types.h:154`).
const GB_DICTIONARY: u8 = 1;

/// The last `SYSTEM_FILE` library (`novel_types.h:158`).
const MERGED_DICTIONARY: u8 = 4;

/// Upstream's first training seed (`initial_seed`, 23·3): the §9 bigram
/// export threshold — counts at or above it export, below it stay.
const INITIAL_SEED: u64 = 23 * 3;

/// The pin's `export_iterator_t` (`pinyin.cpp:124-130` at 074a2219): the
/// sub-index, the token the next call reads (`null_token` when there is
/// none) and the pronunciation it reads. `pinyin_begin_get_phrases` probes
/// the first token with a pronunciation from the range's begin (`:662-688`);
/// `pinyin_iterator_get_next_phrase` fetches that item, renders one
/// pronunciation, then either steps to the item's next pronunciation or
/// probes the next token through a fresh `get_range` (`:698-768`). Nothing
/// is buffered, so a change that reaches an item before the call that reads
/// it is read, as it is at the pin.
///
/// Items come from three places, each read when a call needs it: a system
/// library's own item from the mapped chunk, the session's replacement for
/// it from the user store's override map (borrowed, never cloned), and a
/// `USER_FILE` library's item from the session store. The probes skip a
/// token only where the pin's do, an absent item or one with no
/// pronunciation; a present item that is malformed is read and refused at
/// the pin's assert site ([`ExportAssert`]), and a store that cannot be
/// read is the call's error, which the pin's in-memory sub-index never has.
///
/// What the store answers is held while its write generation stands still
/// and re-read as soon as it moves: the end of the appended system tokens,
/// a `USER_FILE` library's token end, the token the probe found without a
/// replacement, and the `USER_FILE` item under the cursor. The pin re-reads
/// its range and its item on every call because both are offset reads; the
/// session store's are a lock, a map probe or a range scan, and no write
/// can change their answer without moving the generation.
#[derive(Default)]
pub struct PhraseExportCursor {
    index: u8,
    next_token: u32,
    next_pronunciation: usize,
    system: Option<RuntimeDict>,
    user: Option<UserStore>,
    /// Whether `table.conf` gives this `USER_FILE` library a file: fixed
    /// for the session, the pin's `NULL` sub-index otherwise.
    user_library_present: bool,
    memo: Memo,
    current: Option<CurrentUserItem>,
}

/// Store answers valid at one write generation.
#[derive(Default)]
struct Memo {
    generation: Option<u64>,
    /// One past the highest system token the session appended (`1..=4`).
    appended_end: u32,
    /// One past the highest token of the `USER_FILE` library (`5..=7`).
    user_end: u32,
    /// The last system token the probe read from the mapped chunk because
    /// the session holds no replacement for it, so the read that follows
    /// skips the map.
    plain_token: u32,
}

/// The `USER_FILE` item under the cursor's token, as of a phrase generation.
struct CurrentUserItem {
    token: u32,
    generation: u64,
    phrase: UserPhrase,
}

/// The assert sites `pinyin_iterator_get_next_phrase` reaches once the probe
/// has selected a token (`pinyin.cpp:698-739` at 074a2219), in the order the
/// call meets them. The probe itself skips a token only when
/// `get_phrase_item` answers no item or the item has no pronunciation
/// (`:680-682`, `:761-763`); a present item that is malformed is selected,
/// and the read dies at one of these. The oracle build keeps asserts on, so
/// `check_result` is `assert` (`pinyin_utils.h:30`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportAssert {
    /// `assert(ERROR_OK == retval)` after `get_phrase_item` (`:709`): no
    /// item under the probed token, or no probed token at all.
    Item,
    /// `check_result(item.get_phrase_string(phrase_ucs4))` (`:714`): the
    /// item's stored text does not decode.
    PhraseString,
    /// `assert(nth_pronun < n_pronuns)` (`:721`): the item no longer holds
    /// the pronunciation the previous call left the cursor at.
    Pronunciation,
    /// `check_result(item.get_nth_pronunciation(nth_pronun, keys, freq))`
    /// (`:724`): the pronunciation under the cursor cannot be read.
    NthPronunciation,
    /// `assert(m_tone < CHEWING_NUMBER_OF_TONES)` in `get_pinyin_string`
    /// (`chewing_key.cpp:48`, reached from `:728` for each key in turn): a
    /// `USER_FILE` reading carries a tone past `MAX_KEY_TONE`.
    KeyTone,
    /// `assert(index < (int) G_N_ELEMENTS(content_table))` in
    /// `get_pinyin_string` (`chewing_key.cpp:50`): a `USER_FILE` reading
    /// names a syllable this engine has no spelling for.
    KeyTable,
}

impl ExportAssert {
    /// The asserted expression, as the pin's `assert` prints it.
    #[must_use]
    pub const fn expression(self) -> &'static str {
        match self {
            Self::Item => "ERROR_OK == retval",
            Self::PhraseString => "item.get_phrase_string(phrase_ucs4)",
            Self::Pronunciation => "nth_pronun < n_pronuns",
            Self::NthPronunciation => "item.get_nth_pronunciation(nth_pronun, keys, freq)",
            Self::KeyTone => "m_tone < CHEWING_NUMBER_OF_TONES",
            Self::KeyTable => "index < (int) G_N_ELEMENTS(content_table)",
        }
    }
}

/// What one [`PhraseExportCursor::get_next_with`] call produced: a row, or
/// the assert site the pin dies at. A store that cannot be read is the
/// call's `Err`, not a step: the pin's sub-index is in memory and has no
/// such outcome, so it propagates instead of becoming a gap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportStep<R> {
    /// A row.
    Row(R),
    /// One of the pin's assert sites; the cursor stays where it is, so a
    /// repeated call answers the same.
    Assert(ExportAssert),
}

/// Which assert site, if any, a `UserStore::phrase` failure stands for: a
/// present item whose text or pronunciation row does not decode is read at
/// the pin and dies at `:714` or `:724`; every other failure is the store's,
/// with no pin counterpart.
fn assert_for(error: &UserStoreError) -> Option<ExportAssert> {
    match error {
        UserStoreError::Decode => Some(ExportAssert::PhraseString),
        UserStoreError::DecodePronunciation => Some(ExportAssert::NthPronunciation),
        _ => None,
    }
}

/// One row the cursor is reading, borrowed from its item: the phrase, one
/// pronunciation and that pronunciation's count, as
/// `pinyin_iterator_get_next_phrase` renders them (`pinyin.cpp:712-739`).
/// The writers put the text and the `'`-joined pinyin straight into buffers
/// of the reported lengths; the string builders allocate each once.
pub struct ExportRow<'a>(RowSource<'a>);

enum RowSource<'a> {
    /// A system library's own item: the phrase as stored `ucs4_t` code
    /// points, the pronunciation as packed `ChewingKey` bytes.
    Mapped {
        item: PhraseItemView<'a>,
        keys: &'a [u8],
        count: u32,
    },
    /// The session's replacement for a system item.
    Override {
        phrase: &'a [u32],
        keys: &'a [u16],
        count: u32,
    },
    /// A `USER_FILE` item.
    User {
        text: &'a str,
        keys: &'a [PinyinKey],
        count: u64,
    },
}

/// The syllables of one pronunciation: each a spelling and the tone digit
/// `ChewingKey::get_pinyin_string` appends to it (`chewing_key.cpp:47-57`
/// at 074a2219: the content table's pinyin, then `%d` of a non-zero tone).
enum Syllables<'a> {
    Packed(std::slice::ChunksExact<'a, u8>),
    Chewing(std::slice::Iter<'a, u16>),
    User(std::slice::Iter<'a, PinyinKey>),
}

impl Iterator for Syllables<'_> {
    type Item = (&'static str, Option<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Packed(pairs) => pairs.next().map(|pair| {
                chewing_syllable(ChewingKey::from_packed(u16::from_le_bytes([
                    pair[0], pair[1],
                ])))
            }),
            Self::Chewing(keys) => keys
                .next()
                .map(|&key| chewing_syllable(ChewingKey::from_packed(key))),
            Self::User(keys) => keys.next().map(|&key| user_syllable(key)),
        }
    }
}

/// `ChewingKey::get_pinyin_string` without the allocation: the spelling
/// and the digit of a non-zero tone.
fn chewing_syllable(key: ChewingKey) -> (&'static str, Option<u8>) {
    let digit = match key.tone {
        0 => None,
        tone @ 1..=9 => Some(b'0' + tone),
        _ => None,
    };
    (key.pinyin_spelling(), digit)
}

/// A `USER_FILE` reading's syllable: `key_pinyin_string` without the
/// allocation. The cursor renders a reading only after [`user_key_assert`]
/// cleared every key of it; the fallbacks are never read.
fn user_syllable(key: PinyinKey) -> (&'static str, Option<u8>) {
    let spelling = SyllableKey::from_index(key_syllable(key)).map_or("", SyllableKey::text);
    let digit = match key_tone(key) {
        0 => None,
        tone @ 1..=MAX_KEY_TONE => Some(b'0' + tone),
        _ => None,
    };
    (spelling, digit)
}

/// The `get_pinyin_string` assert a `USER_FILE` key dies at, if any, in the
/// pin's order: the tone (`chewing_key.cpp:48`), then the content-table
/// index (`:50`). `key_pinyin_string` answers `None` for exactly these.
fn user_key_assert(key: PinyinKey) -> Option<ExportAssert> {
    if key_tone(key) > MAX_KEY_TONE {
        return Some(ExportAssert::KeyTone);
    }
    if SyllableKey::from_index(key_syllable(key)).is_none() {
        return Some(ExportAssert::KeyTable);
    }
    None
}

/// Writes `chars` as UTF-8 to the front of `out`, stopping before a
/// character that would not fit; returns the bytes written.
fn write_chars(chars: impl Iterator<Item = char>, out: &mut [u8]) -> usize {
    let mut at = 0;
    for c in chars {
        let width = c.len_utf8();
        let Some(slot) = out.get_mut(at..at + width) else {
            break;
        };
        c.encode_utf8(slot);
        at += width;
    }
    at
}

/// The length of `syllables` joined with `'`.
fn joined_len(syllables: Syllables<'_>) -> usize {
    let mut len = 0;
    let mut count = 0_usize;
    for (spelling, digit) in syllables {
        len += spelling.len() + usize::from(digit.is_some());
        count += 1;
    }
    len + count.saturating_sub(1)
}

/// Writes `syllables` joined with `'` to the front of `out`, stopping
/// before a piece that would not fit; returns the bytes written.
fn write_joined(syllables: Syllables<'_>, out: &mut [u8]) -> usize {
    let mut at = 0;
    for (index, (spelling, digit)) in syllables.enumerate() {
        if index > 0 {
            let Some(slot) = out.get_mut(at) else {
                return at;
            };
            *slot = b'\'';
            at += 1;
        }
        let Some(slot) = out.get_mut(at..at + spelling.len()) else {
            return at;
        };
        slot.copy_from_slice(spelling.as_bytes());
        at += spelling.len();
        if let Some(digit) = digit {
            let Some(slot) = out.get_mut(at) else {
                return at;
            };
            *slot = digit;
            at += 1;
        }
    }
    at
}

impl ExportRow<'_> {
    /// The phrase's UTF-8 length in bytes, or `None` when a stored code
    /// point is not a Unicode scalar value: the pin's `g_ucs4_to_utf8` runs
    /// with no error slot and hands the caller a NULL phrase for such an
    /// item (`pinyin.cpp:715-716`), the row's other fields intact.
    #[must_use]
    pub fn text_len(&self) -> Option<usize> {
        match &self.0 {
            RowSource::Mapped { item, .. } => item
                .phrase_chars()
                .map(|c| c.map(char::len_utf8))
                .sum::<Option<usize>>(),
            RowSource::Override { phrase, .. } => phrase
                .iter()
                .map(|&code| char::from_u32(code).map(char::len_utf8))
                .sum::<Option<usize>>(),
            RowSource::User { text, .. } => Some(text.len()),
        }
    }

    /// The `get_pinyin_string` assert the pin dies at while rendering this
    /// row's keys (`pinyin.cpp:728`), if any: a `USER_FILE` key with a tone
    /// past `MAX_KEY_TONE` or a syllable this engine has no spelling for. A
    /// system item's packed keys are not sites: the chewing crate renders an
    /// invalid pattern through its registered zero-row fallback.
    #[must_use]
    pub fn key_assert(&self) -> Option<ExportAssert> {
        match &self.0 {
            RowSource::User { keys, .. } => keys.iter().find_map(|&key| user_key_assert(key)),
            RowSource::Mapped { .. } | RowSource::Override { .. } => None,
        }
    }

    /// Writes the phrase's UTF-8 bytes to the front of `out`, which holds
    /// at least [`Self::text_len`] bytes, and returns how many it wrote.
    pub fn write_text(&self, out: &mut [u8]) -> usize {
        match &self.0 {
            RowSource::Mapped { item, .. } => write_chars(item.phrase_chars().flatten(), out),
            RowSource::Override { phrase, .. } => {
                write_chars(phrase.iter().filter_map(|&code| char::from_u32(code)), out)
            }
            RowSource::User { text, .. } => {
                let width = text.len().min(out.len());
                out[..width].copy_from_slice(&text.as_bytes()[..width]);
                width
            }
        }
    }

    /// The `'`-joined pinyin's length in bytes.
    #[must_use]
    pub fn pinyin_len(&self) -> usize {
        joined_len(self.syllables())
    }

    /// Writes the `'`-joined pinyin to the front of `out`, which holds at
    /// least [`Self::pinyin_len`] bytes, and returns how many it wrote.
    pub fn write_pinyin(&self, out: &mut [u8]) -> usize {
        write_joined(self.syllables(), out)
    }

    /// The pronunciation's stored count (`item.get_nth_pronunciation`'s
    /// `freq`, `pinyin.cpp:724`).
    #[must_use]
    pub fn count(&self) -> u64 {
        match &self.0 {
            RowSource::Mapped { count, .. } | RowSource::Override { count, .. } => {
                u64::from(*count)
            }
            RowSource::User { count, .. } => *count,
        }
    }

    /// The phrase as an owned string, allocated once; `None` where the pin
    /// hands out a NULL phrase ([`Self::text_len`]).
    #[must_use]
    pub fn text_string(&self) -> Option<String> {
        let mut out = String::with_capacity(self.text_len()?);
        match &self.0 {
            RowSource::Mapped { item, .. } => out.extend(item.phrase_chars().flatten()),
            RowSource::Override { phrase, .. } => {
                out.extend(phrase.iter().filter_map(|&code| char::from_u32(code)));
            }
            RowSource::User { text, .. } => out.push_str(text),
        }
        Some(out)
    }

    /// The `'`-joined pinyin as an owned string, allocated once.
    #[must_use]
    pub fn pinyin_string(&self) -> String {
        let mut out = String::with_capacity(self.pinyin_len());
        for (index, (spelling, digit)) in self.syllables().enumerate() {
            if index > 0 {
                out.push('\'');
            }
            out.push_str(spelling);
            if let Some(digit) = digit {
                out.push(char::from(digit));
            }
        }
        out
    }

    fn syllables(&self) -> Syllables<'_> {
        match &self.0 {
            RowSource::Mapped { keys, .. } => Syllables::Packed(keys.chunks_exact(2)),
            RowSource::Override { keys, .. } => Syllables::Chewing(keys.iter()),
            RowSource::User { keys, .. } => Syllables::User(keys.iter()),
        }
    }
}

/// One item under a token, wherever it lives.
enum ItemRef<'a> {
    Mapped(PhraseItemView<'a>),
    Override {
        phrase: &'a [u32],
        prons: &'a [(Vec<u16>, u32)],
    },
    User(&'a UserPhrase),
}

impl ItemRef<'_> {
    /// `item.get_n_pronunciation()`.
    fn n_pronunciations(&self) -> usize {
        match self {
            Self::Mapped(item) => item.n_pronunciations(),
            Self::Override { prons, .. } => prons.len(),
            Self::User(phrase) => phrase.pronunciations().len(),
        }
    }

    /// The pronunciation the cursor moves to after reading `from - 1`:
    /// `nth_pronun ++; if (nth_pronun < n_pronuns)` (`pinyin.cpp:742-746`).
    /// Every stored pronunciation counts, as the pin's `n_pronuns` does; one
    /// this engine cannot spell is read and refused, not skipped.
    fn next_pronunciation(&self, from: usize) -> Option<usize> {
        (from < self.n_pronunciations()).then_some(from)
    }

    /// What the probe asks of an item (`pinyin.cpp:682`): that it holds a
    /// pronunciation. The text is not looked at until the read.
    fn first_pronunciation(&self) -> Option<usize> {
        self.next_pronunciation(0)
    }

    /// Pronunciation `nth` of the item as a row: `get_nth_pronunciation`
    /// (`pinyin.cpp:724`), `None` where the item does not hold it.
    fn row(&self, nth: usize) -> Option<ExportRow<'_>> {
        Some(ExportRow(match self {
            Self::Mapped(item) => {
                let pronunciation = item.pronunciation(nth)?;
                RowSource::Mapped {
                    item: *item,
                    keys: pronunciation.keys,
                    count: pronunciation.freq,
                }
            }
            Self::Override { phrase, prons } => {
                let (keys, count) = prons.get(nth)?;
                RowSource::Override {
                    phrase,
                    keys: keys.as_slice(),
                    count: *count,
                }
            }
            Self::User(phrase) => {
                let pronunciation = phrase.pronunciations().get(nth)?;
                RowSource::User {
                    text: phrase.text(),
                    keys: pronunciation.keys(),
                    count: pronunciation.count(),
                }
            }
        }))
    }
}

impl PhraseExportCursor {
    /// `pinyin_iterator_has_next_phrase` (`pinyin.cpp:690-695`): whether
    /// the begin or the previous call probed a token.
    #[must_use]
    pub fn has_next(&self) -> bool {
        self.next_token != NULL_TOKEN
    }

    /// `pinyin_iterator_get_next_phrase` (`pinyin.cpp:698-768`): reads the
    /// probed item, hands `read` the pronunciation the cursor is at, then
    /// moves to the item's next pronunciation or, when there is none,
    /// re-reads the range and probes the next token with a pronunciation
    /// (`:742-767`). The call meets the pin's assert sites in the pin's
    /// order ([`ExportAssert`]) and answers the first one it hits with the
    /// cursor unmoved.
    ///
    /// # Errors
    ///
    /// The store could not be read: the item under the cursor, or the range
    /// the re-probe needs. The pin has no such outcome, so the error is
    /// handed up rather than read as a gap.
    pub fn get_next_with<R>(
        &mut self,
        read: impl FnOnce(&ExportRow<'_>) -> R,
    ) -> Result<ExportStep<R>, UserStoreError> {
        let token = self.next_token;
        if token == NULL_TOKEN {
            return Ok(ExportStep::Assert(ExportAssert::Item));
        }
        let nth = self.next_pronunciation;
        let step = self.with_item(token, |item| {
            if nth >= item.n_pronunciations() {
                return Err(ExportAssert::Pronunciation);
            }
            let row = item.row(nth).ok_or(ExportAssert::NthPronunciation)?;
            if let Some(site) = row.key_assert() {
                return Err(site);
            }
            Ok((read(&row), item.next_pronunciation(nth + 1)))
        });
        let (out, following) = match step {
            Err(error) => {
                return match assert_for(&error) {
                    Some(site) => Ok(ExportStep::Assert(site)),
                    None => Err(error),
                };
            }
            Ok(None) => return Ok(ExportStep::Assert(ExportAssert::Item)),
            Ok(Some(Err(site))) => return Ok(ExportStep::Assert(site)),
            Ok(Some(Ok(pair))) => pair,
        };
        match following {
            Some(next) => self.next_pronunciation = next,
            None => {
                self.next_pronunciation = 0;
                self.probe_from(token + 1)?;
            }
        }
        Ok(ExportStep::Row(out))
    }

    /// Every row the cursor still yields, driven as a consumer drives the C
    /// iterator (`while has_next { get_next }`). `None` when the store
    /// cannot be read, when the pin would die at an assert site, or when it
    /// would hand out a NULL phrase: its export of this sub-index does not
    /// complete, so neither does this one.
    #[must_use]
    pub fn collect_rows(&mut self) -> Option<Vec<ExportedPhrase>> {
        let mut rows = Vec::new();
        while self.has_next() {
            let step = self.get_next_with(|row| {
                row.text_string().map(|text| ExportedPhrase {
                    text,
                    pinyin: row.pinyin_string(),
                    count: row.count(),
                })
            });
            match step {
                Ok(ExportStep::Row(Some(row))) => rows.push(row),
                Ok(ExportStep::Row(None) | ExportStep::Assert(_)) | Err(_) => return None,
            }
        }
        Some(rows)
    }

    /// Re-reads the store-derived answers when the store's write generation
    /// moved since they were taken. A range end the store cannot read leaves
    /// the memo as it was, so the next call asks again.
    fn refresh(&mut self) -> Result<(), UserStoreError> {
        let Some(user) = self.user.as_ref() else {
            return Ok(());
        };
        let generation = user.generation();
        if self.memo.generation == Some(generation) {
            return Ok(());
        }
        let user_end = if self.user_library_present {
            user.user_library_token_end(self.index)?
        } else {
            NULL_TOKEN
        };
        self.memo = Memo {
            generation: Some(generation),
            appended_end: user.system_item_end(self.index).unwrap_or(NULL_TOKEN),
            user_end,
            plain_token: NULL_TOKEN,
        };
        Ok(())
    }

    /// The probe `pinyin_begin_get_phrases` (`pinyin.cpp:676-687`) and the
    /// re-probe in `get_next_phrase` (`:748-767`) share: a fresh
    /// `get_range`, then the first token from `start` whose item has a
    /// pronunciation. A sub-index that is not loaded ends the iteration.
    fn probe_from(&mut self, start: u32) -> Result<(), UserStoreError> {
        self.next_token = NULL_TOKEN;
        self.refresh()?;
        match self.index {
            GB_DICTIONARY..=MERGED_DICTIONARY => self.probe_system(start),
            ADDON_DICTIONARY | NETWORK_DICTIONARY | USER_DICTIONARY => self.probe_user(start)?,
            _ => {}
        }
        Ok(())
    }

    /// `FacadePhraseIndex::get_range` for a system sub-index
    /// (`phrase_index.cpp:610-622`, the trailing-zero end at `:624-646`):
    /// one past the highest token with an item, past the chunk or past the
    /// session's appended items, whichever is further; then the forward
    /// probe. An unloaded library is `ERROR_NO_SUB_PHRASE_INDEX`: no token.
    fn probe_system(&mut self, start: u32) {
        let Some(dict) = self.system.as_ref() else {
            return;
        };
        if !dict.library_visible(u32::from(self.index)) {
            return;
        }
        let base = u32::from(self.index) << 24;
        let Some(library) = dict.system().libraries().library(base) else {
            return;
        };
        let end = (base | library.token_range().end).max(self.memo.appended_end);
        let mut token = start;
        while token < end {
            let replaced = self.user.as_ref().and_then(|user| {
                user.with_system_item(token, |item| {
                    ItemRef::Override {
                        phrase: &item.phrase,
                        prons: &item.prons,
                    }
                    .first_pronunciation()
                })
            });
            let found = match replaced {
                Some(found) => found,
                None => {
                    // No replacement under this token at this generation.
                    self.memo.plain_token = token;
                    library
                        .item(token)
                        .and_then(|item| ItemRef::Mapped(item).first_pronunciation())
                }
            };
            if let Some(nth) = found {
                self.next_token = token;
                self.next_pronunciation = nth;
                return;
            }
            token += 1;
        }
    }

    /// The same for a `USER_FILE` sub-index: its range ends at the store's
    /// token end; a library `table.conf` gives no file has no sub-index.
    /// The pin's probe reads `n_prons` from the item's header (`:682`,
    /// `:763`), so a present item the store cannot decode is selected, not
    /// skipped; the read that follows is the assert.
    fn probe_user(&mut self, start: u32) -> Result<(), UserStoreError> {
        if !self.user_library_present {
            return Ok(());
        }
        let end = self.memo.user_end;
        let mut token = start;
        while token < end {
            let found = match self
                .with_user_item(token, |phrase| ItemRef::User(phrase).first_pronunciation())
            {
                Ok(found) => found.flatten(),
                Err(error) if assert_for(&error).is_some() => Some(0),
                Err(error) => return Err(error),
            };
            if let Some(nth) = found {
                self.next_token = token;
                self.next_pronunciation = nth;
                return Ok(());
            }
            token += 1;
        }
        Ok(())
    }

    /// `get_phrase_item` for `token` (`phrase_index.cpp:180-206`): the
    /// session's replacement first, then the mapped chunk; a `USER_FILE`
    /// token reads the session store. `Ok(None)` is `ERROR_NO_ITEM`.
    fn with_item<R>(
        &mut self,
        token: u32,
        read: impl FnOnce(ItemRef<'_>) -> R,
    ) -> Result<Option<R>, UserStoreError> {
        self.refresh()?;
        match self.index {
            GB_DICTIONARY..=MERGED_DICTIONARY => {
                let Some(dict) = self.system.as_ref() else {
                    return Ok(None);
                };
                let mut read = Some(read);
                if self.memo.plain_token != token
                    && let Some(user) = self.user.as_ref()
                    && let Some(out) = user
                        .with_system_item(token, |item| {
                            read.take().map(|read| {
                                read(ItemRef::Override {
                                    phrase: &item.phrase,
                                    prons: &item.prons,
                                })
                            })
                        })
                        .flatten()
                {
                    return Ok(Some(out));
                }
                let Some(read) = read else {
                    return Ok(None);
                };
                let Some(library) = dict.system().libraries().library(token) else {
                    return Ok(None);
                };
                let Some(item) = library.item(token) else {
                    return Ok(None);
                };
                Ok(Some(read(ItemRef::Mapped(item))))
            }
            ADDON_DICTIONARY | NETWORK_DICTIONARY | USER_DICTIONARY => {
                self.with_user_item(token, |phrase| read(ItemRef::User(phrase)))
            }
            _ => Ok(None),
        }
    }

    /// The `USER_FILE` item under `token`, read from the session store once
    /// per token and generation. `Ok(None)` is no item; a present item the
    /// store cannot decode, or a store it cannot read, is the `Err`.
    fn with_user_item<R>(
        &mut self,
        token: u32,
        read: impl FnOnce(&UserPhrase) -> R,
    ) -> Result<Option<R>, UserStoreError> {
        let Some(user) = self.user.as_ref() else {
            return Ok(None);
        };
        let Some(generation) = self.memo.generation else {
            return Ok(None);
        };
        let held = self
            .current
            .as_ref()
            .is_some_and(|current| current.token == token && current.generation == generation);
        if !held {
            let Some(phrase) = user.phrase(token)? else {
                return Ok(None);
            };
            self.current = Some(CurrentUserItem {
                token,
                generation,
                phrase,
            });
        }
        let Some(current) = self.current.as_ref() else {
            return Ok(None);
        };
        Ok(Some(read(&current.phrase)))
    }
}

impl ContextCore {
    /// §9 phrase-export materialization: every row the pin's iterator
    /// yields for sub-index `index`, in its order (`pinyin.cpp:662-768`).
    ///
    /// The index is narrowed the way the pin stores it —
    /// `export_iterator_t::m_phrase_index` is a `guint8`
    /// (`pinyin.cpp:126`), so `257` addresses library 1. The system
    /// libraries (`GB_DICTIONARY` … `MERGED_DICTIONARY`) walk their chunk
    /// file; the `USER_FILE` libraries (`ADDON_DICTIONARY`,
    /// [`NETWORK_DICTIONARY`], [`USER_DICTIONARY`]) walk the session store,
    /// and answer `None` without one. Every other nibble, and a system
    /// library that is unloaded, has no sub-index
    /// (`FacadePhraseIndex::get_range` → `ERROR_NO_SUB_PHRASE_INDEX`,
    /// `phrase_index.cpp:610-613`) and exports an empty list. Nibbles
    /// 16..=255 read past the pin's 16-slot array (`phrase_index.cpp:611`),
    /// undefined behaviour this port does not reproduce: they are empty too.
    ///
    /// `None` without a user store for a `USER_FILE` library, when the store
    /// cannot be read (at the begin probe or at any later read), or when the
    /// pin's iterator would not complete the walk: an item it dies at
    /// ([`ExportAssert`]) or a NULL phrase ([`ExportRow::text_len`]). A
    /// failed snapshot is never a partial one.
    #[must_use]
    pub fn export_phrases(&self, index: u32) -> Option<Vec<ExportedPhrase>> {
        let [index, ..] = index.to_le_bytes();
        match index {
            GB_DICTIONARY..=MERGED_DICTIONARY => self
                .phrase_export_cursor(u32::from(index))
                .ok()?
                .collect_rows(),
            ADDON_DICTIONARY | NETWORK_DICTIONARY | USER_DICTIONARY => {
                self.user.as_ref()?;
                self.phrase_export_cursor(u32::from(index))
                    .ok()?
                    .collect_rows()
            }
            _ => Some(Vec::new()),
        }
    }

    /// `pinyin_begin_get_phrases` (`pinyin.cpp:662-688`): the iterator over
    /// sub-index `index`, probed to its first token with a pronunciation.
    ///
    /// # Errors
    ///
    /// The store could not be read during the probe.
    pub fn phrase_export_cursor(&self, index: u32) -> Result<PhraseExportCursor, UserStoreError> {
        let [index, ..] = index.to_le_bytes();
        let user_library_present = matches!(
            index,
            ADDON_DICTIONARY | NETWORK_DICTIONARY | USER_DICTIONARY
        ) && self
            .user
            .as_ref()
            .is_some_and(|user| user.has_user_library(index));
        let mut cursor = PhraseExportCursor {
            index,
            next_token: NULL_TOKEN,
            next_pronunciation: 0,
            system: self.runtime.as_ref().map(|runtime| runtime.dict()),
            user: self.user.clone(),
            user_library_present,
            memo: Memo::default(),
            current: None,
        };
        // `range.m_range_begin`: tokens start at 1 (`phrase_index.cpp:642`).
        cursor.probe_from((u32::from(index) << 24) | 1)?;
        Ok(cursor)
    }

    /// §9 bigram-export materialization with upstream's filters and
    /// rendering (`pinyin_begin_get_bigram_phrases` in `pinyin.cpp`):
    /// skip `sentence_start` predecessors and counts at or below the
    /// first-seed threshold (`initial_seed − 1` = 68); phrase = prev text +
    /// next text; pinyin = prev pinyin + `'` + next pinyin (one row per
    /// pronunciation combination); count = stored × 2 (upstream's local
    /// `unigram_factor`).
    ///
    /// False when this context cannot render every exportable bigram row
    /// (user-store-only, and at least one stored pair needs the system
    /// phrase index). Callers must fail the snapshot rather than skip those
    /// rows into an incomplete file.
    #[must_use]
    pub fn can_render_export_bigrams(&self) -> bool {
        if self.runtime.is_some() {
            return true;
        }
        let Some(store) = self.user.as_ref() else {
            return true;
        };
        let Ok(raw) = store.export_bigrams() else {
            return false;
        };
        !raw.iter().any(|(prev, cur, count)| {
            *prev != SENTENCE_START
                && *count >= INITIAL_SEED
                && (!is_user_file_token(*prev) || !is_user_file_token(*cur))
        })
    }

    /// The pin's bigram export iterator over this context
    /// (`pinyin_begin_get_bigram_phrases`, `pinyin.cpp:775-787`): the
    /// predecessors in the user bigram container's walk order
    /// (`get_all_items`), taken at begin. Each predecessor's gram is
    /// loaded, and each token rendered, only when the walk reaches it
    /// (`pinyin.cpp:885-890`), from the live store. `None` without a user
    /// store.
    #[must_use]
    pub fn bigram_export_walk(&self) -> Option<BigramExportWalk> {
        let store = self.user.as_ref()?;
        let items = store.bigram_predecessors().ok()?;
        Some(BigramExportWalk {
            items: items.into(),
            source: Some(ExportSource {
                user: store.clone(),
                dict: self.runtime.as_ref().map(oxpinyin_runtime::Runtime::dict),
            }),
            ..BigramExportWalk::default()
        })
    }

    /// Every §9 bigram-export row the pin's iterator yields, in its order:
    /// [`BigramExportWalk`] driven the way a caller drives the C iterator
    /// (`while has_next { get_next }`).
    #[must_use]
    pub fn export_bigram_rows(&self) -> Option<Vec<ExportedBigramRow>> {
        let mut walk = self.bigram_export_walk()?;
        let mut rows = Vec::new();
        while walk.has_next() {
            let BigramStep::Row(row, _) = walk.get_next() else {
                break;
            };
            rows.push(row);
        }
        Some(rows)
    }
}

/// What a [`BigramExportWalk`] loads grams and renders tokens from: the
/// context's user store, and its system dictionary when it has one.
#[derive(Clone)]
struct ExportSource {
    user: oxpinyin_user::UserStore,
    dict: Option<oxpinyin_runtime::RuntimeDict>,
}

impl ExportSource {
    /// `load(token, gram, true)` then `retrieve_all` (`pinyin.cpp:889-890`):
    /// the gram's whole items in stored order, duplicates included.
    fn gram(&self, token: u32) -> Vec<(u32, u32)> {
        self.user
            .bigram_successors(token)
            .unwrap_or_default()
            .into_iter()
            .map(|(cur, count)| (cur, u32::try_from(count).unwrap_or(u32::MAX)))
            .collect()
    }

    /// `(text, pinyin spellings)` for a token: user tokens render from the
    /// user store's phrase/pronunciation tables, system tokens from the
    /// system phrase index and the pinyin index (reverse-scanned).
    fn render(&self, token: u32) -> Option<(String, Vec<String>)> {
        if is_user_file_token(token) {
            let phrase = self.user.phrase(token).ok().flatten()?;
            // Render each reading through the shared `render_pinyin` helper,
            // skipping any unrenderable one — the same rule `export_phrases`
            // applies, so the phrase and bigram exports stay consistent.
            let pinyins: Vec<String> = phrase
                .pronunciations()
                .iter()
                .filter_map(UserPronunciation::render_pinyin)
                .collect();
            if pinyins.is_empty() {
                return None;
            }
            Some((phrase.text().to_owned(), pinyins))
        } else {
            let dict = self.dict.as_ref()?;
            let text = dict.system().phrase_text(token)?;
            let pinyins: Vec<String> = dict
                .system()
                .pronunciations(token)
                .into_iter()
                .map(|(pinyin, _freq)| pinyin)
                .collect();
            if pinyins.is_empty() {
                return None;
            }
            Some((text, pinyins))
        }
    }
}
/// One rendered §9 bigram-export row: concatenated phrase text, the
/// `'`-joined pronunciation of the pair, and the scaled count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportedBigramRow {
    /// Concatenated predecessor + successor phrase text.
    pub phrase: String,
    /// The `'`-joined pronunciation of the pair.
    pub pinyin: String,
    /// The rendered bigram count (`stored × 2`).
    pub count: i64,
}

/// `null_token` (`novel_types.h:131`).
const NULL_TOKEN: u32 = 0;

/// The first-seed threshold the export filters on: a pair is exported
/// when its count is above `initial_seed - 1` (`pinyin.cpp:792-793`).
const EXPORT_THRESHOLD: u32 = 23 * 3 - 1;

/// `_bigram_export_iterator_t` (`pinyin.cpp:132-146`) and its two
/// operations, ported state for state over the live store
/// ([`ContextCore::bigram_export_walk`]). Nothing here reorders, filters
/// or repairs what the pin does: `m_items` is the container's walk order
/// taken at begin and consumed from the front; each predecessor's gram is
/// loaded when the walk reaches it and *appended* to `m_phrase_tokens`
/// (`SingleGram::retrieve_all`), so a `sentence_start` gram, which is never
/// scanned, is scanned under the predecessor loaded after it; and the loop
/// ends as soon as loading has emptied `m_items`, so the last
/// predecessor's gram is scanned only by a further `has_next` after one
/// that answered `false`.
#[derive(Clone, Default)]
pub struct BigramExportWalk {
    items: VecDeque<u32>,
    index_token: u32,
    phrase_tokens: VecDeque<(u32, u32)>,
    phrase: Option<String>,
    pinyins: Vec<String>,
    pinyin_index: usize,
    count: u32,
    /// The live store the grams load from and the tokens render from;
    /// `None` for a walk the tests plant through `grams` and `rendered`.
    source: Option<ExportSource>,
    /// Planted grams, read when there is no `source`.
    grams: HashMap<u32, Vec<(u32, u32)>>,
    /// Every token rendered so far: `render` is an O(pinyin-index) scan
    /// and a token recurs across many grams.
    rendered: HashMap<u32, Option<(String, Vec<String>)>>,
}

impl std::fmt::Debug for BigramExportWalk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BigramExportWalk")
            .field("items", &self.items)
            .field("index_token", &self.index_token)
            .field("phrase_tokens", &self.phrase_tokens)
            .field("pinyin_index", &self.pinyin_index)
            .finish_non_exhaustive()
    }
}

/// What one [`BigramExportWalk::get_next`] produced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BigramStep {
    /// A row, and the `has_next` the pin's `get_next` returns after it
    /// (`pinyin.cpp:910`).
    Row(ExportedBigramRow, bool),
    /// No current predecessor, or `sentence_start` — the pin's
    /// `assert(iter->m_index_token != null_token && iter->m_index_token
    /// != sentence_start)` (`pinyin.cpp:902`), which aborts.
    Aborts,
    /// No pinyin at the current position: the pin reads past `m_pinyins`
    /// (`pinyin.cpp:905`), undefined behaviour.
    Undefined,
}

impl BigramExportWalk {
    /// `pinyin_bigram_iterator_has_next_phrase` (`pinyin.cpp:790-893`).
    pub fn has_next(&mut self) -> bool {
        if self.phrase.is_some() && self.pinyin_index < self.pinyins.len() {
            return true;
        }
        // Clean up old values.
        self.pinyin_index = 0;
        self.pinyins.clear();
        let mut retval = false;
        loop {
            if self.index_token != NULL_TOKEN && self.index_token != SENTENCE_START {
                while let Some(&(token, count)) = self.phrase_tokens.front() {
                    // Find the next item above the threshold.
                    if count > EXPORT_THRESHOLD {
                        self.load_row(token, count);
                        self.phrase_tokens.pop_front();
                        retval = true;
                        break;
                    }
                    self.phrase_tokens.pop_front();
                }
            }
            if retval || self.items.is_empty() {
                break;
            }
            let Some(index_token) = self.items.pop_front() else {
                break;
            };
            self.index_token = index_token;
            // `load(…, user_gram, true)` then `retrieve_all`, which appends.
            let gram = match &self.source {
                Some(source) => source.gram(index_token),
                None => self.grams.get(&index_token).cloned().unwrap_or_default(),
            };
            self.phrase_tokens.extend(gram);
            // `} while (iter->m_items->len);`
            if self.items.is_empty() {
                break;
            }
        }
        retval
    }

    /// `pinyin_bigram_iterator_get_next_phrase` (`pinyin.cpp:896-911`):
    /// the current row — count × 2 in `guint32` — then `has_next`.
    pub fn get_next(&mut self) -> BigramStep {
        if self.index_token == NULL_TOKEN || self.index_token == SENTENCE_START {
            return BigramStep::Aborts;
        }
        let (Some(phrase), Some(pinyin)) = (
            self.phrase.clone(),
            self.pinyins.get(self.pinyin_index).cloned(),
        ) else {
            return BigramStep::Undefined;
        };
        let row = ExportedBigramRow {
            phrase,
            pinyin,
            count: i64::from(self.count.wrapping_mul(2).cast_signed()),
        };
        self.pinyin_index += 1;
        let more = self.has_next();
        BigramStep::Row(row, more)
    }

    /// `token`'s rendering, memoised.
    fn render(&mut self, token: u32) {
        if !self.rendered.contains_key(&token) {
            let rendering = self.source.as_ref().and_then(|source| source.render(token));
            self.rendered.insert(token, rendering);
        }
    }

    /// The row `has_next` builds for `(m_index_token, token)`
    /// (`pinyin.cpp:810-870`): the two texts concatenated, and every
    /// first-pronunciation × second-pronunciation pair joined by `'`. A
    /// token the phrase index does not resolve reads as the pin's empty
    /// `PhraseItem` — no text, no pronunciation.
    fn load_row(&mut self, token: u32, count: u32) {
        self.render(self.index_token);
        self.render(token);
        let empty = (String::new(), Vec::new());
        let (first_text, first_pinyins) = self
            .rendered
            .get(&self.index_token)
            .and_then(Option::as_ref)
            .unwrap_or(&empty);
        let (second_text, second_pinyins) = self
            .rendered
            .get(&token)
            .and_then(Option::as_ref)
            .unwrap_or(&empty);
        self.phrase = Some(format!("{first_text}{second_text}"));
        self.count = count;
        self.pinyins = first_pinyins
            .iter()
            .flat_map(|first| {
                second_pinyins
                    .iter()
                    .map(move |second| format!("{first}'{second}"))
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::{BigramExportWalk, BigramStep, ExportedBigramRow, SENTENCE_START};

    fn walk(items: &[u32], grams: &[(u32, &[(u32, u32)])]) -> BigramExportWalk {
        let mut walk = BigramExportWalk {
            items: items.iter().copied().collect(),
            ..BigramExportWalk::default()
        };
        for (prev, gram) in grams {
            walk.grams.insert(*prev, gram.to_vec());
        }
        for (token, text, pinyin) in [
            (0x0100_0001, "你", "ni"),
            (0x0100_0002, "好", "hao"),
            (0x0100_0003, "我", "wo"),
            (0x0100_0004, "爱", "ai"),
        ] {
            walk.rendered
                .insert(token, Some((text.to_owned(), vec![pinyin.to_owned()])));
        }
        walk
    }

    /// A caller that trusts `get_next`'s return: stops on the first `false`.
    fn drain_by_return(walk: &mut BigramExportWalk) -> Vec<(String, bool)> {
        let mut rows = Vec::new();
        let mut more = walk.has_next();
        while more {
            let BigramStep::Row(ExportedBigramRow { phrase, .. }, next) = walk.get_next() else {
                break;
            };
            rows.push((phrase, next));
            more = next;
        }
        rows
    }

    /// ibus-libpinyin's loop (`PYLibPinyin.cc:317-329`): `while (has_next)
    /// get_next`.
    fn drain_by_has_next(walk: &mut BigramExportWalk) -> Vec<(String, bool)> {
        let mut rows = Vec::new();
        while walk.has_next() {
            let BigramStep::Row(ExportedBigramRow { phrase, .. }, next) = walk.get_next() else {
                break;
            };
            rows.push((phrase, next));
        }
        rows
    }

    fn scenario() -> BigramExportWalk {
        walk(
            &[SENTENCE_START, 0x0100_0001, 0x0100_0003],
            &[
                (SENTENCE_START, &[(0x0100_0003, 69)]),
                (0x0100_0001, &[(0x0100_0002, 138)]),
                (0x0100_0003, &[(0x0100_0004, 69)]),
            ],
        )
    }

    /// Register row 36 and the pin's walk (`pinyin.cpp:790-911`):
    /// `get_next` answers `has_next` after the row, so a caller that
    /// trusts it stops before the last predecessor, whose gram was loaded
    /// but never scanned; a `sentence_start` gram's successors are scanned
    /// under the next predecessor.
    #[test]
    fn the_walk_is_the_pins_state_machine() {
        let mut walk = scenario();
        assert_eq!(
            drain_by_return(&mut walk),
            vec![("你我".to_owned(), true), ("你好".to_owned(), false)]
        );
        // A further has_next scans the gram loaded but never scanned.
        assert!(walk.has_next());
        assert_eq!(
            walk.get_next(),
            BigramStep::Row(
                ExportedBigramRow {
                    phrase: "我爱".to_owned(),
                    pinyin: "wo'ai".to_owned(),
                    count: 138,
                },
                false,
            )
        );
        assert!(!walk.has_next());
        assert_eq!(walk.get_next(), BigramStep::Undefined);
    }

    /// ibus-libpinyin's `while (has_next)` loop therefore exports the last
    /// predecessor's rows too.
    #[test]
    fn a_has_next_loop_sees_the_last_predecessor() {
        assert_eq!(
            drain_by_has_next(&mut scenario()),
            vec![
                ("你我".to_owned(), true),
                ("你好".to_owned(), false),
                ("我爱".to_owned(), false),
            ]
        );
    }

    #[test]
    fn get_next_without_a_predecessor_is_the_pins_abort() {
        let mut walk = walk(&[], &[]);
        assert!(!walk.has_next());
        assert_eq!(walk.get_next(), BigramStep::Aborts);
    }
}

#[cfg(test)]
mod phrase_cursor_tests {
    use super::{ContextCore, ExportAssert, ExportStep, PhraseExportCursor, assert_for};
    use oxpinyin_core::{ChewingKey, SyllableKey};
    use oxpinyin_user::{
        ExportedPhrase, KEY_SYLLABLE_MASK, KEY_TONE_SHIFT, MAX_KEY_TONE, USER_DICTIONARY,
        UserConfLaw, UserStoreError, toned_key,
    };

    /// A transient context over the committed mini tables of whichever
    /// backend this build carries (a NULL user directory: imports work, the
    /// store is in memory, #642).
    fn open() -> ContextCore {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3");
        ["db", "kct", "tkt"]
            .into_iter()
            .find_map(|ext| {
                ContextCore::open(
                    &root.join(ext),
                    None,
                    crate::PINYIN_DEFAULT_OPTION_WORD,
                    UserConfLaw::Pinyin,
                )
            })
            .expect("committed backend fixtures")
    }

    fn user_keys(spellings: &[&str], tone: u8) -> Vec<oxpinyin_user::PinyinKey> {
        spellings
            .iter()
            .map(|spelling| {
                toned_key(
                    SyllableKey::from_text(spelling).expect("syllable").index(),
                    tone,
                )
                .expect("key")
            })
            .collect()
    }

    fn row(text: &str, pinyin: &str, count: u64) -> ExportedPhrase {
        ExportedPhrase {
            text: text.to_owned(),
            pinyin: pinyin.to_owned(),
            count,
        }
    }

    /// One `get_next`, read as the materialization reads it.
    fn next_row(cursor: &mut PhraseExportCursor) -> Option<ExportedPhrase> {
        match cursor.get_next_with(|row| {
            row.text_string().map(|text| ExportedPhrase {
                text,
                pinyin: row.pinyin_string(),
                count: row.count(),
            })
        }) {
            Ok(ExportStep::Row(row)) => row,
            Ok(ExportStep::Assert(_)) | Err(_) => None,
        }
    }

    #[test]
    fn begin_probes_one_token_and_draining_matches_the_materialization() {
        let ctx = open();
        let mut cursor = ctx.phrase_export_cursor(257).expect("begin");
        assert!(cursor.has_next());
        assert_eq!(cursor.next_token >> 24, 1, "the guint8 alias of library 1");
        assert_eq!(cursor.next_pronunciation, 0);
        let all = cursor.collect_rows().expect("every row");
        assert!(!all.is_empty());
        assert_eq!(all, ctx.export_phrases(1).unwrap());
    }

    #[test]
    fn an_unused_nibble_and_the_pins_asserts_are_reported() {
        let ctx = open();
        for index in [0, 8, 15, 16, 255] {
            let mut cursor = ctx.phrase_export_cursor(index).expect("begin");
            assert!(!cursor.has_next(), "nibble {index}");
            assert!(matches!(
                cursor.get_next_with(|_| ()),
                Ok(ExportStep::Assert(ExportAssert::Item))
            ));
        }
        assert!(!PhraseExportCursor::default().has_next());
    }

    /// `pinyin.cpp:728` → `chewing_key.cpp:48` and `:50`: a reading the
    /// probe counts (`:682`) but `get_pinyin_string` cannot spell is read
    /// and refused with the cursor unmoved, so the next call refuses again;
    /// the materialization fails rather than skipping it (the parent build
    /// skipped the reading).
    #[test]
    fn a_reading_the_engine_cannot_spell_is_the_pins_pinyin_string_assert() {
        let jia = SyllableKey::from_text("jia").expect("syllable").index();
        let past_the_tones = toned_key(jia, MAX_KEY_TONE).expect("key") + (1 << KEY_TONE_SHIFT);
        for (tag, key, site) in [
            ("tone", past_the_tones, ExportAssert::KeyTone),
            ("table", KEY_SYLLABLE_MASK, ExportAssert::KeyTable),
        ] {
            let mut ctx = open();
            let store = ctx.user.as_mut().unwrap();
            store
                .add_phrase_in(
                    USER_DICTIONARY,
                    "你好",
                    &user_keys(&["ni", "hao"], 0),
                    Some(5),
                )
                .expect("first");
            store
                .add_phrase_in(USER_DICTIONARY, "甲", &[key], Some(2))
                .expect("the add checks lengths only");
            let mut cursor = ctx
                .phrase_export_cursor(u32::from(USER_DICTIONARY))
                .expect("begin");
            assert_eq!(
                next_row(&mut cursor),
                Some(row("你好", "ni'hao", 5)),
                "{tag}"
            );
            assert!(cursor.has_next(), "{tag}: the probe selects the item");
            for _ in 0..2 {
                assert!(
                    matches!(cursor.get_next_with(|_| ()), Ok(ExportStep::Assert(at)) if at == site),
                    "{tag}"
                );
            }
            assert!(
                ctx.export_phrases(u32::from(USER_DICTIONARY)).is_none(),
                "{tag}: no partial snapshot"
            );
        }
    }

    /// A present item the store cannot decode is read at the pin and dies
    /// at `:714` (its text) or `:724` (a pronunciation row); a store that
    /// cannot be read is the error itself, which the pin never has.
    #[test]
    fn an_undecodable_item_is_an_assert_site_and_a_store_failure_an_error() {
        assert_eq!(
            assert_for(&UserStoreError::Decode),
            Some(ExportAssert::PhraseString)
        );
        assert_eq!(
            assert_for(&UserStoreError::DecodePronunciation),
            Some(ExportAssert::NthPronunciation)
        );
        assert_eq!(
            assert_for(&UserStoreError::Io(std::io::Error::other("unreadable"))),
            None
        );
        assert_eq!(assert_for(&UserStoreError::AlreadyOpen), None);
    }

    /// `pinyin.cpp:748-767`: every `get_next_phrase` re-reads the range
    /// and the next item, so a reading imported under the token the cursor
    /// is about to read is exported with it.
    #[test]
    fn the_re_probe_reads_a_reading_imported_under_the_next_system_token() {
        let mut ctx = open();
        let mut cursor = ctx.phrase_export_cursor(1).expect("begin");
        let first = next_row(&mut cursor);
        assert!(first.is_some());
        let next = cursor.next_token;
        assert_eq!(next >> 24, 1);
        assert_eq!(
            cursor.next_pronunciation, 0,
            "the next token's first reading"
        );
        let dict = ctx.runtime.as_ref().unwrap().dict();
        let text = dict.system().phrase_text(next).expect("text");
        let library = dict.system().libraries().library(next).expect("library");
        let item = library.item(next).expect("item");
        let pronunciation = item.pronunciation(0).expect("reading");
        let spellings: Vec<&'static str> = pronunciation
            .keys
            .chunks_exact(2)
            .map(|pair| {
                ChewingKey::from_packed(u16::from_le_bytes([pair[0], pair[1]])).pinyin_spelling()
            })
            .collect();
        let stored = dict.system().pronunciations(next);
        drop(library);
        // The same syllables with tone 1: a reading the item does not hold.
        let keys = user_keys(&spellings, 1);
        ctx.user
            .as_mut()
            .unwrap()
            .add_system_phrase_in(1, Some(next), &text, &keys, Some(5))
            .expect("import");
        let toned = spellings
            .iter()
            .map(|spelling| format!("{spelling}1"))
            .collect::<Vec<_>>()
            .join("'");
        let mut rows = Vec::new();
        for _ in 0..=stored.len() {
            rows.push(next_row(&mut cursor).expect("row"));
        }
        let mut expected: Vec<_> = stored
            .iter()
            .map(|(pinyin, count)| row(&text, pinyin, *count))
            .collect();
        expected.push(row(&text, &toned, 5));
        assert_eq!(rows, expected);
    }

    /// The same for a `USER_FILE` library: a phrase appended after the
    /// begin and a reading merged into the item under the cursor are both
    /// exported, the pin's token walk over the live sub-index.
    #[test]
    fn a_user_library_import_after_begin_is_exported() {
        let mut ctx = open();
        let store = ctx.user.as_mut().unwrap();
        store
            .add_phrase_in(
                USER_DICTIONARY,
                "你好",
                &user_keys(&["ni", "hao"], 0),
                Some(5),
            )
            .expect("first");
        let mut cursor = ctx
            .phrase_export_cursor(u32::from(USER_DICTIONARY))
            .expect("begin");
        assert!(cursor.has_next());
        let store = ctx.user.as_mut().unwrap();
        store
            .add_phrase_in(
                USER_DICTIONARY,
                "好你",
                &user_keys(&["hao", "ni"], 0),
                Some(7),
            )
            .expect("appended");
        store
            .add_phrase_in(
                USER_DICTIONARY,
                "你好",
                &user_keys(&["ni", "hao"], 2),
                Some(3),
            )
            .expect("merged reading");
        let rows = cursor.collect_rows().expect("every row");
        assert_eq!(
            rows,
            vec![
                row("你好", "ni'hao", 5),
                row("你好", "ni2'hao2", 3),
                row("好你", "hao'ni", 7),
            ]
        );
    }
}
