//! The session state machine.
//!
//! One session per input context. Nothing here is `Send` or `Sync` by
//! requirement, because the TSF, IMK and ArkTS models all want a
//! main-thread-friendly, instance-per-context object.
//!
//! At W4-T0 the session is the input buffer and its presentation. The decoder
//! is wired in at W4-T4 **behind these signatures**, which
//! `docs/findings/session-api.md` freezes.

use core::fmt::Display;

use pinyin_core::{Dictionary, LanguageModel, PhraseEntry, PhraseToken, SyllableKey};

use crate::candidate::{Candidate, CandidateKind, CandidateList};
use crate::config::ConfigSource;
use crate::error::EngineError;
use crate::key::{KeyInput, LogicalKey};
use crate::preedit::{Preedit, PreeditSpan, SpanStyle};
use crate::storage::StoragePaths;

/// Largest raw input a session accepts, in bytes.
///
/// Matches the largest input the frozen F-A fixtures and the parity corpus
/// carry. Typing past it is reported as [`KeyOutcome::Ignored`]: refusing more
/// input is a state, not a failure.
pub const MAX_INPUT_BYTES: usize = 4_096;

/// Configuration key for the candidate page size.
const KEY_PAGE_SIZE: &str = "lookup-table-page-size";

/// Page size used when the configuration source does not carry the key.
const DEFAULT_PAGE_SIZE: usize = 5;

/// What a session did with a key.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum KeyOutcome {
    /// The session did not use the key and is unchanged.
    Ignored,
    /// The session used the key.
    Consumed,
    /// The session used the key and finished a composition.
    Commit(String),
}

/// What choosing a candidate left behind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Selection {
    /// Input remains; more candidates are offered.
    Continued,
    /// The whole composition is chosen and can be committed.
    Completed,
}

/// Settings a session reads once, at construction.
#[derive(Clone, Copy, Debug)]
struct Settings {
    page_size: usize,
}

impl Settings {
    fn read(config: &dyn ConfigSource) -> Self {
        let page_size = config
            .get_int(KEY_PAGE_SIZE)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_PAGE_SIZE);
        Self { page_size }
    }
}

/// One input context.
///
/// Swapping fixture adapters for table-backed loaders is a change of `D` and
/// `L` and nothing else.
///
/// **Keep in sync with `docs/findings/session-api.md`.** That SPEC's
/// "deliberately absent" list — no keysyms, no GSettings, no path discovery,
/// no `cfg(target_os)`, no threading or clock contract — is the freeze this
/// type implements, and later findings add cross-references to it
/// (`config-layering.md` for where configuration actually comes from,
/// `session-replay.md` for what consumes the seam). A change here that admits
/// one of those must amend the SPEC's list in the same commit, or the list
/// silently stops describing the code.
#[derive(Clone, Debug)]
pub struct Session<D, L> {
    dictionary: D,
    model: L,
    paths: StoragePaths,
    settings: Settings,
    raw: String,
    selected: String,
    consumed: usize,
    candidates: CandidateList,
}

impl<D, L> Session<D, L>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// Opens a session over the supplied backends.
    ///
    /// Configuration and storage locations arrive as data; the session reads
    /// no environment and discovers no path.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend rejects the settings it is
    /// opened with. No such rejection exists yet, so this currently always
    /// succeeds.
    pub fn new(
        config: &dyn ConfigSource,
        paths: StoragePaths,
        dictionary: D,
        model: L,
    ) -> Result<Self, EngineError> {
        Ok(Self {
            dictionary,
            model,
            paths,
            settings: Settings::read(config),
            raw: String::new(),
            selected: String::new(),
            consumed: 0,
            candidates: CandidateList::default(),
        })
    }

    /// Feeds one key press to the session.
    ///
    /// Characters the parser has syntax for — ASCII lowercase and the
    /// apostrophe — extend the composition. `Backspace` removes the last one,
    /// or undoes a selection when nothing else remains. `Escape` clears the
    /// composition. `Enter` commits it. `Space` chooses the first candidate,
    /// or commits when there is none. Every other key, and any key held with a
    /// command modifier, is [`KeyOutcome::Ignored`] and changes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when refreshing candidates hits a backend
    /// failure.
    pub fn process_key(&mut self, input: &KeyInput) -> Result<KeyOutcome, EngineError> {
        if input.modifiers().has_command_modifier() {
            return Ok(KeyOutcome::Ignored);
        }

        match input.key() {
            LogicalKey::Character(character) => self.type_character(character),
            LogicalKey::Backspace => self.erase(),
            LogicalKey::Escape => {
                if self.is_composing() {
                    self.reset();
                    Ok(KeyOutcome::Consumed)
                } else {
                    Ok(KeyOutcome::Ignored)
                }
            }
            LogicalKey::Enter => {
                if self.is_composing() {
                    Ok(KeyOutcome::Commit(self.commit()?))
                } else {
                    Ok(KeyOutcome::Ignored)
                }
            }
            LogicalKey::Space => self.accept_first(),
            _ => Ok(KeyOutcome::Ignored),
        }
    }

    /// Chooses the candidate at `index`.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::CandidateIndexOutOfRange`] for an index the
    /// current list does not hold — including a stale index left over from an
    /// earlier list — and leaves the session usable.
    pub fn select(&mut self, index: usize) -> Result<Selection, EngineError> {
        let Some(candidate) = self.candidates.get(index) else {
            return Err(EngineError::CandidateIndexOutOfRange {
                index,
                len: self.candidates.len(),
            });
        };

        let text = candidate.text().to_owned();
        let advance = candidate.consumed_bytes();
        self.selected.push_str(&text);
        self.consumed = self.next_boundary(self.consumed.saturating_add(advance));
        self.refresh()?;

        if self.consumed >= self.raw.len() {
            Ok(Selection::Completed)
        } else {
            Ok(Selection::Continued)
        }
    }

    /// Finishes the composition and returns its text.
    ///
    /// Never fails on an empty composition: the text is then empty too.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend fails while the session resets.
    pub fn commit(&mut self) -> Result<String, EngineError> {
        let mut text = core::mem::take(&mut self.selected);
        text.push_str(&self.raw[self.consumed..]);
        self.reset();
        Ok(text)
    }

    /// Discards the composition.
    pub fn reset(&mut self) {
        self.raw.clear();
        self.selected.clear();
        self.consumed = 0;
        self.candidates = CandidateList::default();
    }

    /// What the shell should display.
    #[must_use]
    pub fn preedit(&self) -> Preedit {
        let remaining = &self.raw[self.consumed..];
        if self.selected.is_empty() && remaining.is_empty() {
            return Preedit::default();
        }

        let mut text = self.selected.clone();
        text.push_str(remaining);

        let mut spans = Vec::with_capacity(2);
        if !self.selected.is_empty() {
            spans.push(PreeditSpan::new(
                0,
                self.selected.len(),
                SpanStyle::Selected,
            ));
        }
        if !remaining.is_empty() {
            spans.push(PreeditSpan::new(
                self.selected.len(),
                text.len(),
                SpanStyle::Raw,
            ));
        }

        let cursor = text.len();
        Preedit::new(text, spans, cursor)
    }

    /// The current candidates, in rank order.
    #[must_use]
    pub const fn candidates(&self) -> &CandidateList {
        &self.candidates
    }

    /// The raw input typed so far.
    #[must_use]
    pub fn raw_input(&self) -> &str {
        &self.raw
    }

    /// Whether a composition is in progress.
    #[must_use]
    pub fn is_composing(&self) -> bool {
        !self.raw.is_empty()
    }

    /// Candidates per page, from the configuration the session was opened
    /// with.
    #[must_use]
    pub const fn page_size(&self) -> usize {
        self.settings.page_size
    }

    /// The storage locations the session was opened with.
    #[must_use]
    pub const fn paths(&self) -> &StoragePaths {
        &self.paths
    }

    /// The dictionary backend.
    #[must_use]
    pub const fn dictionary(&self) -> &D {
        &self.dictionary
    }

    /// The language model backend.
    #[must_use]
    pub const fn language_model(&self) -> &L {
        &self.model
    }

    fn type_character(&mut self, character: char) -> Result<KeyOutcome, EngineError> {
        if !is_input_character(character) {
            return Ok(KeyOutcome::Ignored);
        }
        if self.raw.len() + character.len_utf8() > MAX_INPUT_BYTES {
            return Ok(KeyOutcome::Ignored);
        }

        self.raw.push(character);
        self.refresh()?;
        Ok(KeyOutcome::Consumed)
    }

    fn erase(&mut self) -> Result<KeyOutcome, EngineError> {
        if self.consumed < self.raw.len() {
            self.raw.pop();
            self.refresh()?;
            return Ok(KeyOutcome::Consumed);
        }
        if !self.selected.is_empty() {
            self.selected.clear();
            self.consumed = 0;
            self.refresh()?;
            return Ok(KeyOutcome::Consumed);
        }
        Ok(KeyOutcome::Ignored)
    }

    fn accept_first(&mut self) -> Result<KeyOutcome, EngineError> {
        if self.candidates.is_empty() {
            if self.is_composing() {
                return Ok(KeyOutcome::Commit(self.commit()?));
            }
            return Ok(KeyOutcome::Ignored);
        }

        match self.select(0)? {
            Selection::Completed => Ok(KeyOutcome::Commit(self.commit()?)),
            Selection::Continued => Ok(KeyOutcome::Consumed),
        }
    }

    /// Rounds `offset` up to the next character boundary of the raw input.
    ///
    /// The raw buffer only ever holds ASCII, so this is the identity in
    /// practice; it exists so a future input character class cannot turn a
    /// byte count into a slicing panic.
    fn next_boundary(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.raw.len());
        while !self.raw.is_char_boundary(offset) {
            offset += 1;
        }
        offset
    }

    /// Recomputes the candidate list for the current state.
    ///
    /// W4-T4 puts parse, graph, k-best and dictionary lookup here. Until then
    /// the only candidate a session can honestly offer is the input itself,
    /// which is what [`CandidateKind::Fallback`] means.
    fn refresh(&mut self) -> Result<(), EngineError> {
        let remaining = &self.raw[self.consumed..];
        self.candidates = if remaining.is_empty() {
            CandidateList::default()
        } else {
            CandidateList::from_vec(vec![Candidate::new(
                remaining.to_owned(),
                CandidateKind::Fallback,
                0,
                remaining.len(),
                0,
            )])
        };
        Ok(())
    }
}

/// Whether the parser has syntax for `character`.
///
/// `docs/findings/parser-spec.md`: only lowercase ASCII `a`–`z` and the ASCII
/// apostrophe. Everything else belongs to the shell.
const fn is_input_character(character: char) -> bool {
    character.is_ascii_lowercase() || character == '\''
}

#[cfg(test)]
mod tests {
    use pinyin_core::{Cost, Dictionary, LanguageModel, PhraseEntry, PhraseToken, SyllableKey};

    use super::{KeyOutcome, MAX_INPUT_BYTES, Selection, Session};
    use crate::config::EmptyConfigSource;
    use crate::error::EngineError;
    use crate::key::{KeyInput, LogicalKey, Modifiers};
    use crate::preedit::SpanStyle;
    use crate::storage::StoragePaths;

    /// A backend that answers nothing, so these tests measure the state
    /// machine and not a data set.
    struct Silent;

    impl Dictionary for Silent {
        type Entry = PhraseEntry;
        type Error = EngineError;
        type Syllable = SyllableKey;

        fn lookup(&self, _syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
            Ok(Vec::new())
        }
    }

    impl LanguageModel for Silent {
        type Error = EngineError;
        type Token = PhraseToken;

        fn score(
            &self,
            _history: &[PhraseToken],
            _token: &PhraseToken,
            edge_cost: Cost,
        ) -> Result<Cost, EngineError> {
            Ok(edge_cost)
        }
    }

    fn session() -> Session<Silent, Silent> {
        Session::new(
            &EmptyConfigSource,
            StoragePaths::new("user"),
            Silent,
            Silent,
        )
        .expect("opening a session cannot fail yet")
    }

    fn type_text(session: &mut Session<Silent, Silent>, text: &str) {
        for character in text.chars() {
            session
                .process_key(&KeyInput::character(character))
                .expect("typing cannot fail");
        }
    }

    #[test]
    fn only_parser_syntax_extends_the_composition() {
        let mut session = session();
        type_text(&mut session, "ni'hao");
        assert_eq!(session.raw_input(), "ni'hao");
        assert!(session.is_composing());

        for ignored in ['N', '1', ' ', '!', '\u{4f60}'] {
            assert_eq!(
                session
                    .process_key(&KeyInput::character(ignored))
                    .expect("ignored keys cannot fail"),
                KeyOutcome::Ignored,
                "character: {ignored:?}"
            );
        }
        assert_eq!(session.raw_input(), "ni'hao");
    }

    #[test]
    fn command_modifiers_leave_the_session_alone() {
        let mut session = session();
        type_text(&mut session, "ni");

        for modifier in [Modifiers::CONTROL, Modifiers::ALT, Modifiers::SUPER] {
            let input = KeyInput::new(LogicalKey::Character('h'), modifier, "h");
            assert_eq!(
                session.process_key(&input).expect("no failure"),
                KeyOutcome::Ignored
            );
        }
        assert_eq!(session.raw_input(), "ni");

        let shifted = KeyInput::new(LogicalKey::Character('h'), Modifiers::SHIFT, "H");
        assert_eq!(
            session.process_key(&shifted).expect("no failure"),
            KeyOutcome::Consumed
        );
        assert_eq!(session.raw_input(), "nih");
    }

    #[test]
    fn backspace_erases_then_reports_nothing_to_do() {
        let mut session = session();
        type_text(&mut session, "ni");

        let backspace = KeyInput::plain(LogicalKey::Backspace);
        assert_eq!(
            session.process_key(&backspace).expect("no failure"),
            KeyOutcome::Consumed
        );
        assert_eq!(session.raw_input(), "n");
        session.process_key(&backspace).expect("no failure");
        assert_eq!(session.raw_input(), "");
        assert_eq!(
            session.process_key(&backspace).expect("no failure"),
            KeyOutcome::Ignored
        );
    }

    #[test]
    fn enter_commits_and_escape_discards() {
        let mut session = session();
        type_text(&mut session, "nihao");
        assert_eq!(
            session
                .process_key(&KeyInput::plain(LogicalKey::Enter))
                .expect("no failure"),
            KeyOutcome::Commit("nihao".to_owned())
        );
        assert!(!session.is_composing());
        assert_eq!(
            session
                .process_key(&KeyInput::plain(LogicalKey::Enter))
                .expect("no failure"),
            KeyOutcome::Ignored
        );

        type_text(&mut session, "nihao");
        assert_eq!(
            session
                .process_key(&KeyInput::plain(LogicalKey::Escape))
                .expect("no failure"),
            KeyOutcome::Consumed
        );
        assert_eq!(session.raw_input(), "");
    }

    #[test]
    fn keys_the_session_does_not_use_change_nothing() {
        let mut session = session();
        type_text(&mut session, "ni");

        for key in [
            LogicalKey::Tab,
            LogicalKey::Delete,
            LogicalKey::Left,
            LogicalKey::Right,
            LogicalKey::Up,
            LogicalKey::Down,
            LogicalKey::Home,
            LogicalKey::End,
            LogicalKey::PageUp,
            LogicalKey::PageDown,
            LogicalKey::Unknown,
        ] {
            assert_eq!(
                session
                    .process_key(&KeyInput::plain(key))
                    .expect("no failure"),
                KeyOutcome::Ignored,
                "key: {key:?}"
            );
        }
        assert_eq!(session.raw_input(), "ni");
    }

    #[test]
    fn the_preedit_covers_its_text_exactly() {
        let mut session = session();
        assert!(session.preedit().is_empty());

        type_text(&mut session, "nihao");
        let preedit = session.preedit();
        assert_eq!(preedit.text(), "nihao");
        assert_eq!(preedit.cursor(), 5);
        assert_eq!(preedit.spans().len(), 1);
        assert_eq!(preedit.spans()[0].style(), SpanStyle::Raw);
        assert_eq!(preedit.spans()[0].start(), 0);
        assert_eq!(preedit.spans()[0].end(), preedit.text().len());
    }

    #[test]
    fn a_stale_candidate_index_is_an_error_not_a_panic() {
        let mut session = session();
        type_text(&mut session, "nihao");
        let len = session.candidates().len();
        assert_eq!(len, 1, "only the raw fallback exists before the decoder");

        for index in [len, len + 1, usize::MAX] {
            assert_eq!(
                session.select(index),
                Err(EngineError::CandidateIndexOutOfRange { index, len })
            );
        }
        assert_eq!(session.raw_input(), "nihao");
        assert!(session.candidates().get(usize::MAX).is_none());
    }

    #[test]
    fn choosing_the_fallback_completes_the_composition() {
        let mut session = session();
        type_text(&mut session, "nihao");
        assert_eq!(
            session.select(0).expect("the fallback exists"),
            Selection::Completed
        );

        let preedit = session.preedit();
        assert_eq!(preedit.text(), "nihao");
        assert_eq!(preedit.spans().len(), 1);
        assert_eq!(preedit.spans()[0].style(), SpanStyle::Selected);
        assert!(session.candidates().is_empty());
        assert_eq!(session.commit().expect("no failure"), "nihao");
    }

    #[test]
    fn space_accepts_the_first_candidate_and_commits() {
        let mut session = session();
        type_text(&mut session, "nihao");
        assert_eq!(
            session
                .process_key(&KeyInput::plain(LogicalKey::Space))
                .expect("no failure"),
            KeyOutcome::Commit("nihao".to_owned())
        );
        assert!(!session.is_composing());
        assert_eq!(
            session
                .process_key(&KeyInput::plain(LogicalKey::Space))
                .expect("no failure"),
            KeyOutcome::Ignored
        );
    }

    #[test]
    fn backspace_undoes_a_selection_before_reporting_nothing_to_do() {
        let mut session = session();
        type_text(&mut session, "nihao");
        session.select(0).expect("the fallback exists");

        let backspace = KeyInput::plain(LogicalKey::Backspace);
        assert_eq!(
            session.process_key(&backspace).expect("no failure"),
            KeyOutcome::Consumed
        );
        assert_eq!(session.preedit().text(), "nihao");
        assert_eq!(session.preedit().spans()[0].style(), SpanStyle::Raw);
    }

    #[test]
    fn a_full_buffer_ignores_further_input() {
        let mut session = session();
        for _ in 0..MAX_INPUT_BYTES {
            session
                .process_key(&KeyInput::character('a'))
                .expect("no failure");
        }
        assert_eq!(session.raw_input().len(), MAX_INPUT_BYTES);
        assert_eq!(
            session
                .process_key(&KeyInput::character('a'))
                .expect("no failure"),
            KeyOutcome::Ignored
        );
        assert_eq!(session.raw_input().len(), MAX_INPUT_BYTES);
    }

    #[test]
    fn configuration_and_paths_are_the_injected_data() {
        let session = session();
        assert_eq!(session.page_size(), 5);
        assert_eq!(session.paths().user_data_dir().to_str(), Some("user"));
    }

    #[test]
    fn commit_on_an_empty_session_is_empty_text() {
        let mut session = session();
        assert_eq!(session.commit().expect("no failure"), "");
    }
}
