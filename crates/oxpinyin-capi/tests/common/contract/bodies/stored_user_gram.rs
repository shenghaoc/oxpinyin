//! The stored_user_gram family's case bodies: each answers the result document the old case returned.
//!
//! Inventory row 50 (#1175, #757, #791): user grams read and written in their
//! stored order, duplicates kept (`ngram.cpp:135-205,252-358`); training
//! against an unsorted gram (`phonetic_lookup.h:888-903`); item-less grams
//! under `mask_out` (`ngram_tkrzwdb.cpp:195-198`) and an item-less user total
//! merged with the system start gram (`ngram.cpp:298-306`).

use std::collections::BTreeMap;

use oxpinyin_store::{DefaultStore, RawReadStore as _};

use crate::facade;
use crate::fixtures::{craft_hash, gram_bytes, write_user_conf};
use crate::readers::{
    c, candidate_at, candidate_text, candidate_type, n_candidate, owned_out, seeded, segment_out,
    tokens_of,
};
use crate::support::{J, Kit, P};

/// One imported phrase: text, pinyin, zhuyin and count.
type Phrase = (&'static str, &'static str, &'static str, i32);

/// `_DALONG`.
const DALONG: Phrase = ("龘龘", "da2'da2", "ㄉㄚˊ ㄉㄚˊ", 5);
/// `_BINGBING`.
const BINGBING: Phrase = ("靐靐", "da3'da3", "ㄉㄚˇ ㄉㄚˇ", 5);

/// The system tokens `_plant_user_grams` hands its gram builder.
type System = BTreeMap<&'static str, u32>;

/// A gram builder: (predecessor, raw value) rows from the system and user tokens.
type Grams<'a> = &'a dyn Fn(&System, &[u32]) -> Vec<(u32, Vec<u8>)>;

/// Runs the case `id` and answers its result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(rest) = id.strip_prefix("mask-out-itemless-gram-untouched-") {
        let (name, label) = rest.split_once("-total-").expect("a total label");
        let total = if label == "zero" { 0 } else { 17 };
        return itemless_gram_untouched(k, name == "remove-candidate", total);
    }
    if id.starts_with("abort-train-unsorted-set-freq-miss-") {
        return train_unsorted_set_freq_miss(k);
    }
    match id {
        "itemless-user-total-phrase-segment" => itemless_user_total(k, None, true),
        "itemless-user-total-guess" => {
            itemless_user_total(k, Some(&["nihaoshijie", "womenshizhongguoren"]), true)
        }
        "itemless-user-total-guess-above-one" => {
            itemless_user_total(k, Some(&["yanjiushengmingqiyuan"]), true)
        }
        "itemless-user-total-guess-above-one-control" => {
            itemless_user_total(k, Some(&["yanjiushengmingqiyuan"]), false)
        }
        _ => {
            let rest = id.strip_prefix("user-gram-").expect("a user-gram case");
            let (kind, operation) = rest.split_once('-').expect("a kind");
            let operation = operation
                .strip_suffix("-pinyin")
                .or_else(|| operation.strip_suffix("-zhuyin"))
                .unwrap_or(operation);
            if operation == "predicted" {
                return user_gram_predicted(k, kind);
            }
            user_gram_layout(k, kind, operation)
        }
    }
}

/// Frees the cached instance and finalises the cached context; both reopen lazily.
fn close(k: &mut Kit) {
    facade::free_instance(k.inst_if_any());
    facade::fini(k.ctx());
    k.forget();
}

/// `_plant_user_grams`: `phrases` added to library 7 and saved, the context
/// closed, then user_bigram.db replaced by `grams`' rows. Answers the system
/// tokens, the phrases' user tokens and the planted keys.
fn plant_user_grams(
    k: &mut Kit,
    phrases: &[Phrase],
    grams: Grams<'_>,
) -> (System, Vec<u32>, Vec<Vec<u8>>) {
    let it = facade::begin_add_phrases(k.ctx(), 7);
    for (text, pinyin, zhuyin, count) in phrases {
        let reading = if facade::ZHUYIN { zhuyin } else { pinyin };
        assert!(facade::iterator_add_phrase(
            it,
            c(text).as_ptr(),
            c(reading).as_ptr(),
            *count
        ));
    }
    facade::end_add_phrases(it);
    let users: Vec<u32> = phrases
        .iter()
        .map(|(text, ..)| {
            tokens_of(k, text)
                .into_iter()
                .find(|token| token >> 24 == 7)
                .expect("a user token")
        })
        .collect();
    let system: System = ["你", "好", "号", "我"]
        .into_iter()
        .map(|text| (text, tokens_of(k, text)[0]))
        .collect();
    assert!(facade::save(k.ctx()));
    close(k);
    let rows: Vec<(Vec<u8>, Vec<u8>)> = grams(&system, &users)
        .into_iter()
        .map(|(token, value)| (token.to_le_bytes().to_vec(), value))
        .collect();
    craft_hash(&k.user.join("user_bigram.db"), &rows);
    write_user_conf(k);
    let keys = rows.into_iter().map(|(key, _)| key).collect();
    (system, users, keys)
}

/// `_gram_items`: a raw SingleGram's total and its items in stored order.
fn gram_items(raw: &[u8]) -> J {
    let word = |at: usize| u32::from_le_bytes(raw[at..at + 4].try_into().expect("a word"));
    let end = raw.len() - (raw.len() - 4) % 8;
    let items: Vec<(u32, u32)> = (4..end)
        .step_by(8)
        .map(|at| (word(at), word(at + 4)))
        .collect();
    J::from((word(0), items))
}

/// `_close_and_read_grams`: closes the cached objects and decodes the stored
/// user grams of `keys`, `None` where a key is absent.
fn close_and_read_grams(k: &mut Kit, keys: &[Vec<u8>]) -> J {
    close(k);
    let store = DefaultStore::open_user_bigram(&k.user.join("user_bigram.db"))
        .expect("the saved bigram opens");
    J::L(
        keys.iter()
            .map(|key| {
                let mut value = Vec::new();
                if store.get_raw(key, &mut value).expect("read the bigram") {
                    gram_items(&value)
                } else {
                    J::Null
                }
            })
            .collect(),
    )
}

/// `_mark_modified_by_unconstrained_train`: an unconstrained `wo` guess trained.
fn mark_modified(k: &mut Kit) -> bool {
    assert!(facade::set_options(k.ctx(), 1 << 5));
    let inst = k.inst();
    assert_eq!(facade::parse_more_full_pinyins(inst, c("wo").as_ptr()), 2);
    assert!(facade::guess_sentence(inst));
    facade::train_top(inst)
}

/// `row_with_text`: the first candidate of `kind` whose text is `text`.
fn row_with_text(inst: P, kind: i32, text: &str) -> Option<P> {
    (0..n_candidate(inst))
        .filter_map(|index| candidate_at(inst, index))
        .find(|&cand| {
            candidate_type(inst, cand) == kind
                && candidate_text(inst, cand).as_deref() == Some(text)
        })
}

/// `_user_candidate_rows`: the NORMAL user candidates of the current list.
fn user_candidate_rows(inst: P) -> Vec<P> {
    (0..n_candidate(inst))
        .map(|index| candidate_at(inst, index).unwrap_or(std::ptr::null_mut()))
        .filter(|&cand| candidate_type(inst, cand) == 2 && facade::is_user_candidate(inst, cand))
        .collect()
}

/// Constrains nihao to 你 and 好, answering each choice.
fn choose_nihao(inst: P) -> Vec<i32> {
    [(0, "你"), (2, "好")]
        .into_iter()
        .map(|(offset, text)| {
            if facade::ZHUYIN {
                assert!(facade::guess_candidates_after_cursor(inst, offset));
            } else {
                assert!(facade::guess_candidates(inst, offset, 0x1e));
            }
            let candidate = row_with_text(inst, 2, text)
                .unwrap_or_else(|| panic!("missing normal candidate: {text}"));
            facade::choose_candidate(inst, offset, candidate)
        })
        .collect()
}

/// The first user candidate of `dada`, removed.
fn remove_dada(k: &mut Kit) -> bool {
    let inst = k.inst();
    assert_eq!(facade::parse_more_full_pinyins(inst, c("dada").as_ptr()), 4);
    assert!(facade::guess_candidates(inst, 0, 0));
    let rows = user_candidate_rows(inst);
    assert!(!rows.is_empty(), "missing removable user candidate");
    facade::remove_user_candidate(inst, rows[0])
}

/// `user_gram_layout`'s gram under 你: unsorted `[龘龘, 号, 好]` or 好 and 龘龘 twice.
fn layout_grams(kind: &str, system: &System, users: &[u32]) -> Vec<(u32, Vec<u8>)> {
    let (hao, hao2, dalong) = (system["好"], system["号"], users[0]);
    let items = if kind == "unsorted" {
        let mut items = vec![(dalong, 90), (hao2, 80), (hao, 70)];
        items.sort_unstable_by(|a, b| b.cmp(a));
        items
    } else {
        vec![(hao, 70), (hao, 80), (dalong, 90), (dalong, 100)]
    };
    let total = items.iter().map(|(_, count)| count).sum();
    vec![(system["你"], gram_bytes(total, &items))]
}

/// `user_gram_layout`: the planted gram, then a train, mask or removal and
/// a save; the stored grams after the close.
fn user_gram_layout(k: &mut Kit, kind: &str, operation: &str) -> J {
    let (system, users, keys) = plant_user_grams(k, &[DALONG], &|system, users| {
        layout_grams(kind, system, users)
    });
    let mut out = J::map();
    match operation {
        "train-save" => {
            assert!(facade::set_options(k.ctx(), 1 << 5));
            let inst = k.inst();
            out.set(
                "parse",
                facade::parse_more_full_pinyins(inst, c("nihao").as_ptr()),
            );
            out.set("guess", facade::guess_sentence(inst));
            out.set("choices", choose_nihao(inst));
            out.set("chosen-guess", facade::guess_sentence(inst));
            out.set("train", facade::train_top(inst));
            out.set("save", facade::save(k.ctx()));
        }
        "mask-save" | "remove-candidate-save" => {
            if operation == "remove-candidate-save" {
                out.set("removed", remove_dada(k));
            } else {
                let value = if kind == "unsorted" {
                    users[0]
                } else {
                    system["好"]
                };
                out.set("masked", facade::mask_out(k.ctx(), 0xffff_ffff, value));
            }
            out.set("trained", mark_modified(k));
            out.set("save", facade::save(k.ctx()));
        }
        other => panic!("no operation {other}"),
    }
    out.set("grams", close_and_read_grams(k, &keys));
    out
}

/// `user_gram_predicted`: two user phrases under 你, unsorted or 龘龘
/// twice, then the predictions after 你.
fn user_gram_predicted(k: &mut Kit, kind: &str) -> J {
    let grams = |system: &System, users: &[u32]| {
        let (dalong, bingbing) = (users[0], users[1]);
        let items = if kind == "unsorted" {
            let mut items = vec![(dalong, 20), (bingbing, 20)];
            items.sort_unstable_by(|a, b| b.cmp(a));
            items
        } else {
            vec![(dalong, 20), (dalong, 3), (bingbing, 20)]
        };
        let total = items.iter().map(|(_, count)| count).sum();
        vec![(system["你"], gram_bytes(total, &items))]
    };
    plant_user_grams(k, &[DALONG, BINGBING], &grams);
    let inst = k.inst();
    let ret = facade::guess_predicted_candidates(inst, c("你").as_ptr());
    let rows: Vec<(i32, Option<String>)> = (0..n_candidate(inst))
        .map(|index| {
            let cand = candidate_at(inst, index).unwrap_or(std::ptr::null_mut());
            let text = candidate_text(inst, cand).filter(|text| !text.is_empty());
            (candidate_type(inst, cand), text)
        })
        .collect();
    obj! { "ret" => ret, "rows" => rows }
}

/// `train_unsorted_set_freq_miss`: training 你好 against the start gram
/// ordered `[你-4, 你-3, 你-2, 你+1, 你-1]`.
fn train_unsorted_set_freq_miss(k: &mut Kit) -> J {
    plant_user_grams(k, &[], &|system, _| {
        let ni = system["你"];
        let items = [
            (ni - 4, 1),
            (ni - 3, 1),
            (ni - 2, 1),
            (ni + 1, 1),
            (ni - 1, 1),
        ];
        vec![(1, gram_bytes(5, &items))]
    });
    assert!(facade::set_options(k.ctx(), 1 << 5));
    let inst = k.inst();
    assert_eq!(
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr()),
        5
    );
    assert!(facade::guess_sentence(inst));
    assert!(choose_nihao(inst).iter().all(|&chosen| chosen > 0));
    assert!(facade::guess_sentence(inst));
    obj! { "ret" => facade::train_top(inst) }
}

/// `itemless_gram_untouched`: the removable 你 gram beside an item-less 我
/// gram of `total`, the imported phrase removed or masked, then a save.
fn itemless_gram_untouched(k: &mut Kit, remove_candidate: bool, total: u32) -> J {
    let (_, users, keys) = plant_user_grams(k, &[DALONG], &|system, users| {
        vec![
            (
                system["你"],
                gram_bytes(140, &[(system["好"], 70), (users[0], 70)]),
            ),
            (system["我"], total.to_le_bytes().to_vec()),
        ]
    });
    let mut out = J::map();
    if remove_candidate {
        out.set("removed", remove_dada(k));
    } else {
        out.set("masked", facade::mask_out(k.ctx(), 0xffff_ffff, users[0]));
    }
    out.set("trained", mark_modified(k));
    out.set("save", facade::save(k.ctx()));
    out.set("grams", close_and_read_grams(k, &keys));
    out
}

/// `sentence_out(k, inst, index)`: the flag and the text, `None` or `"untouched"`.
fn sentence_out(inst: P, index: u8) -> (bool, Option<String>) {
    let mut out = seeded();
    let ret = facade::get_sentence_at(inst, index, &raw mut out);
    (ret, owned_out(out))
}

/// `itemless_user_total`: an item-less user start gram whose total wraps the
/// merged one to 1 (left out unless `plant`), then the segmentation of two
/// texts (`texts` `None`) or each text's guess and three sentences.
fn itemless_user_total(k: &mut Kit, texts: Option<&[&str]>, plant: bool) -> J {
    let store =
        DefaultStore::open_hash_read_only(&k.data.join("bigram.db")).expect("the system bigram");
    let mut raw = Vec::new();
    assert!(
        store
            .get_raw(b"\x01\x00\x00\x00", &mut raw)
            .expect("read the start row")
    );
    drop(store);
    let system_total = u32::from_le_bytes(raw[..4].try_into().expect("a total"));
    let user_total = 1_u32.wrapping_sub(system_total);
    facade::fini(k.ctx());
    k.forget();
    if plant {
        craft_hash(
            &k.user.join("user_bigram.db"),
            &[(
                b"\x01\x00\x00\x00".to_vec(),
                user_total.to_le_bytes().to_vec(),
            )],
        );
    }
    write_user_conf(k);
    let Some(texts) = texts else {
        let inst = k.inst();
        return J::from(segment_out(inst, &["今天天气很好", "我们是中国人"]));
    };
    let mut out = J::map();
    for text in texts {
        let inst = k.alloc();
        facade::parse_more_full_pinyins(inst, c(text).as_ptr());
        let guessed = facade::guess_sentence(inst);
        let sentences: Vec<(bool, Option<String>)> =
            (0..3).map(|index| sentence_out(inst, index)).collect();
        out.set(*text, (guessed, sentences));
        facade::free_instance(inst);
    }
    out
}
