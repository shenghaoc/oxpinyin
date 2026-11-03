//! Session phrase operations. Standalone stores retain their row persistence.

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

impl<S: WriteStore> GenericUserStore<S> {
    /// `SubPhraseIndex::m_total_freq` of `library`: a system library's live
    /// scalar, or a user library's items sum plus the stored total's
    /// difference from it.
    fn library_total_of(
        &self,
        txn: &impl WriteTxn,
        phrases: &SessionPhrases,
        library: u8,
    ) -> Result<u32, StoreError> {
        if let Some(original) = self.originals.get(&library) {
            return Ok(self
                .inner
                .system_items
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .library_total(library, Some(original))
                .unwrap_or(original.total));
        }
        Ok(
            (txn_get_u64_or(txn, LIBRARY_OFFSET, &[library], 0)? as u32).wrapping_add(
                phrases
                    .libraries
                    .get(&library)
                    .map_or(0, UserLibrary::item_sum),
            ),
        )
    }

    /// `FacadePhraseIndex::add_phrase_item` for an item of `unigram` added to
    /// `library` (`phrase_index.h:669`): the facade total takes it, and a
    /// system library's total takes it too when the item's unigram was kept.
    fn take_item_in(&self, library: u8, unigram: u32, accepted: bool) {
        let mut items = self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let mut items = self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    /// unload, which leaves the library without a sub-index until a load. A libpinyin session's values are process memory (see
    /// [`in_session`]), so this cannot fail, as `FacadePhraseIndex::load`
    /// and `unload` cannot.
    pub(super) fn replace_user_arena(&mut self, nibble: u8, library: Option<UserLibrary>) {
        let Some(target) = &self.inner.libpinyin else {
            return;
        };
        let db = self.database();
        let mut phrases = target.phrases();
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
        let written = db.write(|txn| {
            let key = [nibble];
            let old_offset = txn_get_u64_or(txn, LIBRARY_OFFSET, &key, 0)? as u32;
            if offset == 0 {
                txn.remove(LIBRARY_OFFSET, &key)?;
            } else {
                txn.put(LIBRARY_OFFSET, &key, &codec::encode_u64(u64::from(offset)))?;
            }
            Ok((
                offset.wrapping_sub(old_offset),
                !txn.is_empty(LIBRARY_OFFSET)?,
                has_user_data_in_write_txn(txn)?,
            ))
        });
        // Unreachable `None`: keep the offsets flag set and the user-data
        // gate open, the answers that read every row there is.
        let (moved, offsets_left, tables_live) = in_session(written).unwrap_or((0, true, true));
        // The facade total takes the library's new total in and the old one
        // out (`phrase_index.cpp:237-248`).
        self.inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
        let grams_live = self
            .container()
            .is_some_and(|container| container.keys().is_ok_and(|keys| !keys.is_empty()));
        self.mark_committed_phrase_write(db, arena_live || tables_live || grams_live);
    }

    /// Whether the session holds an item beyond the shipped chunks: an arena
    /// item of a user library, or one a system library's live buffer added.
    pub(super) fn arena_has_items(&self) -> bool {
        self.inner.libpinyin.as_ref().is_some_and(|target| {
            target
                .phrases()
                .libraries
                .values()
                .any(|library| !library.is_empty())
        }) || self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .has_added()
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
        let target = self
            .inner
            .libpinyin
            .as_ref()
            .ok_or(UserStoreError::InvalidPhrase)?;
        let item = ChunkItem {
            phrase: phrase.chars().map(u32::from).collect(),
            unigram,
            prons: readings
                .iter()
                .map(|(keys, count)| Ok((packed(keys)?, *count as u32)))
                .collect::<Result<_, UserStoreError>>()?,
        };
        let bytes = encoded(&item)?;
        let db = self.database();
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
        let token = (u32::from(ADDON_DICTIONARY) << 24) | slot;
        if phrase::next_library_token_after(ADDON_DICTIONARY, token).is_none() {
            return Err(UserStoreError::TokenSpaceExhausted);
        }
        let replacement = library
            .prepare_replace(slot, bytes)
            .map_err(UserStoreError::Persistence)?;
        self.take_item_in(ADDON_DICTIONARY, unigram, false);
        replacement.apply(phrases.libraries.entry(ADDON_DICTIONARY).or_default());
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
        self.mark_committed_phrase_write(db, true);
        Ok(token)
    }

    pub(crate) fn session_target(&self) -> Option<&crate::store_libpinyin::Target> {
        self.inner.libpinyin.as_deref()
    }

    pub(super) fn arena_train(
        &mut self,
        token: Token,
        readings: impl Iterator<Item = Vec<ChewingKey>>,
        delta: u32,
    ) -> Result<(), UserStoreError> {
        let target = self
            .inner
            .libpinyin
            .as_ref()
            .ok_or(UserStoreError::InvalidPhrase)?;
        let originals = &self.originals;
        let db = self.database();
        let mut phrases = target.phrases();
        if phrases.item(token).is_some() {
            if let Some(library) = phrases.item_mut(token) {
                library.train(token & PHRASE_MASK, readings, delta);
            }
            drop(phrases);
            self.mark_committed_write(db, true);
            return Ok(());
        }
        drop(phrases);
        // Any system item is trained in place in its library's live buffer; a
        // removed one answers no item there and is left alone.
        let mut items = self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if items.train_live(
            originals.get(&phrase_index_library_index(token)),
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
        self.mark_committed_write(db, true);
        Ok(())
    }

    /// Borrow a live user item directly from the session arena. The callback
    /// cannot retain the view past the guard or a later writer.
    pub fn with_user_item<R>(
        &self,
        token: Token,
        read: impl FnOnce(PhraseItemView<'_>) -> R,
    ) -> Option<R> {
        let phrases = self.inner.libpinyin.as_ref()?.phrases();
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
        if original.contains_item(token & PHRASE_MASK)
            || original
                .mapped
                .is_some_and(|library| library.is_detached() && !library.has_history())
        {
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
        let target = self
            .inner
            .libpinyin
            .as_ref()
            .ok_or(UserStoreError::InvalidPhrase)?;
        let packed = packed(keys)?;
        let db = self.database();
        let mut phrases = target.phrases();
        // `_add_phrase` searches the phrase table before any change
        // (`pinyin.cpp:536-542` at 074a2219); a record that search would read
        // past (register row 101) is refused here, before any commit.
        phrases
            .phrase_table
            .check_add(words)
            .map_err(|error| UserStoreError::Persistence(error.to_string()))?;
        let (token, new, replacement) = db.write(|txn| {
            let existing = match selected {
                Some(token) => token,
                None => {
                    let tokens = match phrases.phrase_table.tokens(&phrase_table::key_ucs4(words)) {
                        Ok(tokens) => tokens,
                        Err(error) => {
                            return Ok(Err(UserStoreError::Persistence(error.to_string())));
                        }
                    };
                    let tokens: Vec<_> = tokens
                        .into_iter()
                        .filter(|token| phrase_index_library_index(*token) == library)
                        .collect();
                    if tokens.len() > 1 {
                        return Ok(Err(UserStoreError::InvalidPhrase));
                    }
                    tokens.first().copied()
                }
            };
            if let Some(token) = existing {
                let Some(item) = phrases.item(token) else {
                    return Ok(Err(UserStoreError::InvalidPhrase));
                };
                if item.as_bytes()[6..6 + 4 * item.phrase_length()] != phrase_table::key_ucs4(words)
                {
                    return Ok(Err(UserStoreError::InvalidPhrase));
                }
                let result = phrases
                    .libraries
                    .entry(library)
                    .or_default()
                    .add_reading(token & PHRASE_MASK, &packed, delta)
                    .map_err(UserStoreError::Persistence);
                return Ok(result.map(|()| (token, false, None)));
            }
            let end = phrases
                .libraries
                .get(&library)
                .map_or(1, UserLibrary::token_end)
                .max(range_end);
            let raw = (u32::from(library) << 24) | end;
            let Some(token) = phrase::canonicalize_library_token(library, raw) else {
                return Ok(Err(UserStoreError::TokenSpaceExhausted));
            };
            if phrase::next_library_token_after(library, token).is_none() {
                return Ok(Err(UserStoreError::TokenSpaceExhausted));
            }
            let increment = delta.wrapping_mul(ADD_PHRASE_UNIGRAM_FACTOR_U32);
            let accepted = self
                .library_total_of(txn, &phrases, library)?
                .checked_add(increment)
                .is_some();
            let bytes = match encoded(&ChunkItem {
                phrase: words.to_vec(),
                unigram: if accepted { increment } else { 0 },
                prons: vec![(packed.clone(), delta)],
            }) {
                Ok(bytes) => bytes,
                Err(error) => return Ok(Err(error)),
            };
            let empty = UserLibrary::default();
            let replacement = match phrases
                .libraries
                .get(&library)
                .unwrap_or(&empty)
                .prepare_replace(token & PHRASE_MASK, bytes)
            {
                Ok(replacement) => replacement,
                Err(error) => return Ok(Err(UserStoreError::Persistence(error))),
            };
            self.take_item_in(library, increment, accepted);
            Ok(Ok((token, true, Some(replacement))))
        })??;
        if let Some(replacement) = replacement {
            replacement.apply(phrases.libraries.entry(library).or_default());
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
        self.mark_committed_import_write(db, new);
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
        let target = self
            .inner
            .libpinyin
            .as_ref()
            .ok_or(UserStoreError::InvalidPhrase)?;
        let original = self
            .originals
            .get(&library)
            .ok_or(UserStoreError::InvalidPhrase)?;
        let packed = packed(keys)?;
        let db = self.database();
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
        let mut items = self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(token) = existing {
            // The shipped chunk's own items are merged by the caller; only an
            // item the buffer added is held here.
            if original.contains_item(token & PHRASE_MASK)
                || (original
                    .mapped
                    .is_some_and(|library| library.is_detached() && !library.has_history())
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
            items.add_item(original, token, &merged);
            drop(items);
            drop(phrases);
            self.inner.has_system_items.store(true, Ordering::Relaxed);
            self.mark_committed_import_write(db, false);
            return Ok(token);
        }
        let end = items.range_end(library, original);
        let raw = (u32::from(library) << 24) | end;
        let token = phrase::canonicalize_library_token(library, raw)
            .ok_or(UserStoreError::TokenSpaceExhausted)?;
        if phrase::next_library_token_after(library, token).is_none() {
            return Err(UserStoreError::TokenSpaceExhausted);
        }
        let bytes = encoded(&ChunkItem {
            phrase: words.to_vec(),
            unigram: 0,
            prons: vec![(packed.clone(), delta)],
        })?;
        items.add_item(original, token, &bytes);
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
        self.mark_committed_import_write(db, true);
        Ok(token)
    }

    pub(super) fn arena_add_unigram(
        &self,
        txn: &mut impl WriteTxn,
        token: Token,
        delta: u32,
    ) -> Result<bool, StoreError> {
        if self.inner.libpinyin.is_none() {
            return add_unigram_frequency(txn, phrase_index_library_index(token), token, delta);
        }
        // Only the sub-index's overflow is the pin's `ERROR_INTEGER_OVERFLOW`;
        // a missing item answers another error that callers do not act on.
        Ok(self.arena_add_unigram_outcome(txn, token, delta)? != UnigramAdd::Refused)
    }

    /// `FacadePhraseIndex::add_unigram_frequency` over the session's state
    /// (074a2219 `phrase_index.h:628-635`, `phrase_index.cpp:150-176`). The
    /// facade total takes `delta` first; the sub-index then takes it only when
    /// it holds the item and its `guint32` total can: a system library's total
    /// is its live scalar, a user library's the sum of its items. A
    /// sub-index that does not take it leaves the delta in the facade's extra.
    pub(super) fn arena_add_unigram_outcome(
        &self,
        txn: &impl WriteTxn,
        token: Token,
        delta: u32,
    ) -> Result<UnigramAdd, StoreError> {
        let Some(target) = &self.inner.libpinyin else {
            return Ok(UnigramAdd::Missing);
        };
        let library = phrase_index_library_index(token);
        let original = self.originals.get(&library);
        let mut phrases = target.phrases();
        let mut items = self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if original.is_some() {
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
            return Ok(outcome);
        }
        items.add_facade_extra(u64::from(delta));
        drop(items);
        let total = (txn_get_u64_or(txn, LIBRARY_OFFSET, &[library], 0)? as u32).wrapping_add(
            phrases
                .libraries
                .get(&library)
                .map_or(0, UserLibrary::item_sum),
        );
        if total.checked_add(delta).is_none() {
            return Ok(UnigramAdd::Refused);
        }
        match phrases.item_mut(token) {
            Some(library) => {
                library.increment_unigram(token & PHRASE_MASK, delta);
                Ok(UnigramAdd::Accepted)
            }
            None => Ok(UnigramAdd::Missing),
        }
    }

    pub(super) fn arena_remove(&mut self, token: Token) -> Result<bool, UserStoreError> {
        let mask = PHRASE_INDEX_LIBRARY_MASK | PHRASE_MASK;
        // Hold the database guard from the snapshot through the mask.
        let db = self.database();
        let container_keys = self.container_keys();
        let bigram_fault = self.bigram_mask_fault_over(&db, container_keys.as_ref(), mask, token);
        let target = self
            .inner
            .libpinyin
            .as_ref()
            .ok_or(UserStoreError::InvalidPhrase)?;
        let mut phrases = target.phrases();
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
        let removed = db.write(|_txn| {
            if bigram_fault.is_some() {
                return Ok(None);
            }
            self.take_item_out(phrase_index_library_index(token), item.unigram);
            Ok(Some(true))
        })?;
        match removed {
            Some(true) => {}
            Some(false) => return Ok(false),
            None => return Err(bigram_fault.unwrap_or(UserStoreError::InvalidPhrase)),
        }
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
        let has = db.write(|txn| has_user_data_in_write_txn(txn))?
            || !target
                .phrases()
                .libraries
                .values()
                .all(UserLibrary::is_empty);
        self.mark_committed_phrase_write(db, has || grams_left);
        Ok(true)
    }

    pub(super) fn mask_arena(&self, mask: Token, value: Token) -> Result<(), UserStoreError> {
        let Some(target) = &self.inner.libpinyin else {
            return Ok(());
        };
        let mut phrases = target.phrases();
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
            library
                .remove_matching(selected)
                .map_err(UserStoreError::Persistence)?;
        }
        Ok(())
    }
}
