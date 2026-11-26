#![forbid(unsafe_code)]

use std::ffi::{CStr, CString};

use oxpinyin_tool_support::input as tool;
use oxpinyin_tool_support::input::{OptionArg, OptionEntry, OutputStream};

use super::phrase;
use oxpinyin::vocab::NULL_TOKEN;

/// The pin's `assert(cond)` failure: `abort()` without the macro's message
/// (`docs/findings/upstream-divergences.md#utility-check-macros-write-no-message`).
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

/// The line loop shared by `ngseg` and `spseg` (`ngseg.cpp:106-262`,
/// `spseg.cpp:201-345`): one line in, its runs split, each segmentable run
/// handed to `segmentable`, with the pin's diagnostics and blank lines.
/// The final blank line before the caller closes the output is part of the
/// loop.
pub(crate) fn segment_lines(
    input: &mut super::input::Input,
    output: &OutputStream,
    search: &phrase::PhraseSearch<'_>,
    gen_extra_enter: bool,
    mut segmentable: impl FnMut(&[u32]),
) {
    while input.getline() {
        let line = input.c_str();
        let num_of_chars = tool::utf8_strlen(line);
        let (sentence, count) = tool::utf8_to_ucs4(line);
        let Some(sentence) = sentence else {
            tool::segmentation::report_non_ucs4(line);
            tool::segmentation::print_null_line(output);
            continue;
        };
        if count != num_of_chars {
            tool::segmentation::report_non_ucs4(line);
            tool::segmentation::print_null_line(output);
            continue;
        }
        // `/* only new-line persists. */`
        if num_of_chars == 0 {
            tool::segmentation::print_null_line(output);
            continue;
        }

        for run in runs(&sentence, search) {
            if run.segmentable {
                segmentable(&sentence[run.range]);
            } else {
                let text = text_of(&sentence[run.range]);
                tool::segmentation::print_token_line(output, NULL_TOKEN as i32, text.as_deref());
            }
        }
        if gen_extra_enter {
            tool::segmentation::print_null_line(output);
        }
    }
    tool::segmentation::print_null_line(output);
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

/// The `outputfile` option: `fopen(filename, "w")` or the C runtime's
/// `stdout` (`ngseg.cpp:106-129`). A failed `fopen` is
/// `perror("open file failed")` then `exit(EINVAL)`.
pub(crate) fn open_output(name: Option<&CStr>) -> OutputStream {
    if let Some(name) = name {
        match OutputStream::open_write(name) {
            Some(stream) => stream,
            None => {
                tool::perror(c"open file failed");
                tool::exit(tool::EINVAL);
            }
        }
    } else {
        match OutputStream::stdout() {
            Some(stream) => stream,
            // The C runtime's `stdout` is never NULL in a started process.
            None => tool::abort(),
        }
    }
}

/// `argc > 2` after option parsing: `too many arguments.` and
/// `exit(EINVAL)` (`ngseg.cpp:131-134`).
pub(crate) fn refuse_extra_arguments(args: &[CString]) {
    if args.len() > 2 {
        tool::segmentation::report_too_many_arguments();
        tool::exit(tool::EINVAL);
    }
}
