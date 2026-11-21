//! The §9 export cursors: user-store rows rendered the way the
//! C ABI's export iterators yield them.
//!
//! Pure Rust over the context's user store and optional runtime — no C
//! types cross this boundary.

use crate::marshal::phrase_text_bytes;
use crate::runtime::{PhraseItemView, RuntimeDict};
use crate::user::{MAX_KEY_TONE, SystemItem, UserPronunciation, UserStore};
use crate::{ChewingKey, NULL_TOKEN, PhraseText, SENTENCE_START};

use std::collections::VecDeque;

use crate::facade::ContextCore;

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
/// it from the live buffer, and an item from the session arena. The probes skip a
/// token only where the pin's do, an absent item or one with no
/// pronunciation; a present item that is malformed is read and refused at
/// the pin's assert site ([`ExportAssert`]). Every read is infallible, as
/// the pin's in-memory sub-index read is: a libpinyin session keeps its
/// phrases in memory, in per-library arenas (policy row 62).
///
/// What the store answers is held while its write generation stands still
/// and re-read as soon as it moves: an arena's token end. The pin re-reads
/// its range on every call because it is an offset read; the session
/// store's is a range scan, and no write can change its answer without
/// moving the generation. An item is read from the store on every call.
#[derive(Default)]
pub struct PhraseExportCursor {
    index: u8,
    next_token: u32,
    next_pronunciation: usize,
    memo: Memo,
}

/// Store answers valid at one write generation.
#[derive(Default)]
struct Memo {
    generation: Option<u64>,
    /// One past the highest token of a library backed by an arena.
    user_end: u32,
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
    /// `assert(nth_pronun < n_pronuns)` (`:721`): the item no longer holds
    /// the pronunciation the previous call left the cursor at.
    Pronunciation,
    /// `assert(m_tone < CHEWING_NUMBER_OF_TONES)` in `get_pinyin_string`
    /// (`chewing_key.cpp:48`, reached from `:728` for each key in turn): a
    /// reading carries a tone past `MAX_KEY_TONE`.
    KeyTone,
    /// The initial, middle and final asserts of `get_table_index`
    /// (`chewing_key.cpp:30-32`, reached from `:49` after the tone assert): a
    /// key's field lies outside the table dimensions.
    KeyFields,
}

/// What one [`PhraseExportCursor::get_next_with`] call produced: a row, or
/// the assert site the pin dies at. There is no third outcome: the pin's
/// sub-index read cannot fail, and neither can the session's.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportStep<R> {
    /// A row.
    Row(R),
    /// One of the pin's assert sites; the cursor stays where it is, so a
    /// repeated call answers the same.
    Assert(ExportAssert),
}

/// One row the cursor is reading, borrowed from its item: the phrase, one
/// pronunciation and that pronunciation's count, as
/// `pinyin_iterator_get_next_phrase` renders them (`pinyin.cpp:712-739`).
/// The writers put the text and the `'`-joined pinyin straight into buffers
/// of the reported lengths; the string builders allocate each once.
pub struct ExportRow<'a>(RowSource<'a>);

enum RowSource<'a> {
    /// A system library's own item or a session arena's: the phrase as
    /// stored `ucs4_t` code points, the pronunciation as packed `ChewingKey`
    /// bytes.
    Mapped {
        item: PhraseItemView<'a>,
        keys: &'a [u8],
        count: u32,
    },
}

/// The syllables of one pronunciation: each a spelling and the tone digit
/// `ChewingKey::get_pinyin_string` appends to it (`chewing_key.cpp:47-57`
/// at 074a2219: the content table's pinyin, then `%d` of a non-zero tone).
enum Syllables<'a> {
    Packed(std::slice::Iter<'a, [u8; 2]>),
}

impl Iterator for Syllables<'_> {
    type Item = (&'static str, Option<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Packed(pairs) => pairs
                .next()
                .map(|pair| chewing_syllable(ChewingKey::from_packed(u16::from_le_bytes(*pair)))),
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

/// The keys of one stored reading, from its packed little-endian words.
fn packed_keys(keys: &[u8]) -> impl Iterator<Item = ChewingKey> + '_ {
    keys.as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| ChewingKey::from_packed(u16::from_le_bytes(*bytes)))
}

/// The first `get_pinyin_string` assert the pin dies at while it renders
/// the keys of one reading in stored order, if any. Each key asserts its
/// tone (`chewing_key.cpp:48` at 074a2219) and then, in `get_table_index`,
/// its initial, middle and final (`:30-32`, from `:49`). The phrase export
/// (`pinyin.cpp:726-728`) and the bigram export (`:846`, `:859`) render
/// through the same call.
fn reading_assert(mut keys: impl Iterator<Item = ChewingKey>) -> Option<ExportAssert> {
    keys.find_map(|key| {
        if key.tone > MAX_KEY_TONE {
            Some(ExportAssert::KeyTone)
        } else if !key.elements_in_dims() {
            Some(ExportAssert::KeyFields)
        } else {
            None
        }
    })
}

impl ExportRow<'_> {
    /// The row's stored phrase text. The C encoding is applied only in
    /// [`Self::with_text_bytes`].
    fn phrase_text(&self) -> Option<PhraseText> {
        match &self.0 {
            RowSource::Mapped { item, .. } => item.phrase_text(),
        }
    }

    /// Lends the phrase's outward bytes, encoded once, to `with`, or answers
    /// `None` where the pin's `g_ucs4_to_utf8` hands the caller a NULL phrase
    /// (`pinyin.cpp:715-716`), the row's other fields intact.
    pub fn with_text_bytes<R>(&self, with: impl FnOnce(&[u8]) -> R) -> Option<R> {
        let text = self.phrase_text()?;
        Some(with(&phrase_text_bytes(&text)))
    }

    /// The `get_pinyin_string` assert the pin dies at while rendering this
    /// row's keys (`pinyin.cpp:726-728`), if any. Each key in turn asserts
    /// its tone (`chewing_key.cpp:48`) and then, in `get_table_index`, its
    /// initial, middle and final (`:30-32`, from `:49`). The pin renders a
    /// system item's keys and a user item's through the same call. A key
    /// inside the dimensions with no table row is not a site: its index is
    /// zero and the content-table assert (`:50`) holds.
    #[must_use]
    pub fn key_assert(&self) -> Option<ExportAssert> {
        match &self.0 {
            RowSource::Mapped { keys, .. } => reading_assert(packed_keys(keys)),
        }
    }

    /// The pronunciation's key strings, each the bytes of
    /// `_ChewingKey::get_pinyin_string` (`pinyin.cpp:726-728`) as its
    /// spelling and the digit of a non-zero tone: the elements of the pin's
    /// `g_strjoinv("'", …)` (`:733`).
    pub fn pinyin_strings(&self) -> impl Iterator<Item = (&'static str, Option<u8>)> + '_ {
        self.syllables()
    }

    /// The pronunciation's stored count (`item.get_nth_pronunciation`'s
    /// `freq`, `pinyin.cpp:724`).
    #[must_use]
    pub fn count(&self) -> u64 {
        match &self.0 {
            RowSource::Mapped { count, .. } => u64::from(*count),
        }
    }

    fn syllables(&self) -> Syllables<'_> {
        match &self.0 {
            RowSource::Mapped { keys, .. } => Syllables::Packed(keys.as_chunks::<2>().0.iter()),
        }
    }
}

/// One item under a token, wherever it lives.
enum ItemRef<'a> {
    Mapped(PhraseItemView<'a>),
    Arena(PhraseItemView<'a>),
}

impl<'a> From<SystemItem<'a>> for ItemRef<'a> {
    fn from(item: SystemItem<'a>) -> Self {
        match item {
            SystemItem::Shipped(item) | SystemItem::Replayed(item) => Self::Mapped(item),
        }
    }
}

impl ItemRef<'_> {
    /// `item.get_n_pronunciation()`.
    fn n_pronunciations(&self) -> usize {
        match self {
            Self::Mapped(item) | Self::Arena(item) => item.n_pronunciations(),
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
    /// (`pinyin.cpp:724`), `None` for an `nth` at or past the item's count.
    fn row(&self, nth: usize) -> Option<ExportRow<'_>> {
        Some(ExportRow(match self {
            Self::Mapped(item) | Self::Arena(item) => {
                let pronunciation = item.pronunciation(nth)?;
                RowSource::Mapped {
                    item: *item,
                    keys: pronunciation.keys,
                    count: pronunciation.freq,
                }
            }
        }))
    }
}

struct PhraseExportRead<'a> {
    cursor: &'a mut PhraseExportCursor,
    system: RuntimeDict<'a>,
    user: &'a UserStore,
}

impl std::ops::Deref for PhraseExportRead<'_> {
    type Target = PhraseExportCursor;
    fn deref(&self) -> &Self::Target {
        self.cursor
    }
}
impl std::ops::DerefMut for PhraseExportRead<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.cursor
    }
}

impl PhraseExportCursor {
    fn read<'a>(&'a mut self, context: &'a ContextCore) -> PhraseExportRead<'a> {
        PhraseExportRead {
            cursor: self,
            system: context.runtime.dict(),
            user: context.user_store(),
        }
    }

    // pinyin_iterator_get_next_phrase, pinyin.cpp:698–768.
    /// Reads the currently probed item and pronunciation with the supplied context. The
    /// cursor calls `read` only after checking the item and keys, then advances to the next
    /// reading or token. A failed check returns an [`ExportStep`] with the cursor still at
    /// that row.
    pub fn get_next_with<R>(
        &mut self,
        context: &ContextCore,
        read: impl FnOnce(&ExportRow<'_>) -> R,
    ) -> ExportStep<R> {
        self.read(context).get_next_with(read)
    }

    /// `pinyin_iterator_has_next_phrase` (`pinyin.cpp:690-695`): whether
    /// the begin or the previous call probed a token.
    #[must_use]
    pub fn has_next(&self) -> bool {
        self.next_token != NULL_TOKEN
    }
}

impl PhraseExportRead<'_> {
    /// `pinyin_iterator_get_next_phrase` (`pinyin.cpp:698-768`): reads the
    /// probed item, hands `read` the pronunciation the cursor is at, then
    /// moves to the item's next pronunciation or, when there is none,
    /// re-reads the range and probes the next token with a pronunciation
    /// (`:742-767`). The call meets the pin's assert sites in the pin's
    /// order ([`ExportAssert`]) and answers the first one it hits with the
    /// cursor unmoved. The reads cannot fail, as the pin's cannot.
    pub fn get_next_with<R>(&mut self, read: impl FnOnce(&ExportRow<'_>) -> R) -> ExportStep<R> {
        let token = self.next_token;
        if token == NULL_TOKEN {
            return ExportStep::Assert(ExportAssert::Item);
        }
        let nth = self.next_pronunciation;
        let step = self.with_item(token, |item| {
            // An item holds every reading below its count: its view is
            // sized from its own length and count bytes, as the pin's chunk
            // is (`phrase_index.cpp:199-203` at 074a2219). The row is
            // missing only past the count, the pin's `:721` assert; its
            // `:724` read cannot fail after that (`phrase_index.cpp:33-44`).
            let row = item.row(nth).ok_or(ExportAssert::Pronunciation)?;
            if let Some(site) = row.key_assert() {
                return Err(site);
            }
            Ok((read(&row), item.next_pronunciation(nth + 1)))
        });
        let (out, following) = match step {
            Some(Err(site)) => return ExportStep::Assert(site),
            None => return ExportStep::Assert(ExportAssert::Item),
            Some(Ok(pair)) => pair,
        };
        match following {
            Some(next) => self.next_pronunciation = next,
            None => {
                self.next_pronunciation = 0;
                self.probe_from(token + 1);
            }
        }
        ExportStep::Row(out)
    }

    /// Re-reads the store-derived answers when the store's write generation
    /// moved since they were taken: the session answers the range end from
    /// its arena.
    fn refresh(&mut self) {
        let user = self.user;
        let generation = user.generation();
        if self.memo.generation == Some(generation) {
            return;
        }
        let user_end = if user.originals().get(&self.index).is_none() {
            self.system
                .live_items_ignoring_mask()
                .system_range_end(self.index)
                .unwrap_or(NULL_TOKEN)
        } else {
            NULL_TOKEN
        };
        self.memo = Memo {
            generation: Some(generation),
            user_end,
        };
    }

    /// The probe `pinyin_begin_get_phrases` (`pinyin.cpp:676-687`) and the
    /// re-probe in `get_next_phrase` (`:748-767`) share: a fresh
    /// `get_range`, then the first token from `start` whose item has a
    /// pronunciation. A sub-index that is not loaded ends the iteration.
    fn probe_from(&mut self, start: u32) {
        self.next_token = NULL_TOKEN;
        self.refresh();
        if self.user.originals().get(&self.index).is_some() {
            self.probe_system(start);
        } else if self.system.library_visible(u32::from(self.index)) {
            self.probe_user(start);
        }
    }

    /// `FacadePhraseIndex::get_range` for a system sub-index
    /// (`phrase_index.cpp:610-622`, the trailing-zero end at `:624-646`):
    /// one past the highest nonzero offset in the current buffer; then the forward
    /// probe. An unloaded library is `ERROR_NO_SUB_PHRASE_INDEX`: no token.
    fn probe_system(&mut self, start: u32) {
        let dict = &self.system;
        if !dict.library_visible(u32::from(self.index)) {
            return;
        }
        let live = dict.live_items_ignoring_mask();
        let Some(end) = live.system_range_end(self.index) else {
            return;
        };
        let mut token = start;
        while token < end {
            // `get_phrase_item` fails for a token a journal REMOVE took out,
            // and finds nothing in a slot with no item: neither is selected.
            if let Some(Some(nth)) = live
                .read(token, |item| ItemRef::from(item).first_pronunciation())
                .found()
            {
                self.next_token = token;
                self.next_pronunciation = nth;
                return;
            }
            token += 1;
        }
    }

    /// The same for an arena sub-index, including one made by a promotion.
    /// The pin's probe reads `n_prons` from the item's header (`:682`,
    /// `:763`), so a present item the store cannot decode is selected, not
    /// skipped; the read that follows is the assert.
    fn probe_user(&mut self, start: u32) {
        let end = self.memo.user_end;
        let mut token = start;
        while token < end {
            let found = self
                .with_user_item(token, |item| item.first_pronunciation())
                .flatten();
            if let Some(nth) = found {
                self.next_token = token;
                self.next_pronunciation = nth;
                return;
            }
            token += 1;
        }
    }

    /// `get_phrase_item` for `token` (`phrase_index.cpp:180-206`): the
    /// session's replacement first, then the mapped chunk; a `USER_FILE`
    /// token reads the session store. `None` is `ERROR_NO_ITEM`.
    fn with_item<R>(&mut self, token: u32, read: impl FnOnce(ItemRef<'_>) -> R) -> Option<R> {
        self.refresh();
        if self.user.originals().get(&self.index).is_some() {
            return self
                .system
                .live_items_ignoring_mask()
                .read(token, |item| read(ItemRef::from(item)))
                .found();
        }
        self.with_user_item(token, read)
    }

    /// The `USER_FILE` item under `token`: the session's arena item, or
    /// else the item a system library's live buffer appended, read from the
    /// store on every call. `None` is no item; a session's read cannot fail.
    fn with_user_item<R>(&self, token: u32, read: impl FnOnce(ItemRef<'_>) -> R) -> Option<R> {
        self.user
            .with_user_item(token, |item| read(ItemRef::Arena(item)))
    }
}

impl ContextCore {
    /// `pinyin_begin_get_phrases` (`pinyin.cpp:662-688`): the iterator over
    /// sub-index `index`, probed to its first token with a pronunciation.
    /// The probe cannot fail, as the pin's in-memory one cannot.
    #[must_use]
    pub fn phrase_export_cursor(&self, index: u32) -> PhraseExportCursor {
        let [index, ..] = index.to_le_bytes();
        let mut cursor = PhraseExportCursor {
            index,
            next_token: NULL_TOKEN,
            next_pronunciation: 0,
            memo: Memo::default(),
        };
        // `range.m_range_begin`: tokens start at 1 (`phrase_index.cpp:642`),
        // made a token with `PHRASE_INDEX_MAKE_TOKEN` (`:619`).
        cursor
            .read(self)
            .probe_from(crate::phrase_index_make_token(index, 1));
        cursor
    }

    /// The pin's bigram export iterator over this context
    /// (`pinyin_begin_get_bigram_phrases`, `pinyin.cpp:775-787`): the
    /// predecessors in the user bigram container's walk order
    /// (`get_all_items`), taken at begin. Each predecessor's gram is
    /// loaded, and each token rendered, only when the walk reaches it
    /// (`pinyin.cpp:885-890`), from the live store. `None` when the
    /// predecessor list cannot be read.
    #[must_use]
    pub fn bigram_export_walk(&self) -> Option<BigramExportWalk> {
        let store = self.user_store();
        let items = store.bigram_predecessors().ok()?;
        Some(BigramExportWalk {
            items: items.into(),
            ..BigramExportWalk::default()
        })
    }
}

/// One token's item as the bigram export reads it. A token the phrase
/// index does not resolve is the default: the pin's empty `PhraseItem`.
#[derive(Clone, Default)]
struct RenderedItem {
    /// The phrase text.
    text: PhraseText,
    /// The `'`-joined spelling of each reading this engine renders.
    pinyins: Vec<String>,
    /// One entry for each stored reading, in stored order: the assert the
    /// pin dies at when it renders that reading ([`reading_assert`]).
    sites: Vec<Option<ExportAssert>>,
}

impl RenderedItem {
    /// An item with its stored readings checked. The text and the spellings
    /// are kept only together: an item with no text or no rendered reading
    /// gives neither.
    fn new(
        text: Option<PhraseText>,
        pinyins: Vec<String>,
        sites: Vec<Option<ExportAssert>>,
    ) -> Self {
        match text {
            Some(text) if !pinyins.is_empty() => Self {
                text,
                pinyins,
                sites,
            },
            _ => Self {
                sites,
                ..Self::default()
            },
        }
    }
}

/// The assert the pin dies at while it renders the readings of a pair
/// (`pinyin.cpp:839-872` at 074a2219), if any. The pin renders the first
/// item's reading `i`, then every reading of the second item, for each `i`
/// in turn. A first item with no reading renders nothing.
fn pair_assert(
    first: &[Option<ExportAssert>],
    second: &[Option<ExportAssert>],
) -> Option<ExportAssert> {
    let (head, rest) = first.split_first()?;
    head.or_else(|| second.iter().copied().flatten().next())
        .or_else(|| rest.iter().copied().flatten().next())
}

/// What a [`BigramExportWalk`] loads grams and renders tokens from: the
/// context's user store and its system dictionary.
#[derive(Clone)]
struct ExportSource<'a> {
    user: &'a crate::user::UserStore,
    dict: crate::runtime::RuntimeDict<'a>,
}

impl ExportSource<'_> {
    /// `load(token, gram, true)` then `retrieve_all` (`pinyin.cpp:889-890`):
    /// the gram's whole items in stored order, duplicates included, or the
    /// fault where `retrieve_all`'s `get_total_freq` asserts on a value too
    /// short for its total (`ngram.cpp:141`, `memory_chunk.h:390`; register
    /// row 85).
    fn gram(&self, token: u32) -> Result<Vec<(u32, u32)>, GramLoadFault> {
        if self.user.has_short_bigram_value_for(token) {
            return Err(GramLoadFault::ShortTotal { token });
        }
        if self.user.has_oob_bigram_value_for(token) {
            return Err(GramLoadFault::PartialTail { token });
        }
        Ok(self
            .user
            .bigram_successors(token)
            .unwrap_or_default()
            .into_iter()
            .map(|(cur, count)| (cur, u32::try_from(count).unwrap_or(u32::MAX)))
            .collect())
    }

    /// The item of a token: user tokens render from the user store's
    /// phrase/pronunciation tables, system tokens from the system phrase
    /// index and the pinyin index (reverse-scanned). Every stored reading
    /// is checked with [`reading_assert`], the check
    /// [`ExportRow::key_assert`] performs, whether or not it is rendered.
    fn render(&self, token: u32) -> RenderedItem {
        // `get_phrase_item` resolves a gram's token by its masked library
        // (074a2219 pinyin.cpp:812-815, phrase_index.h:647).
        let is_system = self
            .user
            .is_system_library(crate::phrase_index_library_index(token));
        if is_system {
            return self
                .dict
                .live_items()
                .read(token, |item| {
                    let sites = item
                        .pronunciations()
                        .map(|reading| {
                            reading_assert(reading.keys.iter().map(ChewingKey::from_packed))
                        })
                        .collect();
                    let pinyins: Vec<String> = item
                        .pronunciations()
                        .filter_map(|reading| {
                            let words: Vec<_> = reading
                                .keys
                                .iter()
                                .map(|packed| ChewingKey::from_packed(packed).pinyin_spelling())
                                .collect();
                            (!words.is_empty() && words.iter().all(|word| !word.is_empty()))
                                .then(|| words.join("'"))
                        })
                        .collect();
                    RenderedItem::new(item.phrase_text(), pinyins, sites)
                })
                .found()
                .unwrap_or_default();
        }
        let sites = self
            .user
            .with_user_item(token, |item| {
                item.pronunciations()
                    .map(|reading| reading_assert(packed_keys(reading.keys)))
                    .collect()
            })
            .unwrap_or_default();
        let Some(phrase) = self.user.phrase(token) else {
            return RenderedItem::new(None, Vec::new(), sites);
        };
        let pinyins: Vec<String> = phrase
            .pronunciations()
            .iter()
            .filter_map(UserPronunciation::render_pinyin)
            .collect();
        RenderedItem::new(phrase.phrase_text(), pinyins, sites)
    }
}
/// One rendered §9 bigram-export row: the `'`-joined pronunciation of the
/// pair and the scaled count. The row's phrase is the pin's `m_phrase`, the
/// conversion of the pair `has_next` loaded ([`BigramExportWalk::pair`]),
/// which the C adapter converts and keeps on its handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportedBigramRow {
    /// The `'`-joined pronunciation of the pair.
    pub pinyin: String,
    /// The rendered bigram count (`stored × 2`).
    pub count: i64,
}

/// A predecessor's stored user-bigram value the pin's `retrieve_all`
/// cannot read, met where the bigram export walk loads it
/// (`pinyin.cpp:888-889`, `ngram.cpp:133-143`), after the rows of the
/// predecessors before it were handed out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GramLoadFault {
    /// Shorter than its `guint32` total: `get_total_freq`'s assert
    /// (`memory_chunk.h:390`; register row 85). The C adapter aborts.
    ShortTotal {
        /// The predecessor whose gram the walk loaded.
        token: u32,
    },
    /// A partial item tail, which `retrieve_all`'s `cur != end` walk reads
    /// past (register row 98, class (b)). The walk ends there.
    PartialTail {
        /// The predecessor whose gram the walk loaded.
        token: u32,
    },
}

/// What stops a `has_next` of the bigram export walk
/// ([`BigramExportWalk::has_next`]) where the pin's
/// `pinyin_bigram_iterator_has_next_phrase` does not return normally.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BigramProbeFault {
    /// The gram of the predecessor the walk loaded cannot be read.
    Gram(GramLoadFault),
    /// A stored key of the pair the walk loaded lies outside the table: the
    /// `get_pinyin_string` assert the pin dies at while it renders the
    /// pair's readings (`pinyin.cpp:846`, `:859` at 074a2219), the tone
    /// (`chewing_key.cpp:48`) before the fields (`:30-32`, from `:49`). The
    /// site is [`ExportAssert::KeyTone`] or [`ExportAssert::KeyFields`].
    /// The C adapter aborts. The walk stays at the pair, so a repeated
    /// `has_next` answers the same.
    Key(ExportAssert),
}

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
    /// The pair `has_next` loaded last, predecessor then successor: what
    /// the pin converts into `m_phrase` (`pinyin.cpp:810-832`). `None`
    /// before the first load and after [`Self::drop_phrase`].
    pair: Option<(PhraseText, PhraseText)>,
    /// How many pairs `has_next` has loaded.
    loads: u64,
    pinyins: Vec<String>,
    pinyin_index: usize,
    count: u32,
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
    /// A row, and the `has_next` the pin's `get_next` runs after it
    /// (`pinyin.cpp:910`): its answer, or the [`BigramProbeFault`] of a
    /// predecessor it loaded and could not read, or of a pair with a stored
    /// key outside the table. The pin has written the row's out-params by
    /// then (`:904-906`), so the C adapter writes them before it aborts.
    Row(ExportedBigramRow, Result<bool, BigramProbeFault>),
    // pinyin_bigram_iterator_get_next_phrase asserts the predecessor at pinyin.cpp:902.
    /// The caller requested a row without a current predecessor or with the sentence-start
    /// predecessor. The iterator returns this decision to the facade.
    Aborts,
    /// No pinyin at the current position: the pin reads past `m_pinyins`
    /// (`pinyin.cpp:905`), undefined behaviour.
    Undefined,
}

impl BigramExportWalk {
    // pinyin_bigram_iterator_has_next_phrase, pinyin.cpp:790–893; ngram.cpp:133–143.
    /// Probes for another export row. The probe can load a predecessor’s gram and render a
    /// new pair before answering.
    ///
    /// # Errors
    ///
    /// Returns [`BigramProbeFault::Gram`] if a predecessor cannot be loaded for the next pair.
    /// Returns [`BigramProbeFault::Key`] if a stored key of the next pair is outside the table,
    /// where the pin asserts (`pinyin.cpp:846`, `:859`; `chewing_key.cpp:47-50`, `:29-32`). The
    /// C adapter aborts the process on that fault. Earlier rows have already been handed out.
    pub fn has_next(&mut self, context: &ContextCore) -> Result<bool, BigramProbeFault> {
        let source = ExportSource {
            user: context.user_store(),
            dict: context.runtime.dict(),
        };
        self.advance(|token| source.gram(token), |token| source.render(token))
    }

    /// The walk over whatever loads a predecessor's gram and renders a
    /// token's item. `render` is called for both tokens of every pair
    /// loaded, as the pin re-reads both items then (`pinyin.cpp:812-815`).
    fn advance(
        &mut self,
        load_gram: impl Fn(u32) -> Result<Vec<(u32, u32)>, GramLoadFault>,
        render: impl Fn(u32) -> RenderedItem,
    ) -> Result<bool, BigramProbeFault> {
        if self.pair.is_some() && self.pinyin_index < self.pinyins.len() {
            return Ok(true);
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
                        // The pin dies inside this `has_next` (`:846`,
                        // `:859`), before it removes the item (`:874`).
                        self.load_row(token, count, &render)
                            .map_err(BigramProbeFault::Key)?;
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
            let gram = match load_gram(index_token) {
                Ok(gram) => gram,
                Err(fault) => {
                    // The walk ends here: no later predecessor is read.
                    self.items.clear();
                    self.phrase_tokens.clear();
                    return Err(BigramProbeFault::Gram(fault));
                }
            };
            self.phrase_tokens.extend(gram);
            // `} while (iter->m_items->len);`
            if self.items.is_empty() {
                break;
            }
        }
        Ok(retval)
    }

    // pinyin_bigram_iterator_get_next_phrase writes the out parameters before calling
    // has_next (pinyin.cpp:896–911). The count expression at 901,906 multiplies by a guint32
    // factor before assigning to gint.
    /// Returns the current row — count × 2 in `guint32` — and advances the iterator. A
    /// [`BigramStep::Row`] can contain the current row together with an error from probing
    /// the following row. For a [`BigramProbeFault::Key`], the C adapter writes the row's
    /// out-parameters and then aborts the process, as the pin's assert does. Call
    /// [`Self::has_next`] before requesting a row.
    pub fn get_next(&mut self, context: &ContextCore) -> BigramStep {
        self.next_row(|walk| walk.has_next(context))
    }

    fn next_row(
        &mut self,
        advance: impl FnOnce(&mut Self) -> Result<bool, BigramProbeFault>,
    ) -> BigramStep {
        if self.index_token == NULL_TOKEN || self.index_token == SENTENCE_START {
            return BigramStep::Aborts;
        }
        let Some(pinyin) = self.pinyins.get(self.pinyin_index).cloned() else {
            return BigramStep::Undefined;
        };
        let row = ExportedBigramRow {
            pinyin,
            count: i64::from(self.count.wrapping_mul(2).cast_signed()),
        };
        self.pinyin_index += 1;
        let more = advance(self);
        BigramStep::Row(row, more)
    }

    /// The pair the last load read, predecessor then successor, for the C
    /// adapter's conversion into the pin's `m_phrase`.
    #[must_use]
    pub fn pair(&self) -> Option<(&PhraseText, &PhraseText)> {
        self.pair.as_ref().map(|(first, second)| (first, second))
    }

    /// How many pairs have been loaded; a change after `has_next` or
    /// `get_next` means [`Self::pair`] is a new pair.
    #[must_use]
    pub const fn loads(&self) -> u64 {
        self.loads
    }

    /// The pin's `m_phrase` NULL after a failed `g_ucs4_to_utf8` of the
    /// predecessor (`pinyin.cpp:822`, `:830`): the next `has_next` no longer
    /// answers from the loaded pair, and rows taken before then carry no
    /// phrase.
    pub fn drop_phrase(&mut self) {
        self.pair = None;
    }

    /// The row `has_next` builds for `(m_index_token, token)`
    /// (`pinyin.cpp:810-870`): the two texts concatenated, and every
    /// first-pronunciation × second-pronunciation pair joined by `'`. A
    /// token the phrase index does not resolve reads as the pin's empty
    /// `PhraseItem` — no text, no pronunciation.
    ///
    /// Answers the assert site, with the walk unchanged, where a stored key
    /// of the pair is outside the table ([`pair_assert`]): the pin dies
    /// there (`pinyin.cpp:846`, `:859`).
    fn load_row(
        &mut self,
        token: u32,
        count: u32,
        render: &impl Fn(u32) -> RenderedItem,
    ) -> Result<(), ExportAssert> {
        let first = render(self.index_token);
        let second = render(token);
        if let Some(site) = pair_assert(&first.sites, &second.sites) {
            return Err(site);
        }
        self.pinyins = first
            .pinyins
            .iter()
            .flat_map(|first| {
                second
                    .pinyins
                    .iter()
                    .map(move |second| format!("{first}'{second}"))
            })
            .collect();
        self.pair = Some((first.text, second.text));
        self.loads = self.loads.wrapping_add(1);
        self.count = count;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
