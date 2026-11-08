//! The integer_limits_d18 family's case bodies: each answers the result document the old case returned.
//!
//! Inventory row 60 (#1175, #540): the unsigned and signed boundaries of
//! `remember_user_input`, `token_add_unigram_frequency`, the merged bigram
//! (`merge_single_gram`, `ngram.cpp:135-175,277-358`; `phonetic_lookup.h:598-640`),
//! training and the bigram export. Also the NUL-terminated remember inputs
//! (#752) and `save` over a stale temporary index directory (#739, #542).

use std::path::Path;

use oxpinyin_store::{DefaultStore, RawReadStore as _};

use crate::facade;
use crate::fixtures::{craft_hash, private_system, write_user_conf};
use crate::readers::{
    c, candidate_at, candidate_text, candidate_type, export_phrases, final_step_sentences,
    guess_rows, n_candidate, tokens_of, unigram_of, zhuyin_guess_rows,
};
use crate::support::{J, Kit, P, UNTOUCHED, take_text};

/// Runs the case `id` and answers its result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(answer) = fixed_case(k, id) {
        return answer;
    }
    let (name, _mode) = id.rsplit_once('-').expect("a mode suffix");
    if let Some(label) = name.strip_prefix("d18-merge-") {
        return merge(k, &merge_case(label));
    }
    if let Some(label) = name.strip_prefix("d18-train-") {
        return bigram_update(k, train_count(label));
    }
    if name.strip_prefix("issue542-stale-").is_some() {
        return stale_index_tmp(k);
    }
    match name {
        "d18-unigram-total-boundary" => unigram(k, "你"),
        other => panic!("no integer_limits_d18 case {other}"),
    }
}
/// The cases whose IDs carry no mode suffix.
fn fixed_case(k: &mut Kit, id: &str) -> Option<J> {
    if let Some(count) = id.strip_prefix("d18-remember-multiply-") {
        return Some(remember(k, &[count.parse().expect("a count")]));
    }
    if let Some(count) = id.strip_prefix("d18-bigram-export-") {
        return Some(bigram_export(
            k,
            u32::from_str_radix(count, 16).expect("a hex count"),
        ));
    }
    match id {
        "remember-known-nul-suffix-pinyin" => Some(remember_nul(k)),
        _ => None,
    }
}
/// `d18_remember`: parse nihaoshijie once, then remember 你好世界 with each
/// delta: the flag, the user export and the user token's unigram.
fn remember(k: &mut Kit, deltas: &[i32]) -> J {
    let inst = k.inst();
    let parsed = facade::parse_more_full_pinyins(inst, c("nihaoshijie").as_ptr());
    let steps: Vec<J> = deltas
        .iter()
        .map(|&delta| {
            let ret = facade::remember_user_input(inst, c("你好世界").as_ptr(), delta);
            let token = tokens_of(k, "你好世界")
                .into_iter()
                .find(|token| token >> 24 == 7)
                .expect("a library-seven token");
            J::from((delta, ret, export_phrases(k), unigram_of(k, token)))
        })
        .collect();
    obj! { "parsed" => parsed, "steps" => steps }
}

/// `remember_nul`: option word 0x28, the complete input parsed, then
/// `phrase`'s bytes (NUL included) remembered with count 1.
fn remember_nul(k: &mut Kit) -> J {
    let ctx = k.ctx();
    facade::set_options(ctx, 0x28);
    let inst = k.inst();
    let input = b"nihao";
    let text = c(input);
    let parsed = facade::parse_more_full_pinyins(inst, text.as_ptr());
    assert_eq!(
        parsed,
        input.len(),
        "the complete two-syllable input must parse"
    );
    let phrase = "你好\0ignored".as_bytes();
    let bytes = [phrase, b"\0"].concat();
    let ret = facade::remember_user_input(inst, bytes.as_ptr().cast(), 1);
    obj! { "parsed" => parsed, "ret" => ret, "rows" => export_phrases(k) }
}
/// The library name's index in table.conf's `default` lines.
fn library_index(name: &str) -> u32 {
    let names = [
        "RESERVED",
        "GB_DICTIONARY",
        "GBK_DICTIONARY",
        "OPENGRAM_DICTIONARY",
        "MERGED_DICTIONARY",
        "ADDON_DICTIONARY",
        "NETWORK_DICTIONARY",
        "USER_DICTIONARY",
    ];
    let at = names.iter().position(|known| *known == name);
    u32::try_from(at.expect("a known library")).expect("an index")
}

/// `d18_unigram`: unigram increments of `text`'s first token; `boundary`
/// derives them from its system library's stored total.
fn unigram(k: &mut Kit, text: &str) -> J {
    let token = tokens_of(k, text)[0];
    let mut out = obj! { "token" => token, "before" => unigram_of(k, token) };
    let library = token >> 24;
    let conf = std::fs::read_to_string(k.data.join("table.conf")).expect("table.conf");
    let entry: Vec<&str> = conf
        .lines()
        .filter(|line| line.starts_with("default "))
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .find(|row| library_index(row[1]) == library)
        .expect("the token's library");
    assert_ne!(
        entry[3], "NULL",
        "boundary fixture needs a system sub-index"
    );
    let data = std::fs::read(k.data.join(entry[3])).expect("the sub-index");
    let total = u32::from_le_bytes(data[8..12].try_into().expect("a total"));
    out.set("library", library);
    out.set("subindex", entry[3]);
    out.set("library-total", total);
    let deltas = [0xffff_ffff - total - 1, 1, 1, 0xffff_ffff];
    let inst = k.inst();
    let steps: Vec<(u32, bool, (bool, u32))> = deltas
        .iter()
        .map(|&delta| {
            let ret = facade::token_add_unigram_frequency(inst, token, delta);
            (delta, ret, unigram_of(k, token))
        })
        .collect();
    let flags: Vec<bool> = steps.iter().map(|step| step.1).collect();
    assert_eq!(flags, [true, true, false, false]);
    assert!(steps[1].2 == steps[2].2 && steps[2].2 == steps[3].2);
    out.set("steps", steps);
    out
}
/// One merged-bigram fixture (`d18_bigram_fixture`'s and `d18_merge`'s arguments).
struct Merge<'a> {
    system: Option<(u32, u32)>,
    user: (u32, u32),
    current: &'a str,
}

impl<'a> Merge<'a> {
    fn new(system: Option<(u32, u32)>, user: (u32, u32), current: &'a str) -> Self {
        Self {
            system,
            user,
            current,
        }
    }
}
/// The `d18-merge-{label}` fixtures.
fn merge_case(label: &str) -> Merge<'static> {
    match label {
        "total-zero" => Merge::new(Some((1, 1)), (0xffff_ffff, 1), "号"),
        other => panic!("no merge {other}"),
    }
}
/// The `d18-train-{label}` user counts.
fn train_count(label: &str) -> u32 {
    match label {
        "total-overflow" => 0xffff_ffff - 22080 + 1,
        other => panic!("no train {other}"),
    }
}
/// Frees the cached instance and finalises the cached context; both reopen lazily.
fn close(k: &mut Kit) {
    facade::free_instance(k.inst_if_any());
    facade::fini(k.ctx());
    k.forget();
}

/// `d18_gram`: one SingleGram total, token and count.
fn gram(total: u32, token: u32, count: u32) -> Vec<u8> {
    [total, token, count]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect()
}

/// `d18_bigram_fixture`: closes the cached objects, plants the system (on a
/// private copy) and user bigrams and answers the predecessor and current tokens.
fn bigram_fixture(k: &mut Kit, spec: &Merge<'_>) -> (u32, u32) {
    let [prev, cur, other] = ["你", spec.current, "我"].map(|text| tokens_of(k, text)[0]);
    close(k);
    if spec.system.is_some() {
        let copied = private_system(k);
        k.data = copied.clone();
        let rows = match spec.system {
            Some((total, count)) => {
                vec![(prev.to_le_bytes().to_vec(), gram(total, cur, count))]
            }
            _ => Vec::new(),
        };
        craft_hash(&copied.join("bigram.db"), &rows);
    }
    let records = vec![(cur, spec.user.1)];
    let mut value = spec.user.0.to_le_bytes().to_vec();
    for (token, count) in records {
        value.extend(token.to_le_bytes());
        value.extend(count.to_le_bytes());
    }
    let mut rows: Vec<(Vec<u8>, Vec<u8>)> = [prev, other]
        .iter()
        .map(|token| (token.to_le_bytes().to_vec(), value.clone()))
        .collect();
    rows.push((b"\x01\x00\x00\x00".to_vec(), vec![0; 4]));
    craft_hash(&k.user.join("user_bigram.db"), &rows);
    write_user_conf(k);
    (prev, cur)
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

/// The facade's candidate guess at `offset`: pinyin with sort word 0x1e.
fn guess_at(inst: P, offset: usize) -> crate::readers::GuessRows {
    if facade::ZHUYIN {
        zhuyin_guess_rows(inst, offset)
    } else {
        guess_rows(inst, offset, 0x1e)
    }
}

/// `d18_merge_observation`: sentences and candidates before and after nihao's
/// first syllable is constrained to 你.
fn merge_observation(k: &mut Kit) -> J {
    assert!(facade::set_options(k.ctx(), (1 << 5) | (1 << 9)));
    let inst = k.inst();
    let parsed = facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    let guessed = facade::guess_sentence(inst);
    let sentences = final_step_sentences(inst);
    let first = guess_at(inst, 0);
    let candidate = row_with_text(inst, 2, "你");
    assert!(
        first.ret && candidate.is_some(),
        "missing normal predecessor candidate"
    );
    let chosen = facade::choose_candidate(inst, 0, candidate.expect("a candidate"));
    let constrained = facade::guess_sentence(inst);
    let chosen_sentences = final_step_sentences(inst);
    let rows = guess_at(inst, 2);
    let normal: Vec<(i32, String)> = rows
        .rows
        .into_iter()
        .flatten()
        .filter(|row| row.0 == 2)
        .collect();
    assert!(
        !normal.is_empty(),
        "merged-row fixture must reach normal candidates"
    );
    obj! {
        "parsed" => parsed, "guessed" => guessed, "chosen" => chosen,
        "constrained" => constrained, "sentences" => sentences,
        "chosen_sentences" => chosen_sentences, "ret" => rows.ret, "candidates" => normal,
    }
}

/// `d18_merge`: the fixture, an optional lambda rewrite of the private
/// table.conf, then the merge observation.
fn merge(k: &mut Kit, spec: &Merge<'_>) -> J {
    bigram_fixture(k, spec);
    merge_observation(k)
}

/// `d18_bigram_export`: the 好 successor planted with total and count
/// `count`, then one bigram export row.
fn bigram_export(k: &mut Kit, count: u32) -> J {
    bigram_fixture(k, &Merge::new(None, (count, count), "好"));
    let it = facade::begin_get_bigram_phrases(k.ctx());
    let has = facade::bigram_iterator_has_next_phrase(it);
    let (mut phrase, mut reading, mut value) =
        (std::ptr::null_mut(), std::ptr::null_mut(), UNTOUCHED as i32);
    let ret = facade::bigram_iterator_get_next_phrase(
        it,
        &raw mut phrase,
        &raw mut reading,
        &raw mut value,
    );
    let row = J::from((ret, take_text(phrase), take_text(reading), value));
    facade::end_get_bigram_phrases(it);
    obj! { "has" => has, "row" => row }
}

/// `d18_bigram_db(k, path, read_keys=(key,))[0]`: one stored value.
fn stored_value(path: &Path, key: &[u8]) -> Vec<u8> {
    let store = DefaultStore::open_user_bigram(path).expect("the saved bigram opens");
    let mut value = Vec::new();
    assert!(store.get_raw(key, &mut value).expect("read the bigram"));
    value
}

/// The ordinary half of [`bigram_update`]: nihao constrained to 你 and 好.
fn chosen_steps(inst: P, out: &mut J) {
    let mut choices = Vec::new();
    for (offset, text) in [(0, "你"), (2, "好")] {
        if facade::ZHUYIN {
            assert!(facade::guess_candidates_after_cursor(inst, offset));
        } else {
            assert!(facade::guess_candidates(inst, offset, 0x1e));
        }
        let candidate = row_with_text(inst, 2, text)
            .unwrap_or_else(|| panic!("missing normal candidate: {text}"));
        choices.push(facade::choose_candidate(inst, offset, candidate));
    }
    out.set("choices", choices);
    out.set("chosen-guess", facade::guess_sentence(inst));
}

/// `d18_bigram_update`: the gram planted with `count`, a predicted selection
/// or an ordinary constrained guess, train and save; the stored record, then
/// the reopened unigram (and, `user_only`, the reopened merge observation).
fn bigram_update(k: &mut Kit, count: u32) -> J {
    let current = "好";
    let spec = Merge::new(None, (count, count), current);
    let (prev, cur) = bigram_fixture(k, &spec);
    assert!(facade::set_options(k.ctx(), 1 << 5));
    let mut out = obj! { "before" => unigram_of(k, cur) };
    let inst = k.inst();
    out.set(
        "parse",
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr()),
    );
    let inst = k.inst();
    out.set("guess", facade::guess_sentence(inst));
    chosen_steps(inst, &mut out);
    out.set("train", facade::train_top(inst));
    out.set("after", unigram_of(k, cur));
    out.set("save", facade::save(k.ctx()));
    close(k);
    let raw = stored_value(&k.user.join("user_bigram.db"), &prev.to_le_bytes());
    let word = |at: usize| u32::from_le_bytes(raw[at..at + 4].try_into().expect("a word"));
    let records: Vec<(u32, u32)> = (4..raw.len())
        .step_by(8)
        .map(|at| (word(at), word(at + 4)))
        .collect();
    out.set("stored", (word(0), records));
    out.set("reopened-unigram", unigram_of(k, cur));
    out
}

/// The facade's nihao input parsed and guessed on `inst`, then its train flag.
fn train_once(inst: P) -> bool {
    let parsed = if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c("su3cl3").as_ptr())
    } else {
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr())
    };
    assert!(parsed != 0);
    assert!(facade::guess_sentence(inst));
    facade::train_top(inst)
}

/// `stale_index_tmp`: `save` with a stale `index.tmp` directory holding a
/// marker, over an absent, empty-directory or live-file destination, each in
/// a fresh user directory. The old case made each with `mkdtemp`; here the
/// previous one is renamed aside so that the fresh one is `user-X`, the name
/// the frozen stderr shows.
fn stale_index_tmp(k: &mut Kit) -> J {
    let index = "user_phrase_index.bin";
    let marker: &[u8] = b"unchanged temporary directory";
    let mut out = J::map();
    for (round, destination) in ["absent", "empty-directory", "live-file"]
        .into_iter()
        .enumerate()
    {
        if round > 0 {
            let aside = k.scratch.join(format!("user-X{round}"));
            std::fs::rename(&k.user, &aside).expect("set the used user directory aside");
            std::fs::create_dir(&k.user).expect("a fresh user directory");
        }
        let inst = k.inst();
        assert!(train_once(inst));
        let live = k.user.join(index);
        let mut before = None;
        if destination == "live-file" {
            assert!(facade::save(k.ctx()));
            before = Some(std::fs::read(&live).expect("the live index"));
            assert!(train_once(inst));
        } else if destination == "empty-directory" {
            std::fs::create_dir(&live).expect("an empty destination directory");
        }
        let tmp = k.user.join(format!("{index}.tmp"));
        std::fs::create_dir(&tmp).expect("the stale temporary directory");
        std::fs::write(tmp.join("stale-marker"), marker).expect("the stale marker");
        let ret = facade::save(k.ctx());
        let moved = live.join("stale-marker");
        let preserved =
            before.is_some_and(|before| std::fs::read(&live).is_ok_and(|now| now == before));
        out.set(
            destination,
            obj! {
                "save" => ret,
                "tmp-present" => tmp.exists(),
                "stale-moved" => moved.is_file()
                    && std::fs::read(&moved).is_ok_and(|bytes| bytes == marker),
                "live-preserved" => preserved,
                "messages" => k.messages(),
            },
        );
        close(k);
    }
    out
}
