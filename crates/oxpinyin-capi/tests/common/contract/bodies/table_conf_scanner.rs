//! The table_conf_scanner family's case bodies: each answers the result document the old case returned.
//!
//! table.conf rows, header scanning and file paths (inventory row 64, #1175).

use std::path::{Path, PathBuf};

use crate::bodies::case_helpers::{Trained, token_unigram_trained};
use crate::facade;
use crate::fixtures::{
    ConfEdit, ConfEdit::Append as Add, ConfEdit::Drop, ConfEdit::Replace as R, conf_system,
    craft_hash, private_system, user_listing, write_user_conf,
};
use crate::readers::{c, final_step_sentences, tokens_of, unigram_of};
use crate::support::{J, Kit, P, UNTOUCHED, borrowed_text, take_text};

/// The two inputs whose sentences and candidates a loaded library set decides.
fn conf_texts() -> [&'static str; 2] {
    if facade::ZHUYIN {
        ["ni3hao3", "yi4shu4"]
    } else {
        ["nihao", "yishu"]
    }
}

/// The stock λ line every λ case rewrites.
const LAMBDA: &str = "lambda parameter:0.312699";

/// `conf_system_bytes`: a private system copy whose table.conf is `mutate` of
/// the raw stock bytes. Bytes and NUL are preserved, unlike `conf_system`'s text edits.
fn conf_system_bytes(k: &Kit, mutate: impl FnOnce(Vec<u8>) -> Vec<u8>) -> PathBuf {
    let system = private_system(k);
    let path = system.join("table.conf");
    let data = std::fs::read(k.data.join("table.conf")).expect("table.conf");
    std::fs::remove_file(&path).expect("unlink the linked table.conf");
    std::fs::write(&path, mutate(data)).expect("write table.conf");
    system
}

/// Python's `bytes.replace`: every occurrence, none required.
fn replace_bytes(data: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut rest = data;
    while let Some(at) = rest.windows(from.len()).position(|window| window == from) {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(to);
        rest = &rest[at + from.len()..];
    }
    out.extend_from_slice(rest);
    out
}

/// `re.sub(r'database format:\w+', replace, text)` with the substitution
/// asserted to match: `replace` receives the matched text.
fn sub_database_format(data: &[u8], replace: impl Fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    const KEY: &[u8] = b"database format:";
    let mut out = Vec::with_capacity(data.len());
    let mut rest = data;
    let mut matched = false;
    while let Some(at) = rest.windows(KEY.len()).position(|window| window == KEY) {
        let word = rest[at + KEY.len()..]
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
            .count();
        let end = at + KEY.len() + word;
        out.extend_from_slice(&rest[..at]);
        if word == 0 {
            out.extend_from_slice(KEY);
        } else {
            matched = true;
            out.extend(replace(&rest[at..end]));
        }
        rest = &rest[end..];
    }
    out.extend_from_slice(rest);
    assert!(matched, "table.conf has no database format token");
    out
}

/// The init on `system` with the kit's user directory.
fn init_on(k: &Kit, system: &Path) -> P {
    k.init(Some(system), Some(&k.user))
}

/// `conf_guess_rows`: `guess_candidates` at offset 0, the call's answer, the
/// count and the first twelve rows' text.
fn conf_guess_rows(inst: P) -> (bool, u32, Vec<String>) {
    let ok = if facade::ZHUYIN {
        facade::guess_candidates_after_cursor(inst, 0)
    } else {
        facade::guess_candidates(inst, 0, 0)
    };
    let mut count = UNTOUCHED;
    facade::get_n_candidate(inst, &raw mut count);
    let rows = (0..count.min(12))
        .map(|index| {
            let mut cand: P = std::ptr::null_mut();
            facade::get_candidate(inst, index, &raw mut cand);
            let mut text: *const std::ffi::c_char = std::ptr::null();
            facade::get_candidate_string(inst, cand, &raw mut text);
            borrowed_text(text).expect("candidate text")
        })
        .collect();
    (ok, count, rows)
}

/// `conf_sentence`: the first sentence, `None` for both untouched output and NULL.
fn conf_sentence(inst: P) -> Option<String> {
    let mut out = UNTOUCHED as usize as *mut std::ffi::c_char;
    if facade::ZHUYIN {
        facade::get_sentence_only(inst, &raw mut out);
    } else {
        facade::get_sentence_at(inst, 0, &raw mut out);
    }
    if out as usize == UNTOUCHED as usize {
        None
    } else {
        take_text(out)
    }
}

/// `conf_view`: sentences and candidate lists for two inputs, the observable
/// the loaded library set decides.
fn conf_view(ctx: P) -> J {
    let mut view = J::map();
    for text in conf_texts() {
        let inst = facade::alloc_instance(ctx);
        facade::parse_more_full_pinyins(inst, c(text).as_ptr());
        facade::guess_sentence(inst);
        let sentence = conf_sentence(inst);
        let (ok, count, rows) = conf_guess_rows(inst);
        view.set(text, (sentence, ok, count, rows));
    }
    view
}

/// `conf_session`: open on `system`, look up, learn one sentence and one
/// phrase, save: what the rows decide to load, to accept and to write.
fn conf_session(k: &mut Kit, system: &Path) -> J {
    let ctx = init_on(k, system);
    if ctx.is_null() {
        return obj! { "ctx" => false };
    }
    let mut out = obj! { "view" => conf_view(ctx) };
    let inst = facade::alloc_instance(ctx);
    facade::parse_more_full_pinyins(inst, c(conf_texts()[0]).as_ptr());
    facade::guess_sentence(inst);
    out.set("train", facade::train_top(inst));
    let it = facade::begin_add_phrases(ctx, 7);
    let reading = if facade::ZHUYIN {
        "ㄘㄜˋ ㄕˋ ㄘˊ"
    } else {
        "ce'shi'ci"
    };
    out.set(
        "add",
        facade::iterator_add_phrase(it, c("测试词").as_ptr(), c(reading).as_ptr(), 1),
    );
    facade::end_add_phrases(it);
    out.set("save", facade::save(ctx));
    facade::fini(ctx);
    out.set("files", user_listing(k, None));
    out
}

/// `token_unigram_trained`'s document.
fn trained_j(trained: &Trained) -> J {
    let training = &trained.training;
    obj! {
        "token" => trained.token.clone(),
        "parsed" => trained.parsed,
        "before" => trained.before,
        "training" => obj! {
            "chosen" => training.chosen.clone(),
            "kind" => training.kind,
            "cursor" => training.cursor,
            "after-choose" => training.after_choose,
            "guess" => training.guess,
            "train" => training.train,
        },
        "after" => trained.after,
    }
}

/// `token_unigram_user(k, add_frequency=False)`: imports 你好你好 into library
/// 7 with count nine and reads its unigram before and after, adding nothing.
/// Answers the document, the token and the unigram after.
fn token_unigram_user_no_add(k: &mut Kit) -> (J, u32, (bool, u32)) {
    let phrase = "你好你好";
    let reading = if facade::ZHUYIN {
        "ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ"
    } else {
        "ni3'hao3'ni3'hao3"
    };
    let it = facade::begin_add_phrases(k.ctx(), 7);
    assert!(!it.is_null());
    let added = facade::iterator_add_phrase(it, c(phrase).as_ptr(), c(reading).as_ptr(), 9);
    facade::end_add_phrases(it);
    assert!(added);
    let tokens: Vec<u32> = tokens_of(k, phrase)
        .into_iter()
        .filter(|token| token >> 24 == 7)
        .collect();
    assert_eq!(tokens.len(), 1, "{tokens:?}");
    let token = tokens[0];
    let before = unigram_of(k, token);
    let after = unigram_of(k, token);
    let out = obj! {
        "imported" => added,
        "token" => format!("{token:#x}"),
        "before" => before,
        "after" => after,
    };
    (out, token, after)
}

/// `hash_path_fixture`: puts literal `#type=kct` text into the selected
/// private directory or configured filename, updates `k.data` (and `k.user`)
/// and answers the system chunk, journal and user chunk names.
///
/// #1119: direct KC classes open literal paths (074a2219
/// ngram_kyotodb.cpp:117-119; chewing_large_table2_kyotodb.cpp:87-89).
/// table_info.cpp:255-284 preserves '#' in filename fields; pinyin.cpp:
/// 250-276, 303-319, 939-988, 1042-1084 loads/saves those literal names.
fn hash_path_fixture(k: &mut Kit, placement: &str) -> (String, String, String) {
    let mut system = private_system(k);
    let (mut system_name, mut journal_name, mut user_name) = (
        "gb_char.bin".to_owned(),
        "gb_char.dbin".to_owned(),
        "user.bin".to_owned(),
    );
    let suffixed = |path: &Path| {
        let mut name = path.as_os_str().to_owned();
        name.push("#type=kct");
        PathBuf::from(name)
    };
    match placement {
        "system-directory" => {
            let renamed = suffixed(&system);
            std::fs::rename(&system, &renamed).expect("rename the system directory");
            system = renamed;
        }
        "user-directory" => {
            let renamed = suffixed(&k.user);
            std::fs::rename(&k.user, &renamed).expect("rename the user directory");
            k.user = renamed;
        }
        "system-filename" | "user-filename" => {
            let conf = system.join("table.conf");
            let mut text = std::fs::read_to_string(&conf).expect("table.conf");
            if placement == "system-filename" {
                "gb#type=kct.bin".clone_into(&mut system_name);
                std::fs::rename(system.join("gb_char.bin"), system.join(&system_name))
                    .expect("rename gb_char.bin");
                text = text.replace("gb_char.bin", &system_name);
            } else {
                "gb#type=kct.dbin".clone_into(&mut journal_name);
                "user#type=kct.bin".clone_into(&mut user_name);
                text = text
                    .replace("gb_char.dbin", &journal_name)
                    .replace("user.bin", &user_name);
            }
            // private_system linked this file to the immutable pin data.
            std::fs::remove_file(&conf).expect("unlink table.conf");
            std::fs::write(&conf, text).expect("write table.conf");
        }
        other => panic!("unknown hash path placement: {other}"),
    }
    k.data = system;
    (system_name, journal_name, user_name)
}

/// `hash_path_session`: trained and imported counts survive saving through the
/// renamed paths, a reopen, and libpinyin's export of the saved profile in a
/// separate process (libzhuyin has no export iterator, zhuyin.h at 074a2219).
fn hash_path_session(k: &mut Kit, placement: &str) -> J {
    let (system_name, journal_name, user_name) = hash_path_fixture(k, placement);
    let trained = token_unigram_trained(k);
    let (import, import_token, import_after) = token_unigram_user_no_add(k);
    let mut out = obj! { "training" => trained_j(&trained), "import" => import };
    let save = facade::save(k.ctx());
    out.set("save", save);
    assert!(save, "the trained profile must save");
    out.set("files", user_listing(k, None));
    assert!(
        k.data.join(&system_name).is_file(),
        "the configured system file must exist"
    );
    assert!(
        k.user.join(&journal_name).is_file(),
        "the trained journal must be written"
    );
    assert!(
        k.user.join(&user_name).is_file(),
        "the imported library must be written"
    );
    if placement == "system-filename" {
        assert!(!k.data.join("gb_char.bin").exists());
    }
    if placement == "user-filename" {
        assert!(!k.user.join("gb_char.dbin").exists());
        assert!(!k.user.join("user.bin").exists());
    }
    k.close();
    out.set("reopen", !k.ctx().is_null());
    let token = u32::from_str_radix(trained.token.trim_start_matches("0x"), 16).expect("token");
    let trained_after = unigram_of(k, token);
    assert_eq!(trained_after, trained.after);
    out.set("trained-after-reopen", trained_after);
    let import_after_reopen = unigram_of(k, import_token);
    assert_eq!(import_after_reopen, import_after);
    out.set("import-after-reopen", import_after_reopen);
    k.close();
    let document = crate::support::libpinyin_export(&k.data, &k.user).document;
    assert!(
        document.get("rows").is_some_and(J::truthy),
        "reopen must preserve the imported phrase for export"
    );
    out.set("export", document);
    out
}

/// `unigram_only_session`: #1378, phonetic_lookup.h:651-663: the epsilon gate
/// is on elem_poss, before weighting. At lambda=1 the unigram step still exists
/// at log(0). An empty valid system bigram makes this the only first-step
/// branch. Observe branch presence, not the unrelated equal-score tail tie order.
fn unigram_only_session(k: &mut Kit, system: &Path) -> J {
    let path = system.join("bigram.db");
    // private_system's link; never modify the pin data.
    std::fs::remove_file(&path).expect("unlink bigram.db");
    craft_hash(&path, &[]);
    let ctx = init_on(k, system);
    assert!(!ctx.is_null());
    let inst = facade::alloc_instance(ctx);
    let text = if facade::ZHUYIN { "ni3" } else { "ni" };
    let parsed = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
    let guessed = facade::guess_sentence(inst);
    let has_sentence = !final_step_sentences(inst).is_empty();
    facade::free_instance(inst);
    obj! { "parsed" => parsed, "guessed" => guessed, "has_sentence" => has_sentence }
}

/// `addon_session`: the addon libraries a row set loads, and the candidates they add.
fn addon_session(k: &mut Kit, system: &Path) -> J {
    let ctx = init_on(k, system);
    assert!(!ctx.is_null(), "init failed");
    let mut load = J::map();
    for index in [0_u8, 3, 4, 5, 15] {
        load.set(
            index.to_string(),
            facade::load_addon_phrase_library(ctx, index),
        );
    }
    let out = obj! { "load" => load, "view" => conf_view(ctx) };
    facade::fini(ctx);
    out
}

/// What a `conf_abort` probe does after its edits: init only, or a library load.
enum Call {
    Init,
    /// `load_phrase_library` (`pinyin.cpp:457`).
    Load(u8),
    /// `load_addon_phrase_library` (`pinyin.cpp:491`).
    Addon(u8),
}

/// `conf_abort`'s probe: the rows the pin dies on. `table_info.cpp` aborts
/// inside `SystemTableInfo2::load`, before `check_format`; the init loop
/// (`pinyin.cpp:377-392`) runs after it.
fn conf_abort(k: &mut Kit, system: &Path, call: &Call) -> J {
    let ctx = init_on(k, system);
    let ret = match call {
        Call::Init => !ctx.is_null(),
        Call::Load(index) => {
            assert!(!ctx.is_null(), "init failed");
            facade::load_phrase_library(ctx, *index)
        }
        Call::Addon(index) => {
            assert!(!ctx.is_null(), "init failed");
            facade::load_addon_phrase_library(ctx, *index)
        }
    };
    obj! { "ret" => ret }
}

/// `header_probe`: the pin's ordinary `false`: a header directive that does not
/// match answers NULL with the raw `load %s failed!` line and no warning.
fn header_probe(k: &mut Kit, edits: &[ConfEdit<'_>]) -> J {
    let system = conf_system(k, edits);
    obj! { "ctx" => !init_on(k, &system).is_null() }
}

/// `user_conf_unknown_format`: `UserTableInfo::load` converts the `database
/// format:` token of the user's `user.conf` and aborts on one it does not know
/// (`table_info.cpp:132`, reached from `check_format`). `zhuyin_init`'s
/// `check_format` loads the same marker first (`zhuyin.cpp:126-132`).
fn user_conf_unknown_format(k: &mut Kit) -> J {
    write_user_conf(k);
    let path = k.user.join("user.conf");
    let text = std::fs::read(&path).expect("user.conf");
    let text = sub_database_format(&text, |_| b"database format:LMDB".to_vec());
    std::fs::write(&path, text).expect("user.conf");
    obj! { "ret" => !k.init_default().is_null() }
}

/// `missing_conf_with_dbms`: maintainer ruling 2026-10-28 UTC (D3):
/// `SystemTableInfo2::load` is the first step, and a table.conf it cannot open
/// fails the init however complete the rest of the directory is (074a2219
/// table_info.cpp:199-202, pinyin.cpp:335-340, zhuyin.cpp:279-284).
fn missing_conf_with_dbms(k: &mut Kit) -> J {
    let system = private_system(k);
    std::fs::remove_file(system.join("table.conf")).expect("unlink table.conf");
    obj! { "ctx" => !init_on(k, &system).is_null() }
}

/// `absolute_user_file`: a file name never leaves its directory:
/// `g_build_filename(user_dir, "/abs/u.bin")` is `<user_dir>/abs/u.bin`.
fn absolute_user_file(k: &mut Kit) -> J {
    let user = k.user.to_str().expect("a UTF-8 scratch path").to_owned();
    let nested = format!("{user}/nest");
    let beneath = format!("{user}{nested}");
    std::fs::create_dir_all(&nested).expect("the nested directory");
    std::fs::create_dir_all(&beneath).expect("the user-rooted directory");
    let row = format!("{nested}/u.bin USER_FILE");
    let system = conf_system(k, &[R("user.bin USER_FILE", &row)]);
    let mut out = conf_session(k, &system);
    out.set("beside", user_listing(k, Some(Path::new(&nested))));
    out.set("beneath", user_listing(k, Some(Path::new(&beneath))));
    // `k.user + nested` makes the scratch path's first component appear in the
    // user directory: its name is the harness's scratch location, not
    // behaviour. Only that one prefix is replaced.
    let prefix = user.split('/').nth(1).expect("an absolute scratch path");
    if let J::M(map) = &mut out
        && let Some(J::M(files)) = map.get_mut("files")
        && let Some(entry) = files.remove(prefix)
    {
        files.insert("SCRATCH-PREFIX".to_owned(), entry);
    }
    out
}

/// `addon_cleanup`: `_clean_user_files` also walks libpinyin's addon rows
/// (pinyin.cpp:199-200); libzhuyin stops after the default rows (zhuyin.cpp:144).
/// A fresh profile is non-conforming, so check_format wipes before the libraries load.
fn addon_cleanup(k: &mut Kit) -> J {
    let system = conf_system(
        k,
        &[R(
            "addon 4 art.table art.bin NULL DICTIONARY",
            "addon 4 art.table art.bin addon-user.bin DICTIONARY",
        )],
    );
    let path = k.user.join("addon-user.bin");
    std::fs::write(&path, b"stale").expect("addon-user.bin");
    let ctx = init_on(k, &system);
    let out = obj! { "ctx" => !ctx.is_null(), "addon" => path.exists() };
    if !ctx.is_null() {
        facade::fini(ctx);
    }
    out
}

/// `candidates_probe`: class (b): a library whose rows no longer match the index
/// points the candidate listing at items that are not there; the pin's duplicate
/// removal compares a string it never set (`pinyin.cpp:1635-1637`, `:2053`, SIGSEGV).
fn candidates_probe(k: &mut Kit, edits: &[ConfEdit<'_>]) -> J {
    let system = conf_system(k, edits);
    let ctx = init_on(k, &system);
    assert!(!ctx.is_null(), "init failed");
    let inst = facade::alloc_instance(ctx);
    facade::parse_more_full_pinyins(inst, c(conf_texts()[0]).as_ptr());
    let sentence = facade::guess_sentence(inst);
    let (ok, _, _) = conf_guess_rows(inst);
    obj! { "ret" => ok, "sentence" => sentence }
}

/// `one_line_conf`: the first six newline-delimited fields joined by spaces,
/// two newlines, then the rest.
fn one_line_conf(data: &[u8]) -> Vec<u8> {
    let parts: Vec<&[u8]> = data.splitn(7, |&byte| byte == b'\n').collect();
    assert_eq!(parts.len(), 7, "table.conf has six header lines");
    [&parts[..6].join(&b' ')[..], b"\n\n", parts[6]].concat()
}

/// A session on table.conf with `edits` applied.
fn session(k: &mut Kit, edits: &[ConfEdit<'_>]) -> J {
    let system = conf_system(k, edits);
    conf_session(k, &system)
}

/// A session on table.conf's raw bytes mutated (#919: `SystemTableInfo2::load`
/// is one source for both facades).
fn session_bytes(k: &mut Kit, mutate: impl FnOnce(Vec<u8>) -> Vec<u8>) -> J {
    let system = conf_system_bytes(k, mutate);
    conf_session(k, &system)
}

/// A λ line rewritten to `value`.
fn lambda(value: &str) -> String {
    format!("lambda parameter:{value}")
}

/// A `conf_abort` probe on table.conf with `edits` applied.
fn abort(k: &mut Kit, edits: &[ConfEdit<'_>], call: &Call) -> J {
    let system = conf_system(k, edits);
    conf_abort(k, &system, call)
}

/// Runs the case `id` and answers its result document.
#[allow(clippy::too_many_lines)]
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(rest) = id.strip_prefix("hash-path/") {
        let placement = rest
            .strip_suffix("-pinyin")
            .or_else(|| rest.strip_suffix("-zhuyin"))
            .expect("a hash-path mode");
        return hash_path_session(k, placement);
    }
    let name = if facade::ZHUYIN {
        id.strip_suffix("-zhuyin").expect("a zhuyin case")
    } else {
        id
    };
    let name = name.strip_prefix("table-conf-").unwrap_or(name);
    if let Some(value) = name.strip_prefix("1378-unigram-lambda-") {
        let value = match value {
            "one" => "1",
            "zero-control" => "0",
            other => panic!("unknown 1378 case {other}"),
        };
        let system = conf_system(k, &[R(LAMBDA, &lambda(value))]);
        return unigram_only_session(k, &system);
    }
    // λ spelled inside [0, 1] reads the file's λ in the candidate law
    // `(λ·bigram + (1−λ)·unigram)·2²⁴`; outside it the costs go negative (below 0)
    // or NaN (above 1), sticky through the trellis.
    let lambda_value = match name {
        "lambda-zero" => Some("0"),
        "lambda-half" => Some("0.5"),
        "lambda-one" => Some("1"),
        "lambda-negative-half" => Some("-0.5"),
        "lambda-negative-two" => Some("-2"),
        "lambda-one-and-a-half" => Some("1.5"),
        "lambda-two" => Some("2"),
        "lambda-million" => Some("1e6"),
        _ => None,
    };
    if let Some(value) = lambda_value {
        return session(k, &[R(LAMBDA, &lambda(value))]);
    }
    match name {
        // Valid files that are not the stock one: the pin follows the rows.
        "row-removed-gbk" => session(k, &[Drop("GBK_DICTIONARY")]),
        "row-removed-gb" => session(k, &[Drop("GB_DICTIONARY")]),
        "not-used-opengram" => session(
            k,
            &[R("opengram.dbin SYSTEM_FILE", "opengram.dbin NOT_USED")],
        ),
        "not-used-merged" => session(k, &[R("merged.dbin SYSTEM_FILE", "merged.dbin NOT_USED")]),
        "not-used-user-dictionary" => session(k, &[R("user.bin USER_FILE", "user.bin NOT_USED")]),
        "not-used-network-dictionary" => {
            session(k, &[R("network.bin USER_FILE", "network.bin NOT_USED")])
        }
        "user-files-renamed" => session(
            k,
            &[
                R("gb_char.dbin", "x_gb.dbin"),
                R("merged.dbin", "x_merged.dbin"),
                R("user.bin", "x_user.bin"),
                R("addon.bin", "x_addon.bin"),
            ],
        ),
        "tsi-is-gb" => session(k, &[R("default GB_DICTIONARY", "default TSI_DICTIONARY")]),
        "later-row-wins" => session(
            k,
            &[Add(
                "default OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin NOT_USED",
            )],
        ),
        "network-as-system-file" => session(
            k,
            &[R(
                "default NETWORK_DICTIONARY NULL NULL network.bin USER_FILE",
                "default NETWORK_DICTIONARY art.table art.bin network.dbin SYSTEM_FILE",
            )],
        ),
        "reserved-as-system-file" => session(
            k,
            &[R(
                "default RESERVED NULL NULL NULL NOT_USED",
                "default RESERVED merged.table merged.bin x_reserved.dbin SYSTEM_FILE",
            )],
        ),
        // The header fields the user marker conforms against.
        "database-format-other" => session_bytes(k, |data| {
            sub_database_format(&data, |found| {
                if found.ends_with(b"Tkrzw") {
                    b"database format:BerkeleyDB".to_vec()
                } else {
                    b"database format:Tkrzw".to_vec()
                }
            })
        }),
        "versions-other" => session(
            k,
            &[
                R("binary format version:7", "binary format version:8"),
                R("model data version:14", "model data version:15"),
            ],
        ),
        "source-format-zhuyin" => session(
            k,
            &[R(
                "source table format:pinyin",
                "source table format:zhuyin",
            )],
        ),
        // Words are read in groups of six: a short row and a seventh word at the
        // end are dropped, and an addon index goes through a `guint8`.
        "short-row-dropped" => session(k, &[Add("default GB_DICTIONARY a b c")]),
        "extra-word-dropped" => session(
            k,
            &[Add(
                "default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE EXTRA",
            )],
        ),
        // The addon rows decide what `pinyin_load_addon_phrase_library` loads.
        "addon-index-wraps"
        | "addon-moved"
        | "addon-files-swapped"
        | "addon-not-used"
        | "addon-index-zero" => {
            let edits: &[ConfEdit<'_>] = match name {
                "addon-index-wraps" => &[Add("addon 256 art.table art.bin NULL DICTIONARY")],
                "addon-moved" => &[R("addon 4 art.table", "addon 3 art.table")],
                "addon-files-swapped" => &[
                    R("addon 4 art.table art.bin", "addon 4 art.table culture.bin"),
                    R(
                        "addon 5 culture.table culture.bin",
                        "addon 5 culture.table art.bin",
                    ),
                ],
                "addon-not-used" => &[R("art.bin NULL DICTIONARY", "art.bin NULL NOT_USED")],
                _ => &[R("addon 4 art.table", "addon 0 art.table")],
            };
            let system = conf_system(k, edits);
            addon_session(k, &system)
        }
        // The rows the pin dies on: `table_info.cpp` aborts inside
        // `SystemTableInfo2::load`, before `check_format`.
        "abort-source-format-unknown" => abort(
            k,
            &[R("source table format:pinyin", "source table format:foo")],
            &Call::Init,
        ),
        "abort-database-format-unknown" => {
            let system = conf_system_bytes(k, |data| {
                sub_database_format(&data, |_| b"database format:LMDB".to_vec())
            });
            conf_abort(k, &system, &Call::Init)
        }
        "abort-default-name-unknown" => abort(
            k,
            &[Add("default FOO_DICTIONARY a b c SYSTEM_FILE")],
            &Call::Init,
        ),
        "abort-file-type-unknown" => abort(
            k,
            &[Add("default GB_DICTIONARY a b c FOO_FILE")],
            &Call::Init,
        ),
        "abort-addon-index-16" => abort(
            k,
            &[Add("addon 16 x.table x.bin NULL DICTIONARY")],
            &Call::Init,
        ),
        // The init loop (`pinyin.cpp:377-392`) runs after `check_format`.
        "abort-default-dictionary" => abort(
            k,
            &[R("gb_char.dbin SYSTEM_FILE", "gb_char.dbin DICTIONARY")],
            &Call::Init,
        ),
        // A library call on a row the pin asserts on (`pinyin.cpp:457`, `:491`).
        "abort-load-library-not-used" => abort(
            k,
            &[R("opengram.dbin SYSTEM_FILE", "opengram.dbin NOT_USED")],
            &Call::Load(3),
        ),
        "abort-load-addon-system-file" => abort(
            k,
            &[R("art.bin NULL DICTIONARY", "art.bin NULL SYSTEM_FILE")],
            &Call::Addon(4),
        ),
        // Class (b): the header's `source table format:` line is missing.
        "abort-source-format-line-missing" => abort(k, &[Drop("source table format")], &Call::Init),
        "abort-919-word-width-overlong" => {
            let row = format!("{} x.table x.bin x.dbin SYSTEM_FILE", "x".repeat(300));
            abort(k, &[Add(&row)], &Call::Init)
        }
        "user-conf-abort-database-format-unknown" => user_conf_unknown_format(k),
        "lambda-line-missing" => header_probe(k, &[Drop("lambda parameter")]),
        // A spelling glibc's `%f` accepts that is not a decimal: header acceptance
        // with an already conforming user marker.
        "lambda-hex" => {
            let system = conf_system(k, &[R(LAMBDA, "lambda parameter:0x1p-1")]);
            write_user_conf(k);
            obj! { "ctx" => !init_on(k, &system).is_null() }
        }
        // #919: table.conf scanner partitions.
        "919-header-one-line" => session_bytes(k, |data| one_line_conf(&data)),
        "919-header-no-final-newline" => session_bytes(k, |mut data| {
            while data.last() == Some(&b'\n') {
                data.pop();
            }
            data
        }),
        "919-word-nul-truncates" => session_bytes(k, |data| {
            replace_bytes(
                &data,
                b"gb_char.table gb_char.bin gb_char.dbin",
                b"gb_char.table gb_char.bin\x00junk gb_char.dbin",
            )
        }),
        "919-source-token-nul" => session_bytes(k, |data| {
            replace_bytes(
                &data,
                b"source table format:pinyin",
                b"source table format:pinyin\x00suffix",
            )
        }),
        "919-database-token-nul" => session_bytes(k, |mut data| {
            for format in ["BerkeleyDB", "KyotoCabinet", "Tkrzw"] {
                let from = format!("database format:{format}");
                let to = format!("database format:{format}\0suffix");
                data = replace_bytes(&data, from.as_bytes(), to.as_bytes());
            }
            data
        }),
        "919-binary-version-line-missing" => header_probe(k, &[Drop("binary format version")]),
        "919-model-version-line-missing" => header_probe(k, &[Drop("model data version")]),
        "919-versions-positive-overflow" => session(
            k,
            &[
                R(
                    "binary format version:7",
                    "binary format version:9223372036854775808",
                ),
                R(
                    "model data version:14",
                    "model data version:99999999999999999999",
                ),
            ],
        ),
        "919-versions-int-wrap" => session(
            k,
            &[
                R(
                    "binary format version:7",
                    "binary format version:4294967296",
                ),
                R("model data version:14", "model data version:2147483648"),
            ],
        ),
        "919-versions-negative" => session(
            k,
            &[
                R("binary format version:7", "binary format version:-1"),
                R("model data version:14", "model data version:-2"),
            ],
        ),
        "missing-with-dbms" => missing_conf_with_dbms(k),
        "user-file-absolute" => absolute_user_file(k),
        "addon-cleanup" => addon_cleanup(k),
        // The `strtol` behind `%d` and `atoi` saturates at LONG_MIN on a negative
        // overflow, which the stores into an `int` and a `guint8` cut to 0.
        "versions-negative-overflow" => session(
            k,
            &[
                R(
                    "binary format version:7",
                    "binary format version:-9223372036854775808",
                ),
                R(
                    "model data version:14",
                    "model data version:-99999999999999999999",
                ),
            ],
        ),
        // The names are the bytes the word holds.
        "user-file-not-utf8" => session_bytes(k, |data| {
            let from: &[u8] = b"user.bin USER_FILE";
            assert!(data.windows(from.len()).any(|window| window == from));
            replace_bytes(&data, from, b"u\xff.bin USER_FILE")
        }),
        // `_write_files` writes the library files by sub-index, then the two
        // indices and the bigram, and `_rename_files` renames in the same order.
        "colliding-library-files" => session(
            k,
            &[
                R("network.bin USER_FILE", "shared.bin USER_FILE"),
                R("user.bin USER_FILE", "shared.bin USER_FILE"),
            ],
        ),
        "library-named-like-an-index" => session(
            k,
            &[R("user.bin USER_FILE", "user_pinyin_index.bin USER_FILE")],
        ),
        "log-named-like-a-library" => {
            session(k, &[R("gb_char.dbin SYSTEM_FILE", "user.bin SYSTEM_FILE")])
        }
        // libpinyin's check_format saves the marker, then wipes a non-conforming
        // profile (074a2219 pinyin.cpp:187, :194-215).
        "user-library-named-user-conf" => {
            session(k, &[R("user.bin USER_FILE", "user.conf USER_FILE")])
        }
        "candidates-files-swapped" => candidates_probe(
            k,
            &[
                R("gb_char.table gb_char.bin", "gb_char.table TMP.bin"),
                R("gbk_char.table gbk_char.bin", "gbk_char.table gb_char.bin"),
                R("TMP.bin", "gbk_char.bin"),
            ],
        ),
        other => panic!("unknown table_conf_scanner case {other}"),
    }
}
