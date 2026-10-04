//! The §9 export materialization: user-store rows rendered the way the
//! C ABI's export iterators yield them.
//!
//! Moved from `oxpinyin-capi`'s state layer so the standalone migration
//! tool (`oxpinyin-dictool`) drives the exact code the C iterators drive,
//! instead of a Rust-only re-implementation growing beside it. Pure Rust
//! over the context's user store and optional runtime — no C types cross
//! this boundary.

use oxpinyin_user::{
    ADDON_DICTIONARY, ExportedPhrase, NETWORK_DICTIONARY, SENTENCE_START, USER_DICTIONARY,
    UserPronunciation, is_user_file_token,
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

/// Phrase export state. System libraries retain only the current item's rows,
/// following the pin's token/pronunciation cursor (`pinyin.cpp:662-768`).
#[derive(Default)]
pub struct PhraseExportCursor {
    system: Option<oxpinyin_runtime::RuntimeDict>,
    user: Option<oxpinyin_user::UserStore>,
    next_token: u32,
    end_token: u32,
    rows: std::collections::VecDeque<ExportedPhrase>,
}

impl PhraseExportCursor {
    /// Whether the begin/previous-next probe found another pronunciation.
    #[must_use]
    pub fn has_next(&self) -> bool {
        !self.rows.is_empty()
    }

    fn probe(&mut self) {
        let Some(dict) = self.system.as_ref() else {
            return;
        };
        while self.rows.is_empty() && self.next_token < self.end_token {
            let token = self.next_token;
            self.next_token += 1;
            if !dict.library_visible_token(token) {
                continue;
            }
            if let Some(item) = self
                .user
                .as_ref()
                .and_then(|user| user.system_item_override(token).ok())
                .flatten()
            {
                let Some(text) = item
                    .phrase
                    .iter()
                    .copied()
                    .map(char::from_u32)
                    .collect::<Option<String>>()
                else {
                    continue;
                };
                self.rows
                    .extend(item.prons.into_iter().map(|(keys, frequency)| {
                        ExportedPhrase {
                            text: text.clone(),
                            pinyin: keys
                                .into_iter()
                                .map(|key| {
                                    oxpinyin_core::ChewingKey::from_packed(key).pinyin_string()
                                })
                                .collect::<Vec<_>>()
                                .join("'"),
                            count: u64::from(frequency),
                        }
                    }));
                continue;
            }
            let system = dict.system();
            let Some(text) = system.phrase_text(token) else {
                continue;
            };
            self.rows.extend(
                system
                    .pronunciations(token)
                    .into_iter()
                    .map(|(pinyin, count)| ExportedPhrase {
                        text: text.clone(),
                        pinyin,
                        count,
                    }),
            );
        }
    }
}

impl Iterator for PhraseExportCursor {
    type Item = ExportedPhrase;

    fn next(&mut self) -> Option<Self::Item> {
        let row = self.rows.pop_front()?;
        self.probe();
        Some(row)
    }
}

impl ContextCore {
    /// §9 phrase-export materialization (`pinyin_begin_get_phrases`,
    /// `pinyin.cpp:662-768`): every item of sub-index `index` that has a
    /// pronunciation, token order, one row per pronunciation in stored
    /// order.
    ///
    /// The index is narrowed the way the pin stores it —
    /// `export_iterator_t::m_phrase_index` is a `guint8`
    /// (`pinyin.cpp:126`), so `257` addresses library 1. The system
    /// libraries (`GB_DICTIONARY` … `MERGED_DICTIONARY`) walk their chunk
    /// file; the `USER_FILE` libraries (`ADDON_DICTIONARY`,
    /// [`NETWORK_DICTIONARY`], [`USER_DICTIONARY`]) export their stored
    /// rows. Every other nibble, and a system library that is unloaded,
    /// has no sub-index (`FacadePhraseIndex::get_range` →
    /// `ERROR_NO_SUB_PHRASE_INDEX`, `phrase_index.cpp:610-613`) and
    /// exports an empty list. Nibbles 16..=255 read past the pin's
    /// 16-slot array (`phrase_index.cpp:611`), undefined behaviour this
    /// port does not reproduce: they are empty too.
    #[must_use]
    pub fn export_phrases(&self, index: u32) -> Option<Vec<ExportedPhrase>> {
        let [index, ..] = index.to_le_bytes();
        match index {
            GB_DICTIONARY..=MERGED_DICTIONARY => Some(self.export_system_phrases(index)),
            ADDON_DICTIONARY | NETWORK_DICTIONARY | USER_DICTIONARY => {
                self.user.as_ref()?.export_phrases_in(index).ok()
            }
            _ => Some(Vec::new()),
        }
    }

    /// Opens a phrase export cursor, probing only the first system item.
    /// User-library exports keep their existing transaction-scoped snapshot.
    ///
    /// # Errors
    /// Returns the user-store error if its rows cannot be read.
    pub fn phrase_export_cursor(
        &self,
        index: u32,
    ) -> Result<PhraseExportCursor, oxpinyin_user::UserStoreError> {
        let [index, ..] = index.to_le_bytes();
        let mut cursor = PhraseExportCursor::default();
        match index {
            GB_DICTIONARY..=MERGED_DICTIONARY => {
                if let Some(runtime) = self.runtime.as_ref() {
                    let dict = runtime.dict();
                    let base = u32::from(index) << 24;
                    if dict.library_visible(u32::from(index))
                        && let Some(library) = dict.system().libraries().library(base)
                    {
                        let range = library.token_range();
                        cursor.next_token = base | range.start;
                        cursor.end_token = (base | range.end).max(
                            self.user
                                .as_ref()
                                .and_then(|user| user.system_item_end(index))
                                .unwrap_or(0),
                        );
                        cursor.user = self.user.clone();
                        cursor.system = Some(dict);
                        cursor.probe();
                    }
                }
            }
            ADDON_DICTIONARY | NETWORK_DICTIONARY | USER_DICTIONARY => {
                if let Some(user) = self.user.as_ref() {
                    cursor.rows = user.export_phrases_in(index)?.into();
                }
            }
            _ => {}
        }
        Ok(cursor)
    }

    fn export_system_phrases(&self, nibble: u8) -> Vec<ExportedPhrase> {
        self.phrase_export_cursor(u32::from(nibble))
            .map(Iterator::collect)
            .unwrap_or_default()
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
    /// (`pinyin_begin_get_bigram_phrases`, `pinyin.cpp:776-787`): the
    /// predecessors in the user bigram container's walk order
    /// (`get_all_items`), each predecessor's gram, and the rendering of
    /// every token they name. `None` without a user store.
    #[must_use]
    pub fn bigram_export_walk(&self) -> Option<BigramExportWalk> {
        let store = self.user.as_ref()?;
        let items = store.bigram_predecessors().ok()?;
        let mut grams: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
        let mut rendered: HashMap<u32, Option<(String, Vec<String>)>> = HashMap::new();
        for &prev in &items {
            let gram: Vec<(u32, u32)> = store
                .bigram_successors(prev)
                .ok()?
                .into_iter()
                .map(|(cur, count)| (cur, u32::try_from(count).unwrap_or(u32::MAX)))
                .collect();
            for &token in std::iter::once(&prev).chain(gram.iter().map(|(cur, _)| cur)) {
                // Memoized: a token recurs across many grams and
                // `render_token` is an O(pinyin-index) scan.
                rendered
                    .entry(token)
                    .or_insert_with(|| self.render_token(token));
            }
            grams.insert(prev, gram);
        }
        Some(BigramExportWalk {
            items: items.into(),
            index_token: NULL_TOKEN,
            phrase_tokens: VecDeque::new(),
            phrase: None,
            pinyins: Vec::new(),
            pinyin_index: 0,
            count: 0,
            grams,
            rendered,
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

    /// `(text, pinyin spellings)` for a token: user tokens render from the
    /// user store's phrase/pronunciation tables, system tokens from the
    /// system phrase index and the pinyin index (reverse-scanned).
    fn render_token(&self, token: u32) -> Option<(String, Vec<String>)> {
        if is_user_file_token(token) {
            let store = self.user.as_ref()?;
            let phrase = store.phrase(token).ok().flatten()?;
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
            let dict = self.runtime.as_ref()?.dict();
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
/// operations, ported state for state over a snapshot of the store
/// ([`ContextCore::bigram_export_walk`]). Nothing here reorders, filters
/// or repairs what the pin does: `m_items` is the container's walk
/// order and is consumed from the front; the gram of each predecessor
/// is *appended* to `m_phrase_tokens` (`SingleGram::retrieve_all`), so a
/// `sentence_start` gram, which is never scanned, is scanned under the
/// predecessor loaded after it; and the loop ends as soon as loading has
/// emptied `m_items`, so the last predecessor's gram is scanned only by
/// a further `has_next` after one that answered `false`.
#[derive(Clone, Debug, Default)]
pub struct BigramExportWalk {
    items: VecDeque<u32>,
    index_token: u32,
    phrase_tokens: VecDeque<(u32, u32)>,
    phrase: Option<String>,
    pinyins: Vec<String>,
    pinyin_index: usize,
    count: u32,
    grams: HashMap<u32, Vec<(u32, u32)>>,
    rendered: HashMap<u32, Option<(String, Vec<String>)>>,
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
            if let Some(gram) = self.grams.get(&index_token) {
                self.phrase_tokens.extend(gram.iter().copied());
            }
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

    /// The row `has_next` builds for `(m_index_token, token)`
    /// (`pinyin.cpp:810-870`): the two texts concatenated, and every
    /// first-pronunciation × second-pronunciation pair joined by `'`. A
    /// token the phrase index does not resolve reads as the pin's empty
    /// `PhraseItem` — no text, no pronunciation.
    fn load_row(&mut self, token: u32, count: u32) {
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
    #[test]
    fn system_begin_probes_one_item_and_draining_matches_materialization() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3");
        let ctx = ["db", "kct", "tkt"]
            .into_iter()
            .find_map(|ext| {
                super::ContextCore::open(
                    root.join(ext).to_str().unwrap(),
                    None,
                    crate::PINYIN_DEFAULT_OPTION_WORD,
                    oxpinyin_user::UserConfLaw::Pinyin,
                )
            })
            .expect("committed backend fixtures");
        let cursor = ctx.phrase_export_cursor(257).unwrap();
        assert!(cursor.has_next());
        assert!(
            cursor.next_token < cursor.end_token,
            "begin scanned the whole library"
        );
        let buffered = cursor.rows.len();
        let all: Vec<_> = cursor.collect();
        assert!(buffered < all.len(), "begin buffered the whole library");
        assert_eq!(all, ctx.export_phrases(1).unwrap());
    }
}
