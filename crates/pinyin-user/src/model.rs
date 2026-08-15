//! [`UserModel`] wiring for [`UserStore`].
//!
//! This is the first implementor of the `pinyin-core` seam — T0
//! (`docs/findings/user-store.md` §7) found the trait had zero implementors.
//! T1 wires only the `observe` (write) side. `score` is a neutral placeholder:
//! the decode-time additive merge of user counts with the system model is
//! W6-T4, and until then nothing consults `score`, so it must not influence
//! decode.

use pinyin_core::{Cost, UserModel};

use crate::store::{SENTENCE_START, Token, UserStore, UserStoreError};

impl UserModel for UserStore {
    type Token = Token;
    type Error = UserStoreError;

    /// Neutral placeholder — always `0` (no cost adjustment).
    ///
    /// The additive user/system merge is W6-T4; T1 expresses no preference so
    /// that adding the store cannot move any decode result.
    fn score(&self, _history: &[Self::Token], _token: &Self::Token) -> Result<Cost, Self::Error> {
        Ok(0)
    }

    /// Record an accepted `token` after `history` via the training-selection
    /// path (§2). The predecessor is the last token of `history`, or
    /// [`SENTENCE_START`] when `history` is empty.
    fn observe(&mut self, history: &[Self::Token], token: &Self::Token) -> Result<(), Self::Error> {
        let last = history.last().copied().unwrap_or(SENTENCE_START);
        self.observe_selection(last, *token)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "pinyin-user-model-{tag}-{}.redb",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn observe_uses_last_of_history_as_predecessor() {
        let path = temp_path("history");
        let mut store = UserStore::open(&path).unwrap();

        // history [5, 7] -> predecessor is 7.
        UserModel::observe(&mut store, &[5u32, 7], &42u32).unwrap();
        assert_eq!(store.bigram_count(7, 42).unwrap(), 69);
        assert_eq!(store.bigram_count(5, 42).unwrap(), 0);

        // empty history -> sentence_start.
        UserModel::observe(&mut store, &[], &9u32).unwrap();
        assert_eq!(store.bigram_count(SENTENCE_START, 9).unwrap(), 69);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn score_is_neutral_zero() {
        let path = temp_path("score");
        let store = UserStore::open(&path).unwrap();
        assert_eq!(UserModel::score(&store, &[1u32, 2], &3u32).unwrap(), 0);
        let _ = std::fs::remove_file(&path);
    }
}
