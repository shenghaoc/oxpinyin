//! #851: a journal REMOVE of a system item takes it out of the session. A
//! system library's `.dbin` is merged at init by `SubPhraseIndex::merge`
//! (`phrase_index.cpp:439-517` at 074a2219); REMOVE (`:460-475`) calls
//! `remove_phrase_item` (`:219-236`), which zeroes the item's offset and takes its
//! unigram off the library total. From then on `get_phrase_item` fails for the
//! token (`:180-206`): the item is not readable, a unigram add is refused, an
//! export skips it and a save diffs the removal out as a REMOVE record under a
//! lowered header, while the phrase table still holds the token.
//! `_load_phrase_library` ignores `merge`'s answer (`pinyin.cpp:276`).
//!
//! The probe crafts the `.dbin` before the only init, then observes the state
//! after replay: the touched items through the API, a trailing ADD (whether a
//! later record applied), and the `gb_char.dbin` a following `save` diffs out of
//! the visible items and the library total (`phrase_index.cpp:392-436`), whose
//! header carries the total the replay left. With `reopen` it then closes the
//! context and opens the saved profile again, which replays what the save wrote.
//!
//! #850: journal replay compares and sizes items as the pin does. `merge`
//! compares a record's old chunk with the visible item by `PhraseItem::operator==`
//! (equal chunk sizes, then `memcmp`: `phrase_index.h:235-240`); the visible
//! item's length comes from its header, `6 + 4L + n(2L + 4)` (`get_phrase_item`,
//! `phrase_index.cpp:180-206`), not from the stored payload; `add_phrase_item`
//! stores the whole payload, trailing bytes included (`:208-217`). REMOVE deletes
//! the item before it compares (`:460-475`), so a mismatch returns false with
//! the removal kept. MODIFY compares first (`:477-483`); a chunk longer than the
//! visible item is a remove plus an add of the whole new payload, otherwise it
//! is `memmove`d over the stored bytes (`:486-500`). The first failure ends the
//! merge and the records before it stay applied (`pinyin.cpp:276` ignores the
//! answer).

use super::{
    Library, add_record, export, facade_total, flip, hex, import_nihao, item_text, item_with,
    log_walk, merge, modify_record, system_items, token_phrase_of, unigram_j, user_file,
};
use crate::facade;
use crate::fixtures::{modify_header, write_log, write_user_conf};
use crate::readers::tokens_of;
use crate::support::{J, Kit};

/// A token and its item bytes.
type Item = (u32, Vec<u8>);

/// The tokens a replay watches, by name.
type Watched = Vec<(&'static str, u32)>;

/// What a builder answers: the records after the header and the watched tokens.
type Built = (Vec<Vec<u8>>, Watched);

/// What `system_replay` hands a builder: library 1's total, `one`, `two` (an
/// item of two or more readings) and `last` (the item at the highest
/// item-area offset), and `fresh` and `absent`, tokens it does not hold: one
/// for a trailing ADD, one no record adds.
struct Params<'a> {
    total: u32,
    one: Item,
    two: Item,
    last: Item,
    fresh: Item,
    absent: u32,
    library: &'a Library,
}

/// The observation options of `system_replay`.
#[derive(Clone, Copy)]
struct Opts {
    reopen: bool,
    unigram_add: bool,
    observe_add: bool,
    observe: Option<fn(&mut Kit) -> J>,
    observe_saved: bool,
}

/// The default probe: the unigram add, no reopen.
const ADD: Opts = Opts {
    reopen: false,
    unigram_add: true,
    observe_add: false,
    observe: None,
    observe_saved: false,
};

/// `_compare_case`: the probe without its unigram add: a live item's added
/// delta is a different path than the compare and sizing these cases hold.
const COMPARE: Opts = Opts {
    unigram_add: false,
    ..ADD
};

impl Opts {
    const fn reopen(self) -> Self {
        Self {
            reopen: true,
            ..self
        }
    }
}

/// The watched items' phrase and unigram outputs, the phrase table's tokens
/// of `phrase` and library one's export of it.
fn observe_system_item(k: &mut Kit, watched: &Watched, phrase: &str) -> J {
    let mut out = J::map();
    for (name, token) in watched {
        let observed = J::L(vec![token_phrase_of(k, *token), unigram_j(k, *token)]);
        out.set(*name, observed);
    }
    // The phrase table keeps the token whatever the sub-index holds.
    out.set("phrase-table", tokens_of(k, phrase));
    out.set("export", export(k, 1, Some(phrase), false));
    out
}

fn watched_token(watched: &Watched, name: &str) -> u32 {
    watched
        .iter()
        .find(|(key, _)| *key == name)
        .expect("a watched token")
        .1
}

/// `system_replay`: plants the records `build` answers after a header that
/// keeps library 1's total, opens the profile and observes it.
fn system_replay(k: &mut Kit, build: &dyn Fn(&Params<'_>) -> Built, opts: Opts) -> J {
    let library = system_items(&k.data, "gb_char.bin");
    let one = library.first(|item| item[0] == 1 && item[1] == 1);
    let two = library.first(|item| item[0] == 1 && item[1] >= 2);
    let last = library.by_offset().last().expect("an item").0;
    let fresh = library.max_slot() + 2;
    let item = |slot: u32| (0x0100_0000 | slot, library.items[&slot].clone());
    let params = Params {
        total: library.total,
        one: item(one),
        two: item(two),
        last: item(last),
        fresh: (
            0x0100_0000 | fresh,
            item_with(&library.items[&one], Some(11), None),
        ),
        absent: 0x0100_0000 | (fresh + 3),
        library: &library,
    };
    let (records, watched) = build(&params);
    let phrase = item_text(&library.items[&one]).expect("the item's text");
    write_user_conf(k);
    let mut log = modify_header(0, library.total, library.total);
    log.extend(records.concat());
    write_log(k, &log, "gb_char.dbin");
    let mut out = obj! { "total" => library.total, "ctx" => !k.ctx().is_null() };
    merge(&mut out, observe_system_item(k, &watched, &phrase));
    if let Some(observe) = opts.observe {
        out.set("detail", observe(k));
    }
    if opts.unigram_add {
        // A unigram add reaches only an item the sub-index holds.
        let token = watched_token(&watched, "one");
        out.set(
            "add-unigram",
            facade::token_add_unigram_frequency(k.inst(), token, 7),
        );
    }
    if opts.observe_add {
        out.set("after-add", observe_system_item(k, &watched, &phrase));
        if !facade::ZHUYIN {
            out.set("facade-after-add", facade_total(k));
        }
    }
    // An import marks the context modified, so the save writes the diff.
    assert!(import_nihao(k));
    out.set("save", facade::save(k.ctx()));
    let dbin = user_file(k, "gb_char.dbin");
    out.set("dbin", hex(&dbin));
    if opts.observe_saved {
        let (header, counts) = log_walk(&dbin);
        out.set("saved-header", header);
        out.set("saved-counts", counts);
    }
    if opts.reopen {
        k.close();
        out.set("reopen", observe_system_item(k, &watched, &phrase));
        if let Some(observe) = opts.observe {
            out.set("reopen-detail", observe(k));
        }
    }
    out
}

/// The standard watched tokens: the existing `one` and the `fresh` one.
fn watch(p: &Params<'_>) -> Watched {
    vec![("one", p.one.0), ("fresh", p.fresh.0)]
}

fn watch_two(p: &Params<'_>) -> Watched {
    let mut watched = watch(p);
    watched.push(("two", p.two.0));
    watched
}

fn watch_last(p: &Params<'_>) -> Watched {
    let mut watched = watch(p);
    watched.push(("last", p.last.0));
    watched
}

/// An ADD of the fresh token and item bytes.
fn add_fresh(p: &Params<'_>) -> Vec<u8> {
    add_record(p.fresh.0, &p.fresh.1)
}

/// `one`'s MODIFY to `item_with(one, unigram, freq)`.
fn modify_one(p: &Params<'_>, unigram: Option<u32>, freq: Option<u32>) -> Vec<u8> {
    modify_record(p.one.0, &p.one.1, &item_with(&p.one.1, unigram, freq))
}

/// Runs the `_replay_case` named `name` (without its `replay-` prefix and mode).
pub(super) fn case(k: &mut Kit, name: &str) -> J {
    match name {
        "modify-header-overlong" => system_replay(
            k,
            &|p| {
                (
                    vec![
                        modify_record(p.last.0, &p.last.1, &overlong()),
                        add_fresh(p),
                    ],
                    watch_last(p),
                )
            },
            COMPARE,
        ),
        "touched-duplicate-invalid" => system_replay(
            k,
            &|p| {
                let changed = item_with(&p.last.1, None, Some(77));
                (
                    vec![
                        modify_record(p.last.0, &p.last.1, &changed),
                        modify_record(p.last.0, &changed, &overlong()),
                        add_fresh(p),
                    ],
                    watch_last(p),
                )
            },
            COMPARE,
        ),
        "short-add-five-bytes" => short_add(k, &[1, 1, 7, 0, 0]),
        "stop-before-header-token-modify" => {
            stop_before_header_token(k, &|p| modify_record(p.two.0, &flip(&p.two.1), &p.two.1))
        }
        "unigram-add-live-item-reopen" => system_replay(k, &|p| (vec![], watch(p)), ADD.reopen()),
        other => panic!("no replay case {other}"),
    }
}

/// A six-byte item whose header claims 255 characters and 255 readings.
fn overlong() -> Vec<u8> {
    [&[255_u8, 255][..], &200_u32.to_le_bytes()].concat()
}

/// An ADD payload shorter than the six bytes `add_phrase_item` reads its
/// unigram from: `PhraseItem::get_unigram_frequency` (`phrase_index.h:124-126`)
/// reads four bytes at offset 2 through `MemoryChunk::get_content<guint32>`
/// (`memory_chunk.h:388-392`), whose `check_result` is an `assert` in the oracle
/// build, so `pinyin_init`/`zhuyin_init` dies of SIGABRT (row 108). oxpinyin's
/// open stops at the record and the C adapters abort (member 5a).
///
/// A valid MODIFY of `one`, the short ADD, then a valid fresh ADD.
fn short_add(k: &mut Kit, payload: &[u8]) -> J {
    let mut out = system_replay(
        k,
        &|p| {
            (
                vec![
                    modify_one(p, None, Some(77)),
                    add_record(p.fresh.0, payload),
                    add_fresh(p),
                ],
                watch(p),
            )
        },
        COMPARE,
    );
    let ctx = out.get("ctx").expect("ctx").clone();
    out.set("ret", ctx);
    out
}

/// Row 91's replay order: `merge` returns at a mismatched REMOVE or MODIFY
/// (`phrase_index.cpp:468-471`, `:483-484`) before `next_record` reads the
/// non-null-token `MODIFY_HEADER` behind it, so init completes with the
/// records before the stop applied (a REMOVE's removal included) and the
/// assert is never reached.
///
/// A valid MODIFY of `one`, the stopping record `stop(p)`, the bad header,
/// then a fresh ADD the merge never reaches.
fn stop_before_header_token(k: &mut Kit, stop: &dyn Fn(&Params<'_>) -> Vec<u8>) -> J {
    system_replay(
        k,
        &|p| {
            (
                vec![
                    modify_one(p, None, Some(77)),
                    stop(p),
                    modify_header(0x0100_0001, 1, 1),
                    add_fresh(p),
                ],
                watch_two(p),
            )
        },
        COMPARE,
    )
}
