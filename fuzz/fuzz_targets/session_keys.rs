#![no_main]
//! Arbitrary key sequences through the Rust session API — the G5 gap in
//! the testing-strategy assessment (`testing-strategy.md` §2): the C ABI
//! command stream has a fuzzer (`capi_commands`), but the internal Rust
//! session layer (`oxpinyin-engine::Session`) had only the authored
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
//! - the composition offset stays within the raw input at a character boundary;
//! - a lookup offset past the raw buffer is a typed refusal
//!   (`EngineError::LookupOffsetOutOfRange`), never a panic — the pin
//!   reads out of bounds there, so this is exactly the class of input
//!   the seam must refuse;
//! - `reset` returns to the initial state (empty raw input, empty
//!   selection text and token history) from any state whatever.

use libfuzzer_sys::fuzz_target;
use oxpinyin_engine::{EmptyConfigSource, EngineError, Session};
use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

const VOCAB: &str = include_str!("../../fixtures/w4/mini-vocab.txt");
const BIGRAM: &str = include_str!("../../fixtures/w4/mini-bigram.txt");

type Fixtures = Session<FixtureDictionary, FixtureLanguageModel>;

fn session() -> Fixtures {
    let dictionary = FixtureDictionary::parse(VOCAB).expect("committed fixture");
    let model = FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("committed fixture");
    Session::new(&EmptyConfigSource, dictionary, model).expect("the fixtures open")
}

/// Selection and raw input share the same bounded byte coordinate space.
fn check_composition(session: &Fixtures) {
    let offset = session.composition_offset();
    assert!(offset <= session.raw_input().len());
    assert!(session.raw_input().is_char_boundary(offset));
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
                    .type_pinyin(character.encode_utf8(&mut [0; 4]))
                    .expect("type_pinyin is total");
            }
            // Direct editing and reset operations, without key dispatch.
            1 => match (command >> 3) % 3 {
                0 => {
                    let _ = session.erase().expect("erase is total");
                }
                1 => session.reset(),
                _ => {
                    let _ = session
                        .clear_constraint(usize::from(payload.first().copied().unwrap_or(0)));
                }
            },
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
                        let _ = session.select_anchored(
                            index,
                            &list,
                            offset.min(session.raw_input().len()),
                        );
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
                assert!(
                    session.selected_text().is_empty(),
                    "reset must clear selected text"
                );
                assert!(
                    session.selected_tokens().is_empty(),
                    "reset must clear token history"
                );
                assert_eq!(session.composition_offset(), 0);
            }
        }
        check_composition(&session);
    }
    check_composition(&session);
});
