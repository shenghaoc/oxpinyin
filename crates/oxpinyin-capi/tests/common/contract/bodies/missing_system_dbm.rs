//! The missing_system_dbm family's case bodies: each answers the result document the old case returned.
//!
//! Missing system DBM attachments (#1516): each case removes one table from its
//! own system directory, initialises, exercises and finalises.

use crate::facade;
use crate::fixtures::{private_system, write_user_conf};
use crate::readers::{c, candidate_rows, lookup, owned_out, seeded};
use crate::support::{J, Kit, P};

/// `TABLES`: each table's system file.
fn table_file(table: &str) -> &'static str {
    match table {
        "pinyin" => "pinyin_index.bin",
        "phrase" => "phrase_index.bin",
        "bigram" => "bigram.db",
        "punctuation" => "punct.bin",
        "addon-pinyin" => "addon_pinyin_index.bin",
        "addon-phrase" => "addon_phrase_index.bin",
        other => panic!("unknown table {other}"),
    }
}

/// `sentence_out(k, inst, 0)` / `zhuyin_sentence_out`.
fn sentence_out(inst: P) -> J {
    let mut out = seeded();
    let ret = if facade::ZHUYIN {
        facade::get_sentence_only(inst, &raw mut out)
    } else {
        facade::get_sentence_at(inst, 0, &raw mut out)
    };
    J::from((ret, owned_out(out)))
}

/// `candidate_rows` as the document's rows.
fn rows_j(inst: P) -> J {
    J::from(candidate_rows(inst))
}

/// The pinyin facade's predictions, with and without punctuation, for 我们.
fn predictions(inst: P, out: &mut J) {
    let prefix = c("我们");
    out.set(
        "predicted",
        facade::guess_predicted_candidates(inst, prefix.as_ptr()),
    );
    out.set("predicted_rows", rows_j(inst));
    out.set(
        "punctuation",
        facade::guess_predicted_candidates_with_punctuations(inst, prefix.as_ptr()),
    );
    out.set("punctuation_rows", rows_j(inst));
}

/// `exercise`: read, prediction, learning and save without assuming a hit.
fn exercise(ctx: P, inst: P, out: &mut J) {
    facade::set_options(ctx, (1 << 5) | (1 << 9));
    out.set(
        "parsed",
        facade::parse_more_full_pinyins(inst, c("ni3hao3").as_ptr()),
    );
    out.set("guessed", facade::guess_sentence(inst));
    out.set("sentence", sentence_out(inst));
    out.set("tokens", lookup(inst, "你好".as_bytes()));
    out.set(
        "segmented",
        facade::phrase_segment(inst, c("你好").as_ptr()),
    );
    let candidates = if facade::ZHUYIN {
        facade::guess_candidates_after_cursor(inst, 0)
    } else {
        facade::guess_candidates(inst, 0, 0x1C)
    };
    out.set("candidates", candidates);
    out.set("rows", rows_j(inst));
    let mut length = 99_usize;
    let ret = facade::get_character_offset(inst, c("你好").as_ptr(), 0, &raw mut length);
    out.set("character_offset", (ret, length));
    out.set(
        "prefix_guess",
        facade::guess_sentence_with_prefix(inst, c("你好").as_ptr()),
    );
    if !facade::ZHUYIN {
        predictions(inst, out);
    }
    out.set("train_guess", facade::guess_sentence(inst));
    out.set("trained", facade::train_top(inst));
    out.set("saved", facade::save(ctx));
}

/// The overlay import: 你好 into library 7 with count 100000.
fn overlay(ctx: P, out: &mut J) {
    let it = facade::begin_add_phrases(ctx, 7);
    let reading = if facade::ZHUYIN {
        "ㄋㄧˇ ㄏㄠˇ"
    } else {
        "ni3'hao3"
    };
    out.set(
        "added",
        facade::iterator_add_phrase(it, c("你好").as_ptr(), c(reading).as_ptr(), 100_000),
    );
    facade::end_add_phrases(it);
}

/// `missing_case`: remove exactly one system DBM, initialise and explicitly finalise.
fn missing_case(k: &mut Kit, table: Option<&str>, operations: bool, over: bool, unload: bool) -> J {
    k.data = private_system(k);
    if let Some(table) = table {
        std::fs::remove_file(k.data.join(table_file(table))).expect("unlink the table");
    }
    write_user_conf(k);
    let ctx = k.init_default();
    let mut out = obj! { "init" => !ctx.is_null() };
    if ctx.is_null() {
        return out;
    }
    if unload {
        out.set("unloaded", facade::unload_phrase_library(ctx, 2));
    }
    if over {
        overlay(ctx, &mut out);
    }
    if operations {
        let inst = facade::alloc_instance(ctx);
        assert!(!inst.is_null(), "alloc_instance failed");
        exercise(ctx, inst, &mut out);
        facade::free_instance(inst);
    }
    facade::fini(ctx);
    out.set("fini", true);
    out
}

/// The case `id`'s result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    let name = id
        .strip_prefix("missing-system-dbm/")
        .expect("a missing-system-dbm case");
    let name = name.strip_prefix("bdb-").unwrap_or(name);
    let rest = name
        .strip_prefix("pinyin-")
        .or_else(|| name.strip_prefix("zhuyin-"))
        .expect("a facade prefix");
    if rest == "intact" {
        return missing_case(k, None, true, false, false);
    }
    let (table, kind) = rest.rsplit_once('-').expect("a table and a kind");
    match kind {
        "lifecycle" => missing_case(k, Some(table), false, false, false),
        "operations" => missing_case(k, Some(table), true, false, false),
        "overlay" => missing_case(k, Some(table), true, true, false),
        "unloaded" => missing_case(k, Some(table), true, false, true),
        other => panic!("unknown missing-system-dbm kind {other}"),
    }
}
