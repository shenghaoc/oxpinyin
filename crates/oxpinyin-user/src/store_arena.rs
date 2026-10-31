//! Session phrase operations. Standalone stores retain their row persistence.

use super::*;
use crate::user_library::{SessionPhrases, UserLibrary};
use oxpinyin_data::chunk_write::{ChunkItem, decode_phrase_item, encode_phrase_item};
use oxpinyin_data::phrase_library::PhraseItemView;

fn encoded(item: &ChunkItem) -> Result<Vec<u8>, UserStoreError> {
    encode_phrase_item(item).map_err(|error| UserStoreError::Persistence(error.to_string()))
}

fn packed(keys: &[PinyinKey]) -> Result<Vec<u16>, UserStoreError> {
    crate::store_libpinyin::pinyin_keys_to_packed(keys).ok_or(UserStoreError::InvalidPhrase)
}

fn library_total(
    txn: &impl WriteTxn,
    phrases: &SessionPhrases,
    library: u8,
) -> Result<u32, StoreError> {
    let mut total = (txn_get_u64_or(txn, SYSTEM_BASE, &[library], 0)? as u32)
        .wrapping_add(txn_get_u64_or(txn, LIBRARY_OFFSET, &[library], 0)? as u32)
        .wrapping_add(
            phrases
                .libraries
                .get(&library)
                .map_or(0, UserLibrary::item_sum),
        );
    let lo = codec::encode_token(u32::from(library) << 24);
    let hi = codec::encode_token(u32::from(library + 1) << 24);
    txn.range(
        UNIGRAM,
        Bound::Included(&lo),
        Bound::Excluded(&hi),
        |_, value| {
            total = total.wrapping_add(
                codec::decode_u64(value)
                    .map_err(|_| StoreError::Backend("corrupt unigram".into()))?
                    as u32,
            );
            Ok(())
        },
    )?;
    Ok(total)
}

impl<S: WriteStore> GenericUserStore<S> {
    pub(super) fn replace_user_arena(
        &mut self,
        nibble: u8,
        library: Option<UserLibrary>,
    ) -> Result<(), UserStoreError> {
        self.flush_system_unigrams()?;
        let Some(target) = &self.inner.libpinyin else {
            return Ok(());
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
        let (moved, offsets_left, tables_live) = db.write(|txn| {
            let key = [nibble];
            let old_offset = txn_get_u64_or(txn, LIBRARY_OFFSET, &key, 0)? as u32;
            if offset == 0 {
                txn.remove(LIBRARY_OFFSET, &key)?;
            } else {
                txn.put(LIBRARY_OFFSET, &key, &codec::encode_u64(u64::from(offset)))?;
            }
            let total = txn_get_u64_or(txn, UNIGRAM_TOTAL, &[UNIGRAM_TOTAL_KEY], 0)?;
            txn.put(
                UNIGRAM_TOTAL,
                &[UNIGRAM_TOTAL_KEY],
                &codec::encode_u64(total.wrapping_sub(before).wrapping_add(after)),
            )?;
            #[cfg(test)]
            if self.inner.fail_replace.swap(false, Ordering::SeqCst) {
                return Err(StoreError::Backend("injected write failure".into()));
            }
            Ok((
                offset.wrapping_sub(old_offset),
                !txn.is_empty(LIBRARY_OFFSET)?,
                has_user_data_in_write_txn(txn)?,
            ))
        })?;
        phrases.libraries.remove(&nibble);
        if let Some(library) = library {
            phrases.libraries.insert(nibble, library);
        }
        let arena_live = phrases
            .libraries
            .values()
            .any(|library| !library.is_empty());
        drop(phrases);
        self.inner.facade_offset.fetch_add(moved, Ordering::SeqCst);
        self.inner
            .has_library_offsets
            .store(offsets_left, Ordering::SeqCst);
        let grams_live = self
            .container()
            .is_some_and(|container| container.keys().is_ok_and(|keys| !keys.is_empty()));
        self.mark_committed_phrase_write(db, arena_live || tables_live || grams_live);
        Ok(())
    }

    pub(super) fn arena_has_system_items(&self) -> bool {
        self.inner.libpinyin.as_ref().is_some_and(|target| {
            target
                .phrases()
                .libraries
                .iter()
                .any(|(library, items)| (1..=4).contains(library) && !items.is_empty())
        })
    }

    pub(super) fn arena_has_items(&self) -> bool {
        self.inner.libpinyin.as_ref().is_some_and(|target| {
            target
                .phrases()
                .libraries
                .values()
                .any(|library| !library.is_empty())
        })
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
        self.flush_system_unigrams()?;
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
        db.write(|txn| {
            phrase_table::add_index(txn, phrase, token)?;
            bump_unigram_total(txn, u64::from(unigram))
        })?;
        replacement.apply(phrases.libraries.entry(ADDON_DICTIONARY).or_default());
        for (keys, _) in item.prons {
            phrases.indexed.insert((token, keys));
        }
        drop(phrases);
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        for (keys, _) in readings {
            self.note_index_add(keys, token, false);
        }
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
        self.flush_system_unigrams()?;
        if self.system_item_removed(token) {
            return Ok(());
        }
        let target = self
            .inner
            .libpinyin
            .as_ref()
            .ok_or(UserStoreError::InvalidPhrase)?;
        let originals = target.originals();
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
        let mut items = self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !items.contains_key(&token)
            && let Some(item) = originals
                .get(&phrase_index_library_index(token))
                .and_then(|library| library.item(token & PHRASE_MASK))
        {
            items.insert(token, item);
            self.inner.has_system_items.store(true, Ordering::Relaxed);
        }
        if let Some(item) = items.get_mut(&token) {
            for reading in readings {
                let mut total = 0_u32;
                for (keys, frequency) in &mut item.prons {
                    total = total.wrapping_add(*frequency);
                    if !crate::store_libpinyin::pronunciation_matches(
                        &reading,
                        keys.iter().copied(),
                    ) {
                        continue;
                    }
                    let Some(next) = total.checked_add(delta) else {
                        break;
                    };
                    total = next;
                    *frequency = frequency.wrapping_add(delta);
                }
            }
        }
        drop(items);
        drop(originals);
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
        phrases.item(token).map(read)
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
        self.flush_system_unigrams()?;
        let target = self
            .inner
            .libpinyin
            .as_ref()
            .ok_or(UserStoreError::InvalidPhrase)?;
        let packed = packed(keys)?;
        let db = self.database();
        let mut phrases = target.phrases();
        let (token, new, replacement) = db.write(|txn| {
            let existing = match selected {
                Some(token) => token,
                None => {
                    let mut tokens = Vec::new();
                    if let Some(found) = txn.read_with(
                        phrase_table::TABLE,
                        &phrase_table::key_ucs4(words),
                        |bytes| {
                            phrase_table::search_bytes(bytes, &mut tokens, |token| {
                                phrase_index_library_index(token) == library
                            })
                        },
                    )? {
                        found?;
                    }
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
            let accepted = library_total(txn, &phrases, library)?
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
            phrase_table::add_index_words(txn, words, token)?;
            bump_unigram_total(txn, u64::from(increment))?;
            Ok(Ok((token, true, Some(replacement))))
        })??;
        if let Some(replacement) = replacement {
            replacement.apply(phrases.libraries.entry(library).or_default());
            phrases.indexed.insert((token, packed));
        }
        drop(phrases);
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        if new {
            self.note_index_add(keys, token, false);
        }
        if (1..=4).contains(&library) {
            self.revive_system_item(token);
            self.inner.has_system_items.store(true, Ordering::Relaxed);
        }
        self.mark_committed_import_write(db, new);
        Ok(token)
    }

    pub(super) fn arena_add_unigram(
        &self,
        txn: &mut impl WriteTxn,
        token: Token,
        delta: u32,
    ) -> Result<bool, StoreError> {
        let Some(target) = &self.inner.libpinyin else {
            return add_unigram_frequency(txn, phrase_index_library_index(token), token, delta);
        };
        let mut phrases = target.phrases();
        let library = phrase_index_library_index(token);
        bump_unigram_total(txn, u64::from(delta))?;
        if library_total(txn, &phrases, library)?
            .checked_add(delta)
            .is_none()
        {
            return Ok(false);
        }
        if phrases.item(token).is_some() {
            if let Some(library) = phrases.item_mut(token) {
                library.increment_unigram(token & PHRASE_MASK, delta);
            }
        } else {
            let key = codec::encode_token(token);
            let old = txn_get_u64_or(txn, UNIGRAM, &key, 0)?;
            txn.put(
                UNIGRAM,
                &key,
                &codec::encode_u64(old.wrapping_add(u64::from(delta))),
            )?;
        }
        Ok(true)
    }

    pub(super) fn arena_remove(&mut self, token: Token) -> Result<bool, UserStoreError> {
        let mask = PHRASE_INDEX_LIBRARY_MASK | PHRASE_MASK;
        if let Some(error) = self.bigram_mask_fault(mask, token) {
            return Err(error);
        }
        let target = self
            .inner
            .libpinyin
            .as_ref()
            .ok_or(UserStoreError::InvalidPhrase)?;
        let db = self.database();
        let mut phrases = target.phrases();
        let Some(view) = phrases.item(token) else {
            return Ok(false);
        };
        let item = decode_phrase_item(view.as_bytes())
            .map_err(|e| UserStoreError::Persistence(e.to_string()))?;
        if item
            .prons
            .iter()
            .any(|(keys, _)| !phrases.indexed.contains(&(token, keys.clone())))
        {
            return Ok(false);
        }
        let removed = db.write(|txn| {
            if phrase_table::remove_index_ucs4(txn, &item.phrase, token)? != phrase_table::ERROR_OK
            {
                return Ok(false);
            }
            let total = txn_get_u64_or(txn, UNIGRAM_TOTAL, &[UNIGRAM_TOTAL_KEY], 0)?;
            txn.put(
                UNIGRAM_TOTAL,
                &[UNIGRAM_TOTAL_KEY],
                &codec::encode_u64(total.saturating_sub(u64::from(item.unigram))),
            )?;
            Ok(true)
        })?;
        if !removed {
            return Ok(false);
        }
        let rows = IndexRows::from_index(
            item.prons
                .iter()
                .map(|(keys, _)| (token, keys.clone()))
                .collect::<Vec<_>>()
                .iter(),
        );
        for (keys, _) in &item.prons {
            phrases.indexed.remove(&(token, keys.clone()));
        }
        if let Some(library) = phrases.item_mut(token) {
            library.remove(token & PHRASE_MASK);
        }
        drop(phrases);
        self.keep_index_rows(&rows);
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        for keys in rows.readings() {
            self.note_index_remove(keys, token);
        }
        let grams_left = self.mask_container(mask, token)?;
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
            library
                .remove_matching(|slot| (((u32::from(nibble) << 24) | slot) & mask) == value)
                .map_err(UserStoreError::Persistence)?;
        }
        Ok(())
    }

    pub(super) fn arena_replacement(
        &self,
        originals: &crate::SystemOriginals,
        state: &crate::persistence::UserState,
    ) -> Result<std::collections::BTreeMap<u8, UserLibrary>, UserStoreError> {
        let mut libraries = std::collections::BTreeMap::<u8, UserLibrary>::new();
        for (&nibble, items) in &state.libraries {
            let library = libraries.entry(nibble).or_default();
            for (&slot, item) in items {
                library
                    .replace(slot, &encoded(item)?)
                    .map_err(UserStoreError::Persistence)?;
            }
        }
        for (&nibble, items) in &state.system_overrides {
            for (&slot, item) in items {
                if !originals
                    .get(&nibble)
                    .is_some_and(|library| library.contains_item(slot))
                    && let Some(item) = item
                {
                    libraries
                        .entry(nibble)
                        .or_default()
                        .replace(slot, &encoded(item)?)
                        .map_err(UserStoreError::Persistence)?;
                }
            }
        }
        Ok(libraries)
    }
}
