//! `ngseg` — the n-gram sentence segmenter
//! (`utils/segment/ngseg.cpp` at 074a2219).
//!
//! The three-state run splitter is shared with `spseg`. A segmentable run uses
//! `PhraseLookup::get_best_match` with the `table.conf` λ and the system
//! bigram. `deal_with_segmentable`
//! (`ngseg.cpp:70-94`) prints `Un-segmentable sentence encountered:%s` on
//! stderr when `convert_to_utf8` answers NULL, and its caller ignores the
//! answer.

use std::ffi::CString;
use std::ffi::OsString;

use oxpinyin_tool_support::input::{self as tool, OutputStream};

use crate::segment::data;
use crate::segment::input::Input;
use crate::segment::phrase::PhraseSearch;
use crate::segment::{segment_lines, text_of, trellis};
use oxpinyin::facade::NULL_TOKEN;

/// One phrase result line: `%d %s` with the phrase item's text
/// (`convert_to_utf8`, `src/lookup/lookup.cpp:27-69`). A missing item is
/// the pin's default `PhraseItem`, whose length is zero, so the line is
/// `"{token} \n"`.
fn print_phrase(output: &OutputStream, search: &PhraseSearch<'_>, token: u32) {
    let text = match search.item(token) {
        Some(item) => text_of(&item.words),
        None => Some(CString::default()),
    };
    tool::segmentation::print_token_line(output, token as i32, text.as_deref());
}

/// `deal_with_segmentable` (`ngseg.cpp:70-94`).
fn deal_with_segmentable(
    search: &PhraseSearch<'_>,
    bigram: Option<&oxpinyin::__internal::data::BigramTable>,
    lambda: f32,
    run: &[u32],
    output: &OutputStream,
) {
    let result = trellis::get_best_match(search, bigram, lambda, run);
    if result.iter().all(|&token| token == NULL_TOKEN) {
        // `convert_to_utf8` answered NULL: no token survived, so the pin
        // writes the run it could not segment.
        let text = text_of(run);
        tool::segmentation::report_unsegmentable(text.as_deref());
        return;
    }
    for token in result {
        if token != NULL_TOKEN {
            print_phrase(output, search, token);
        }
    }
}

/// `main` (`ngseg.cpp:106-262`).
///
/// # Safety
///
/// If the entry point closes C `stdout`, it must still be open. No other
/// code on any thread, including teardown and exit handlers, may use C
/// `stdout` during or after that close.
#[allow(
    unsafe_code,
    reason = "callers must uphold the process-level C stdout close precondition"
)]
pub unsafe fn main(args: impl IntoIterator<Item = OsString>) {
    #[deny(unsafe_code)]
    {
        let _ = tool::set_locale_from_environment();
        let entries = [
            crate::segment::outputfile_entry(),
            crate::segment::extra_enter_entry(),
        ];
        let parsed = tool::parse_options_or_exit(c"- n-gram segment", &entries, args);
        let output = crate::segment::open_output(parsed.filename(0));
        crate::segment::refuse_extra_arguments(&parsed.args);
        let mut input = Input::open(&parsed.args);
        let tables = data::load_or_exit(data::Gram::Bigram);
        let search = PhraseSearch::new(tables.phrase.as_ref(), &tables.libraries);
        let lambda = tables.conf.lambda_f32();
        let gen_extra_enter = parsed.flag(1);

        segment_lines(&mut input, &output, &search, gen_extra_enter, |run| {
            deal_with_segmentable(&search, tables.bigram.as_ref(), lambda, run, &output);
        });
        #[allow(
            unsafe_code,
            reason = "the utility closes its output including C stdout as the pin does"
        )]
        // SAFETY: This entry point's caller guarantees the C stdout close
        // precondition. An owned file is closed only here.
        unsafe {
            output.close_including_stdout();
        }
    }
}
