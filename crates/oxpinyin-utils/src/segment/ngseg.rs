//! `ngseg` — the n-gram sentence segmenter
//! (`utils/segment/ngseg.cpp` at 074a2219).
//!
//! The three-state run splitter is `spseg`'s; a segmentable run goes
//! through [`crate::segment::trellis`] (`PhraseLookup::get_best_match`)
//! with the `table.conf` λ and the system bigram. `deal_with_segmentable`
//! (`ngseg.cpp:70-94`) prints `Un-segmentable sentence encountered:%s` on
//! stderr when `convert_to_utf8` answers NULL, and its caller ignores the
//! answer.

use std::ffi::CString;
use std::ffi::OsString;

use oxpinyin_input::tool::{self, OptionValue, OutputStream};

use crate::segment::data;
use crate::segment::input::Input;
use crate::segment::phrase::PhraseSearch;
use crate::segment::{NULL_TOKEN, runs, text_of, trellis};

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
    bigram: Option<&oxpinyin_data::BigramTable>,
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
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let _ = tool::set_locale_from_environment();
    let entries = [
        crate::segment::outputfile_entry(),
        crate::segment::extra_enter_entry(),
    ];
    let parsed = crate::segment::parse_options(c"- n-gram segment", &entries, args);
    let output = crate::segment::open_output(&parsed.values[0]);
    crate::segment::refuse_extra_arguments(&parsed.args);
    let mut input = Input::open(&parsed.args);
    let tables = data::load_or_exit(true);
    let search = PhraseSearch::new(tables.phrase.as_ref(), &tables.libraries);
    let lambda = tables.conf.lambda_f32();
    let gen_extra_enter = matches!(parsed.values[1], OptionValue::Flag(true));

    while input.getline() {
        let line = input.c_str();
        let num_of_chars = tool::utf8_strlen(line);
        let (sentence, count) = tool::utf8_to_ucs4(line);
        let Some(sentence) = sentence else {
            tool::segmentation::report_non_ucs4(line);
            tool::segmentation::print_null_line(&output);
            continue;
        };
        if count != num_of_chars {
            tool::segmentation::report_non_ucs4(line);
            tool::segmentation::print_null_line(&output);
            continue;
        }
        // `/* only new-line persists. */`
        if num_of_chars == 0 {
            tool::segmentation::print_null_line(&output);
            continue;
        }

        for run in runs(&sentence, &search) {
            if run.segmentable {
                deal_with_segmentable(
                    &search,
                    tables.bigram.as_ref(),
                    lambda,
                    &sentence[run.range],
                    &output,
                );
            } else {
                let text = text_of(&sentence[run.range]);
                tool::segmentation::print_token_line(&output, NULL_TOKEN as i32, text.as_deref());
            }
        }
        if gen_extra_enter {
            tool::segmentation::print_null_line(&output);
        }
    }
    tool::segmentation::print_null_line(&output);
    output.close_including_stdout();
}
