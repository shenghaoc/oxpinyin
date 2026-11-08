//! The sort_pinyin_length family's case bodies: each answers the result document the old case returned.
//!
//! #921, #927: every sort that orders a C-visible list is GLib's merge
//! (`g_sort_array`, glib 2.84.4 `gqsort.c:69-193`) over a comparator that is not
//! a total order (074a2219 `phonetic_lookup.h:174-178,337`; `pinyin.cpp:1678-1709,
//! 2286,2443`; `zhuyin.cpp:1129-1144,1529,1610`). #1275: the pin's
//! SORT_BY_PINYIN_LENGTH key is `guint8 len = m_end - m_begin`
//! (`pinyin.cpp:1692-1694`), which wraps from a 256-byte span.

use crate::facade;
use crate::readers::{
    GuessRows, c, candidate_rows, final_step_sentences, guess_rows, zhuyin_guess_rows,
};
use crate::support::{J, Kit};

const GLIB_SORT_TAIL_INPUTS: [&str; 20] = [
    "xian",
    "fangan",
    "shishi",
    "yiyi",
    "jiandan",
    "tiantian",
    "woaini",
    "nihaoshijie",
    "zhongguoren",
    "shijie",
    "dada",
    "mama",
    "lilun",
    "jiqi",
    "shuxue",
    "diannao",
    "yuyan",
    "jintian",
    "mingtian",
    "shangwu",
];
const GLIB_SORT_LIST_INPUTS: [&str; 7] = ["n", "zh", "a", "de", "yi", "shi", "ji"];
const PINYIN_LENGTH_SORTS: [u32; 8] = [0x08, 0x09, 0x0a, 0x0c, 0x18, 0x1a, 0x1c, 0x1e];

/// A `{'ret', 'rows'}` document.
fn rows_j(rows: GuessRows) -> J {
    obj! { "ret" => rows.ret, "rows" => rows.rows }
}

/// `glib_sort_zhuyin_options`: USE_TONE without FORCE_TONE on the zhuyin facade.
fn zhuyin_options(k: &mut Kit) {
    if facade::ZHUYIN {
        assert!(facade::set_options(k.ctx(), 1 << 5));
    }
}

/// `glib_sort_tails`: each input on a fresh instance, guessed, every sentence rank read.
fn tails(k: &mut Kit, texts: &[&str], options: Option<u32>) -> J {
    zhuyin_options(k);
    if let Some(options) = options {
        assert!(facade::set_options(k.ctx(), options));
    }
    let mut out = J::map();
    for text in texts {
        let inst = k.alloc();
        let parsed = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
        let guessed = facade::guess_sentence(inst);
        let sentences = final_step_sentences(inst);
        out.set(
            *text,
            obj! { "parsed" => parsed, "guessed" => guessed, "sentences" => sentences },
        );
    }
    out
}

/// `glib_sort_candidates`: three pinyin sort words, or both zhuyin cursors.
fn candidates(k: &mut Kit) -> J {
    zhuyin_options(k);
    let mut out = J::map();
    for text in GLIB_SORT_LIST_INPUTS {
        let inst = k.alloc();
        let parsed = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
        let mut lists = J::map();
        if facade::ZHUYIN {
            lists.set("after", rows_j(zhuyin_guess_rows(inst, 0)));
            let before = facade::guess_candidates_before_cursor(inst, parsed);
            lists.set(
                "before",
                obj! { "ret" => before, "rows" => candidate_rows(inst) },
            );
        } else {
            for sort in [0x14, 0x1c, 0x1e] {
                lists.set(format!("{sort:#x}"), rows_j(guess_rows(inst, 0, sort)));
            }
        }
        out.set(text, lists);
    }
    out
}

/// `glib-sort-predicted`: full plain predicted lists after one-character prefixes.
fn predicted(k: &mut Kit) -> J {
    let mut out = J::map();
    for prefix in ["你", "我", "中", "的", "一", "是"] {
        let inst = k.inst();
        let ret = facade::guess_predicted_candidates(inst, c(prefix).as_ptr());
        out.set(
            prefix,
            obj! { "ret" => ret, "rows" => candidate_rows(inst) },
        );
    }
    out
}

/// `pinyin_length_rows(apostrophes)`.
fn pinyin_length(k: &mut Kit, apostrophes: usize) -> J {
    let text = format!("ni{}hao", "'".repeat(apostrophes));
    let inst = k.alloc();
    let parsed = facade::parse_more_full_pinyins(inst, c(&text).as_ptr());
    assert_eq!(parsed, text.len());
    let mut out = obj! { "span" => text.len(), "parsed" => parsed };
    for sort in PINYIN_LENGTH_SORTS {
        let rows = guess_rows(inst, 0, sort);
        assert!(rows.ret && !rows.rows.is_empty(), "{sort:#x}");
        out.set(format!("{sort:#x}"), rows_j(rows));
    }
    out
}

/// The table options of an `on` or `off` trellis case.
fn table_options(enabled: &str) -> u32 {
    match enabled {
        "on" => 0x1fe0_0198,
        "off" => 0x1fe0_0018,
        other => panic!("no table state {other}"),
    }
}

/// The family's cases by ID.
pub fn case(k: &mut Kit, id: &str) -> J {
    let mode = if facade::ZHUYIN { "zhuyin" } else { "pinyin" };
    if id == "glib-sort-predicted" {
        return predicted(k);
    }
    if id == format!("glib-sort-tails-{mode}") {
        return tails(k, &GLIB_SORT_TAIL_INPUTS, None);
    }
    if id == format!("glib-sort-candidates-{mode}") {
        return candidates(k);
    }
    if let Some(enabled) = id.strip_prefix("trellis-tail-sort/qian-") {
        return tails(k, &["qian"], Some(table_options(enabled)));
    }
    if let Some(rest) = id.strip_prefix("trellis-adjusted-score/") {
        let parts: Vec<&str> = rest.split('-').collect();
        let [_site, text, enabled] = parts.as_slice() else {
            panic!("{id}: not a trellis case")
        };
        return tails(k, &[text], Some(table_options(enabled)));
    }
    if let Some(span) = id.strip_prefix("pinyin-length-wrap/span-") {
        let span: usize = span.parse().expect("a span");
        return pinyin_length(k, span - 5);
    }
    panic!("{id}: not a sort_pinyin_length case")
}
