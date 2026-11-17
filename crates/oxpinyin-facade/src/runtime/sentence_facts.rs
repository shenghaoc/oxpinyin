//! One sentence's ordinary live-system item reads, shared by pricing/scoring.
//! Pin 074a2219 phonetic_lookup.h:643-697 reads one PhraseItem for length,
//! unigram and pronunciation. Keep exceptional resolution on the old path.
use super::*;
use oxpinyin_core::model::TokenPhrase;
use oxpinyin_core::sentence_facts::{SentenceFacts, TokenFacts};
use oxpinyin_user::data::phrase_library::PhraseItemView;

impl RuntimeDict<'_> {
    fn sentence_token(&self, token: u32, facts: &mut SentenceFacts) -> Option<TokenFacts> {
        let user = self.user;
        if !facts.validate(user.generation()) {
            return None;
        }
        if let Some(found) = facts.tokens.get(&token) {
            return *found;
        }
        // Classification runs outside LiveSystemItems::read's callback. In
        // particular, with_user_item probes the arena and live replay buffer before
        // the ordinary body read; an imported item retains its old error reads.
        let found = if self
            .live_items()
            .is_system_library(oxpinyin_user::phrase_index_library_index(token))
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
        facts.tokens.insert(token, found);
        found
    }

    pub(super) fn priced_with_facts(
        &self,
        path: &PathPricing,
        token: u32,
        want_text: bool,
        facts: &mut SentenceFacts,
    ) -> Option<PricedToken> {
        if !facts.validate(self.user.generation()) {
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
        let text = want_text.then(|| TokenPhrase {
            phrase_length: view.phrase_length(),
            text: view.phrase_text(),
        });
        let (matched, total) =
            Self::item_possibility(oxpinyin_user::SystemItem::Replayed(view), &path.keys);
        let pronunciation = Some((u64::from(matched), u64::from(total)));
        let result = PricedToken {
            item: TrellisItem {
                chars: saved.chars,
                pronunciation,
            },
            text,
        };
        Some(result)
    }
}

impl RuntimeLm<'_> {
    pub(super) fn costs_with_facts(
        &self,
        token: &PhraseToken,
        item: Option<(u32, u32)>,
        facts: &mut SentenceFacts,
    ) -> Option<Result<NbestStepCosts, LmError>> {
        let user = self.user;
        if !facts.validate(user.generation()) {
            return None;
        }
        let saved = facts.tokens.get(&token.value()).copied().flatten()?;
        let total = *facts
            .total
            .get_or_insert_with(|| user.unigram_total_over(|nibble| self.inner.is_visible(nibble)));
        // The unchanged live-visible branch ignores unigram_delta. No score is cached: each row occurrence supplies its
        // own freq/total, and expand_one applies path pronunciation and log.
        let extra = UserCountDelta {
            unigram_live: Some(saved.unigram),
            unigram_total_delta: total,
            ..UserCountDelta::ZERO
        };
        Some(self.inner.nbest_item_costs(token, extra, item))
    }
}

#[cfg(test)]
mod tests;
