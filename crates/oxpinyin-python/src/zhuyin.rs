//! The zhuyin facade state machine, minus the C types.
//!
//! `oxpinyin-zhuyin-capi` proves this law against the pinned oracle through
//! its 52 `extern "C"` symbols, but every piece of that state machine is
//! `pub(crate)` there: the only public surface is raw-pointer FFI, which
//! this crate's `unsafe_code = "forbid"` lint rules out. So this module
//! re-homes the same law — scheme/option state, the `begin_parse`
//! continuation rule, the chewing/full-pinyin batch seams, the
//! candidate-snapshot tagging, the choose/clear-constraint coordinate maps —
//! over the same `Runtime`/`RuntimeSession` assembly, with no C marshalling
//! anywhere. Each port cites the `oxpinyin-zhuyin-capi` file it mirrors and,
//! through it, the upstream pin.
//!
//! The binding layer (`crate::zhuyin_binding`) and the parity driver
//! (`crate::dump`) both go through this type, so Python↔native parity is
//! structural: the two sides cannot run different facade laws. What this
//! module does *not* do is track the C facade's future changes — a later
//! `oxpinyin-zhuyin-capi` fix must be ported here too, and the parity corpus
//! only guards this copy against the binding, not against the C ABI. If the
//! two copies drift apart often enough to hurt, the fix is extracting a
//! shared facade crate, not growing this one.

use oxpinyin_core::{
    ChewingKey, FORCE_TONE, FullPinyinIndexParse, FullPinyinParser, FullPinyinScheme, OptionBits,
    PINYIN_CORRECT_ALL, PhraseToken, USE_TONE, ZHUYIN_CORRECT_ALL, ZhuyinKey, ZhuyinParse,
    ZhuyinParser, ZhuyinScheme, graph::ExactSegment, parse_full_pinyin_index,
};
use oxpinyin_engine::{
    CandidateKind, CandidateList, EngineError, check_lookup_offset_range, normalize_lookup_offset,
};
use oxpinyin_runtime::{Runtime, RuntimeDict, RuntimeSession};
use oxpinyin_user::UserStore;

/// `USE_TONE | FORCE_TONE` — the option word `zhuyin_init` seeds
/// (`zhuyin.cpp:272` at the pin 0c5e80e1), mirrored from
/// `oxpinyin-zhuyin-capi::state::ZHUYIN_DEFAULT_OPTIONS`. This is the zhuyin
/// facade's distinguishing default: `pinyin_init` seeds only
/// `PINYIN_INCOMPLETE`.
pub const ZHUYIN_DEFAULT_OPTIONS: u32 = USE_TONE | FORCE_TONE;

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
/// 4-value enum (`zhuyin.h:41-45` at the pin), mirrored from
/// `oxpinyin-zhuyin-capi::types::lookup_candidate_type_t`.
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
    /// session coordinates through the active parse's key spans, exactly as
    /// `snapshot_candidates` does.
    consumed_bytes: usize,
    /// The index this candidate held in the window it was snapshotted from;
    /// `choose` resolves through it, the way `zhuyin_choose_candidate`
    /// resolves through `source_index`.
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

/// State behind one Python `zhuyin.Engine`: the shared runtime session plus
/// the facade law `oxpinyin-zhuyin-capi` keeps in `CapiContext`/`CapiInstance`
/// (scheme/options state, the active parse, the candidate snapshot).
///
/// All coordinates are documented per method: session coordinates are byte
/// offsets in the `'`-joined full-pinyin buffer the decoder sees; original
/// coordinates are byte offsets in the keystroke string the caller parsed.
pub struct ZhuyinFacade {
    session: RuntimeSession,
    user: Option<UserStore>,
    dict: RuntimeDict,
    zhuyin_scheme: ZhuyinScheme,
    full_scheme: FullPinyinScheme,
    options: OptionBits,
    zhuyin_parse: Option<ZhuyinParse>,
    zhuyin_input: String,
    full_parse: Option<FullPinyinIndexParse>,
    full_input: String,
    parsed_len: usize,
    candidates: Vec<ZhuyinCandidate>,
    anchored_window: Option<(usize, CandidateList)>,
}

impl ZhuyinFacade {
    /// Wraps an opened runtime and a fresh session — the
    /// `zhuyin_init`+`zhuyin_alloc_instance` shape without the C handles.
    ///
    /// Seeds the `USE_TONE | FORCE_TONE` option word, the Standard chewing
    /// scheme and the Hanyu full-pinyin scheme, and enables the libzhuyin
    /// sentence-row display law (every `BEST_MATCH` row reads the 1-best, so
    /// the observable list carries exactly one sentence row — see
    /// `Session::set_collapse_sentence_rows_to_best`).
    #[must_use]
    pub fn wrap(runtime: &Runtime, mut session: RuntimeSession) -> Self {
        session.set_collapse_sentence_rows_to_best(true);
        Self {
            session,
            user: runtime.user_store(),
            dict: runtime.dict(),
            zhuyin_scheme: ZhuyinScheme::Standard,
            full_scheme: FullPinyinScheme::Hanyu,
            options: OptionBits::from_bits(ZHUYIN_DEFAULT_OPTIONS),
            zhuyin_parse: None,
            zhuyin_input: String::new(),
            full_parse: None,
            full_input: String::new(),
            parsed_len: 0,
            candidates: Vec::new(),
            anchored_window: None,
        }
    }

    /// The live chewing keyboard scheme.
    #[must_use]
    pub const fn chewing_scheme(&self) -> ZhuyinScheme {
        self.zhuyin_scheme
    }

    /// Selects a chewing keyboard — the `zhuyin_set_chewing_scheme` law
    /// (`config.rs`): every implemented keyboard switches, the
    /// `STANDARD_DVORAK` upstream-abort slot reports `false` instead of
    /// aborting (no-abort policy, divergence class (c)).
    #[must_use]
    pub fn set_chewing_scheme(&mut self, scheme: ZhuyinScheme) -> bool {
        if matches!(scheme, ZhuyinScheme::StandardDvorak) {
            return false;
        }
        self.zhuyin_scheme = scheme;
        true
    }

    /// The live full-pinyin scheme backing `key_pinyin_string`.
    #[must_use]
    pub const fn full_scheme(&self) -> FullPinyinScheme {
        self.full_scheme
    }

    /// Selects the full-pinyin scheme — the `zhuyin_set_full_pinyin_scheme`
    /// law. Total: the three enum variants are exactly the accepted set.
    pub fn set_full_scheme(&mut self, scheme: FullPinyinScheme) {
        self.full_scheme = scheme;
    }

    /// Probes one chewing keystroke string — the `zhuyin_parse_chewing` law
    /// (`keys.rs`): the live scheme parses after the API's
    /// `ZHUYIN_CORRECT_ALL` strip. `None` is upstream's `false`.
    #[must_use]
    pub fn parse_one_chewing(&self, text: &str) -> Option<ChewingKey> {
        let options = self.options.bits() & !ZHUYIN_CORRECT_ALL;
        ZhuyinParser::with_scheme(self.zhuyin_scheme).parse_one_key(options, text.as_bytes())
    }

    /// Probes one full-pinyin spelling — the `zhuyin_parse_full_pinyin` law
    /// (`keys.rs`): `FullPinyinParser2::parse_one_key` over the live option
    /// word with `PINYIN_CORRECT_ALL` masked first. `None` is upstream's
    /// `false` (which additionally leaves the zero key).
    #[must_use]
    pub fn parse_one_full_pinyin(&self, text: &str) -> Option<ChewingKey> {
        let options = self.options.bits() & !PINYIN_CORRECT_ALL;
        FullPinyinParser.parse_one_key(options, text.as_bytes())
    }

    /// Batch-parses chewing keystrokes — the `zhuyin_parse_more_chewings`
    /// law (`parse.rs`): continue or restart the parse, run the live scheme
    /// through `ZhuyinParser::parse_with_options` under the full option word
    /// (so the default `FORCE_TONE` is honoured), and drive the decoder with
    /// the `'`-joined full-pinyin spelling as exact segments.
    ///
    /// Returns the original-input bytes consumed, 0 on failure or empty
    /// input.
    #[must_use]
    pub fn parse_chewing(&mut self, text: &str) -> usize {
        self.begin_parse(text.as_bytes());
        let options = self.options.bits();
        let parsed = ZhuyinParser::with_scheme(self.zhuyin_scheme)
            .parse_with_options(text.as_bytes(), options);
        if text.is_empty() {
            self.parsed_len = 0;
            return 0;
        }
        let keys: Vec<&ZhuyinKey> = parsed.keys().iter().collect();
        let (full, segments) = exact_input(&keys);
        if !full.is_empty() && self.session.replace_raw_exact(&full, &segments).is_err() {
            return 0;
        }
        self.parsed_len = parsed.consumed();
        self.zhuyin_input = text.to_owned();
        self.zhuyin_parse = Some(parsed);
        self.parsed_len
    }

    /// Batch-parses full pinyin — the `zhuyin_parse_more_full_pinyins` law
    /// (`parse.rs`): remask the session under the live option word, then for
    /// LUOMA / SECONDARY_ZHUYIN parse through the scheme's pinned index, else
    /// (Hanyu) replace the raw buffer directly.
    ///
    /// Returns the original-input bytes consumed, 0 on failure or empty
    /// input.
    #[must_use]
    pub fn parse_full_pinyin(&mut self, text: &str) -> usize {
        self.begin_parse(text.as_bytes());
        if self.session.set_options(self.options).is_err() {
            return 0;
        }
        if text.is_empty() {
            return 0;
        }
        if let Some(index) = self.full_scheme.index() {
            let use_tone = self.options.contains(USE_TONE);
            let parsed = parse_full_pinyin_index(text.as_bytes(), use_tone, index);
            let full = parsed.full_pinyin();
            if !full.is_empty() && self.session.replace_raw(&full).is_err() {
                return 0;
            }
            self.parsed_len = parsed.consumed();
            self.full_input = text.to_owned();
            self.full_parse = Some(parsed);
            return self.parsed_len;
        }
        let consumed = match self.session.replace_raw(text) {
            Ok(()) => self.session.full_parsed_len(),
            Err(_) => 0,
        };
        self.parsed_len = consumed;
        consumed
    }

    /// Discards composition and parse state — the `zhuyin_reset` law
    /// (`instance.rs`): the parse-path reset plus the full session reset.
    pub fn reset(&mut self) {
        self.reset_parse_state();
        self.session.reset();
    }

    /// Bytes of original input consumed by the most recent parse call — the
    /// `zhuyin_get_parsed_input_length` law.
    #[must_use]
    pub const fn parsed_len(&self) -> usize {
        self.parsed_len
    }

    /// The zhuyin symbol(s) one keystroke maps to — the
    /// `zhuyin_in_chewing_keyboard` mapping half (`parse.rs`): the live
    /// scheme's symbols under the live `USE_TONE` flag. Empty means the key
    /// is not on the keyboard (upstream's `false`).
    #[must_use]
    pub fn in_keyboard(&self, key: u8) -> Vec<String> {
        let use_tone = self.options.contains(USE_TONE);
        ZhuyinParser::with_scheme(self.zhuyin_scheme).symbols_for(key, use_tone)
    }

    /// Rebuilds the candidate snapshot at `offset` — the shared
    /// candidate-build shell (`sentence.rs::guess_candidates`): remask the
    /// session, refuse a non-composing session, validate the offset in the
    /// active parse mode's own coordinates, then search spans starting at
    /// the offset (after-cursor) or ending at it (before-cursor, the
    /// backward-anchored window builder).
    ///
    /// `offset` is in original input coordinates. Returns upstream's bool:
    /// `true` for a valid lookup into a non-empty matrix even when no span
    /// covers the offset — only an empty matrix answers `false`.
    #[must_use]
    pub fn guess_candidates(&mut self, offset: usize, before_cursor: bool) -> bool {
        if self.session.set_options(self.options).is_err() {
            return false;
        }
        if !self.session.is_composing() {
            return false;
        }
        let normalized = match self.validate_lookup_offset(offset) {
            Ok(normalized) => normalized,
            Err(_) => {
                self.candidates.clear();
                return false;
            }
        };
        self.candidates.clear();
        let session_offset = match self.zhuyin_parse.as_ref() {
            Some(parse) => zhuyin_lookup_session_offset(
                parse,
                self.session.raw_input().len(),
                normalized,
                before_cursor,
            ),
            None => normalized,
        };
        let window_owned: CandidateList = if before_cursor {
            self.anchored_window = None;
            match self.session.candidates_ending_at(session_offset) {
                Ok(window) => window,
                Err(_) => {
                    self.candidates.clear();
                    return false;
                }
            }
        } else {
            self.anchored_window = if session_offset <= self.session.composition_offset() {
                None
            } else {
                match self.session.candidates_at(session_offset) {
                    Ok(window) => Some((session_offset, window)),
                    Err(_) => {
                        self.candidates.clear();
                        return false;
                    }
                }
            };
            match self.anchored_window.as_ref() {
                Some((_, window)) => window.clone(),
                None => self.session.candidates().clone(),
            }
        };
        let before_end = if before_cursor {
            Some(normalized)
        } else {
            None
        };
        snapshot_candidates(self, &window_owned, before_cursor, before_end);
        if self.candidates.is_empty() && self.parsed_len == 0 {
            return false;
        }
        true
    }

    /// The last snapshot built by [`ZhuyinFacade::guess_candidates`], best
    /// first — the `zhuyin_get_n_candidate`/`zhuyin_get_candidate` surface
    /// without the handles.
    #[must_use]
    pub fn candidates(&self) -> &[ZhuyinCandidate] {
        &self.candidates
    }

    /// Chooses snapshot row `index` — the `zhuyin_choose_candidate` law
    /// (`candidates.rs`): resolve through the snapshotted `source_index`
    /// against the same window the guess built (the re-anchored one when the
    /// guess re-anchored), then answer the new cursor in original
    /// coordinates — the parse end for a `BEST_MATCH` row, the chosen span's
    /// end mapped back otherwise.
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
        let selection = match self.anchored_window.as_ref() {
            Some((anchor, window)) => self.session.select_anchored(source_index, window, *anchor),
            None => self.session.select(source_index),
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
        self.anchored_window = None;
        let end = if candidate_type == ZhuyinCandidateType::BestMatch {
            self.parsed_len
        } else if let Some(parse) = self.zhuyin_parse.as_ref() {
            zhuyin_original_offset(parse, self.session.composition_offset())
        } else {
            self.session.composition_offset()
        };
        Ok(end)
    }

    /// Clears the constraint a prior choose pinned — the
    /// `zhuyin_clear_constraint` law: the caller's original-coordinate
    /// offset maps into session coordinates under the active chewing parse
    /// first. Returns the session's bool (false for a free cell).
    pub fn clear_constraint(&mut self, offset: usize) -> bool {
        let session_offset = match self.zhuyin_parse.as_ref() {
            Some(parse) => zhuyin_session_offset(parse, offset),
            None => offset,
        };
        self.session.clear_constraint(session_offset)
    }

    /// Runs the n-best sentence decode for the current composition —
    /// `zhuyin_guess_sentence`. Returns whether a lookup ran at all.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] from the decode (where the C bool folds
    /// backend failures into `false`, the binding reports them).
    pub fn guess_sentence(&mut self) -> Result<bool, EngineError> {
        self.session.guess_sentence()
    }

    /// Runs the sentence decode seeded with prefix tokens — the
    /// `zhuyin_guess_sentence_with_prefix` law (`sentence.rs`): resolve the
    /// prefix string to phrase tokens tail-first (the
    /// `oxpinyin-capi::predict::compute_prefixes` port below) and decode
    /// under them.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] from the decode.
    pub fn guess_sentence_with_prefix(&mut self, prefix: &str) -> Result<bool, EngineError> {
        let tokens: Vec<PhraseToken> = compute_prefixes(&self.dict, self.user.as_ref(), prefix)
            .iter()
            .map(|&token| PhraseToken::new(token))
            .collect();
        self.session.guess_sentence_with_prefix(&tokens)
    }

    /// The decoded text of n-best row `index`, or `None`.
    #[must_use]
    pub fn sentence_text(&self, index: u8) -> Option<&str> {
        self.session.sentence_text(index)
    }

    /// Finishes the composition and returns its text.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend fails while resetting.
    pub fn commit(&mut self) -> Result<String, EngineError> {
        self.session.commit()
    }

    /// Trains the recorded history/sentence through `user` — the
    /// `zhuyin_train` selection half (`candidates.rs`): a no-op `Ok` without
    /// a recorded selection, exactly like the session call the C symbol
    /// ends in.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::UserModel`] when the user model rejects an
    /// observation.
    pub fn train(&self, user: &mut UserStore) -> Result<(), EngineError> {
        self.session.train(user)
    }

    /// The original keystroke string of the active parse, else the session's
    /// raw buffer — what a shell echoes as the typed input.
    #[must_use]
    pub fn input(&self) -> &str {
        if self.zhuyin_parse.is_some() {
            &self.zhuyin_input
        } else if self.full_parse.is_some() {
            &self.full_input
        } else {
            self.session.raw_input()
        }
    }

    /// Whether a composition is in progress.
    #[must_use]
    pub fn is_composing(&self) -> bool {
        self.session.is_composing()
    }

    /// Bytes of session input already consumed by selections, in session
    /// coordinates.
    #[must_use]
    pub const fn composition_offset(&self) -> usize {
        self.session.composition_offset()
    }

    /// What a shell should display: selected text plus the raw remainder.
    #[must_use]
    pub fn preedit(&self) -> String {
        self.session.preedit().text().to_owned()
    }

    /// Clone of the user-learning handle, when opened with a usable user
    /// directory.
    #[must_use]
    pub fn user(&self) -> Option<UserStore> {
        self.user.clone()
    }

    /// Renders the key's zhuyin spelling — the `zhuyin_get_zhuyin_string`
    /// law (`keys.rs`): refuse the zero key (`get_table_index() == 0`),
    /// render otherwise. `None` is upstream's `false`.
    #[must_use]
    pub fn key_zhuyin_string(&self, key: ChewingKey) -> Option<String> {
        if key.table_index() == 0 {
            return None;
        }
        Some(key.zhuyin_string())
    }

    /// Renders the key's pinyin spelling under the live full-pinyin scheme —
    /// the `zhuyin_get_pinyin_string` law (`keys.rs:1743-1766` at the pin):
    /// Luoma and SecondaryZhuyin dispatch to their own renderers, Hanyu and
    /// everything else to the plain pinyin one. `None` is upstream's
    /// `false`.
    #[must_use]
    pub fn key_pinyin_string(&self, key: ChewingKey) -> Option<String> {
        if key.table_index() == 0 {
            return None;
        }
        Some(match self.full_scheme {
            FullPinyinScheme::Luoma => key.luoma_pinyin_string(),
            FullPinyinScheme::SecondaryZhuyin => key.secondary_zhuyin_string(),
            _ => key.pinyin_string(),
        })
    }

    /// The parse-path reset (`state.rs`): the composition's parse state goes,
    /// the selection record and the constraint store stay.
    fn reset_parse_state(&mut self) {
        self.session.reset_composition();
        self.candidates.clear();
        // Drop a stale re-anchored window so a later choose cannot re-read
        // it against a different composition.
        self.anchored_window = None;
        self.parsed_len = 0;
        self.zhuyin_parse = None;
        self.zhuyin_input.clear();
        self.full_parse = None;
        self.full_input.clear();
    }

    /// Continue a parse when the buffer evolved from the stored one; a
    /// divergent buffer starts fresh (`state.rs::begin_parse`).
    fn begin_parse(&mut self, original: &[u8]) {
        let stored: &[u8] = if self.zhuyin_parse.is_some() {
            self.zhuyin_input.as_bytes()
        } else if self.full_parse.is_some() {
            self.full_input.as_bytes()
        } else {
            self.session.raw_input().as_bytes()
        };
        let continues = self.session.parse_continues(stored, original);
        let committed_continues =
            !continues && self.session.committed_parse_continues(stored, original);
        self.reset_parse_state();
        if !continues && !committed_continues {
            self.session.reset();
        }
    }

    /// The generalized lookup-offset law in the active parse mode's own
    /// coordinates (`state.rs::validate_lookup_offset`). Zhuyin keyboards
    /// hold no zero-key columns, so only the range refusal against the
    /// consumed length applies.
    fn validate_lookup_offset(&self, offset: usize) -> Result<usize, EngineError> {
        if let Some(parse) = self.zhuyin_parse.as_ref() {
            check_lookup_offset_range(parse.consumed(), offset)
        } else if let Some(parse) = self.full_parse.as_ref() {
            let consumed = parse.consumed().min(self.full_input.len());
            normalize_lookup_offset(&self.full_input.as_bytes()[..consumed], offset)
        } else {
            self.session.normalized_lookup_offset(offset)
        }
    }
}

/// Builds the exact-decoder input for a scheme parse (`parse.rs`): the
/// `'`-joined full-pinyin text plus one [`ExactSegment`] per key over that
/// text.
fn exact_input(keys: &[&ZhuyinKey]) -> (String, Vec<ExactSegment>) {
    let mut text = String::new();
    let mut segments = Vec::with_capacity(keys.len());
    for key in keys {
        if !text.is_empty() {
            text.push('\'');
        }
        let start = text.len();
        text.push_str(key.key().text());
        segments.push(ExactSegment::new(start, text.len(), key.key(), key.tone()));
    }
    (text, segments)
}

/// Fills the snapshot from a candidate window (`candidates.rs`:
/// `snapshot_candidates`): sentence rows tag `BEST_MATCH` at the head,
/// fallbacks are skipped, every other row tags the guess direction, and a
/// before-cursor guess keeps only spans ending at the requested
/// original-coordinate offset. Consumed spans map back to original
/// coordinates under the active chewing parse.
fn snapshot_candidates(
    facade: &mut ZhuyinFacade,
    window: &CandidateList,
    before_cursor: bool,
    before_end: Option<usize>,
) {
    let normal_type = if before_cursor {
        ZhuyinCandidateType::NormalBeforeCursor
    } else {
        ZhuyinCandidateType::NormalAfterCursor
    };
    let zhuyin_parse = facade.zhuyin_parse.clone();
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
        facade.candidates.push(ZhuyinCandidate {
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

/// Maps a byte offset in the transformed `'`-joined full-pinyin string back
/// to the original zhuyin input offset (`sentence.rs`).
fn zhuyin_original_offset(parse: &ZhuyinParse, offset: usize) -> usize {
    let mut transformed = 0;
    for item in parse.keys() {
        let key_len = item.key().text().len();
        let boundary = transformed + key_len;
        if offset <= boundary {
            return item.end();
        }
        transformed = boundary + 1; // apostrophe between keys
    }
    parse.consumed()
}

/// Maps an original-input offset to the transformed session offset — the
/// inverse of [`zhuyin_original_offset`] (`sentence.rs`).
fn zhuyin_session_offset(parse: &ZhuyinParse, offset: usize) -> usize {
    let mut transformed = 0;
    for item in parse.keys() {
        if offset < item.end() {
            return transformed;
        }
        transformed += item.key().text().len() + 1; // key + apostrophe
    }
    transformed
}

/// Maps an original zhuyin-input lookup offset to the session raw-buffer
/// offset for the candidate-guess family (`sentence.rs`:
/// `zhuyin_lookup_session_offset`). The terminal offset maps to the session
/// buffer's one-past-end (upstream's matrix reserved slot); a key boundary
/// between two syllables is two session positions at once, so the mapping is
/// direction-dependent — after-cursor takes the right-key start, the
/// before-cursor family the left-key end.
fn zhuyin_lookup_session_offset(
    parse: &ZhuyinParse,
    session_len: usize,
    offset: usize,
    before_cursor: bool,
) -> usize {
    if offset >= parse.consumed() {
        return session_len;
    }
    if before_cursor {
        let mut transformed = 0;
        for item in parse.keys() {
            let key_len = item.key().text().len();
            if offset == item.end() {
                return transformed + key_len;
            }
            transformed += key_len + 1; // apostrophe between keys
        }
        return session_len;
    }
    zhuyin_session_offset(parse, offset)
}

/// Resolves a prefix string to the phrase tokens its tail substrings name —
/// the `oxpinyin-capi::predict::compute_prefixes` port
/// (`oxpinyin-zhuyin-capi::predict` carries the same port for the C
/// symbol): system tokens ride the loaded-library mask; user tokens come
/// from the user store's own phrase inventory.
fn compute_prefixes(dict: &RuntimeDict, user: Option<&UserStore>, prefix: &str) -> Vec<u32> {
    let chars: Vec<char> = prefix.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let user_lookup = user.and_then(|store| oxpinyin_user::UserLookup::from_store(store).ok());
    let max = chars.len().min(oxpinyin_user::MAX_PHRASE_LENGTH);
    let mut tokens = Vec::new();
    for length in 1..=max {
        let suffix: String = chars[chars.len() - length..].iter().collect();
        tokens.extend(
            dict.system()
                .tokens_for_text(&suffix)
                .unwrap_or_default()
                .into_iter()
                .filter(|token| dict.library_visible_token(*token)),
        );
        if let Some(lookup) = user_lookup.as_ref() {
            tokens.extend(lookup.tokens_for_text(&suffix).iter().copied());
        }
    }
    tokens
}
