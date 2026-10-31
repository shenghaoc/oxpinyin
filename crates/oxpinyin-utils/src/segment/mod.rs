//! `ngseg`, `spseg` and `mergeseq` (`utils/segment/*.cpp` at libpinyin
//! 074a2219): the pin's names, options, messages, inputs and outputs.
//!
//! The three utilities read `table.conf`, `phrase_index.bin` and the
//! `SYSTEM_FILE` chunk files from the current working directory, relative
//! names included (`src/include/pinyin_internal.h:55-66`); `ngseg` also
//! attaches `bigram.db`. Their edge calls — the GLib option parser,
//! `std::getline`'s contract, `fprintf`, `perror`, `exit` and `abort` —
//! live in `oxpinyin_input::tool`; this module owns what each line means.
//!
//! The phrase table is the pin's DBM (`FacadePhraseTable3`), not an
//! inverted index over the chunk files: [`phrase::PhraseSearch`] filters
//! its stored tokens by the loaded-library set exactly as
//! `PhraseTableEntry::search` does, and answers `SEARCH_CONTINUED` for
//! every present key.

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

use std::ffi::{CStr, CString, OsString};

use oxpinyin_input::tool;
use oxpinyin_input::tool::{OptionArg, OptionEntry, OptionValue, OutputStream, ParsedOptions};

pub mod data;
pub mod input;
pub mod mergeseq;
pub mod ngseg;
pub mod phrase;
pub mod spseg;
pub mod trellis;

/// `null_token` (`src/include/novel_types.h:121`).
pub(crate) const NULL_TOKEN: u32 = 0;

/// `sentence_start` (`src/include/novel_types.h:122`).
pub(crate) const SENTENCE_START: u32 = 1;

/// `MAX_PHRASE_LENGTH` (`src/include/novel_types.h:119`).
pub(crate) const MAX_PHRASE_LENGTH: usize = 16;

/// The pin's `assert(cond)` failure: `abort()` without the macro's message
/// (maintainer ruling 2026-10-31 UTC, `docs/findings/eio-tool-check-messages.md`).
pub(crate) fn abort_on_pin_assert() -> ! {
    tool::abort()
}

/// A NUL-free C string from `bytes`, cut at the first NUL as `%s` and
/// `strcmp` read an embedded one; `None` only for bytes that cannot form
/// one.
pub(crate) fn c_string(bytes: &[u8]) -> Option<CString> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    CString::new(&bytes[..end]).ok()
}

/// `g_ucs4_to_utf8(words, words.len(), NULL, NULL, NULL)` as `%s` reads it:
/// the pin's bytes up to their first NUL, or `None` where the call answers
/// NULL (a word at or above `0x8000_0000`).
pub(crate) fn text_of(words: &[u32]) -> Option<CString> {
    c_string(&tool::ucs4_to_utf8(words)?)
}

/// One character run of a line and the state the pin classified it with.
pub(crate) struct Run {
    /// `CONTEXT_SEGMENTABLE` rather than `CONTEXT_UNKNOWN`.
    pub segmentable: bool,
    /// The run's character range in the line's UCS-4 sentence.
    pub range: std::ops::Range<usize>,
}

/// The pin's three-state run splitter (`ngseg.cpp:206-237`,
/// `spseg.cpp:287-319`): consecutive characters whose
/// `search(1, …) & SEARCH_OK` answers agree.
pub(crate) fn runs(sentence: &[u32], search: &phrase::PhraseSearch<'_>) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut start = 0;
    let mut segmentable = search.search(&sentence[..1]).ok();
    for index in 1..sentence.len() {
        let next = search.search(&sentence[index..index + 1]).ok();
        if next == segmentable {
            continue;
        }
        runs.push(Run {
            segmentable,
            range: start..index,
        });
        start = index;
        segmentable = next;
    }
    runs.push(Run {
        segmentable,
        range: start..sentence.len(),
    });
    runs
}

/// `{"outputfile", 'o', 0, G_OPTION_ARG_FILENAME, &outputfile, "output",
/// "filename"}` (`ngseg.cpp:45`, `spseg.cpp:45`, `mergeseq.cpp:45`).
pub(crate) fn outputfile_entry() -> OptionEntry {
    OptionEntry {
        long_name: c"outputfile",
        short_name: b'o',
        flags: 0,
        arg: OptionArg::Filename(None),
        description: Some(c"output"),
        arg_description: Some(c"filename"),
    }
}

/// `{"generate-extra-enter", 0, 0, G_OPTION_ARG_NONE,
/// &gen_extra_enter, "generate ", NULL}` (`ngseg.cpp:46`, `spseg.cpp:46`).
pub(crate) fn extra_enter_entry() -> OptionEntry {
    OptionEntry {
        long_name: c"generate-extra-enter",
        short_name: 0,
        flags: 0,
        arg: OptionArg::Flag(false),
        description: Some(c"generate "),
        arg_description: None,
    }
}

/// `g_option_context_parse` with the pin's failure line already written;
/// a refused parse is `exit(EINVAL)` (`ngseg.cpp:118-121`).
pub(crate) fn parse_options(
    parameter_string: &'static CStr,
    entries: &[OptionEntry],
    args: impl IntoIterator<Item = OsString>,
) -> ParsedOptions {
    match tool::parse_options(parameter_string, entries, args) {
        Ok(parsed) => parsed,
        Err(_) => tool::exit(tool::EINVAL),
    }
}

/// The `outputfile` option: `fopen(filename, "w")` or the C runtime's
/// `stdout` (`ngseg.cpp:106-129`). A failed `fopen` is
/// `perror("open file failed")` then `exit(EINVAL)`.
pub(crate) fn open_output(value: &OptionValue) -> OutputStream {
    match value {
        OptionValue::Filename(Some(bytes)) => {
            let name = c_string(bytes).unwrap_or_default();
            match OutputStream::open_write(&name) {
                Some(stream) => stream,
                None => {
                    tool::perror(c"open file failed");
                    tool::exit(tool::EINVAL);
                }
            }
        }
        _ => match OutputStream::stdout() {
            Some(stream) => stream,
            // The C runtime's `stdout` is never NULL in a started process.
            None => tool::abort(),
        },
    }
}

/// `argc > 2` after option parsing: `too many arguments.` and
/// `exit(EINVAL)` (`ngseg.cpp:131-134`).
pub(crate) fn refuse_extra_arguments(args: &[Vec<u8>]) {
    if args.len() > 2 {
        tool::segmentation::report_too_many_arguments();
        tool::exit(tool::EINVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::c_string;

    #[test]
    fn c_strings_stop_at_the_first_nul() {
        assert_eq!(
            c_string(b"abc\0def").map(|text| text.into_bytes()),
            Some(b"abc".to_vec())
        );
        assert_eq!(
            c_string(b"abc").map(|text| text.into_bytes()),
            Some(b"abc".to_vec())
        );
    }
}
