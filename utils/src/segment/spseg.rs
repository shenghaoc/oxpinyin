//! `spseg` — the fewest-words shortest-path segmenter
//! (`utils/segment/spseg.cpp` at 074a2219).
//!
//! The driver is the same three-state run splitter as `ngseg`; a
//! segmentable run is split by the O(n²) dynamic program at
//! `spseg.cpp:83-141`, whose running `nword` grows inside the inner `k`
//! loop (`:124`) — the pin's quirk, kept. The phrase text of an output
//! step is the run's own UCS-4 slice, not the phrase item's
//! (`:180-185`).

use std::ffi::OsString;

use oxpinyin_tool_support::input::{self as tool, OutputStream};

use crate::segment::data;
use crate::segment::input::Input;
use crate::segment::phrase::PhraseSearch;
use crate::segment::{segment_lines, text_of};
use oxpinyin::vocab::NULL_TOKEN;

/// One `SegmentStep` (`spseg.cpp:65-81`).
#[derive(Clone, Copy)]
struct Step {
    /// `guint m_nword`; `u32::MAX` is `UINT_MAX`, unreachable.
    nword: u32,
    /// `phrase_token_t m_handle`.
    token: u32,
    /// Start of the phrase within the run (`m_phrase`).
    phrase_start: usize,
    /// `size_t m_phrase_len`. The pin's `gint m_backward_nstep` is
    /// `-m_phrase_len`, so the backtrace steps to `phrase_start`.
    phrase_len: usize,
}

impl Step {
    const UNREACHABLE: Self = Self {
        nword: u32::MAX,
        token: NULL_TOKEN,
        phrase_start: 0,
        phrase_len: 0,
    };
}

/// `segment` + `backtrace` (`spseg.cpp:86-168`): the chosen phrases as
/// `(token, start, length)` within `run`.
fn segment_run(search: &PhraseSearch<'_>, run: &[u32]) -> Vec<(u32, usize, usize)> {
    let len = run.len();
    let mut steps = vec![Step::UNREACHABLE; len + 1];
    steps[0].nword = 0;

    for start in 0..len {
        let mut nword = u64::from(steps[start].nword);
        for end in start + 1..=len {
            let span_len = end - start;
            let found = search.search(&run[start..end]);
            let mut token = NULL_TOKEN;
            if found.ok() {
                token = found.first_token();
            } else if span_len != 1 {
                // `if (1 != len) continue;` — the `++nword` and the
                // continuation check are skipped with it.
                continue;
            }
            // `size_t nword` in the pin, `++nword` once per surviving span.
            nword = nword.wrapping_add(1);
            let reachable = u64::from(steps[end].nword);
            if nword < reachable {
                steps[end] = Step {
                    // `m_nword` is a `guint`; the assignment truncates.
                    nword: nword as u32,
                    token,
                    phrase_start: start,
                    phrase_len: span_len,
                };
            }
            if !found.continued {
                break;
            }
        }
    }

    let mut phrases = Vec::new();
    let mut cursor = len;
    while cursor != 0 {
        let step = steps[cursor];
        phrases.push((step.token, step.phrase_start, step.phrase_len));
        // A relaxed cell always has `i < k`, so its phrase is not empty. An
        // empty phrase is the default cell an unreachable tail would leave;
        // the pin would loop there forever, and refusing to emit instead
        // is the only safe reading of that impossible state.
        if step.phrase_len == 0 {
            break;
        }
        cursor = step.phrase_start;
    }
    phrases.reverse();
    phrases
}

/// `deal_with_segmentable` (`spseg.cpp:170-189`).
fn deal_with_segmentable(search: &PhraseSearch<'_>, run: &[u32], output: &OutputStream) {
    for (token, start, len) in segment_run(search, run) {
        let text = text_of(&run[start..start + len]);
        tool::segmentation::print_token_line(output, token as i32, text.as_deref());
    }
}

/// `main` (`spseg.cpp:201-345`).
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
        let parsed = tool::parse_options_or_exit(c"- shortest path segment", &entries, args);
        let output = crate::segment::open_output(parsed.filename(0));
        crate::segment::refuse_extra_arguments(&parsed.args);
        let mut input = Input::open(&parsed.args);
        let tables = data::load_or_exit(false);
        let search = PhraseSearch::new(tables.phrase.as_ref(), &tables.libraries);
        let gen_extra_enter = matches!(
            parsed.values[1],
            oxpinyin_tool_support::input::OptionValue::Flag(true)
        );

        segment_lines(&mut input, &output, &search, gen_extra_enter, |run| {
            deal_with_segmentable(&search, run, &output);
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
