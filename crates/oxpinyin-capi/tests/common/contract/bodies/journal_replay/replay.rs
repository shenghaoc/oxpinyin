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
    Library, add_record, export, facade_total, flip, hex, import_nihao, item_size, item_text,
    item_with, log_walk, merge, modify_record, phrase_text, reload_pron_keys, remove_record,
    system_items, token_phrase_of, unigram_j, user_file,
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

fn watch_absent(p: &Params<'_>) -> Watched {
    let mut watched = watch(p);
    watched.push(("absent", p.absent));
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
        // #853: accepted live-item increments survive a later dirty save.
        "unigram-add-live-item" => system_replay(k, &|p| (vec![], watch(p)), ADD),
        "unigram-add-live-item-reopen" => system_replay(k, &|p| (vec![], watch(p)), ADD.reopen()),
        "unigram-add-live-item-control" => system_replay(k, &|p| (vec![], watch(p)), COMPARE),
        "unigram-add-overridden-item-reopen" => system_replay(
            k,
            &|p| (vec![modify_one(p, Some(19), None)], watch(p)),
            ADD.reopen(),
        ),
        // #853/#876 overlap: unigram ownership must not erase a replayed reading's
        // count, whether the sub-index accepts or refuses the increment. The facade
        // still takes the refused delta before dispatch (phrase_index.h:628-636).
        "unigram-add-changed-reading-reopen" => system_replay(
            k,
            &|p| (vec![modify_one(p, Some(19), Some(250))], watch(p)),
            Opts {
                observe_add: true,
                ..ADD.reopen()
            },
        ),
        "unigram-refused-changed-reading-reopen" => system_replay(
            k,
            &|p| {
                (
                    vec![
                        modify_one(p, None, Some(250)),
                        modify_header(0, p.total, 0xffff_fffc),
                    ],
                    watch(p),
                )
            },
            Opts {
                observe_add: true,
                ..ADD.reopen()
            },
        ),
        // A matching REMOVE, then an ADD: the item is gone, the total is lower and the
        // ADD after it applied.
        "unigram-add-appended-item-reopen" => system_replay(
            k,
            &|p| {
                (
                    vec![add_fresh(p)],
                    vec![("one", p.fresh.0), ("original", p.one.0)],
                )
            },
            ADD.reopen(),
        ),
        "matched-remove" => system_replay(k, &matched_remove, ADD),
        // The same, saved and opened again: the REMOVE the save wrote replays to the
        // same state.
        "matched-remove-reopen" => system_replay(k, &matched_remove, ADD.reopen()),
        "touched-order-duplicates-reopen" => system_replay(k, &touched_order, COMPARE.reopen()),
        "seed-mixed-repeated-unigrams-reopen" => system_replay(k, &seed_mixed, COMPARE.reopen()),
        "seed-restored-unigram-reopen" => system_replay(
            k,
            &|p| {
                let restored = item_with(&p.one.1, Some(99), None);
                (
                    vec![
                        modify_one(p, Some(99), None),
                        modify_record(p.one.0, &restored, &item_with(&p.one.1, None, Some(77))),
                    ],
                    watch(p),
                )
            },
            COMPARE.reopen(),
        ),
        "seed-add-remove-unigrams-reopen" => system_replay(
            k,
            &|p| {
                (
                    vec![
                        add_fresh(p),
                        remove_record(p.one.0, &p.one.1),
                        modify_record(p.two.0, &p.two.1, &item_with(&p.two.1, Some(91), None)),
                    ],
                    watch_two(p),
                )
            },
            COMPARE.reopen(),
        ),
        "seed-header-total-reopen" => system_replay(
            k,
            &|p| {
                (
                    vec![
                        modify_one(p, None, Some(77)),
                        modify_header(0, p.total, 123_456),
                    ],
                    watch(p),
                )
            },
            COMPARE.reopen(),
        ),
        "seed-stale-storage-unigram-reopen" => system_replay(
            k,
            &|p| {
                let changed = item_with(&p.one.1, None, Some(77));
                (
                    vec![
                        modify_one(p, None, Some(77)),
                        modify_record(p.absent, &changed, &item_with(&p.one.1, Some(99), Some(77))),
                    ],
                    watch(p),
                )
            },
            COMPARE.reopen(),
        ),
        "touched-sparse-add-remove-relocate-reopen" => {
            system_replay(k, &touched_sparse_edits, COMPARE.reopen())
        }
        // REMOVE whose old chunk carries a trailing byte: the sizes differ, the item
        // is gone already and the merge stops before the ADD.
        "remove-trailing-old" => system_replay(
            k,
            &|p| {
                let old = [&p.one.1[..], &[0xaa]].concat();
                (vec![remove_record(p.one.0, &old), add_fresh(p)], watch(p))
            },
            COMPARE,
        ),
        // MODIFY whose old chunk carries a trailing byte: the compare refuses it and
        // nothing is changed.
        "modify-trailing-old" => system_replay(
            k,
            &|p| {
                let old = [&p.one.1[..], &[0xaa]].concat();
                (
                    vec![
                        modify_record(p.one.0, &old, &item_with(&p.one.1, None, Some(77))),
                        add_fresh(p),
                    ],
                    watch(p),
                )
            },
            COMPARE,
        ),
        "modify-trailing-new-grow" => system_replay(k, &|p| modify_grow(p, false), COMPARE),
        "modify-trailing-new-grow-stored-old" => {
            system_replay(k, &|p| modify_grow(p, true), COMPARE)
        }
        "modify-trailing-new-in-place" => system_replay(k, &|p| modify_in_place(p, false), COMPARE),
        "modify-trailing-new-in-place-stored-old" => {
            system_replay(k, &|p| modify_in_place(p, true), COMPARE)
        }
        "modify-empty-phrase" => system_replay(k, &modify_empty, COMPARE),
        // A mismatch ends the merge: the record before it applied, the one after it
        // did not.
        "mismatch-then-valid" => system_replay(
            k,
            &|p| {
                let (token, item) = &p.two;
                (
                    vec![
                        modify_record(*token, item, &item_with(item, None, Some(21))),
                        modify_record(
                            p.one.0,
                            &flip(&p.one.1),
                            &item_with(&p.one.1, None, Some(33)),
                        ),
                        modify_record(
                            *token,
                            &item_with(item, None, Some(21)),
                            &item_with(item, None, Some(22)),
                        ),
                        add_fresh(p),
                    ],
                    watch(p),
                )
            },
            COMPARE,
        ),
        // A REMOVE whose old chunk has the right size and the wrong bytes: the item
        // and its unigram are gone from the total already, and the merge stops.
        "failed-remove" => system_replay(
            k,
            &|p| {
                (
                    vec![remove_record(p.one.0, &flip(&p.one.1)), add_fresh(p)],
                    watch(p),
                )
            },
            COMPARE,
        ),
        // A record for a token the library does not hold. `get_phrase_item` fails
        // (`ERROR_OUT_OF_RANGE`, `phrase_index.cpp:187-189`) and `merge` goes on with
        // what its result leaves behind: REMOVE dereferences the NULL `tmpitem`
        // (`:468`, `remove_phrase_item` `:219-224` leaves it unset); MODIFY ignores
        // the failure and compares a stale `item` (`:478-483`), the default one with no
        // earlier MODIFY, else the last item a MODIFY fetched.
        //
        // The pin dies of SIGSEGV at init; the subject opens, goes on past the
        // record and answers the library as it holds it.
        "remove-absent" => system_replay(
            k,
            &|p| {
                (
                    vec![remove_record(p.absent, &p.one.1), add_fresh(p)],
                    watch_absent(p),
                )
            },
            COMPARE,
        ),
        "modify-absent" => system_replay(
            k,
            &|p| {
                (
                    vec![
                        modify_record(p.absent, &p.one.1, &item_with(&p.one.1, None, Some(77))),
                        add_fresh(p),
                    ],
                    watch_absent(p),
                )
            },
            COMPARE,
        ),
        "modify-absent-stale" => system_replay(k, &|p| modify_absent_stale(p, false), COMPARE),
        // A MODIFY's new chunk is not checked against its own header: only the old
        // chunk is compared (`phrase_index.cpp:481-483`), and a new chunk no longer
        // than the visible item is `memmove`d over the stored bytes (`:493-500`). The
        // stored item keeps its length, so its header still names the whole item.
        "modify-short-in-place" => system_replay(
            k,
            &|p| {
                let new = [&p.one.1[..2], &200_u32.to_le_bytes()].concat();
                (
                    vec![modify_record(p.one.0, &p.one.1, &new), add_fresh(p)],
                    watch(p),
                )
            },
            COMPARE,
        ),
        "modify-absent-default" => system_replay(k, &|p| modify_absent_default(p, false), COMPARE),
        // A new chunk longer than the six-byte default reaches the NULL of REMOVE's
        // assert (`:486-491`): the pin dies of SIGSEGV, the subject stops at the record.
        "modify-absent-default-grow" => system_replay(
            k,
            &|p| {
                let new = [&[0_u8, 0][..], &50_u32.to_le_bytes(), &[0]].concat();
                (
                    vec![modify_record(p.absent, &ZERO_ITEM, &new), add_fresh(p)],
                    watch_absent(p),
                )
            },
            COMPARE,
        ),
        // A small over-long header on a non-last item: the compare passes, the
        // six-byte chunk is written in place, and the next read takes the twenty-two
        // bytes its header names, six of them from the following item. That read ends
        // inside the content buffer, so the pin's answer is reproduced.
        "modify-header-overlong-small" => system_replay(
            k,
            &|p| {
                let new = [&[2_u8, 1][..], &200_u32.to_le_bytes()].concat();
                (
                    vec![modify_record(p.one.0, &p.one.1, &new), add_fresh(p)],
                    watch(p),
                )
            },
            COMPARE,
        ),
        "cross-item-unigram-unrecorded" => system_replay(k, &cross_item_records, CROSS),
        "cross-item-unigram-unrecorded-reopen" => {
            system_replay(k, &cross_item_records, CROSS.reopen())
        }
        "cross-item-unigram-unrecorded-past-area" => {
            system_replay(k, &cross_item_records_past_area, CROSS)
        }
        // The pin does not check a new chunk's header against its size: the compare is
        // on the old chunk only. A stored item whose header then names more bytes than
        // were written is read past by a later `get_phrase_item` (class (b); row 107).
        // The last item of the item area sits at the end of the pin's `m_phrase_content`
        // copy once the trailing ADD has appended: a 132,096-byte header (255, 255)
        // reads past the item, the copy and its allocation, and the pin dies of
        // SIGSEGV at the first read. The subject keeps its safe answer: it applies the
        // write and the ADD, and the over-read item is unreadable at the finish.
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
        // Same row-107 refusal after a duplicate touch. The pin still over-reads the
        // last slot; the safe finish reports it once and keeps that slot's baseline.
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
        "touched-many-invalid" => touched_many_invalid(k),
        "short-add-five-bytes" => short_add(k, &[1, 1, 7, 0, 0]),
        "over-read-tail" => over_read_tail_probe(k),
        "stop-before-header-token-remove" => {
            stop_before_header_token(k, &|p| remove_record(p.two.0, &flip(&p.two.1)))
        }
        "stop-before-header-token-modify" => {
            stop_before_header_token(k, &|p| modify_record(p.two.0, &flip(&p.two.1), &p.two.1))
        }
        other => panic!("no replay case {other}"),
    }
}

/// A matching REMOVE followed by a fresh ADD.
fn matched_remove(p: &Params<'_>) -> Built {
    (
        vec![remove_record(p.one.0, &p.one.1), add_fresh(p)],
        watch(p),
    )
}

/// #870: merge walks the journal in record order, while a later diff walks
/// ascending slots (074a2219 phrase_index.cpp:392-436,439-517). Repeated,
/// out-of-order writes leave each slot's final value, including after reopen.
///
/// Descending-token MODIFY records and a repeated update to the lower token:
/// last pronunciation frequencies 77 then 99.
fn touched_order(p: &Params<'_>) -> Built {
    let mut rows = [&p.one, &p.two];
    rows.sort_by_key(|row| std::cmp::Reverse(row.0));
    let mut records: Vec<Vec<u8>> = rows
        .iter()
        .map(|(token, item)| modify_record(*token, item, &item_with(item, None, Some(77))))
        .collect();
    let (token, item) = rows[1];
    records.push(modify_record(
        *token,
        &item_with(item, None, Some(77)),
        &item_with(item, None, Some(99)),
    ));
    (records, watch_two(p))
}

/// #870: unigram candidates are per slot, not a whole-library zero-change
/// proof. Counts stay absolute in the pin's mapped items (phrase_index.h:124-126).
///
/// 77 for one's last frequency and 71 then 93 for two's unigram.
fn seed_mixed(p: &Params<'_>) -> Built {
    let (token, item) = &p.two;
    (
        vec![
            modify_one(p, None, Some(77)),
            modify_record(*token, item, &item_with(item, Some(71), None)),
            modify_record(
                *token,
                &item_with(item, Some(71), None),
                &item_with(item, Some(93), None),
            ),
        ],
        watch_two(p),
    )
}

/// Relocation, replacement and sparse-slot add/remove records in a fixed
/// sequence, with the high token 0x01040000 (sparse slot 262144).
fn touched_sparse_edits(p: &Params<'_>) -> Built {
    let (token, item) = &p.one;
    let token = *token;
    let moved = item_with(item, None, Some(77));
    let high = 0x0104_0000;
    let records = vec![
        modify_record(token, item, &[&moved[..], &[1, 2, 3]].concat()),
        modify_record(token, &moved, &item_with(item, None, Some(99))),
        add_record(high, &p.fresh.1),
        remove_record(token, &item_with(item, None, Some(99))),
        add_record(token, item),
        remove_record(high, &p.fresh.1),
        add_record(high, &item_with(&p.fresh.1, None, Some(55))),
    ];
    let mut watched = watch(p);
    watched.push(("high", high));
    (records, watched)
}

/// MODIFY whose new chunk is longer than the visible item by trailing bytes:
/// the growing path stores all of it, and the item is still its header's
/// length, so the next record compares against the header-length bytes, or
/// with `stored_old` against those trailing bytes too.
fn modify_grow(p: &Params<'_>, stored_old: bool) -> Built {
    let (token, item) = &p.one;
    let new = item_with(item, Some(u32::from(item[2]) + 500), Some(5));
    let trailing: &[u8] = &[1, 2, 3];
    // The second record's old chunk is the item at its header length, or
    // with the trailing bytes the first record stored.
    let second = item_with(&new, None, Some(9));
    let old = [&new[..], if stored_old { trailing } else { &[] }].concat();
    (
        vec![
            modify_record(*token, item, &[&new[..], trailing].concat()),
            modify_record(*token, &old, &second),
            add_fresh(p),
        ],
        watch(p),
    )
}

/// The in-place path: a new chunk with a pronunciation fewer and trailing bytes
/// that still fits the old size is `memmove`d over the item. Its header now
/// gives a shorter item, so the second record compares against that, and not
/// against the bytes the memmove left behind it (`stored_old` compares them).
fn modify_in_place(p: &Params<'_>, stored_old: bool) -> Built {
    let (token, item) = &p.two;
    let new = [&[item[0], item[1] - 1][..], &item[2..item.len() - 6]].concat();
    assert_eq!(item_size(&new), item.len() - 6);
    let trailing: &[u8] = &[1, 2, 3, 4];
    let second = item_with(&new, None, Some(9));
    let old = [&new[..], if stored_old { trailing } else { &[] }].concat();
    (
        vec![
            modify_record(*token, item, &[&new[..], trailing].concat()),
            modify_record(*token, &old, &second),
            add_fresh(p),
        ],
        watch(p),
    )
}

/// An empty phrase is an item of `6 + 4n` bytes. In place, then once more
/// against the shorter item.
fn modify_empty(p: &Params<'_>) -> Built {
    let (token, item) = &p.one;
    let empty = [&[0_u8, 0][..], &item[2..6]].concat();
    (
        vec![
            modify_record(*token, item, &empty),
            modify_record(*token, &empty, &item_with(&empty, Some(3), None)),
            add_fresh(p),
        ],
        watch(p),
    )
}

/// After a MODIFY of `two`, the stale `item` is `two`'s bytes. A MODIFY of the
/// absent token whose old chunk equals them passes the compare: the same-size
/// new chunk is `memmove`d over `two` (`:495-500`), the growing one meets the
/// NULL of REMOVE's assert (`:486-491`).
fn modify_absent_stale(p: &Params<'_>, grow: bool) -> Built {
    let (token, item) = &p.two;
    let first = item_with(item, None, Some(21));
    let new = item_with(&first, None, Some(22));
    let tail: &[u8] = if grow { &[1] } else { &[] };
    (
        vec![
            modify_record(*token, item, &first),
            modify_record(p.absent, &first, &[&new[..], tail].concat()),
            add_fresh(p),
        ],
        watch_absent(p),
    )
}

/// The default `PhraseItem` is six zero bytes (`phrase_index.h:81-84`), not an
/// empty item: a MODIFY of an absent token whose old chunk is those zeros passes
/// the compare with no earlier MODIFY, and a new chunk that fits the six bytes
/// is written into `item`'s own buffer. No entry is created and the merge goes
/// on; a later compare sees what the write left there.
const ZERO_ITEM: [u8; 6] = [0; 6];

/// Absent-token unigram 50 and with `twice` 60, then a fresh ADD.
fn modify_absent_default(p: &Params<'_>, twice: bool) -> Built {
    let first = [&[0_u8, 0][..], &50_u32.to_le_bytes()].concat();
    let mut records = vec![modify_record(p.absent, &ZERO_ITEM, &first)];
    if twice {
        let second = [&[0_u8, 0][..], &60_u32.to_le_bytes()].concat();
        records.push(modify_record(p.absent, &first, &second));
    }
    records.push(add_fresh(p));
    (records, watch_absent(p))
}

/// A six-byte item whose header claims 255 characters and 255 readings.
fn overlong() -> Vec<u8> {
    [&[255_u8, 255][..], &200_u32.to_le_bytes()].concat()
}

/// The cross-item cases observe the detail and the saved journal.
const CROSS: Opts = Opts {
    observe: Some(observe_cross_item),
    observe_saved: true,
    ..COMPARE
};

/// #981: the same borrowed bytes, one record later. A first MODIFY leaves a
/// six-byte payload whose header claims two readings (30 bytes), so slot A's
/// header-derived item spans the adjacent slot B. A second in-place MODIFY of A
/// compares the 30 bytes and rewrites them with B's unigram changed; B has no
/// record of its own. The replay view exposes only the slots it touched
/// (`ReplayedLibrary::contains`), so a reader that falls back to the original
/// item for B answers B's original unigram, while the pin's `get_phrase_item`
/// reads the changed bytes. A is a one-character, one-reading slot so the fixed
/// 30-byte header always spans B; B's item must hold those 14 bytes. In the
/// pinned gb_char.bin the pair is slot 1 (token 0x01000001, 锕, unigram 3) and
/// slot 2 (token 0x01000002, 吖, two readings, unigram 11).
///
/// `_cross_item_pair`: the first adjacent pair whose A is a one-character,
/// one-reading item and whose B holds the 14 bytes A's (2, 2) header borrows.
fn cross_item_pair(library: &Library) -> (u32, Vec<u8>, u32, Vec<u8>) {
    let ordered = library.by_offset();
    for pair in ordered.windows(2) {
        let [(slot_a, offset_a), (slot_b, offset_b)] = [pair[0], pair[1]];
        let item_a = &library.items[&slot_a];
        let item_b = &library.items[&slot_b];
        if item_a[0] != 1 || item_a[1] != 1 || offset_b as usize != offset_a as usize + item_a.len()
        {
            continue;
        }
        // The synthesized second phrase character is A's first pronunciation
        // key plus its frequency's low half; keep it a Unicode scalar so the
        // pin's `g_ucs4_to_utf8` has a value to render.
        let second = u32::from_le_bytes(item_a[10..14].try_into().expect("a word"));
        if second > 0x10FFFF || (0xD800..=0xDFFF).contains(&second) || item_b.len() < 14 {
            continue;
        }
        return (
            0x0100_0000 | slot_a,
            item_a.clone(),
            0x0100_0000 | slot_b,
            item_b.clone(),
        );
    }
    panic!("no adjacent one-reading items the header can span")
}

/// `old` with B's unigram (at `at`) changed to 170, or 171 where it was 170.
fn with_b_unigram(old: &[u8], at: usize) -> Vec<u8> {
    let original = u32::from_le_bytes(old[at..at + 4].try_into().expect("a word"));
    let changed: u32 = if original == 170 { 171 } else { 170 };
    [&old[..at], &changed.to_le_bytes(), &old[at + 4..]].concat()
}

/// A 30-byte claimed A item whose second MODIFY changes B's overlapping unigram.
fn cross_item_records(p: &Params<'_>) -> Built {
    let (token_a, item_a, token_b, item_b) = cross_item_pair(p.library);
    let expanded = [&[2_u8, 2][..], &200_u32.to_le_bytes()].concat();
    let claimed = item_size(&expanded);
    let borrowed = claimed - item_a.len();
    assert!(claimed == 30 && 0 < borrowed && borrowed <= item_b.len());
    // The first in-place write copies only the six-byte payload, so the rest
    // of A's slot keeps its bytes and the read borrows B's first 14.
    let old = [&expanded[..], &item_a[6..], &item_b[..borrowed]].concat();
    assert_eq!(old.len(), claimed);
    let new = with_b_unigram(&old, item_a.len() + 2);
    (
        vec![
            modify_record(token_a, &item_a, &expanded),
            modify_record(token_a, &old, &new),
            add_fresh(p),
        ],
        vec![("a", token_a), ("b", token_b), ("fresh", p.fresh.0)],
    )
}

/// An ADD followed by two A modifications claiming a 38-byte cross-item view:
/// the comparison bytes are A, all of B and a fresh-item prefix.
fn cross_item_records_past_area(p: &Params<'_>) -> Built {
    let (token_a, item_a, token_b, item_b) = cross_item_pair(p.library);
    let expanded = [&[2_u8, 3][..], &200_u32.to_le_bytes()].concat();
    let claimed = item_size(&expanded);
    assert_eq!(claimed, 38);
    let fresh = &p.fresh.1;
    let borrowed = claimed - item_a.len();
    let old = [
        &expanded[..],
        &item_a[6..],
        &item_b,
        &fresh[..borrowed - item_b.len()],
    ]
    .concat();
    assert_eq!(old.len(), claimed);
    let new = with_b_unigram(&old, item_a.len() + 2);
    // The ADD comes first so the last original item B is followed by the fresh
    // item; A's 34-byte header reaches past the original area into it.
    (
        vec![
            add_fresh(p),
            modify_record(token_a, &item_a, &expanded),
            modify_record(token_a, &old, &new),
        ],
        vec![("a", token_a), ("b", token_b), ("fresh", p.fresh.0)],
    )
}

/// A replayed token's phrase, pronunciation keys, unigram and matching export rows.
fn observe_replayed_item(k: &mut Kit, token: u32) -> J {
    let phrase = token_phrase_of(k, token);
    let text = if phrase.items()[0] == J::B(true) {
        phrase_text(&phrase)
    } else {
        None
    };
    let export = match text.as_deref() {
        Some(text) if !text.is_empty() => export(k, 1, Some(text), false),
        _ => J::from("NO PHRASE"),
    };
    obj! {
        "token" => format!("0x{token:08x}"),
        "phrase" => phrase,
        "n-pron" => reload_pron_keys(k, token),
        "unigram" => unigram_j(k, token),
        "export" => export,
    }
}

/// The adjacent fixture pair from the original system bytes, both live tokens observed.
fn observe_cross_item(k: &mut Kit) -> J {
    let library = system_items(&k.data, "gb_char.bin");
    let (token_a, _, token_b, _) = cross_item_pair(&library);
    obj! {
        "a" => observe_replayed_item(k, token_a),
        "b" => observe_replayed_item(k, token_b),
    }
}

/// Many row-107 invalidations, in descending order and each after an earlier
/// valid touch. Finalization must emit each diagnostic once in ascending order;
/// every rejected slot answers no item (row 107, option 1), and the rest stay.
///
/// The last 256 system items by offset, in reverse: each one's last frequency
/// changed, then a six-byte header claiming 255 characters and readings.
fn touched_many_invalid(k: &mut Kit) -> J {
    let library = system_items(&k.data, "gb_char.bin");
    let ordered = library.by_offset();
    let slots: Vec<u32> = ordered[ordered.len().saturating_sub(256)..]
        .iter()
        .map(|&(slot, _)| slot)
        .collect();
    let items = library.items;
    system_replay(
        k,
        &|p| {
            let mut records = Vec::new();
            for slot in slots.iter().rev() {
                let (token, item) = (0x0100_0000 | slot, &items[slot]);
                let changed = item_with(item, None, Some(77));
                records.push(modify_record(token, item, &changed));
                records.push(modify_record(token, &changed, &overlong()));
            }
            records.push(add_fresh(p));
            (records, watch_last(p))
        },
        COMPARE,
    )
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

/// An ADD whose declared six-byte payload runs past the log's end.
fn over_read_add(token: u32) -> Vec<u8> {
    [
        &LOG_ADD_BYTES[..],
        &token.to_le_bytes(),
        &6_u16.to_le_bytes(),
        &[1, 1, 7],
    ]
    .concat()
}

const LOG_ADD_BYTES: [u8; 4] = super::LOG_ADD.to_le_bytes();

/// An over-read tail (class (b)): the walk reaches an ADD whose declared
/// payload runs past the log's end, so the pin's `next_record` would read past
/// its allocation and the pin is not run. The subject applies the records in
/// place, reaches the faulty one and reverses them: the library keeps its
/// pre-replay content, so the MODIFY that came first leaves no trace. Both
/// facades read the changed unigram; pinyin export also reads the changed
/// pronunciation count (zhuyin has no export).
fn over_read_tail_probe(k: &mut Kit) -> J {
    let out = system_replay(
        k,
        &|p| {
            (
                vec![modify_one(p, Some(10), Some(77)), over_read_add(p.absent)],
                watch(p),
            )
        },
        COMPARE,
    );
    let field = |key: &str| out.get(key).expect("a field").clone();
    obj! { "ctx" => field("ctx"), "one" => field("one"), "export" => field("export") }
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
