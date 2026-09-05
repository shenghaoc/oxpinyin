//! The zhuyin Python session: an [`InstanceCore`] plus the Python-side
//! candidate snapshot.
//!
//! The orchestration — scheme/option state, the `begin_parse`
//! continuation rule, the batch seams, the offset maps — lives in
//! [`oxpinyin_facade`], shared with both C-ABI facades; this module keeps
//! only what is Python-side: the candidate snapshot (plain data, no C
//! handles), the guess/choose/clear driver over the core, the scheme
//! setters the C layer never shared, and the int↔enum representation
//! helpers the binding translates through.
//!
//! The snapshot split is the one place this layer must be careful: the
//! core clears its own state on every parse, but it cannot see this
//! struct's snapshot — so every parse entry and `reset` clears the
//! snapshot here, exactly where the C facades clear theirs (the
//! regression class the extraction review caught: a snapshot that
//! survives a re-parse reads back stale rows and selects rows the
//! caller never saw).

use oxpinyin_core::{ChewingKey, FullPinyinScheme, ZhuyinScheme};
use oxpinyin_engine::{CandidateKind, CandidateList, EngineError};
use oxpinyin_facade::{
    BEFORE_CURSOR_ANCHOR, InstanceCore, LiveOptions, ToneForwarding, ZHUYIN_DEFAULT_OPTION_WORD,
    compute_prefixes, zhuyin_lookup_session_offset, zhuyin_original_offset, zhuyin_session_offset,
};
use oxpinyin_runtime::{Runtime, RuntimeSession};
use oxpinyin_user::UserStore;

/// The discriminant the C ABI uses for each chewing keyboard
/// (`zhuyin.cpp` at the pin; `oxpinyin-zhuyin-capi::parse::zhuyin_scheme`).
#[must_use]
pub const fn chewing_scheme_value(scheme: ZhuyinScheme) -> u8 {
    scheme as u8
}

/// The inverse: out-of-enum integers are `None` (the C setters' refusal of
/// anything outside 1..=9, abort slot included).
#[must_use]
pub const fn chewing_scheme_from_value(value: u8) -> Option<ZhuyinScheme> {
    match value {
        1 => Some(ZhuyinScheme::Standard),
        2 => Some(ZhuyinScheme::Hsu),
        3 => Some(ZhuyinScheme::Ibm),
        4 => Some(ZhuyinScheme::Ginyieh),
        5 => Some(ZhuyinScheme::Eten),
        6 => Some(ZhuyinScheme::Eten26),
        7 => Some(ZhuyinScheme::StandardDvorak),
        8 => Some(ZhuyinScheme::HsuDvorak),
        9 => Some(ZhuyinScheme::DachenCp26),
        _ => None,
    }
}

/// The discriminant the C ABI uses for each full-pinyin scheme
/// (`oxpinyin-zhuyin-capi::parse::full_scheme`).
#[must_use]
pub const fn full_scheme_value(scheme: FullPinyinScheme) -> u8 {
    scheme as u8
}

/// The inverse over the accepted 1..=3.
#[must_use]
pub const fn full_scheme_from_value(value: u8) -> Option<FullPinyinScheme> {
    match value {
        1 => Some(FullPinyinScheme::Hanyu),
        2 => Some(FullPinyinScheme::Luoma),
        3 => Some(FullPinyinScheme::SecondaryZhuyin),
        _ => None,
    }
}

/// Message for an out-of-enum keyboard discriminant — shared by the binding
/// (which raises it) and the parity driver (which records it), so the two
/// transcripts agree byte for byte.
#[must_use]
pub fn unknown_chewing_scheme_message(value: u8) -> String {
    format!("unknown zhuyin keyboard scheme {value}")
}

/// Message for the unimplemented StandardDvorak slot — shared like above.
#[must_use]
pub const fn dvorak_scheme_message() -> &'static str {
    "zhuyin keyboard StandardDvorak (7) is not implemented"
}

/// Message for an out-of-enum full-pinyin discriminant — shared like above.
#[must_use]
pub fn unknown_full_scheme_message(value: u8) -> String {
    format!("unknown full-pinyin scheme {value}")
}

/// Message for a multi-character `in_keyboard` probe — shared like above.
#[must_use]
pub const fn in_keyboard_arity_message() -> &'static str {
    "in_keyboard takes a single keystroke character"
}

/// Which candidate list a snapshotted row came from — the zhuyin-local
/// 4-value enum (`zhuyin.h:41-45` at the pin).
///
/// The discriminants collide with the pinyin eight at 3 and 4, so this enum
/// is never aliased to the pinyin one; the snapshotter below tags with this
/// enum only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZhuyinCandidateType {
    /// The sentence row at the list head.
    BestMatch,
    /// A normal row from an after-cursor guess.
    NormalAfterCursor,
    /// A normal row from a before-cursor guess.
    NormalBeforeCursor,
    /// Reserved by the C ABI; the snapshotter never assigns it.
    Zombie,
}

impl ZhuyinCandidateType {
    /// The C enum discriminant (`zhuyin.h:41-45`).
    #[must_use]
    pub const fn value(self) -> u32 {
        match self {
            Self::BestMatch => 1,
            Self::NormalAfterCursor => 2,
            Self::NormalBeforeCursor => 3,
            Self::Zombie => 4,
        }
    }

    /// The stable Python label, mirroring the `kind_label` precedent: a
    /// future variant degrades at the translation layer, never here.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::BestMatch => "best_match",
            Self::NormalAfterCursor => "normal_after_cursor",
            Self::NormalBeforeCursor => "normal_before_cursor",
            Self::Zombie => "zombie",
        }
    }
}

/// One snapshotted candidate: the text plus the coordinates the C facade's
/// `CapiCandidate` carries across calls, minus the C string and pointer.
#[derive(Clone, Debug)]
pub struct ZhuyinCandidate {
    text: String,
    kind: CandidateKind,
    candidate_type: ZhuyinCandidateType,
    nbest_index: u8,
    /// Bytes of *original* input this candidate consumed — mapped back from
    /// session coordinates through the active parse's key spans.
    consumed_bytes: usize,
    /// The index this candidate held in the window it was snapshotted from;
    /// `choose` resolves through it.
    source_index: usize,
    /// Decoder cost that ranked this candidate; opaque — trust list order.
    cost: i64,
}

impl ZhuyinCandidate {
    /// The Chinese text this candidate would insert.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Where the candidate came from (phrase/addon/sentence/fallback).
    #[must_use]
    pub const fn kind(&self) -> CandidateKind {
        self.kind
    }

    /// Which zhuyin list produced this row.
    #[must_use]
    pub const fn candidate_type(&self) -> ZhuyinCandidateType {
        self.candidate_type
    }

    /// Tail rank when this is an n-best sentence row, else 0.
    #[must_use]
    pub const fn nbest_index(&self) -> u8 {
        self.nbest_index
    }

    /// Original-input bytes absorbed by this candidate.
    #[must_use]
    pub const fn consumed_bytes(&self) -> usize {
        self.consumed_bytes
    }

    /// Decoder cost that ranked this candidate; opaque — trust list order.
    #[must_use]
    pub const fn cost(&self) -> i64 {
        self.cost
    }
}

/// One opened zhuyin session: the shared [`InstanceCore`] plus the
/// Python-side candidate snapshot.
///
/// The snapshot is the one piece of facade state the core deliberately
/// does not own (it is plain Rust data shaped for the binding, the way
/// the C facades' `CString` snapshots are shaped for the ABI) — so this
/// struct clears it at exactly the points the C facades clear theirs:
/// every parse entry and reset. The core clears its own state; it cannot
/// reach this vec.
pub struct ZhuyinSession {
    core: InstanceCore,
    candidates: Vec<ZhuyinCandidate>,
}

impl ZhuyinSession {
    /// Wraps an opened runtime and a fresh session — the
    /// `zhuyin_init`+`zhuyin_alloc_instance` shape without the C handles.
    ///
    /// Seeds the `USE_TONE | FORCE_TONE` option word through the shared
    /// seed constant, and enables the libzhuyin sentence-row display law
    /// (every `BEST_MATCH` row reads the 1-best, so the observable list
    /// carries exactly one sentence row — see
    /// `Session::set_collapse_sentence_rows_to_best`), exactly as
    /// `oxpinyin-zhuyin-capi` constructs them.
    #[must_use]
    pub fn open(runtime: &Runtime, mut session: RuntimeSession) -> Self {
        session.set_collapse_sentence_rows_to_best(true);
        let user = runtime.user_store();
        let core = InstanceCore::new(
            session,
            user,
            runtime.dict(),
            runtime.lm(),
            LiveOptions::new(ZHUYIN_DEFAULT_OPTION_WORD),
        );
        Self {
            core,
            candidates: Vec::new(),
        }
    }

    /// The live chewing keyboard scheme.
    #[must_use]
    pub fn chewing_scheme(&self) -> ZhuyinScheme {
        let value = self
            .core
            .live
            .zhuyin_scheme
            .load(std::sync::atomic::Ordering::Relaxed);
        chewing_scheme_from_value(value as u8).unwrap_or(ZhuyinScheme::Standard)
    }

    /// Selects a chewing keyboard — the `zhuyin_set_chewing_scheme` law:
    /// every implemented keyboard switches, the `STANDARD_DVORAK`
    /// upstream-abort slot reports `false` instead of aborting (no-abort
    /// policy, divergence class (c)).
    pub fn set_chewing_scheme(&mut self, scheme: ZhuyinScheme) -> bool {
        if matches!(scheme, ZhuyinScheme::StandardDvorak) {
            return false;
        }
        self.core
            .live
            .zhuyin_scheme
            .store(scheme as i32, std::sync::atomic::Ordering::Relaxed);
        true
    }

    /// The live full-pinyin scheme backing `key_pinyin_string`.
    #[must_use]
    pub fn full_scheme(&self) -> FullPinyinScheme {
        let value = self
            .core
            .live
            .full_scheme
            .load(std::sync::atomic::Ordering::Relaxed);
        full_scheme_from_value(value as u8).unwrap_or(FullPinyinScheme::Hanyu)
    }

    /// Selects the full-pinyin scheme — the `zhuyin_set_full_pinyin_scheme`
    /// law. Total: the three enum variants are exactly the accepted set.
    pub fn set_full_scheme(&mut self, scheme: FullPinyinScheme) {
        self.core
            .live
            .full_scheme
            .store(scheme as i32, std::sync::atomic::Ordering::Relaxed);
    }

    /// Probes one chewing keystroke string — the `zhuyin_parse_chewing`
    /// law. `None` is upstream's `false`.
    #[must_use]
    pub fn parse_one_chewing(&self, text: &str) -> Option<ChewingKey> {
        self.core.parse_one_chewing(text)
    }

    /// Probes one full-pinyin spelling — the `zhuyin_parse_full_pinyin`
    /// law, with the `PINYIN_CORRECT_ALL` mask the C facade applies
    /// (`zhuyin.cpp:1013`). `None` is upstream's `false`.
    #[must_use]
    pub fn parse_one_full_pinyin(&self, text: &str) -> Option<ChewingKey> {
        self.core.parse_one_full_pinyin(text, true)
    }

    /// Batch-parses chewing keystrokes — the `zhuyin_parse_more_chewings`
    /// law, forwarded `ToneForwarding::ZhuyinFacade` exactly as
    /// `oxpinyin-zhuyin-capi` passes it. Returns the original-input bytes
    /// consumed, 0 on failure or empty input.
    ///
    /// Clears the snapshot first: the core clears its own parse state,
    /// and this layer clears what the core cannot reach.
    #[must_use]
    pub fn parse_chewing(&mut self, text: &str) -> usize {
        self.candidates.clear();
        self.core
            .parse_chewing_more(text, ToneForwarding::ZhuyinFacade)
    }

    /// Batch-parses full pinyin — the `zhuyin_parse_more_full_pinyins`
    /// law. Returns the original-input bytes consumed, 0 on failure or
    /// empty input.
    ///
    /// Clears the snapshot first, as above.
    #[must_use]
    pub fn parse_full_pinyin(&mut self, text: &str) -> usize {
        self.candidates.clear();
        self.core.parse_full_more(text)
    }

    /// Discards composition and parse state — the `zhuyin_reset` law:
    /// the shared full reset plus this layer's snapshot.
    pub fn reset(&mut self) {
        self.core.full_reset();
        self.candidates.clear();
    }

    /// Bytes of original input consumed by the most recent parse call — the
    /// `zhuyin_get_parsed_input_length` law.
    #[must_use]
    pub const fn parsed_len(&self) -> usize {
        self.core.parsed_len
    }

    /// The zhuyin symbol(s) one keystroke maps to — the
    /// `zhuyin_in_chewing_keyboard` mapping half. Empty means the key is
    /// not on the keyboard (upstream's `false`).
    #[must_use]
    pub fn in_keyboard(&self, key: u8) -> Vec<String> {
        self.core.in_keyboard(key)
    }

    /// Rebuilds the candidate snapshot at `offset` — the
    /// `zhuyin_guess_candidates_*` shared shell: remask the session,
    /// refuse a non-composing session, validate the offset in the active
    /// parse mode's own coordinates, then search spans starting at the
    /// offset (after-cursor) or ending at it (before-cursor, the
    /// backward-anchored window builder).
    ///
    /// `offset` is in original input coordinates. Returns upstream's bool:
    /// `true` for a valid lookup into a non-empty matrix even when no span
    /// covers the offset — only an empty matrix answers `false`.
    pub fn guess_candidates(&mut self, offset: usize, before_cursor: bool) -> bool {
        if self.core.session.set_options(self.core.options()).is_err() {
            return false;
        }
        if !self.core.session.is_composing() {
            return false;
        }
        let normalized = match self.core.validate_lookup_offset(offset) {
            Ok(normalized) => normalized,
            Err(_) => {
                self.candidates.clear();
                return false;
            }
        };
        self.candidates.clear();
        let session_offset = match self.core.zhuyin_parse.as_ref() {
            Some(parse) => zhuyin_lookup_session_offset(
                parse,
                self.core.session.raw_input().len(),
                normalized,
                before_cursor,
            ),
            None => normalized,
        };
        let window_owned: CandidateList = if before_cursor {
            let window = match self.core.session.candidates_ending_at(session_offset) {
                Ok(window) => window,
                Err(_) => {
                    self.core.anchored_window = None;
                    self.candidates.clear();
                    return false;
                }
            };
            // The before-cursor window is re-anchored just like the
            // after-cursor one: `snapshot_candidates` records each row's
            // index into THIS list, so a later `choose` must resolve it
            // here and not against the composition-anchored cached list —
            // the two differ in general, and the caller would commit a row
            // it never displayed. The anchor is the buffer start rather
            // than the lookup offset because the ending-at window is
            // END-anchored; see `oxpinyin_facade::BEFORE_CURSOR_ANCHOR`.
            self.core.anchored_window = Some((BEFORE_CURSOR_ANCHOR, window.clone()));
            window
        } else {
            self.core.anchored_window = if session_offset <= self.core.session.composition_offset()
            {
                None
            } else {
                match self.core.session.candidates_at(session_offset) {
                    Ok(window) => Some((session_offset, window)),
                    Err(_) => {
                        self.candidates.clear();
                        return false;
                    }
                }
            };
            match self.core.anchored_window.as_ref() {
                Some((_, window)) => window.clone(),
                None => self.core.session.candidates().clone(),
            }
        };
        let before_end = if before_cursor {
            Some(normalized)
        } else {
            None
        };
        snapshot_candidates(self, &window_owned, before_cursor, before_end);
        if self.candidates.is_empty() && self.core.parsed_len == 0 {
            return false;
        }
        true
    }

    /// The last snapshot built by [`ZhuyinSession::guess_candidates`], best
    /// first — the `zhuyin_get_n_candidate`/`zhuyin_get_candidate` surface
    /// without the handles.
    #[must_use]
    pub fn candidates(&self) -> &[ZhuyinCandidate] {
        &self.candidates
    }

    /// Chooses snapshot row `index` — the `zhuyin_choose_candidate` law:
    /// resolve through the snapshotted `source_index` against the same
    /// window the guess built (the re-anchored one when the guess
    /// re-anchored), then answer the new cursor in original coordinates —
    /// the parse end for a `BEST_MATCH` row, the chosen span's end mapped
    /// back otherwise. There is deliberately no full-parse branch here,
    /// matching the C facade's port of the pin's snapshot-span law
    /// (`zhuyin.cpp:1634-1666`), not the pinyin chain.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::CandidateIndexOutOfRange`] for a stale index
    /// and forwards the session's selection failures.
    pub fn choose(&mut self, index: usize) -> Result<usize, EngineError> {
        let (source_index, candidate_type) = match self.candidates.get(index) {
            Some(candidate) => (candidate.source_index, candidate.candidate_type),
            None => {
                return Err(EngineError::CandidateIndexOutOfRange {
                    index,
                    len: self.candidates.len(),
                });
            }
        };
        let selection = match self.core.anchored_window.as_ref() {
            Some((anchor, window)) => {
                self.core
                    .session
                    .select_anchored(source_index, window, *anchor)
            }
            None => self.core.session.select(source_index),
        };
        // A snapshot index that no longer resolves in the session is a
        // stale index, whatever the session's own complaint: the binding
        // reports it through the same `CandidateIndexOutOfRange` the
        // pinyin surface uses.
        if selection.is_err() {
            return Err(EngineError::CandidateIndexOutOfRange {
                index,
                len: self.candidates.len(),
            });
        }
        self.core.anchored_window = None;
        let end = if candidate_type == ZhuyinCandidateType::BestMatch {
            self.core.parsed_len
        } else if let Some(parse) = self.core.zhuyin_parse.as_ref() {
            zhuyin_original_offset(parse, self.core.session.composition_offset())
        } else {
            self.core.session.composition_offset()
        };
        Ok(end)
    }

    /// Clears the constraint a prior choose pinned — the
    /// `zhuyin_clear_constraint` law: the caller's original-coordinate
    /// offset maps into session coordinates under the active chewing parse
    /// first. Returns the session's bool (false for a free cell).
    pub fn clear_constraint(&mut self, offset: usize) -> bool {
        let session_offset = match self.core.zhuyin_parse.as_ref() {
            Some(parse) => zhuyin_session_offset(parse, offset),
            None => offset,
        };
        self.core.session.clear_constraint(session_offset)
    }

    /// Runs the n-best sentence decode for the current composition —
    /// `zhuyin_guess_sentence`. Returns whether a lookup ran at all.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] from the decode (where the C bool folds
    /// backend failures into `false`, the binding reports them).
    pub fn guess_sentence(&mut self) -> Result<bool, EngineError> {
        self.core.session.guess_sentence()
    }

    /// Runs the sentence decode seeded with prefix tokens — the
    /// `zhuyin_guess_sentence_with_prefix` law: resolve the prefix string
    /// to phrase tokens tail-first and decode under them.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] from the decode.
    pub fn guess_sentence_with_prefix(&mut self, prefix: &str) -> Result<bool, EngineError> {
        let tokens: Vec<oxpinyin_core::PhraseToken> =
            compute_prefixes(&self.core.dict, self.core.user.as_ref(), prefix)
                .iter()
                .map(|&token| oxpinyin_core::PhraseToken::new(token))
                .collect();
        self.core.session.guess_sentence_with_prefix(&tokens)
    }

    /// The decoded text of n-best row `index`, or `None`.
    #[must_use]
    pub fn sentence_text(&self, index: u8) -> Option<&str> {
        self.core.session.sentence_text(index)
    }

    /// Finishes the composition and returns its text.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend fails while resetting.
    pub fn commit(&mut self) -> Result<String, EngineError> {
        self.core.session.commit()
    }

    /// Trains the recorded history/sentence through `user`.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::UserModel`] when the user model rejects an
    /// observation. The missing-user refusal lives in the binding (which
    /// raises `OxpinyinError` there, like the pinyin surface); an empty
    /// selection trains nothing and answers `Ok`, like the session call.
    pub fn train(&self, user: &mut UserStore) -> Result<(), EngineError> {
        self.core.session.train(user)
    }

    /// The original keystroke string of the active parse, else the session's
    /// raw buffer — what a shell echoes as the typed input.
    #[must_use]
    pub fn input(&self) -> &str {
        if self.core.zhuyin_parse.is_some() {
            &self.core.zhuyin_input
        } else if self.core.full_parse.is_some() {
            &self.core.full_input
        } else {
            self.core.session.raw_input()
        }
    }

    /// Whether a composition is in progress.
    #[must_use]
    pub fn is_composing(&self) -> bool {
        self.core.session.is_composing()
    }

    /// Bytes of session input already consumed by selections, in session
    /// (joined-pinyin) coordinates.
    #[must_use]
    pub const fn composition_offset(&self) -> usize {
        self.core.session.composition_offset()
    }

    /// What a shell should display: selected text plus the raw remainder.
    #[must_use]
    pub fn preedit(&self) -> String {
        self.core.session.preedit().text().to_owned()
    }

    /// Clone of the user-learning handle, when opened with a usable user
    /// directory.
    #[must_use]
    pub fn user(&self) -> Option<UserStore> {
        self.core.user.clone()
    }

    /// Renders the key's zhuyin spelling — the `zhuyin_get_zhuyin_string`
    /// law: refuse the zero key (`get_table_index() == 0`), render
    /// otherwise. `None` is upstream's `false`.
    #[must_use]
    pub fn key_zhuyin_string(&self, key: ChewingKey) -> Option<String> {
        if key.table_index() == 0 {
            return None;
        }
        Some(key.zhuyin_string())
    }

    /// Renders the key's pinyin spelling under the live full-pinyin scheme
    /// — the `zhuyin_get_pinyin_string` law: Luoma and SecondaryZhuyin
    /// dispatch to their own renderers, Hanyu and everything else to the
    /// plain pinyin one. `None` is upstream's `false`.
    #[must_use]
    pub fn key_pinyin_string(&self, key: ChewingKey) -> Option<String> {
        if key.table_index() == 0 {
            return None;
        }
        Some(match self.full_scheme() {
            FullPinyinScheme::Luoma => key.luoma_pinyin_string(),
            FullPinyinScheme::SecondaryZhuyin => key.secondary_zhuyin_string(),
            _ => key.pinyin_string(),
        })
    }
}

/// Fills the snapshot from a candidate window (the `snapshot_candidates`
/// law): sentence rows tag `BEST_MATCH` at the head, fallbacks are
/// skipped, every other row tags the guess direction, and a before-cursor
/// guess keeps only spans ending at the requested original-coordinate
/// offset. Consumed spans map back to original coordinates under the
/// active chewing parse.
fn snapshot_candidates(
    session: &mut ZhuyinSession,
    window: &CandidateList,
    before_cursor: bool,
    before_end: Option<usize>,
) {
    let normal_type = if before_cursor {
        ZhuyinCandidateType::NormalBeforeCursor
    } else {
        ZhuyinCandidateType::NormalAfterCursor
    };
    let zhuyin_parse = session.core.zhuyin_parse.clone();
    for (window_index, candidate) in window.iter().enumerate() {
        if candidate.kind() == CandidateKind::Fallback {
            continue;
        }
        let consumed_bytes = match zhuyin_parse.as_ref() {
            Some(parse) => zhuyin_original_offset(parse, candidate.consumed_bytes()),
            None => candidate.consumed_bytes(),
        };
        // Before-cursor law: only candidates whose span ENDS at the
        // requested original offset. At offset 0 no span ends there, so the
        // before-cursor window is empty — not the whole composition. The
        // sentence rows are exempt: upstream prepends them regardless of the
        // offset.
        if let Some(end) = before_end
            && candidate.kind() != CandidateKind::Sentence
            && consumed_bytes != end
        {
            continue;
        }
        session.candidates.push(ZhuyinCandidate {
            text: candidate.text().to_owned(),
            kind: candidate.kind(),
            candidate_type: match candidate.kind() {
                CandidateKind::Sentence => ZhuyinCandidateType::BestMatch,
                _ => normal_type,
            },
            nbest_index: candidate.nbest_index(),
            consumed_bytes,
            source_index: window_index,
            cost: candidate.cost(),
        });
    }
}
