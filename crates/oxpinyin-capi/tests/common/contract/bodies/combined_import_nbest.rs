//! The combined_import_nbest family's case bodies: each answers the result document the old case returned.
//!
//! #525 and register row 74: `reduce_tokens` asserts when a search holds more
//! than four tokens (`phrase_large_table3.h:95`), reached by a sixth library's
//! add (`pinyin.cpp:541`), the prefix guess, the prediction, the character
//! offset and `remember_user_input`; a refusal must leave the earlier
//! libraries untouched. #757: the n-best tie between two equally counted user
//! phrases follows the user table's order (`phonetic_lookup.h:560-572`).

use std::collections::BTreeMap;
use std::path::Path;

use crate::facade;
use crate::readers::{c, owned_out, remember_export, seeded, tokens_of, unigram_of};
use crate::support::{J, Kit};

/// The phrase's fixture tokens in libraries 1 to 5.
const TOKENS: [u32; 5] = [16_802_309, 33_570_045, 50_359_794, 67_109_866, 83_886_081];

/// `remember_export`, `"NO EXPORT API"` on the zhuyin facade.
fn export(k: &mut Kit, library: u32, phrase: &str) -> J {
    if facade::ZHUYIN {
        return J::from("NO EXPORT API");
    }
    J::from(remember_export(k, library, phrase))
}

/// `transient_files`: every file under `root` by relative path, its mode and bytes.
fn transient_files(root: &Path) -> BTreeMap<String, (u32, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<String, (u32, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).expect("a user directory") {
            let path = entry.expect("a user entry").path();
            if path.is_dir() {
                walk(root, &path, files);
            } else if path.is_file() {
                let metadata = std::fs::metadata(&path).expect("a file's mode");
                #[cfg(unix)]
                let mode = std::os::unix::fs::PermissionsExt::mode(&metadata.permissions());
                #[cfg(not(unix))]
                let mode = u32::from(metadata.permissions().readonly());
                let relative = path.strip_prefix(root).expect("under the root");
                files.insert(
                    relative.to_string_lossy().into_owned(),
                    (mode, std::fs::read(&path).expect("a file's bytes")),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files);
    files
}

/// The facade's reading of 你好.
fn reading() -> Vec<u8> {
    if facade::ZHUYIN {
        "ㄋㄧˇ ㄏㄠˇ".as_bytes().to_vec()
    } else {
        b"ni3'hao3".to_vec()
    }
}

/// Imports 你好 into libraries 1 to 5 at count 100000, each add required.
fn five_libraries(k: &mut Kit) -> Vec<bool> {
    let reading = reading();
    let added: Vec<bool> = (1..=5)
        .map(|library| {
            let it = facade::begin_add_phrases(k.ctx(), library);
            let ret =
                facade::iterator_add_phrase(it, c("你好").as_ptr(), c(&reading).as_ptr(), 100_000);
            facade::end_add_phrases(it);
            ret
        })
        .collect();
    assert_eq!(added, [true; 5]);
    added
}

/// `combined_import_bound` with the guard: a sixth library's add of `sixth`.
fn sixth_add(k: &mut Kit, sixth: Option<Vec<u8>>) -> J {
    let phrase = "你好";
    five_libraries(k);
    assert!(facade::save(k.ctx()));
    let files = transient_files(&k.user);
    let frequencies: Vec<(bool, u32)> = TOKENS.iter().map(|&token| unigram_of(k, token)).collect();
    let exports: Vec<J> = (1..8).map(|library| export(k, library, phrase)).collect();
    let it = facade::begin_add_phrases(k.ctx(), 6);
    let sixth = sixth.unwrap_or_else(reading);
    let ret = facade::iterator_add_phrase(it, c(phrase).as_ptr(), c(&sixth).as_ptr(), 100_000);
    let tokens = tokens_of(k, phrase) == TOKENS;
    let unigrams = TOKENS
        .iter()
        .map(|&token| unigram_of(k, token))
        .collect::<Vec<_>>()
        == frequencies;
    let readings = (1..8)
        .map(|library| export(k, library, phrase))
        .collect::<Vec<_>>()
        == exports;
    let dirty = !facade::save(k.ctx());
    let out = obj! {
        "ret" => ret,
        "untouched-tokens" => tokens,
        "untouched-frequencies" => unigrams,
        "untouched-readings" => readings,
        "untouched-dirty" => dirty,
        "untouched-files" => transient_files(&k.user) == files,
    };
    facade::end_add_phrases(it);
    k.close();
    out
}

/// `hold_in_libraries`: `phrase` imported into libraries 1 to `libraries`.
fn hold(k: &mut Kit, phrase: &str, pinyin: &str, zhuyin: &str, libraries: u8) {
    let reading = if facade::ZHUYIN { zhuyin } else { pinyin };
    let added: Vec<bool> = (1..=libraries)
        .map(|library| {
            let it = facade::begin_add_phrases(k.ctx(), library);
            let ret =
                facade::iterator_add_phrase(it, c(phrase).as_ptr(), c(reading).as_ptr(), 100_000);
            facade::end_add_phrases(it);
            ret
        })
        .collect();
    assert!(added.iter().all(|&ok| ok), "{added:?}");
}

fn prefix_guess(k: &mut Kit, libraries: u8) -> J {
    hold(k, "你好", "ni3'hao3", "ㄋㄧˇ ㄏㄠˇ", libraries);
    let inst = k.alloc();
    obj! { "ret" => facade::guess_sentence_with_prefix(inst, c("你好").as_ptr()) }
}

fn character_offset(k: &mut Kit, libraries: u8) -> J {
    hold(k, "你", "ni3", "ㄋㄧˇ", libraries);
    if facade::ZHUYIN {
        facade::set_options(k.ctx(), 0x18a);
    }
    let inst = k.alloc();
    facade::parse_more_full_pinyins(inst, c("ni").as_ptr());
    let mut length = 0_usize;
    let ret = facade::get_character_offset(inst, c("你").as_ptr(), 0, &raw mut length);
    obj! { "ret" => ret, "length" => length }
}

fn remember_input(k: &mut Kit, parse: bool) -> J {
    hold(k, "你", "ni3", "ㄋㄧˇ", 5);
    let inst = k.alloc();
    if parse {
        facade::parse_more_full_pinyins(inst, c("ni").as_ptr());
    }
    obj! { "ret" => facade::remember_user_input(inst, c("你").as_ptr(), 1) }
}

/// `import_review`: an invalid reading into library 6 or an add into unused library 8.
fn import_review(k: &mut Kit, invalid: bool) -> J {
    let mut out = obj! { "added" => five_libraries(k) };
    let it = facade::begin_add_phrases(k.ctx(), if invalid { 6 } else { 8 });
    let reading = match (facade::ZHUYIN, invalid) {
        (false, true) => "ni3",
        (false, false) => "ni3'hao3",
        (true, true) => "ㄋㄧˇ",
        (true, false) => "ㄋㄧˇ ㄏㄠˇ",
    };
    out.set(
        "ret",
        facade::iterator_add_phrase(it, c("你好").as_ptr(), c(reading).as_ptr(), 3),
    );
    facade::end_add_phrases(it);
    facade::free_instance(k.inst());
    facade::fini(k.ctx());
    k.forget();
    out
}

/// `nbest_user_table_order`: two equally counted user phrases, toneless `dada`.
fn nbest_user_table_order(k: &mut Kit) -> J {
    let phrases = [
        ("龘龘", "da3'da3", "ㄉㄚˇ ㄉㄚˇ"),
        ("靐靐", "da2'da2", "ㄉㄚˊ ㄉㄚˊ"),
    ];
    let it = facade::begin_add_phrases(k.ctx(), 7);
    assert!(!it.is_null());
    let added: Vec<bool> = phrases
        .iter()
        .map(|(text, pinyin, zhuyin)| {
            let reading = if facade::ZHUYIN { zhuyin } else { pinyin };
            facade::iterator_add_phrase(it, c(text).as_ptr(), c(reading).as_ptr(), 100_000)
        })
        .collect();
    facade::end_add_phrases(it);
    assert!(added.iter().all(|&ok| ok), "{added:?}");
    let tokens: Vec<Vec<u32>> = phrases
        .iter()
        .map(|(text, _, _)| {
            tokens_of(k, text)
                .into_iter()
                .filter(|t| t >> 24 == 7)
                .collect()
        })
        .collect();
    assert!(tokens.iter().all(|found| found.len() == 1), "{tokens:?}");
    assert!(
        tokens[0][0] < tokens[1][0],
        "the first phrase added holds the lower token"
    );
    let shown: Vec<String> = tokens
        .iter()
        .map(|found| format!("{:#x}", found[0]))
        .collect();
    let mut out = obj! { "tokens" => shown };
    if facade::ZHUYIN {
        assert!(facade::set_options(k.ctx(), 1 << 5));
    }
    let inst = k.alloc();
    out.set(
        "parsed",
        facade::parse_more_full_pinyins(inst, c("dada").as_ptr()),
    );
    out.set("guess", facade::guess_sentence(inst));
    if facade::ZHUYIN {
        let mut text = seeded();
        let ret = facade::get_sentence_only(inst, &raw mut text);
        out.set("sentence", (ret, owned_out(text)));
    } else {
        let sentences: Vec<J> = (0..3)
            .map(|index| {
                let mut text = seeded();
                let ret = facade::get_sentence_at(inst, index, &raw mut text);
                J::from((ret, owned_out(text)))
            })
            .collect();
        out.set("sentences", sentences);
    }
    out
}

/// Runs the case `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    let mode = if facade::ZHUYIN { "-zhuyin" } else { "-pinyin" };
    let rest = id
        .strip_suffix(mode)
        .unwrap_or_else(|| panic!("no case {id}"));
    match rest {
        "combined-library-sixth-add" => sixth_add(k, None),
        "combined-library-fifth-add" => obj! { "added" => five_libraries(k) },
        "combined-library-sixth-add-long-reading" => sixth_add(
            k,
            Some(if facade::ZHUYIN {
                ["ㄋㄧˇ ㄏㄠˇ".as_bytes(), &[b' '; 70_000]].concat()
            } else {
                [&b"ni3'hao3"[..], &[b'\''; 70_000]].concat()
            }),
        ),
        "combined-library-sixth-add-key-past-parse-bound" => {
            sixth_add(k, Some([&b"ni3"[..], &[b'\''; 40_000], b"hao3"].concat()))
        }
        "combined-library-sixth-add-one-key" => sixth_add(k, Some("ㄋㄧˇ".as_bytes().to_vec())),
        "prefix-guess-five-libraries" => prefix_guess(k, 5),
        "prefix-guess-four-libraries" => prefix_guess(k, 4),
        "predicted-candidates-five-libraries" => {
            hold(k, "你好", "ni3'hao3", "ㄋㄧˇ ㄏㄠˇ", 5);
            let inst = k.alloc();
            obj! { "ret" => facade::guess_predicted_candidates(inst, c("你好").as_ptr()) }
        }
        "character-offset-five-libraries" => character_offset(k, 5),
        "character-offset-four-libraries" => character_offset(k, 4),
        "remember-user-input-five-libraries" => remember_input(k, true),
        "remember-user-input-five-libraries-unparsed" => remember_input(k, false),
        "combined-library-invalid-reading" => import_review(k, true),
        "combined-library-unused-index" => import_review(k, false),
        "nbest-user-table-order" => nbest_user_table_order(k),
        _ => panic!("no case {id}"),
    }
}
