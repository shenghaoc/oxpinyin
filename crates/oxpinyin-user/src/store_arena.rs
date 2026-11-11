//! Session phrase operations over the arenas and the live system items.

use super::*;
use crate::replay::UnigramAdd;
use crate::user_library::{SessionPhrases, UserLibrary};
use oxpinyin_data::chunk_write::{ChunkItem, decode_phrase_item, encode_phrase_item};
use oxpinyin_data::phrase_library::PhraseItemView;

fn encoded(item: &ChunkItem) -> Result<Vec<u8>, UserStoreError> {
    encode_phrase_item(item).map_err(|error| UserStoreError::Persistence(error.to_string()))
}

fn packed(keys: &[PinyinKey]) -> Result<Vec<u16>, UserStoreError> {
    crate::store_libpinyin::pinyin_keys_to_packed(keys).ok_or(UserStoreError::InvalidPhrase)
}

impl UserStore {
    /// `SubPhraseIndex::m_total_freq` of `library`: a system library's live
    /// scalar, or a user library's items sum plus the stored total's
    /// difference from it.
    fn library_total_of(&self, offsets: &Offsets, phrases: &SessionPhrases, library: u8) -> u32 {
        if let Some(original) = self.originals.get(&library) {
            return lock_result_or_abort(self.inner.system_items.lock())
                .library_total(library, Some(original))
                .unwrap_or(original.total);
        }
        // The library's own total is the pin's `m_total_freq`: it follows
        // every add and remove, and not the bytes a wrapped add overwrote.
        let offset = offsets.get(&library).copied().unwrap_or(0);
        phrases
            .libraries
            .get(&library)
            .map_or(offset, UserLibrary::total)
    }

    /// `FacadePhraseIndex::add_phrase_item` for an item of `unigram` added to
    /// `library` (`phrase_index.h:669`): the facade total takes it, and a
    /// system library's total takes it too when the item's unigram was kept.
    fn take_item_in(&self, library: u8, unigram: u32, accepted: bool) {
        let mut items = lock_result_or_abort(self.inner.system_items.lock());
        match self.originals.get(&library) {
            Some(original) if accepted => {
                let total = items
                    .library_total(library, Some(original))
                    .unwrap_or(original.total);
                items.set_library_total(library, total.wrapping_add(unigram));
            }
            _ => items.add_facade_extra(u64::from(unigram)),
        }
    }

    /// `FacadePhraseIndex::remove_phrase_item` (`phrase_index.h:691`): the
    /// facade and the library's totals lose the item's unigram.
    fn take_item_out(&self, library: u8, unigram: u32) {
        let mut items = lock_result_or_abort(self.inner.system_items.lock());
        match self.originals.get(&library) {
            Some(original) => {
                let total = items
                    .library_total(library, Some(original))
                    .unwrap_or(original.total);
                items.set_library_total(library, total.wrapping_sub(unigram));
            }
            None => items.sub_facade_extra(u64::from(unigram)),
        }
    }

    /// The session side of a `USER_FILE` library transition: `library` takes
    /// the arena's place, with its share of the facade total; `None` is an
    /// unload, which leaves the library without a sub-index until a load. The
    /// session's state is process memory, so this cannot fail, as
    /// `FacadePhraseIndex::load` and `unload` cannot (policy row 103).
    pub(super) fn replace_user_arena(&mut self, nibble: u8, library: Option<UserLibrary>) {
        let mut offsets = self.offsets();
        let mut phrases = self.inner.libpinyin.phrases();
        let sum = |library: &UserLibrary| {
            library.items().fold(0_u64, |sum, (_, item)| {
                sum.wrapping_add(u64::from(item.unigram()))
            })
        };
        let before = phrases.libraries.get(&nibble).map_or(0, sum);
        let after = library.as_ref().map_or(0, sum);
        let offset = library
            .as_ref()
            .map_or(0, |library| library.total().wrapping_sub(after as u32));
        self.inner.has_library_offsets.store(true, Ordering::SeqCst);
        let old_offset = if offset == 0 {
            offsets.remove(&nibble)
        } else {
            offsets.insert(nibble, offset)
        }
        .unwrap_or(0);
        let moved = offset.wrapping_sub(old_offset);
        let offsets_left = !offsets.is_empty();
        // The facade total takes the library's new total in and the old one
        // out (`phrase_index.cpp:237-248`).
        lock_result_or_abort(self.inner.system_items.lock())
            .add_facade_extra(after.wrapping_sub(before).wrapping_add(u64::from(moved)));
        phrases.libraries.remove(&nibble);
        if let Some(library) = library {
            phrases.libraries.insert(nibble, library);
            phrases.unloaded.remove(&nibble);
        } else {
            phrases.unloaded.insert(nibble);
        }
        let arena_live = phrases
            .libraries
            .values()
            .any(|library| !library.is_empty());
        drop(phrases);
        self.inner
            .has_library_offsets
            .store(offsets_left, Ordering::SeqCst);
        let grams_live = self.container().keys().is_ok_and(|keys| !keys.is_empty());
        self.mark_committed_phrase_write(offsets, arena_live || grams_live);
    }

    /// Whether the session holds an item beyond the shipped chunks: an arena
    /// item of a user library, or one a system library's live buffer added.
    pub(super) fn arena_has_items(&self) -> bool {
        self.inner
            .libpinyin
            .phrases()
            .libraries
            .values()
            .any(|library| !library.is_empty())
            || lock_result_or_abort(self.inner.system_items.lock()).has_added()
    }

    /// Addon selection copies the complete item to the current range end,
    /// including its unigram, and indexes every pronunciation
    /// (074a2219 pinyin.cpp:2531-2562).
    pub(super) fn arena_promote(
        &mut self,
        phrase: &str,
        readings: &[&(Vec<PinyinKey>, u64)],
        unigram: u32,
    ) -> Result<Token, UserStoreError> {
        let target = &self.inner.libpinyin;
        let item = ChunkItem {
            phrase: phrase.chars().map(u32::from).collect(),
            unigram,
            prons: readings
                .iter()
                .map(|(keys, count)| Ok((packed(keys)?, *count as u32)))
                .collect::<Result<_, UserStoreError>>()?,
        };
        let bytes = encoded(&item)?;
        let offsets = self.offsets();
        let mut phrases = target.phrases();
        // The pin's promote reaches `phrase_table->add_index`
        // (`pinyin.cpp:2557` at 074a2219) before it changes anything it
        // keeps; a record it would read past (register row 101) is refused
        // here, before any commit.
        phrases
            .phrase_table
            .check_add(&item.phrase)
            .map_err(|error| UserStoreError::Persistence(error.to_string()))?;
        let empty = UserLibrary::default();
        let library = phrases.libraries.get(&ADDON_DICTIONARY).unwrap_or(&empty);
        let slot = library.token_end();
        // `pinyin.cpp:2541-2545` takes `get_range`'s end as the token with no
        // overflow check and no reserved-id bump; `FacadePhraseIndex::get_range`
        // masks the end into the nibble (`phrase_index.cpp:620`), so a full
        // library wraps to slot 0, whose offset word `add_phrase_item`
        // overwrites (`phrase_index.cpp:208-217`; register row 119).
        let token = phrase::phrase_index_make_token(ADDON_DICTIONARY, slot);
        let replacement = library
            .prepare_add(token & PHRASE_MASK, bytes)
            .map_err(UserStoreError::Persistence)?;
        self.take_item_in(ADDON_DICTIONARY, unigram, false);
        let retained = offsets.get(&ADDON_DICTIONARY).copied().unwrap_or(0);
        replacement.apply(phrases.arena(ADDON_DICTIONARY, retained));
        phrases
            .phrase_table
            .add(&item.phrase, token)
            .map_err(|error| UserStoreError::Persistence(error.to_string()))?;
        for (packed, _) in &item.prons {
            let keys: Vec<oxpinyin_core::ChewingKey> = packed
                .iter()
                .copied()
                .map(oxpinyin_core::ChewingKey::from_packed)
                .collect();
            phrases.index.add(&keys, token);
        }
        drop(phrases);
        self.mark_committed_phrase_write(offsets, true);
        Ok(token)
    }

    pub(crate) fn session_target(&self) -> &crate::store_libpinyin::Target {
        &self.inner.libpinyin
    }

    pub(super) fn arena_train(
        &mut self,
        token: Token,
        readings: impl Iterator<Item = Vec<ChewingKey>>,
        delta: u32,
    ) -> Result<(), UserStoreError> {
        let offsets = self.offsets();
        let mut phrases = self.inner.libpinyin.phrases();
        // The unloaded sub-index owns no item. The pin instead uses its
        // stale context cache here (phonetic_lookup.h:924-927); the
        // unloaded-cache ruling in compatibility-policy.md skips that write.
        if phrases
            .unloaded
            .contains(&phrase_index_library_index(token))
        {
            return Ok(());
        }
        if phrases.item(token).is_some() {
            if let Some(library) = phrases.item_mut(token) {
                library.train(token & PHRASE_MASK, readings, delta);
            }
            drop(phrases);
            self.mark_committed_write(offsets, true);
            return Ok(());
        }
        drop(phrases);
        // Any system item is trained in place in its library's live buffer; a
        // removed one answers no item there and is left alone.
        let mut items = lock_result_or_abort(self.inner.system_items.lock());
        if items.train_live(
            self.originals.get(&phrase_index_library_index(token)),
            token,
            readings,
            delta,
        ) {
            self.inner.has_system_items.store(true, Ordering::Relaxed);
            // A trained count may overlap another item's header; the live
            // per-token check decides whether that item is still readable.
            self.inner
                .has_removed_system_items
                .store(true, Ordering::Relaxed);
        }
        drop(items);
        self.mark_committed_write(offsets, true);
        Ok(())
    }

    /// Borrow a live user item directly from the session arena. The callback
    /// cannot retain the view past the guard or a later writer.
    pub fn with_user_item<R>(
        &self,
        token: Token,
        read: impl FnOnce(PhraseItemView<'_>) -> R,
    ) -> Option<R> {
        let phrases = self.inner.libpinyin.phrases();
        match phrases.item(token) {
            Some(view) => Some(read(view)),
            None => self.with_appended_item(token, read),
        }
    }

    /// Reads the item a system library's live buffer holds under `token` and
    /// its shipped chunk lacks: an import or a replayed ADD. The buffer is the
    /// only copy; no arena holds it. Locks the buffers after the arena, as
    /// every caller does.
    pub(crate) fn with_appended_item<R>(
        &self,
        token: Token,
        read: impl FnOnce(PhraseItemView<'_>) -> R,
    ) -> Option<R> {
        let original = self.originals.get(&phrase_index_library_index(token))?;
        if original.mapped.is_some_and(|library| library.is_detached()) {
            // The chunk's bytes are the live buffer's: it alone knows which
            // of its slots the chunk did not hold.
            if !self.has_system_items() {
                return None;
            }
            return lock_result_or_abort(self.inner.system_items.lock())
                .with_added(token, |bytes| PhraseItemView::from_bytes(bytes).map(read))
                .flatten();
        }
        if original.contains_item(token & PHRASE_MASK) {
            return None;
        }
        self.with_live_item(token, |bytes| PhraseItemView::from_bytes(bytes).map(read))
            .flatten()
    }

    pub(super) fn arena_add_phrase(
        &mut self,
        library: u8,
        words: &[u32],
        keys: &[PinyinKey],
        delta: u32,
        selected: Option<Option<Token>>,
        range_end: u32,
    ) -> Result<Token, UserStoreError> {
        if self.originals.contains_key(&library) {
            return self.buffer_add_phrase(library, words, keys, delta, selected);
        }
        let target = &self.inner.libpinyin;
        let packed = packed(keys)?;
        let offsets = self.offsets();
        let mut phrases = target.phrases();
        // `_add_phrase` searches the phrase table before any change
        // (`pinyin.cpp:536-542` at 074a2219); a record that search would read
        // past (register row 101) is refused here, before any commit.
        phrases
            .phrase_table
            .check_add(words)
            .map_err(|error| UserStoreError::Persistence(error.to_string()))?;
        let existing = match selected {
            Some(token) => token,
            None => {
                let tokens: Vec<_> = phrases
                    .phrase_table
                    .tokens(&phrase_table::key_ucs4(words))
                    .map_err(|error| UserStoreError::Persistence(error.to_string()))?
                    .into_iter()
                    .filter(|token| phrase_index_library_index(*token) == library)
                    .collect();
                if tokens.len() > 1 {
                    return Err(UserStoreError::InvalidPhrase);
                }
                tokens.first().copied()
            }
        };
        if let Some(token) = existing {
            let Some(item) = phrases.item(token) else {
                return Err(UserStoreError::InvalidPhrase);
            };
            if item.as_bytes()[6..6 + 4 * item.phrase_length()] != phrase_table::key_ucs4(words) {
                return Err(UserStoreError::InvalidPhrase);
            }
            phrases
                .libraries
                .entry(library)
                .or_default()
                .add_reading(token & PHRASE_MASK, &packed, delta)
                .map_err(UserStoreError::Persistence)?;
            drop(phrases);
            self.mark_committed_import_write(offsets, false);
            return Ok(token);
        }
        let end = phrases
            .libraries
            .get(&library)
            .map_or(1, UserLibrary::token_end)
            .max(range_end);
        // `_add_phrase` takes `get_range`'s end, masked into the nibble
        // (`phrase_index.cpp:620`), and bumps only a zero low word past
        // the reserved id (`pinyin.cpp:589-594`): a full library wraps to
        // slot 1, and nothing refuses (register row 119).
        let token = phrase::add_phrase_token(library, end);
        let increment = delta.wrapping_mul(ADD_PHRASE_UNIGRAM_FACTOR_U32);
        let accepted = self
            .library_total_of(&offsets, &phrases, library)
            .checked_add(increment)
            .is_some();
        let bytes = encoded(&ChunkItem {
            phrase: words.to_vec(),
            unigram: if accepted { increment } else { 0 },
            prons: vec![(packed.clone(), delta)],
        })?;
        let empty = UserLibrary::default();
        let replacement = phrases
            .libraries
            .get(&library)
            .unwrap_or(&empty)
            .prepare_add(token & PHRASE_MASK, bytes)
            .map_err(UserStoreError::Persistence)?;
        self.take_item_in(library, increment, accepted);
        {
            let retained = offsets.get(&library).copied().unwrap_or(0);
            replacement.apply(phrases.arena(library, retained));
            phrases
                .phrase_table
                .add(words, token)
                .map_err(|error| UserStoreError::Persistence(error.to_string()))?;
            let chewing: Vec<oxpinyin_core::ChewingKey> = packed
                .iter()
                .copied()
                .map(oxpinyin_core::ChewingKey::from_packed)
                .collect();
            phrases.index.add(&chewing, token);
        }
        drop(phrases);
        if (1..=4).contains(&library) {
            self.inner.has_system_items.store(true, Ordering::Relaxed);
        }
        self.mark_committed_import_write(offsets, true);
        Ok(token)
    }

    /// `_add_phrase` for a system library that has an original (074a2219
    /// `pinyin.cpp:515-611`), over the library's live buffer, which holds the
    /// only copy of the item. A phrase the library already holds is removed
    /// (`remove_phrase_item`), takes the reading (`add_pronunciation`) and is
    /// added back (`add_phrase_item`); a new one is added after the range's
    /// end with no unigram and then takes `count * 3` through
    /// `add_unigram_frequency`.
    fn buffer_add_phrase(
        &mut self,
        library: u8,
        words: &[u32],
        keys: &[PinyinKey],
        delta: u32,
        selected: Option<Option<Token>>,
    ) -> Result<Token, UserStoreError> {
        let target = &self.inner.libpinyin;
        let original = self
            .originals
            .get(&library)
            .ok_or(UserStoreError::InvalidPhrase)?;
        let packed = packed(keys)?;
        let offsets = self.offsets();
        let mut phrases = target.phrases();
        // `_add_phrase` searches the phrase table before any change
        // (`pinyin.cpp:536-542`); a record that search would read past (register
        // row 101) is refused here, before any commit.
        phrases
            .phrase_table
            .check_add(words)
            .map_err(|error| UserStoreError::Persistence(error.to_string()))?;
        let existing = match selected {
            Some(token) => token,
            None => {
                let tokens = phrases
                    .phrase_table
                    .tokens(&phrase_table::key_ucs4(words))
                    .map_err(|error| UserStoreError::Persistence(error.to_string()))?;
                let tokens: Vec<_> = tokens
                    .into_iter()
                    .filter(|token| phrase_index_library_index(*token) == library)
                    .collect();
                if tokens.len() > 1 {
                    return Err(UserStoreError::InvalidPhrase);
                }
                tokens.first().copied()
            }
        };
        let mut items = lock_result_or_abort(self.inner.system_items.lock());
        // An unloaded library has no sub-index: `get_range` fails and
        // `_add_phrase` adds nothing (`pinyin.cpp:589-591`).
        if items.lacks_sub_index(library, original) {
            return Err(UserStoreError::InvalidPhrase);
        }
        if let Some(token) = existing {
            // The shipped chunk's own items are merged by the caller; only an
            // item the buffer added is held here.
            if original.contains_item(token & PHRASE_MASK)
                || (original.mapped.is_some_and(|library| library.is_detached())
                    && !items.is_added(token)
                    && items.with_live(token, |_| ()).is_some())
            {
                return Err(UserStoreError::InvalidPhrase);
            }
            let merged = items
                .with_live(token, |bytes| {
                    let view = PhraseItemView::from_bytes(bytes)?;
                    if view.as_bytes()[6..6 + 4 * view.phrase_length()]
                        != phrase_table::key_ucs4(words)
                    {
                        return None;
                    }
                    Some(UserLibrary::merged_reading(view, &packed, delta))
                })
                .flatten()
                .ok_or(UserStoreError::InvalidPhrase)?
                .map_err(UserStoreError::Persistence)?;
            items.remove_item(original, token);
            let overwrote = items.add_item(original, token, &merged);
            drop(items);
            drop(phrases);
            self.inner.has_system_items.store(true, Ordering::Relaxed);
            if overwrote {
                self.inner
                    .has_removed_system_items
                    .store(true, Ordering::Relaxed);
            }
            self.mark_committed_import_write(offsets, false);
            return Ok(token);
        }
        let end = items.range_end(library, original);
        // `_add_phrase`'s token (`pinyin.cpp:589-594`; register row 119).
        let token = phrase::add_phrase_token(library, end);
        let bytes = encoded(&ChunkItem {
            phrase: words.to_vec(),
            unigram: 0,
            prons: vec![(packed.clone(), delta)],
        })?;
        if items.add_item(original, token, &bytes) {
            self.inner
                .has_removed_system_items
                .store(true, Ordering::Relaxed);
        }
        let increment = delta.wrapping_mul(ADD_PHRASE_UNIGRAM_FACTOR_U32);
        let outcome = items
            .add_unigram(Some(original), token, increment)
            .unwrap_or(UnigramAdd::Missing);
        if !outcome.is_accepted() {
            items.add_facade_extra(u64::from(increment));
        }
        drop(items);
        phrases
            .phrase_table
            .add(words, token)
            .map_err(|error| UserStoreError::Persistence(error.to_string()))?;
        let chewing: Vec<oxpinyin_core::ChewingKey> = packed
            .iter()
            .copied()
            .map(oxpinyin_core::ChewingKey::from_packed)
            .collect();
        phrases.index.add(&chewing, token);
        drop(phrases);
        self.inner.has_system_items.store(true, Ordering::Relaxed);
        self.mark_committed_import_write(offsets, true);
        Ok(token)
    }

    pub(super) fn arena_add_unigram(&self, token: Token, delta: u32) -> bool {
        // Only the sub-index's overflow is the pin's `ERROR_INTEGER_OVERFLOW`;
        // a missing item answers another error that callers do not act on.
        self.arena_add_unigram_outcome(token, delta) != UnigramAdd::Refused
    }

    /// `FacadePhraseIndex::add_unigram_frequency` over the session's state
    /// (074a2219 `phrase_index.h:628-635`, `phrase_index.cpp:150-176`). The
    /// facade total takes `delta` first; the sub-index then takes it only when
    /// it holds the item and its `guint32` total can: a system library's total
    /// is its live scalar, a user library's the sum of its items. A
    /// sub-index that does not take it leaves the delta in the facade's extra.
    pub(super) fn arena_add_unigram_outcome(&self, token: Token, delta: u32) -> UnigramAdd {
        let library = phrase_index_library_index(token);
        let mut phrases = self.inner.libpinyin.phrases();
        // phrase_index.h:631-632 returns before the facade total changes.
        if phrases.unloaded.contains(&library) {
            return UnigramAdd::Missing;
        }
        let original = self.originals.get(&library);
        let mut items = lock_result_or_abort(self.inner.system_items.lock());
        if original.is_some() {
            // An unloaded library has no sub-index: the pin answers
            // `ERROR_NO_SUB_PHRASE_INDEX` before the facade total moves
            // (`phrase_index.h:631-633`), so nothing moves here either.
            if original.is_some_and(|original| items.lacks_sub_index(library, original)) {
                return UnigramAdd::Missing;
            }
            // A slot the buffer holds no item at (a removed or absent one)
            // answers `Missing`.
            let outcome = items
                .add_unigram(original, token, delta)
                .unwrap_or(UnigramAdd::Missing);
            if outcome.is_accepted() {
                self.inner.has_system_items.store(true, Ordering::Relaxed);
            } else {
                items.add_facade_extra(u64::from(delta));
            }
            return outcome;
        }
        items.add_facade_extra(u64::from(delta));
        drop(items);
        let slot = token & PHRASE_MASK;
        let Some(library) = phrases.item_mut(token) else {
            return UnigramAdd::Missing;
        };
        // The pin checks the slot and reads the item before the sub-index's
        // total overflow guard (074a2219 phrase_index.cpp:153-171).
        if library.item(slot).is_none() {
            return UnigramAdd::Missing;
        }
        if library.total().checked_add(delta).is_none() {
            return UnigramAdd::Refused;
        }
        if library.increment_unigram(slot, delta) {
            UnigramAdd::Accepted
        } else {
            UnigramAdd::Missing
        }
    }

    pub(super) fn arena_remove(&mut self, token: Token) -> Result<bool, UserStoreError> {
        let mask = PHRASE_INDEX_LIBRARY_MASK | PHRASE_MASK;
        // Hold the offsets guard from the snapshot through the mask.
        let offsets = self.offsets();
        let container_keys = self.container_keys();
        let bigram_fault = self.bigram_mask_fault_over(&container_keys, mask, token);
        let mut phrases = self.inner.libpinyin.phrases();
        let Some(view) = phrases.item(token) else {
            return Ok(false);
        };
        let item = decode_phrase_item(view.as_bytes())
            .map_err(|e| UserStoreError::Persistence(e.to_string()))?;
        // The phrase-table assertion precedes the reading removals.
        let phrase_tokens = phrases
            .phrase_table
            .tokens(&phrase_table::key_ucs4(&item.phrase))
            .map_err(|error| UserStoreError::Persistence(error.to_string()))?;
        if !phrase_tokens.contains(&token) {
            return Ok(false);
        }
        // Simulate the pin's ordered removals, including repeated projections
        // and readings, before changing any live table.
        let mut index = phrases.index.clone();
        for (packed, _) in &item.prons {
            let keys: Vec<_> = packed
                .iter()
                .copied()
                .map(oxpinyin_core::ChewingKey::from_packed)
                .collect();
            if !index.remove(&keys, token) {
                return Err(UserStoreError::UserReadingRemoval);
            }
        }
        if let Some(fault) = bigram_fault {
            return Err(fault);
        }
        self.take_item_out(phrase_index_library_index(token), item.unigram);
        phrases
            .phrase_table
            .remove(&item.phrase, token)
            .map_err(|error| UserStoreError::Persistence(error.to_string()))?;
        for (packed, _) in &item.prons {
            let keys: Vec<oxpinyin_core::ChewingKey> = packed
                .iter()
                .copied()
                .map(oxpinyin_core::ChewingKey::from_packed)
                .collect();
            if !phrases.index.remove(&keys, token) {
                return Err(UserStoreError::UserReadingRemoval);
            }
        }
        if let Some(library) = phrases.item_mut(token) {
            library.remove(token & PHRASE_MASK);
        }
        drop(phrases);
        let grams_left = self.mask_container_over(container_keys, mask, token)?;
        let has = !self
            .inner
            .libpinyin
            .phrases()
            .libraries
            .values()
            .all(UserLibrary::is_empty);
        self.mark_committed_phrase_write(offsets, has || grams_left);
        Ok(true)
    }

    pub(super) fn mask_arena(&self, mask: Token, value: Token) -> Result<(), UserStoreError> {
        let mut phrases = self.inner.libpinyin.phrases();
        for (&nibble, library) in &mut phrases.libraries {
            let selected = |slot: u32| (((u32::from(nibble) << 24) | slot) & mask) == value;
            // `FacadePhraseIndex::mask_out` takes the old sub-index total out
            // of the facade and the new one in (`phrase_index.cpp:699-718`):
            // each removed item's frequency leaves the totals.
            let removed: Vec<u32> = library
                .items()
                .filter(|&(slot, _)| selected(slot))
                .map(|(_, item)| item.unigram())
                .collect();
            for unigram in removed {
                self.take_item_out(nibble, unigram);
            }
            library.remove_matching(selected);
        }
        Ok(())
    }
}
