//! Live system items: private replay views plus payloads decoded on first use.

use std::collections::BTreeMap;

use oxpinyin_core::ChewingKey;
use oxpinyin_data::chunk_write::ChunkItem;
use oxpinyin_data::phrase_library::PhraseItemView;

use crate::replay::ReplayedLibrary;
use crate::store::Token;

#[derive(Default)]
pub(crate) struct SystemItems {
    decoded: BTreeMap<Token, ChunkItem>,
    replayed: BTreeMap<u8, ReplayedLibrary>,
}

impl SystemItems {
    pub(crate) fn new(
        decoded: BTreeMap<Token, ChunkItem>,
        replayed: BTreeMap<u8, ReplayedLibrary>,
    ) -> Self {
        Self { decoded, replayed }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.decoded.is_empty() && self.replayed.is_empty()
    }

    pub(crate) fn contains_key(&self, token: &Token) -> bool {
        self.decoded.contains_key(token)
            || self
                .replayed
                .get(&((*token >> 24) as u8))
                .is_some_and(|library| library.contains(*token & 0x00ff_ffff))
    }

    pub(crate) fn get(&mut self, token: &Token) -> Option<&ChunkItem> {
        if !self.decoded.contains_key(token) {
            let library = self.replayed.get(&((*token >> 24) as u8))?;
            self.decoded
                .insert(*token, library.item(*token & 0x00ff_ffff)?);
        }
        self.decoded.get(token)
    }

    /// Prices the live readings without materializing a replayed payload.
    /// Edited items take precedence over the private replay view, as in `get`.
    pub(crate) fn pronunciation_possibility(
        &self,
        token: Token,
        query: &[ChewingKey],
    ) -> Option<(u32, u32)> {
        if let Some(item) = self.decoded.get(&token) {
            return Some(possibility(
                query,
                item.prons
                    .iter()
                    .map(|(keys, frequency)| (keys.iter().copied(), *frequency)),
            ));
        }
        let library = self.replayed.get(&((token >> 24) as u8))?;
        let bytes = library.bytes(token & 0x00ff_ffff)?;
        let item = PhraseItemView::from_bytes(&bytes).ok()?;
        Some(possibility(
            query,
            item.pronunciations().map(|row| {
                (
                    row.keys
                        .chunks_exact(2)
                        .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
                    row.freq,
                )
            }),
        ))
    }

    pub(crate) fn get_mut(&mut self, token: &Token) -> Option<&mut ChunkItem> {
        self.get(token)?;
        self.decoded.get_mut(token)
    }

    pub(crate) fn insert(&mut self, token: Token, item: ChunkItem) -> Option<ChunkItem> {
        self.decoded.insert(token, item)
    }

    pub(crate) fn snapshot(&self) -> BTreeMap<Token, ChunkItem> {
        let mut items = BTreeMap::new();
        for (&library, replayed) in &self.replayed {
            for slot in replayed.slots() {
                let token = (u32::from(library) << 24) | slot;
                if !self.decoded.contains_key(&token)
                    && let Some(item) = replayed.item(slot)
                {
                    items.insert(token, item);
                }
            }
        }
        items.extend(self.decoded.clone());
        items
    }

    fn materialize(&mut self) {
        if self.replayed.is_empty() {
            return;
        }
        self.decoded = self.snapshot();
        self.replayed.clear();
    }

    pub(crate) fn retain(&mut self, keep: impl FnMut(&Token, &mut ChunkItem) -> bool) {
        self.materialize();
        self.decoded.retain(keep);
    }

    pub(crate) fn keys(&mut self) -> std::collections::btree_map::Keys<'_, Token, ChunkItem> {
        self.materialize();
        self.decoded.keys()
    }

    pub(crate) fn end(&self, library: u8) -> Option<Token> {
        let decoded = self
            .decoded
            .range((u32::from(library) << 24)..(u32::from(library + 1) << 24))
            .next_back()
            .map(|(&token, _)| token);
        let replayed = self
            .replayed
            .get(&library)
            .and_then(|view| view.slots().next_back())
            .map(|slot| (u32::from(library) << 24) | slot);
        decoded
            .into_iter()
            .chain(replayed)
            .max()
            .and_then(|token| token.checked_add(1))
    }
}

/// The live item's pronunciation sums, shared by decoded edits and replayed
/// bytes. The arithmetic and key match are the runtime's effective-item rule.
fn possibility<K: ExactSizeIterator<Item = u16>>(
    query: &[ChewingKey],
    rows: impl Iterator<Item = (K, u32)>,
) -> (u32, u32) {
    let mut total = 0_u32;
    let mut matched = 0_u32;
    for (keys, frequency) in rows {
        total = total.wrapping_add(frequency);
        if query.len() == keys.len()
            && query.iter().zip(keys).all(|(q, bits)| {
                let s = ChewingKey::from_packed(bits);
                q.initial == s.initial
                    && ((q.middle == s.middle && q.final_ == s.final_)
                        || (q.middle == 0 && q.final_ == 0)
                        || (s.middle == 0 && s.final_ == 0))
                    && (q.tone == s.tone || q.tone == 0 || s.tone == 0)
            })
        {
            matched = matched.wrapping_add(frequency);
        }
    }
    (matched, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{SystemLibrary, journal_item_bytes};
    use crate::replay::Merge;
    use oxpinyin_data::user_files::LogRecord;

    const TOKEN: Token = 0x0100_0001;

    fn key(text: &str, tone: u8) -> ChewingKey {
        ChewingKey::from_pinyin(text).unwrap().with_tone(tone)
    }

    fn replayed(item: ChunkItem) -> SystemItems {
        let original = SystemLibrary {
            total: item.unigram,
            range_end: 2,
            items: BTreeMap::from([(1, item.clone())]),
            mapped: None,
        };
        let mut merge = Merge::new(Some(&original));
        let payload = journal_item_bytes(&item);
        assert!(
            merge
                .apply(LogRecord::Modify {
                    token: TOKEN,
                    old_item: payload.clone(),
                    new_item: payload,
                })
                .is_ok()
        );
        let mut skipped = Vec::new();
        let (_, typed, view) = merge.finish_lazy(&mut skipped, &"pricing fixture");
        assert!(skipped.is_empty());
        assert!(typed.is_empty());
        SystemItems::new(BTreeMap::new(), BTreeMap::from([(1, view.unwrap())]))
    }

    #[test]
    fn pricing_borrows_replayed_readings_and_observes_later_edits() {
        let item = ChunkItem {
            // Pricing must not decode or interpret the phrase text.
            phrase: vec![u32::MAX],
            unigram: 10,
            prons: vec![
                (vec![key("ni", 2).to_packed()], 11),
                (vec![key("ni", 3).to_packed()], 13),
                (vec![key("ni", 3).to_packed()], 17),
                (vec![key("hao", 3).to_packed()], 19),
            ],
        };
        let mut items = replayed(item);
        for (query, expected) in [
            (vec![key("ni", 0)], (41, 60)),
            (vec![key("ni", 3)], (30, 60)),
            (vec![key("n", 0)], (41, 60)),
            (vec![key("ma", 0)], (0, 60)),
            (vec![key("ni", 0), key("hao", 0)], (0, 60)),
            (vec![], (0, 60)),
        ] {
            assert_eq!(
                items.pronunciation_possibility(TOKEN, &query),
                Some(expected)
            );
            assert!(items.decoded.is_empty(), "pricing filled the payload cache");
        }
        assert_eq!(items.pronunciation_possibility(TOKEN + 1, &[]), None);
        items.get_mut(&TOKEN).unwrap().prons[1].1 = 23;
        assert_eq!(
            items.pronunciation_possibility(TOKEN, &[key("ni", 3)]),
            Some((40, 70))
        );
    }

    #[test]
    fn replayed_and_decoded_pricing_keep_empty_and_overflowing_counts() {
        for prons in [
            vec![],
            vec![(vec![key("ni", 0).to_packed()], 0)],
            vec![
                (vec![key("ni", 0).to_packed()], u32::MAX),
                (vec![key("ni", 3).to_packed()], 5),
            ],
        ] {
            let mut items = replayed(ChunkItem {
                phrase: vec![0],
                unigram: 1,
                prons,
            });
            let query = [key("ni", 3)];
            let borrowed = items.pronunciation_possibility(TOKEN, &query);
            assert!(items.decoded.is_empty());
            items.get(&TOKEN).unwrap();
            assert_eq!(items.pronunciation_possibility(TOKEN, &query), borrowed);
            assert_eq!(
                borrowed,
                Some(if items.decoded[&TOKEN].prons.len() == 2 {
                    (4, 4)
                } else {
                    (0, 0)
                })
            );
        }
    }
}
