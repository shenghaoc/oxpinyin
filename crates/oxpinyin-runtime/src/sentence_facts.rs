//! One sentence's ordinary live-system item reads, shared by pricing/scoring.
//! Pin 074a2219 phonetic_lookup.h:643-697 reads one PhraseItem for length,
//! unigram and pronunciation. Keep exceptional resolution on the old path.
use super::*;
use oxpinyin_core::sentence_facts::{SentenceFacts, TokenFacts};
use oxpinyin_data::phrase_library::PhraseItemView;

impl RuntimeDict<'_> {
    fn sentence_token(&self, token: u32, facts: &mut SentenceFacts) -> Option<TokenFacts> {
        let user = self.user?;
        if !facts.validate(user.generation()) {
            return None;
        }
        if let Some(found) = facts.tokens.get(&token) {
            return *found;
        }
        // Classification runs outside LiveSystemItems::read's callback. In
        // particular, with_user_item probes arena and replay provenance before
        // the ordinary body read; an imported item retains its old error reads.
        let found = if self.live_items().is_system_library((token >> 24) as u8)
            && user.with_user_item(token, |_| ()).is_none()
        {
            self.live_items()
                .read(token, |item| {
                    if item.origin() != ItemOrigin::Overlay {
                        return None;
                    }
                    let offset = facts.bytes.len();
                    let bytes = item.as_bytes();
                    facts.bytes.extend_from_slice(bytes);
                    Some(TokenFacts {
                        chars: item.phrase_length() as u32,
                        unigram: item.unigram(),
                        pronunciations: item.n_pronunciations() as u32,
                        flags: 1, // readable, visible ordinary overlay; no user/import
                        offset,
                        len: bytes.len(),
                    })
                })
                .found()
                .flatten()
        } else {
            None
        };
        #[cfg(test)]
        if test_boundary(Boundary::Fill) {
            facts.validate(user.generation().wrapping_add(1));
        }
        if !facts.validate(user.generation()) {
            return None;
        }
        facts.tokens.insert(token, found);
        found
    }

    pub(super) fn priced_with_facts(
        &self,
        path: &PathPricing,
        token: u32,
        want_text: bool,
        facts: &mut SentenceFacts,
    ) -> Result<Option<PricedToken>, DictError> {
        if !facts.validate(self.user.map_or(0, UserStore::generation)) {
            return self.priced_on_path(path, token, want_text);
        }
        // The last user entry is path-specific. Never replace it with facts
        // captured while another path priced this token as an ordinary item.
        if path.user.iter().any(|entry| entry.token().value() == token) {
            facts.tokens.insert(token, None);
            return self.priced_on_path(path, token, want_text);
        }
        let Some(saved) = self.sentence_token(token, facts) else {
            return self.priced_on_path(path, token, want_text);
        };
        let Some(bytes) = facts
            .bytes
            .get(saved.offset..)
            .and_then(|bytes| bytes.get(..saved.len))
        else {
            return self.priced_on_path(path, token, want_text);
        };
        let Some(view) = PhraseItemView::from_bytes(bytes) else {
            return self.priced_on_path(path, token, want_text);
        };
        let text = if want_text { view.phrase_text() } else { None };
        if want_text && text.is_none() {
            return Ok(None);
        }
        let pronunciation = path.keys.as_deref().map(|query| {
            let (matched, total) =
                Self::item_possibility(oxpinyin_user::SystemItem::Replayed(view), query);
            (u64::from(matched), u64::from(total))
        });
        let result = PricedToken {
            item: TrellisItem {
                chars: saved.chars,
                pronunciation,
            },
            text,
        };
        if !facts.validate(self.user.map_or(0, UserStore::generation)) {
            return self.priced_on_path(path, token, want_text);
        }
        Ok(Some(result))
    }
}

impl RuntimeLm<'_> {
    pub(super) fn costs_with_facts(
        &self,
        token: &PhraseToken,
        item: Option<(u32, u32)>,
        facts: &mut SentenceFacts,
    ) -> Option<Result<NbestStepCosts, LmError>> {
        let user = self.user?;
        if !facts.validate(user.generation()) {
            return None;
        }
        let saved = facts.tokens.get(&token.value()).copied().flatten()?;
        let total = *facts.total.get_or_insert_with(|| {
            self.total_extra(user.unigram_total_over(|nibble| self.inner.is_visible(nibble)))
        });
        // The unchanged live-visible branch ignores both unigram_delta and
        // imported_item. No score is cached: each row occurrence supplies its
        // own freq/total, and expand_one applies path pronunciation and log.
        let extra = UserCountDelta {
            unigram_live: Some(saved.unigram),
            unigram_total_delta: total,
            ..UserCountDelta::ZERO
        };
        let result = self.inner.nbest_item_costs(token, extra, false, item);
        #[cfg(test)]
        if test_boundary(Boundary::Score) {
            facts.validate(user.generation().wrapping_add(1));
        }
        if !facts.validate(user.generation()) {
            return None;
        }
        Some(result)
    }
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq)]
enum Boundary {
    Fill,
    Score,
}
#[cfg(test)]
std::thread_local! {
    // Inject the changed observation at the same boundary as a concurrent
    // commit. Shared-context C calls themselves must be serialized.
    static BOUNDARY: std::cell::Cell<Option<Boundary>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
fn test_boundary(at: Boundary) -> bool {
    BOUNDARY.with(|slot| {
        if slot.get() == Some(at) {
            slot.set(None);
            true
        } else {
            false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/w3")
            .join(oxpinyin_data::DEFAULT_STORE_EXT)
    }
    fn opened() -> Runtime {
        Runtime::open(&fixture(), None).unwrap()
    }
    fn token(runtime: &Runtime) -> u32 {
        runtime.dict().system().tokens_for_text("你").unwrap()[0]
    }
    fn path(runtime: &Runtime, spelling: &str) -> PathPricing {
        runtime
            .dict()
            .path_pricing(&[SyllableKey::from_text(spelling).unwrap()])
            .unwrap()
    }
    fn price(runtime: &Runtime, path: &PathPricing, token: u32, facts: &mut SentenceFacts) {
        let old = runtime.dict().priced_on_path(path, token, true).unwrap();
        let new = runtime
            .dict()
            .priced_with_facts(path, token, true, facts)
            .unwrap();
        assert_eq!(
            old.as_ref()
                .map(|p| (p.item.chars, p.item.pronunciation, p.text.clone())),
            new.as_ref()
                .map(|p| (p.item.chars, p.item.pronunciation, p.text.clone()))
        );
    }
    fn score(runtime: &Runtime, token: u32, facts: &mut SentenceFacts) {
        let lm = runtime.lm();
        for item in [None, Some((0, 0)), Some((3, 7)), Some((5, 7))] {
            let old = lm
                .nbest_item_costs_in_row(
                    &PhraseToken::new(1),
                    &PhraseToken::new(token),
                    item,
                    &mut Default::default(),
                )
                .unwrap();
            let new = lm
                .nbest_item_costs_with_facts(
                    &PhraseToken::new(1),
                    &PhraseToken::new(token),
                    item,
                    &mut Default::default(),
                    facts,
                )
                .unwrap();
            assert_eq!(
                (
                    old.blended.map(f64::to_bits),
                    old.unigram.map(f64::to_bits),
                    old.unigram_lambda.to_bits()
                ),
                (
                    new.blended.map(f64::to_bits),
                    new.unigram.map(f64::to_bits),
                    new.unigram_lambda.to_bits()
                )
            );
        }
    }
    #[test]
    fn pricing_and_scoring_share_one_item_on_two_paths() {
        let runtime = opened();
        let token = token(&runtime);
        let mut facts = SentenceFacts::default();
        for spelling in ["ni", "hao", "ni"] {
            price(&runtime, &path(&runtime, spelling), token, &mut facts);
            score(&runtime, token, &mut facts);
            let saved = facts.tokens.get(&token).copied().flatten().unwrap();
            assert_eq!(facts.tokens.len(), 1);
            assert_eq!(
                facts.bytes.len(),
                saved.len,
                "one copy despite different paths and transitions"
            );
            assert!(facts.total.is_some());
        }
        let ni = runtime
            .dict()
            .priced_with_facts(&path(&runtime, "ni"), token, false, &mut facts)
            .unwrap()
            .unwrap();
        let hao = runtime
            .dict()
            .priced_with_facts(&path(&runtime, "hao"), token, false, &mut facts)
            .unwrap()
            .unwrap();
        assert_ne!(ni.item.pronunciation, hao.item.pronunciation);
        assert_eq!(std::mem::size_of::<TokenFacts>(), 32);
    }
    #[test]
    fn next_guess_reads_committed_training() {
        let mut runtime = opened();
        let token = token(&runtime);
        let mut first = SentenceFacts::default();
        price(&runtime, &path(&runtime, "ni"), token, &mut first);
        let before = first.tokens[&token].unwrap().unigram;
        runtime
            .user_store_mut()
            .unwrap()
            .observe_selection(1, token)
            .unwrap();
        let mut next = SentenceFacts::default();
        price(&runtime, &path(&runtime, "ni"), token, &mut next);
        assert!(next.tokens[&token].unwrap().unigram > before);
        score(&runtime, token, &mut next);
        // Even a mistakenly reused context retires rather than serving old facts.
        score(&runtime, token, &mut first);
        assert!(first.tokens.is_empty());
    }

    #[test]
    fn generation_change_during_fill_or_score_retires_only_the_facts() {
        for boundary in [Boundary::Fill, Boundary::Score] {
            let runtime = opened();
            let token = token(&runtime);
            let path = path(&runtime, "ni");
            let mut facts = SentenceFacts::default();
            if boundary == Boundary::Score {
                price(&runtime, &path, token, &mut facts);
            }
            BOUNDARY.with(|slot| slot.set(Some(boundary)));
            if boundary == Boundary::Fill {
                price(&runtime, &path, token, &mut facts);
            } else {
                score(&runtime, token, &mut facts);
            }
            assert!(facts.tokens.is_empty());
            assert!(facts.bytes.is_empty());
            assert!(facts.total.is_none());
            price(&runtime, &path, token, &mut facts);
            score(&runtime, token, &mut facts);
            assert!(
                facts.tokens.is_empty(),
                "disabled for remainder, not refilled"
            );
        }
    }
    fn fallback(runtime: &Runtime, token: u32) {
        let mut facts = SentenceFacts::default();
        price(runtime, &path(runtime, "ni"), token, &mut facts);
        score(runtime, token, &mut facts);
        assert!(facts.tokens.get(&token).copied().flatten().is_none());
        assert!(facts.bytes.is_empty());
    }
    #[test]
    fn user_imported_masked_missing_and_detached_keep_old_readers() {
        let mut runtime = opened();
        let ni = SyllableKey::from_text("ni").unwrap();
        let keys = [oxpinyin_user::toned_key(ni.index(), 0).unwrap()];
        let user = runtime
            .user_store_mut()
            .unwrap()
            .add_phrase_in(7, &"词".into(), &keys, Some(5))
            .unwrap();
        fallback(&runtime, user);
        let imported = runtime
            .user_store_mut()
            .unwrap()
            .add_system_phrase_in(1, None, &"龘".into(), &keys, Some(0))
            .unwrap();
        fallback(&runtime, imported);
        fallback(&runtime, 0x01ff_fffe);
        let ordinary = token(&runtime);
        runtime
            .library_mask
            .store(1 << (ordinary >> 24), Ordering::SeqCst);
        fallback(&runtime, ordinary);
        runtime.library_mask.store(0, Ordering::SeqCst);
        // A session detached from this runtime's system libraries.
        runtime.user = Some(
            UserStore::open_transient(
                oxpinyin_user::SystemOriginals::new(
                    Default::default(),
                    oxpinyin_data::user_files::UserFileLayout::stock(),
                ),
                oxpinyin_data::user_files::SystemVersions::for_this_build(7, 14),
                oxpinyin_user::UserConfLaw::Pinyin,
            )
            .unwrap(),
        );
        fallback(&runtime, ordinary);
        runtime.user = None;
        let (_, libraries) = SystemDictionary::open_with(&fixture(), runtime.table_conf())
            .unwrap()
            .into_parts();
        runtime.originals = oxpinyin_user::system_originals(
            libraries,
            UserFileLayout::from_conf(runtime.table_conf()),
        );
        assert_eq!(
            runtime
                .dict()
                .live_items()
                .read(ordinary, |item| item.origin())
                .found(),
            Some(ItemOrigin::Shipped)
        );
        fallback(&runtime, ordinary);
    }
    fn replayed(
        change: impl FnOnce(u32, Vec<u8>) -> oxpinyin_data::user_files::LogRecord,
    ) -> (tempfile::TempDir, Runtime, u32) {
        let runtime = opened();
        let token = token(&runtime);
        let old = runtime
            .dict()
            .live_items()
            .read(token, |item| item.as_bytes().to_vec())
            .found()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let payload = oxpinyin_data::user_files::encode_log_records(&[change(
            token & oxpinyin_user::PHRASE_MASK,
            old,
        )])
        .unwrap();
        std::fs::write(
            dir.path().join("gb_char.dbin"),
            oxpinyin_data::chunk_format::build_memory_chunk(&payload).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&SystemVersions::from_conf(
                runtime.table_conf(),
            ))
            .to_text(),
        )
        .unwrap();
        drop(runtime);
        let runtime = Runtime::open(&fixture(), Some(dir.path())).unwrap();
        (dir, runtime, token)
    }
    #[test]
    fn present_zero_unigram_is_a_live_fact() {
        let (_dir, runtime, token) = replayed(|token, old_item| {
            let mut new_item = old_item.clone();
            new_item[2..6].copy_from_slice(&0u32.to_le_bytes());
            oxpinyin_data::user_files::LogRecord::Modify {
                token,
                old_item,
                new_item,
            }
        });
        let mut facts = SentenceFacts::default();
        price(&runtime, &path(&runtime, "ni"), token, &mut facts);
        assert_eq!(facts.tokens[&token].unwrap().unigram, 0);
        score(&runtime, token, &mut facts);
    }
    #[test]
    fn removed_and_unreadable_items_keep_old_resolution() {
        let (_dir, runtime, token) =
            replayed(
                |token, old_item| oxpinyin_data::user_files::LogRecord::Remove { token, old_item },
            );
        fallback(&runtime, token);
        let (_dir, runtime, token) = replayed(|token, old_item| {
            let mut new_item = old_item.clone();
            new_item[1] = 255;
            // Force remove-and-append, so the declared readings run past
            // the live buffer rather than into the next ordinary item.
            new_item.push(0);
            oxpinyin_data::user_files::LogRecord::Modify {
                token,
                old_item,
                new_item,
            }
        });
        fallback(&runtime, token);
    }
}
