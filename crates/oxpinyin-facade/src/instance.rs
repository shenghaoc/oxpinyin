//! One instance's orchestration state: the session, the shared handles,
//! the parse-mode state machine, and the snapshot-adjacent fields both
//! facades' C layers borrow against.

use std::sync::atomic::Ordering;

use oxpinyin_core::{
    DoublePinyinParse, FullPinyinIndexParse, OptionBits, PhraseToken, ZhuyinParse,
};
use oxpinyin_engine::{
    CandidateList, EngineError, check_lookup_offset_range, normalize_lookup_offset,
};
use oxpinyin_runtime::{RuntimeDict, RuntimeLm, RuntimeSession};
use oxpinyin_user::UserStore;

use crate::context::LiveOptions;

/// The [`InstanceCore::anchored_window`] anchor for a **before-cursor**
/// window — the buffer start, not the lookup offset.
///
/// The two window builders measure a candidate's `consumed_bytes` in
/// different coordinates, and [`oxpinyin_engine::Session::select_anchored`]
/// reads the chosen span as `[anchor, anchor + consumed_bytes)`:
///
/// - **After-cursor** (`Session::candidates_at(offset)`) rebases its graph
///   onto `raw[offset..]`, so `consumed_bytes` is a LENGTH measured from
///   the lookup offset. Its anchor is that offset.
/// - **Before-cursor** (`Session::candidates_ending_at(offset)`) runs on
///   the prefix graph `raw[..offset]`, whose coordinates are absolute from
///   the buffer start, so every row's `consumed_bytes` is the span's
///   ABSOLUTE END — the lookup offset itself, shared by every row in the
///   window, whatever byte each span starts on. The anchor that reproduces
///   that end (and so the consumed advance) is therefore the coordinate
///   those ends are measured from: 0.
///
/// Reusing the after-cursor anchor here would read the span as
/// `[offset, 2 * offset)` and walk the composition off the end of the
/// buffer; leaving the window unanchored resolves the row's index against
/// the composition-anchored cached list instead, which is a different list
/// — the caller would commit a row it never displayed.
pub const BEFORE_CURSOR_ANCHOR: usize = 0;

/// State behind a facade's instance handle, minus the C parts: everything
/// the two C-ABI crates' `CapiInstance`s held in identical shape. The C
/// layers hold one of these plus their ABI-only fields (the context
/// back-pointer, the `#[repr(C)]` key slots, the CString candidate
/// snapshot).
pub struct InstanceCore {
    /// The shared runtime's concrete session — the same assembly every
    /// facade and the Python binding drive.
    pub session: RuntimeSession,
    /// Clone of the context's user store. `None` under no user dir.
    pub user: Option<UserStore>,
    /// Shared dictionary handle (system + user-file + addon set) for
    /// prediction.
    pub dict: RuntimeDict,
    /// Clone of the context's language model, for the predicted-candidate
    /// frequency key.
    pub lm: RuntimeLm,
    /// The context's live option/scheme state, by handle.
    pub live: LiveOptions,
    /// The phrase-segment span DP's output, written by `phrase_segment`
    /// and read by the `get_n_phrase` / `get_phrase_token` pair. Cleared
    /// by the full reset and by nothing else.
    pub phrase_result: Vec<PhraseToken>,
    /// Bytes of raw input consumed by the most recent parse call —
    /// upstream `m_parsed_len`, in the active parse mode's own
    /// coordinates. Allocation and reset both store 0.
    pub parsed_len: usize,
    /// The re-anchored candidate window from the most recent guess when
    /// it ran at an offset other than the composition's own, as
    /// `(anchor, window)` — retained so a later choose resolves its index
    /// against the SAME window the caller saw. `None` when the last guess
    /// ran at the composition offset.
    pub anchored_window: Option<(usize, CandidateList)>,
    /// Most recent double-pinyin parse, when the last parse call was the
    /// double-pinyin entry point. Used for aux text and
    /// candidate-offset mapping back to the original input bytes.
    pub double_parse: Option<DoublePinyinParse>,
    /// Original double-pinyin input for sentence/preedit fallback
    /// display.
    pub double_input: String,
    /// Most recent Zhuyin parse, when the last parse call was the chewing
    /// entry point.
    pub zhuyin_parse: Option<ZhuyinParse>,
    /// Original Zhuyin input for sentence/preedit fallback display.
    pub zhuyin_input: String,
    /// Most recent full-pinyin index parse (LUOMA / SECONDARY_ZHUYIN),
    /// when the last parse call was the full-pinyin entry point under
    /// such a scheme. Used for aux-text rendering over the raw input.
    pub full_parse: Option<FullPinyinIndexParse>,
    /// Original full-pinyin input for aux-text cursor mapping.
    pub full_input: String,
}

impl InstanceCore {
    /// Assembles an instance's state from the context's allocation — the
    /// `alloc_instance` law, minus the C handle wiring.
    #[must_use]
    pub fn new(
        session: RuntimeSession,
        user: Option<UserStore>,
        dict: RuntimeDict,
        lm: RuntimeLm,
        live: LiveOptions,
    ) -> Self {
        Self {
            session,
            user,
            dict,
            lm,
            live,
            phrase_result: Vec::new(),
            parsed_len: 0,
            anchored_window: None,
            double_parse: None,
            double_input: String::new(),
            zhuyin_parse: None,
            zhuyin_input: String::new(),
            full_parse: None,
            full_input: String::new(),
        }
    }

    /// The current live option word.
    #[must_use]
    pub fn options(&self) -> OptionBits {
        OptionBits::from_bits(self.live.options.load(Ordering::Relaxed))
    }

    /// The parse-path reset: the composition's parse state goes, the
    /// selection record and the constraint store stay — upstream's
    /// parse-never-touches-constraints rule; the frontend re-sends the
    /// whole buffer every keystroke, so the chosen cursor must survive
    /// the re-parse.
    ///
    /// The re-anchored window drops here too: the snapshot that indexed
    /// into it is cleared in the same breath, so a stale window was
    /// already unreachable through a later choose — dropping it is the
    /// defensive union of the two facades' bodies (the zhuyin facade
    /// dropped it, the pinyin facade relied on the cleared snapshot) and
    /// is observably identical for both.
    pub fn reset_parse_state(&mut self) {
        self.session.reset_composition();
        self.anchored_window = None;
        self.parsed_len = 0;
        self.double_parse = None;
        self.double_input.clear();
        self.zhuyin_parse = None;
        self.zhuyin_input.clear();
        self.full_parse = None;
        self.full_input.clear();
    }

    /// The full reset — `pinyin_reset`/`zhuyin_reset`'s law: the parse
    /// path, the phrase result, and the whole session (input, selection
    /// record, n-best rows, constraint store) all go.
    pub fn full_reset(&mut self) {
        self.reset_parse_state();
        self.phrase_result.clear();
        self.session.reset();
    }

    /// Begin a parse of `original` (the caller's input, in the active
    /// mode's own coordinates): continue the current composition when the
    /// buffer evolved from the stored one — extension, backspace, or
    /// re-send — whether the composition is open or a selection consumed
    /// it (the store survives every re-parse; only the full reset clears
    /// it). Only a divergent buffer starts fresh: a different string is a
    /// different composition, and a stale selection-derived cursor must
    /// not mis-anchor its window before validate could drop the
    /// mismatched forcings.
    pub fn begin_parse(&mut self, original: &[u8]) {
        let stored: &[u8] = if self.zhuyin_parse.is_some() {
            self.zhuyin_input.as_bytes()
        } else if self.double_parse.is_some() {
            self.double_input.as_bytes()
        } else if self.full_parse.is_some() {
            self.full_input.as_bytes()
        } else {
            self.session.raw_input().as_bytes()
        };
        let continues = self.session.parse_continues(stored, original);
        let committed_continues =
            !continues && self.session.committed_parse_continues(stored, original);
        // The committed-continues shape needs exactly `reset_parse_state`:
        // its `reset_composition` keeps the store and the selection
        // record, so the full reset below must not run there.
        self.reset_parse_state();
        if !continues && !committed_continues {
            self.session.reset();
        }
    }

    /// The generalized lookup-offset law in the active parse mode's own
    /// coordinates — the space the caller's guess/choose offsets live in.
    ///
    /// - Plain full pinyin: the full law over the session's raw buffer,
    ///   whose `'` bytes are the matrix's zero-key columns.
    /// - LUOMA / `SECONDARY_ZHUYIN`: the full law over the consumed
    ///   prefix of the stored original input — the pinned index parse
    ///   consumes `'` as the same separator, and bytes past its consumed
    ///   length never entered the composition.
    /// - Double pinyin and the zhuyin keyboards: no zero-key column can
    ///   exist (`'` is out of scheme or a content symbol), so only the
    ///   range refusal against the parsed original length applies.
    ///
    /// The chain order is the union of the two facades'; a facade that
    /// never populates one of the parse states simply never takes that
    /// branch, which preserves its law exactly.
    pub fn validate_lookup_offset(&self, offset: usize) -> Result<usize, EngineError> {
        if let Some(parse) = self.zhuyin_parse.as_ref() {
            return check_lookup_offset_range(parse.consumed(), offset);
        }
        if let Some(parse) = self.double_parse.as_ref() {
            return check_lookup_offset_range(parse.consumed(), offset);
        }
        if let Some(parse) = self.full_parse.as_ref() {
            // The min is defensive only: `full_input` and the parse are
            // set together and cleared together, so consumed never
            // exceeds the buffer — but a desync must refuse, not
            // slice-panic.
            let consumed = parse.consumed().min(self.full_input.len());
            return normalize_lookup_offset(&self.full_input.as_bytes()[..consumed], offset);
        }
        self.session.normalized_lookup_offset(offset)
    }

    /// `train`'s law: refuse without a user store or without a recorded
    /// selection, otherwise walk the recorded sentence through the store.
    /// The bool is the C surface's contract, verbatim.
    pub fn train(&mut self) -> bool {
        let Some(user) = self.user.as_mut() else {
            return false;
        };
        if self.session.selected_tokens().is_empty() {
            return false;
        }
        self.session.train(user).is_ok()
    }
}
