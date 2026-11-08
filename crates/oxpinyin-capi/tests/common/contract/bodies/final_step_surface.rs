//! The final_step_surface family's case bodies: each answers the result document the old case returned.
//!
//! Inventory row 43 (#1175, #594): the final-step sentences and candidate
//! windows of short full-pinyin inputs, with and without `PINYIN_INCOMPLETE`
//! and with an imported user phrase 罢跨. The pinyin facade records six sort
//! words' lists and each NBEST row's original rank; zhuyin one after-cursor list.

use crate::facade;
use crate::readers::{c, candidate_at, final_step_sentences, guess_rows, zhuyin_guess_rows};
use crate::support::{J, Kit, P};

/// One pinyin sort word's window, with the NBEST rows' original ranks.
fn pinyin_window(inst: P, sort: u32) -> J {
    let window = guess_rows(inst, 0, sort);
    let indexes: Vec<Option<u8>> = window
        .rows
        .iter()
        .enumerate()
        .map(|(ordinal, row)| {
            row.as_ref().filter(|(kind, _)| *kind == 1).map(|_| {
                let cand = candidate_at(inst, u32::try_from(ordinal).expect("an ordinal"))
                    .expect("get_candidate");
                let mut rank = 0xff_u8;
                assert!(facade::get_candidate_nbest_index(inst, cand, &raw mut rank));
                rank
            })
        })
        .collect();
    obj! { "ret" => window.ret, "rows" => window.rows, "nbest_indexes" => indexes }
}

/// `final_step_surface`'s probe.
fn probe(k: &mut Kit, text: &str, options: u32, imported: bool) -> J {
    assert!(facade::set_options(k.ctx(), options));
    if imported {
        let reading = if facade::ZHUYIN {
            "ㄅㄚ ㄎㄨㄚ"
        } else {
            "ba'kua"
        };
        let it = facade::begin_add_phrases(k.ctx(), 7);
        assert!(!it.is_null());
        assert!(facade::iterator_add_phrase(
            it,
            c("罢跨").as_ptr(),
            c(reading).as_ptr(),
            5
        ));
        facade::end_add_phrases(it);
    }
    let inst = k.inst();
    let parsed = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
    let guessed = facade::guess_sentence(inst);
    let sentences = final_step_sentences(inst);
    let mut lists = J::map();
    if facade::ZHUYIN {
        let window = zhuyin_guess_rows(inst, 0);
        lists.set(
            "after-cursor",
            obj! { "ret" => window.ret, "rows" => window.rows },
        );
    } else {
        for sort in [0, 1, 0x1c, 0x1d, 0x1e, 0x1f] {
            lists.set(format!("{sort:#x}"), pinyin_window(inst, sort));
        }
    }
    obj! {
        "parsed" => parsed,
        "guessed" => guessed,
        "nbest_count" => sentences.len(),
        "sentences" => sentences,
        "candidates" => lists,
    }
}

/// Runs the case `id`: `final-step-{mode}-{text}-{options:x}` or
/// `final-step-{mode}-import-ba-kua`.
pub fn case(k: &mut Kit, id: &str) -> J {
    let rest = id
        .strip_prefix(if facade::ZHUYIN {
            "final-step-zhuyin-"
        } else {
            "final-step-pinyin-"
        })
        .unwrap_or_else(|| panic!("no case {id}"));
    if rest == "import-ba-kua" {
        return probe(k, "ba'kua", 1 << 5, true);
    }
    let (text, options) = rest
        .rsplit_once('-')
        .unwrap_or_else(|| panic!("no case {id}"));
    let options = u32::from_str_radix(options, 16).expect("an option word");
    probe(k, text, options, false)
}
