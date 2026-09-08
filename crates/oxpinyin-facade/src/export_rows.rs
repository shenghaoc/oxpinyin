//! The §9 export materialization: user-store rows rendered the way the
//! C ABI's export iterators yield them.
//!
//! Moved from `oxpinyin-capi`'s state layer so the standalone migration
//! tool (`oxpinyin-dictool`) drives the exact code the C iterators drive,
//! instead of a Rust-only re-implementation growing beside it. Pure Rust
//! over the context's user store and optional runtime — no C types cross
//! this boundary.

use oxpinyin_user::{
    ExportedPhrase, NETWORK_DICTIONARY, SENTENCE_START, USER_DICTIONARY, UserPronunciation,
    is_user_file_token,
};

use crate::ContextCore;

/// Upstream's first training seed (`initial_seed`, 23·3): the §9 bigram
/// export threshold — counts at or above it export, below it stay.
const INITIAL_SEED: u64 = 23 * 3;

impl ContextCore {
    /// §9 phrase-export materialization. [`USER_DICTIONARY`] and
    /// [`NETWORK_DICTIONARY`] export their stored rows; any other index
    /// exports an empty list.
    #[must_use]
    pub fn export_phrases(&self, index: u32) -> Option<Vec<ExportedPhrase>> {
        let index = u8::try_from(index).ok()?;
        if index != USER_DICTIONARY && index != NETWORK_DICTIONARY {
            return Some(Vec::new());
        }
        self.user.as_ref()?.export_phrases_in(index).ok()
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

    /// Every rendered §9 bigram-export row, in stored order.
    #[must_use]
    pub fn export_bigram_rows(&self) -> Option<Vec<ExportedBigramRow>> {
        let store = self.user.as_ref()?;
        let raw = store.export_bigrams().ok()?;
        let mut rows = Vec::new();
        // Memoize the (text, pinyins) rendering: a system token recurs across
        // many bigram rows and `render_token` is an O(pinyin-index) scan, so
        // resolving it once per distinct token keeps the export off the
        // rows×index quadratic.
        let mut rendered: std::collections::HashMap<u32, Option<(String, Vec<String>)>> =
            std::collections::HashMap::new();
        for (prev, cur, count) in raw {
            if prev == SENTENCE_START {
                continue;
            }
            // Upstream's threshold is `initial_seed - 1` = 68.
            if count < INITIAL_SEED {
                continue;
            }
            let Some((prev_text, prev_pinyins)) = rendered
                .entry(prev)
                .or_insert_with(|| self.render_token(prev))
                .clone()
            else {
                continue;
            };
            let Some((cur_text, cur_pinyins)) = rendered
                .entry(cur)
                .or_insert_with(|| self.render_token(cur))
                .clone()
            else {
                continue;
            };
            let phrase = format!("{prev_text}{cur_text}");
            for first in &prev_pinyins {
                for second in &cur_pinyins {
                    rows.push(ExportedBigramRow {
                        phrase: phrase.clone(),
                        pinyin: format!("{first}'{second}"),
                        count: i64::try_from(count.saturating_mul(2)).unwrap_or(i64::MAX),
                    });
                }
            }
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
