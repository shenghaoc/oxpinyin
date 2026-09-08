#![no_main]
//! Arbitrary key sequences through the Rust session API — the G5 gap in
//! the testing-strategy assessment (`testing-strategy.md` §2): the C ABI
//! command stream has a fuzzer (`capi_commands`), but the supported Rust
//! surface (`oxpinyin-engine::Session`) had only the authored
//! `session_replay` proptest scenarios, nothing hostile.
//!
//! Each input builds a fresh session over the committed w4 fixture
//! doubles (inputs stay independent; no state leaks across them), then
//! walks the input as a command stream in the `capi_commands` shape:
//! one byte selects the command, the remaining bytes feed it. The
//! postconditions, per the assessment:
//!
//! - no panic, abort, or sanitizer report on any command sequence
//!   (constitution rule 4; the fuzz crate's ASan default);
//! - the preedit law: spans ascending, non-overlapping, together
//!   covering `text` exactly, cursor within `text` on a character
//!   boundary (`Preedit`'s own doc contract);
//! - a lookup offset past the raw buffer is a typed refusal
//!   (`EngineError::LookupOffsetOutOfRange`), never a panic — the pin
//!   reads out of bounds there, so this is exactly the class of input
//!   the seam must refuse;
//! - `reset` returns to the initial state (empty raw input, empty
//!   preedit) from any state whatever.

use libfuzzer_sys::fuzz_target;
use oxpinyin_engine::{
    EmptyConfigSource, EngineError, KeyInput, LogicalKey, Session, StoragePaths,
};
use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

const VOCAB: &str = include_str!("../../fixtures/w4/mini-vocab.txt");
const BIGRAM: &str = include_str!("../../fixtures/w4/mini-bigram.txt");

type Fixtures = Session<FixtureDictionary, FixtureLanguageModel>;

fn session() -> Fixtures {
    let dictionary = FixtureDictionary::parse(VOCAB).expect("committed fixture");
    let model = FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("committed fixture");
    Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        dictionary,
        model,
    )
    .expect("the fixtures open")
}

/// The `Preedit` doc contract, checked after every mutating command:
/// ascending non-overlapping spans covering `text` exactly, cursor in
/// range on a character boundary.
fn check_preedit(session: &Fixtures) {
    let preedit = session.preedit();
    let text = preedit.text();
    let mut covered = 0_usize;
    for span in preedit.spans() {
        assert!(
            span.start() >= covered,
            "preedit span {}..{} overlaps or regresses (cursor {covered})",
            span.start(),
            span.end()
        );
        assert!(span.end() > span.start(), "empty preedit span");
        assert!(
            span.end() <= text.len(),
            "preedit span {}..{} runs past the text (len {})",
            span.start(),
            span.end(),
            text.len()
        );
        covered = span.end();
    }
    assert_eq!(
        covered,
        text.len(),
        "preedit spans must partition the text exactly"
    );
    let cursor = preedit.cursor();
    assert!(cursor <= text.len(), "cursor {cursor} past the text");
    assert!(
        text.is_char_boundary(cursor),
        "cursor {cursor} inside a character"
    );
}

/// One non-text key per selector value; `Character` comes from the
/// payload byte so the raw buffer sees both ASCII and Latin-1.
fn plain_key(selector: u8) -> LogicalKey {
    match selector % 11 {
        0 => LogicalKey::Backspace,
        1 => LogicalKey::Delete,
        2 => LogicalKey::Enter,
        3 => LogicalKey::Escape,
        4 => LogicalKey::Space,
        5 => LogicalKey::Tab,
        6 => LogicalKey::Left,
        7 => LogicalKey::Right,
        8 => LogicalKey::Up,
        9 => LogicalKey::Down,
        _ => LogicalKey::Unknown,
    }
}

fuzz_target!(|data: &[u8]| {
    let mut session = session();
    let mut cursor = 0;
    while cursor < data.len() {
        let command = data[cursor];
        cursor += 1;
        let payload = &data[cursor..];
        match command % 7 {
            // a character keystroke from the payload byte (Latin-1
            // supplement: ASCII plus the two-byte-when-encoded range)
            0 => {
                let byte = payload.first().copied().unwrap_or(b'a');
                let character = char::from_u32(u32::from(byte)).expect("byte is a scalar");
                let _ = session
                    .process_key(&KeyInput::character(character))
                    .expect("process_key is total");
            }
            // a non-text key from the command byte
            1 => {
                let _ = session
                    .process_key(&KeyInput::plain(plain_key(command >> 3)))
                    .expect("process_key is total");
            }
            // bulk pinyin text from the payload, capped like capi_commands
            // so the fuzzer explores parse states, not multi-KB memcpys
            2 => {
                let text: String =
                    String::from_utf8_lossy(&payload.iter().copied().take(64).collect::<Vec<_>>())
                        .into_owned();
                let _ = session.type_pinyin(&text).expect("type_pinyin is total");
            }
            // candidate window at an in-bounds offset derived from the
            // payload, plus the typed out-of-range refusal one past a
            // far bound
            3 => {
                let offset = usize::from(payload.first().copied().unwrap_or(0));
                if let Ok(list) = session.candidates_at(offset.min(session.raw_input().len())) {
                    // selecting exercises the selection path when the
                    // window is non-empty; index clamped into range
                    if !list.is_empty() {
                        let index =
                            usize::from(payload.get(1).or(payload.first()).copied().unwrap_or(0))
                                % list.len();
                        let _ = session.select(index);
                    }
                }
                let far = session.raw_input().len() + 64;
                assert!(
                    matches!(
                        session.candidates_at(far),
                        Err(EngineError::LookupOffsetOutOfRange { .. })
                    ),
                    "an offset past the buffer must be the typed refusal"
                );
            }
            // n-best sentence guess + read-back
            4 => {
                let _ = session.guess_sentence();
                let _ = session.sentence_text(0);
            }
            // the before-cursor window at an in-bounds offset
            5 => {
                let offset = usize::from(payload.first().copied().unwrap_or(0));
                let _ = session.candidates_ending_at(offset.min(session.raw_input().len()));
            }
            // reset from whatever state the input built, then the
            // initial-state law
            _ => {
                session.reset();
                assert!(
                    session.raw_input().is_empty(),
                    "reset must clear the raw input"
                );
                assert!(session.preedit().is_empty(), "reset must clear the preedit");
            }
        }
        check_preedit(&session);
    }
    check_preedit(&session);
});
