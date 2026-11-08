//! The facade_totals_counts family's case bodies: each answers the result document the old case returned.
//!
//! Inventory row 48 (#1175, #798, #801, #540): the n-best unigram denominator
//! is the facade's `guint32` total (`phrase_index.h:440`, `:615-617`), raised by
//! imports, unigram adds and library loads with no overflow check
//! (`phrase_index.h:628-634`), and read by the ranking
//! (`phonetic_lookup.h:651`, `:679`; `pinyin.cpp:1855-1866`).

use crate::facade;
use crate::fixtures::{
    ConfEdit, chunk, conf_system, craft_hash, craft_tree, items_system, write_user_conf,
};
use crate::readers::{
    c, final_step_sentences, guess_rows, import_rows, lookup, row_of, segment_out,
};
use crate::support::{J, Kit, P, UNTOUCHED};

/// `FACADE_TOTAL`: the pinned data's loaded libraries, summed.
const FACADE_TOTAL: u64 = 51_051_831;

/// `unigram_of` on `inst`.
fn unigram(inst: P, token: u32) -> (bool, u32) {
    let mut freq = UNTOUCHED;
    let ret = facade::token_get_unigram_frequency(inst, token, &raw mut freq);
    (ret, freq)
}

/// `_guess_out(k, text)` on `inst`, merged into `out`.
fn guess_out(out: &mut J, inst: P, text: &str) {
    out.set(
        "parsed",
        facade::parse_more_full_pinyins(inst, c(text).as_ptr()),
    );
    out.set("guessed", facade::guess_sentence(inst));
    out.set("sentences", final_step_sentences(inst));
}

/// `_segment_out(k, (text,))` on `inst`.
fn segment(inst: P, text: &str) -> J {
    J::from(segment_out(inst, &[text]))
}

/// `_facade_total_out` on `inst`.
fn facade_total_out(inst: P) -> J {
    let mut out = J::map();
    guess_out(&mut out, inst, "ni3'hao3");
    let mut rows = guess_rows(inst, 0, 0x1e).rows;
    rows.truncate(8);
    out.set("candidates", rows);
    out.set("segment", segment(inst, "你好"));
    out
}

/// `import_facade_total`: imports 妈 at the count that leaves the facade at `total`.
fn import_facade_total(k: &mut Kit, total: u64) -> J {
    let difference = total - FACADE_TOTAL;
    assert_eq!(difference % 3, 0);
    let count = difference / 3;
    assert!(3 * count <= 0xFFFF_FFFF && count < 1 << 31, "{total}");
    let count = i32::try_from(count).expect("a count");
    let mut out = obj! {
        "count" => count,
        "added" => import_rows(k, 7, &[("妈", "ma1", count)]),
    };
    let inst = k.inst();
    guess_out(&mut out, inst, "ni3'hao3");
    out.set("segment", segment(inst, "你好"));
    out
}

/// `stored_header_total`: user.bin's stored total replaced by `header` before a reopen.
fn stored_header_total(k: &mut Kit, header: u32) -> J {
    let mut out = obj! { "added" => import_rows(k, 7, &[("你好", "ni3'hao3", 5)]) };
    out.set("saved", facade::save(k.ctx()));
    facade::fini(k.ctx());
    k.forget();
    let path = k.user.join("user.bin");
    let mut payload = std::fs::read(&path).expect("user.bin")[8..].to_vec();
    out.set(
        "stored",
        u32::from_le_bytes(payload[..4].try_into().expect("a word")),
    );
    payload[..4].copy_from_slice(&header.to_le_bytes());
    std::fs::write(&path, chunk(&payload)).expect("write user.bin");
    write_user_conf(k);
    let inst = k.inst();
    guess_out(&mut out, inst, "ni3'hao3");
    out.set("segment", segment(inst, "你好"));
    out.set("import", import_rows(k, 7, &[("好你", "hao3'ni3", 2)]));
    out.set("resaved", facade::save(k.ctx()));
    let saved = std::fs::read(k.user.join("user.bin")).expect("user.bin");
    out.set(
        "header",
        u32::from_le_bytes(saved[8..12].try_into().expect("a word")),
    );
    out
}

/// `system_add_reload`: 龘龘 imported into library 1 at `count`, saved and reopened.
fn system_add_reload(k: &mut Kit, count: i32) -> J {
    let mut out = obj! { "added" => import_rows(k, 1, &[("龘龘", "da3'da3", count)]) };
    out.set("saved", facade::save(k.ctx()));
    facade::fini(k.ctx());
    k.forget();
    let inst = k.inst();
    guess_out(&mut out, inst, "ni3'hao3");
    out.set("segment", segment(inst, "你好"));
    out
}

/// `bumped_item_guess`: five million added to library 1's first 号 token.
fn bumped_item_guess(k: &mut Kit) -> J {
    let inst = k.inst();
    let token = lookup(inst, "号".as_bytes())
        .into_iter()
        .find(|token| token >> 24 == 1)
        .expect("a library 1 token");
    let mut out = obj! { "before" => unigram(inst, token) };
    out.set(
        "add",
        facade::token_add_unigram_frequency(inst, token, 5_000_000),
    );
    out.set("after", unigram(inst, token));
    let J::M(rest) = facade_total_out(inst) else {
        unreachable!()
    };
    for (key, value) in rest {
        out.set(key, value);
    }
    out
}

/// `longer_choices_guess`: up to two thousand LONGER choices for `ni`.
fn longer_choices_guess(k: &mut Kit) -> J {
    let inst = k.inst();
    facade::parse_more_full_pinyins(inst, c("ni").as_ptr());
    let mut chosen = 0_u32;
    for _ in 0..2000 {
        facade::guess_candidates(inst, 0, 0);
        let Some(cand) = row_of(inst, 7) else { break };
        if cand.is_null() || facade::choose_candidate(inst, 0, cand) != 1 {
            break;
        }
        chosen += 1;
    }
    let mut out = obj! { "chosen" => chosen };
    for text in ["gennimen", "nimen"] {
        let fresh = k.alloc();
        facade::parse_more_full_pinyins(fresh, c(text).as_ptr());
        facade::guess_sentence(fresh);
        let sentences = final_step_sentences(fresh);
        let mut rows = guess_rows(fresh, 0, 0x1e).rows;
        rows.truncate(6);
        out.set(text, (sentences, rows));
    }
    let inst = k.alloc();
    out.set("segment", segment(inst, "跟你们"));
    out
}

/// `unload_accepted_bump`: a bump on a GBK token, before unload and after reload.
fn unload_accepted_bump(k: &mut Kit) -> J {
    let token = 0x0200_0001;
    let gbk = std::fs::read(k.data.join("gbk_char.bin")).expect("gbk_char.bin");
    let inst = k.inst();
    let mut out = obj! {
        "gbk" => u32::from_le_bytes(gbk[8..12].try_into().expect("a word")),
        "before" => unigram(inst, token),
    };
    let delta = u32::try_from(((1 << 32) + 2_000_000 - FACADE_TOTAL) % (1 << 32)).expect("delta");
    out.set(
        "add",
        facade::token_add_unigram_frequency(inst, token, delta),
    );
    out.set("loaded", facade_total_out(inst));
    out.set("unload", facade::unload_phrase_library(k.ctx(), 2));
    let inst = k.alloc();
    out.set("unloaded", facade_total_out(inst));
    out.set("load", facade::load_phrase_library(k.ctx(), 2));
    let inst = k.alloc();
    out.set("reloaded", facade_total_out(inst));
    out.set("after", unigram(inst, token));
    out
}

/// `conf_system`'s `('re', 'lambda parameter:[^\n]*', 'lambda parameter:0')`
/// and three dropped libraries, as the kit's system data.
fn lambda_zero_system(k: &mut Kit) {
    let conf = std::fs::read_to_string(k.data.join("table.conf")).expect("table.conf");
    let mut lambdas: Vec<&str> = conf
        .lines()
        .filter_map(|line| line.find("lambda parameter:").map(|at| &line[at..]))
        .collect();
    assert!(!lambdas.is_empty());
    lambdas.sort_unstable();
    lambdas.dedup();
    let mut edits: Vec<ConfEdit<'_>> = lambdas
        .iter()
        .map(|lambda| ConfEdit::Replace(lambda, "lambda parameter:0"))
        .collect();
    edits.extend([
        ConfEdit::Drop("default GBK_DICTIONARY"),
        ConfEdit::Drop("default OPENGRAM_DICTIONARY"),
        ConfEdit::Drop("default MERGED_DICTIONARY"),
    ]);
    let system = conf_system(k, &edits);
    // `private_system` names its directory alone; `items_system` makes another.
    let moved = k.scratch.join("sys-Xconf");
    std::fs::rename(&system, &moved).expect("move the edited system");
    k.data = moved;
}

/// `nonwrapping_denominator_candidates`: an unrelated add makes two candidates tie.
fn denominator_candidates(k: &mut Kit) -> J {
    lambda_zero_system(k);
    let total = 1_u32 << 24;
    items_system(
        k,
        &[
            (&[0x7532], &[0x1081], 1),
            (&[0x4e59], &[0x1081], 2),
            (&[0x4e19], &[0x1084], total - 3),
        ],
        total,
        false,
    );
    for name in [
        "addon_pinyin_index.bin",
        "addon_phrase_index.bin",
        "bigram.db",
    ] {
        let path = k.data.join(name);
        std::fs::remove_file(&path).expect("unlink a system file");
        if name == "bigram.db" {
            craft_hash(&path, &[]);
        } else {
            craft_tree(&path, &[]);
        }
    }
    assert!(facade::set_options(k.ctx(), 1 << 5));
    let inst = k.inst();
    assert_eq!(facade::parse_more_full_pinyins(inst, c("ba1").as_ptr()), 3);
    let rows = |inst: P| {
        let guess = guess_rows(inst, 0, 0x13);
        obj! { "ret" => guess.ret, "rows" => guess.rows }
    };
    let before = rows(inst);
    let added = facade::token_add_unigram_frequency(inst, 0x0100_0003, 3 * total);
    obj! { "before" => before, "add" => added, "after" => rows(inst) }
}

/// Runs the case `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    let wrap = 1_u64 << 32;
    match id {
        "import-wrapped-facade-total-guess" => import_facade_total(k, wrap + 51_050_831),
        "import-wrapped-facade-total-segment" => import_facade_total(k, wrap + 2_000_000),
        "import-facade-total-below-wrap" => import_facade_total(k, wrap - 1),
        "stored-header-facade-total" => stored_header_total(
            k,
            u32::try_from(wrap + 2_000_000 - FACADE_TOTAL).expect("a header"),
        ),
        "stored-header-facade-total-small" => stored_header_total(k, 7),
        "system-add-reload-facade-total" => system_add_reload(
            k,
            i32::try_from((wrap + 2_000_003 - FACADE_TOTAL) / 6).expect("a count"),
        ),
        "system-add-reload-facade-total-small" => system_add_reload(k, 5),
        "token-add-unigram-numerator-guess" => bumped_item_guess(k),
        "longer-choices-numerator-guess" => longer_choices_guess(k),
        "unload-drops-accepted-bump" => unload_accepted_bump(k),
        "token-add-unigram-denominator-candidates" => denominator_candidates(k),
        other => panic!("no case {other}"),
    }
}
