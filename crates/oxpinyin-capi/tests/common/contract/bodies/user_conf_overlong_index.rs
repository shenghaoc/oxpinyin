//! The user_conf_overlong_index family's case bodies: each answers the result document the old case returned.
//!
//! user.conf integer overflow and state preservation (#920), and over-long
//! chewing index keys (inventory row 56, row 84, #525, #1175).

use std::path::Path;

use oxpinyin_store::{DefaultStore, WriteStore as _, WriteTxn as _};

use crate::bodies::case_helpers::{Trained, token_unigram_trained};
use crate::facade;
use crate::fixtures::{craft_tree, private_system, user_listing, write_user_conf};
use crate::readers::{c, tokens_of, unigram_of};
use crate::support::{J, Kit, P, UNTOUCHED, borrowed_text};

/// The two version keys of table.conf and user.conf.
const VERSION_KEYS: [&str; 2] = ["binary format version:", "model data version:"];

/// `_user_conf_values`: table.conf's version integers, later lines winning.
fn user_conf_values(k: &Kit) -> [i128; 2] {
    let text = std::fs::read_to_string(k.data.join("table.conf")).expect("table.conf");
    let mut values = [None, None];
    for line in text.lines() {
        for (slot, key) in values.iter_mut().zip(VERSION_KEYS) {
            if let Some(rest) = line.strip_prefix(key) {
                *slot = Some(rest.trim().parse::<i128>().expect("an integer version"));
            }
        }
    }
    values.map(|value| value.expect("table.conf names both versions"))
}

/// user.conf's bytes split into lines, each without its `\n`.
fn conf_lines(k: &Kit) -> Vec<Vec<u8>> {
    let data = std::fs::read(k.user.join("user.conf")).expect("user.conf");
    data.split(|byte| *byte == b'\n')
        .map(<[u8]>::to_vec)
        .collect()
}

/// The lines joined back with `\n`.
fn join_lines(lines: &[Vec<u8>]) -> Vec<u8> {
    lines.join(&b'\n')
}

/// `_replace_conf_value`: the first line beginning with `key` becomes `key` and `value`.
fn replace_conf_value(lines: &mut [Vec<u8>], key: &str, value: &str) {
    if let Some(line) = lines
        .iter_mut()
        .find(|line| line.starts_with(key.as_bytes()))
    {
        *line = format!("{key}{value}").into_bytes();
    }
}

/// `re.sub(r'(?m)^key.*$', b'', data)`: every line beginning with `key` emptied.
fn empty_lines(lines: &mut [Vec<u8>], key: &str) {
    for line in lines.iter_mut() {
        if line.starts_with(key.as_bytes()) {
            line.clear();
        }
    }
}

/// `_edit_user_conf_nul`: a NUL and junk after the first database-format value.
fn edit_nul(lines: &mut [Vec<u8>]) {
    const KEY: &[u8] = b"database format:";
    for line in lines.iter_mut() {
        if let Some(rest) = line.strip_prefix(KEY) {
            let run = rest
                .iter()
                .take_while(|byte| !matches!(byte, b'\r' | b'\0'))
                .count();
            if run > 0 {
                let at = KEY.len() + run;
                line.splice(at..at, b"\x00junk".iter().copied());
                return;
            }
        }
    }
}

/// The edited user.conf bytes the case `edit` names.
fn edited_conf(k: &Kit, edit: &str) -> Vec<u8> {
    let mut lines = conf_lines(k);
    match edit {
        "nul-database-token" => edit_nul(&mut lines),
        "version-overflow-conforms" | "version-overflow-nonconforms" => {
            let extra = i128::from(edit == "version-overflow-nonconforms");
            for (key, value) in VERSION_KEYS.into_iter().zip(user_conf_values(k)) {
                replace_conf_value(&mut lines, key, &(value + (1 << 32) + extra).to_string());
            }
        }
        "counter-overflow-conforms" => {
            replace_conf_value(&mut lines, "open counter:", &(1_u64 << 32).to_string());
        }
        "counter-overflow-nonconforms" => {
            replace_conf_value(
                &mut lines,
                "open counter:",
                &((1_u64 << 32) + 7).to_string(),
            );
        }
        "negative-overflow-nonconforms" => {
            for key in VERSION_KEYS {
                replace_conf_value(&mut lines, key, &i64::MIN.to_string());
            }
        }
        "database-missing-at-eof" => {
            empty_lines(&mut lines, "database format:");
            empty_lines(&mut lines, "open counter:");
        }
        "counter-missing" => empty_lines(&mut lines, "open counter:"),
        other => panic!("unknown user.conf edit {other}"),
    }
    join_lines(&lines)
}

/// Whether the edit keeps the trained and imported state.
fn keeps_state(edit: &str) -> bool {
    matches!(
        edit,
        "nul-database-token"
            | "version-overflow-conforms"
            | "counter-overflow-conforms"
            | "counter-missing"
    )
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

/// `token_unigram_user(k, add_frequency=False)`: the document, the token and
/// the unigram after.
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

/// `unigram_of` on `inst`.
fn unigram_on(inst: P, token: u32) -> (bool, u32) {
    let mut freq = UNTOUCHED;
    let ret = facade::token_get_unigram_frequency(inst, token, &raw mut freq);
    (ret, freq)
}

/// `_user_conf_920_session`: train and import, save, replace the marker, reopen.
fn user_conf_920_session(k: &mut Kit, edit: &str) -> J {
    let trained = token_unigram_trained(k);
    let (imported, import_token, import_after) = token_unigram_user_no_add(k);
    let saved = facade::save(k.ctx());
    assert!(saved, "the trained profile must save");
    k.close();
    std::fs::write(k.user.join("user.conf"), edited_conf(k, edit)).expect("write user.conf");
    let ctx = k.init_default();
    let mut out = obj! {
        "reopen" => !ctx.is_null(),
        "training" => trained_j(&trained),
        "import" => imported,
        "save" => saved,
    };
    if !ctx.is_null() {
        let inst = facade::alloc_instance(ctx);
        let token = u32::from_str_radix(trained.token.trim_start_matches("0x"), 16).expect("token");
        let trained_after = unigram_on(inst, token);
        let import_after_reopen = unigram_on(inst, import_token);
        if keeps_state(edit) {
            assert_eq!(trained_after, trained.after, "trained state was wiped");
            assert_eq!(
                import_after_reopen, import_after,
                "imported state was wiped"
            );
        }
        out.set("trained-after", trained_after);
        out.set("import-after", import_after_reopen);
    }
    out.set("files", user_listing(k, None));
    out
}

/// `_unwritable_marker`: train and save over a user dir whose user.conf is a directory.
fn unwritable_marker(k: &mut Kit) -> J {
    std::fs::create_dir(k.user.join("user.conf")).expect("user.conf directory");
    let trained = token_unigram_trained(k);
    let saved = facade::save(k.ctx());
    obj! {
        "training" => trained_j(&trained),
        "save" => saved,
        "files" => user_listing(k, None),
    }
}

/// The first two bytes of `parse`'s first pinyin key, on a fresh instance of `ctx`.
fn first_key(ctx: P, parse: &[u8]) -> Vec<u8> {
    let inst = facade::alloc_instance(ctx);
    assert!(facade::parse_more_full_pinyins(inst, c(parse).as_ptr()) > 0);
    let mut key: P = std::ptr::null_mut();
    assert!(facade::get_pinyin_key(inst, 0, &raw mut key));
    // SAFETY: the instance's key is a live ChewingKey of two bytes until the
    // instance is freed below.
    let first = unsafe { std::slice::from_raw_parts(key.cast::<u8>(), 2) }.to_vec();
    facade::free_instance(inst);
    first
}

/// The options and the crafted key rows of [`overlong_index_context`].
struct Overlong<'a> {
    parse: Option<&'a [u8]>,
    rows: fn(&[u8]) -> Vec<Vec<u8>>,
    options: Option<u32>,
}

/// The default rows: the first key and its 17-fold repetition.
fn default_rows(first: &[u8]) -> Vec<Vec<u8>> {
    vec![first.to_vec(), first.repeat(17)]
}

/// The first key and its 17-fold repetition with one trailing byte.
fn odd_rows(first: &[u8]) -> Vec<Vec<u8>> {
    vec![first.to_vec(), [first.repeat(17), vec![7]].concat()]
}

impl Default for Overlong<'_> {
    fn default() -> Self {
        Self {
            parse: None,
            rows: default_rows,
            options: None,
        }
    }
}

/// The init on `system` (the kit's data when `None`), asserted.
fn init_on(k: &Kit, system: Option<&Path>, what: &str) -> P {
    let ctx = k.init(Some(system.unwrap_or(&k.data)), Some(&k.user));
    assert!(!ctx.is_null(), "{what}");
    ctx
}

/// `overlong_index_context`: a context whose user pinyin index holds crafted keys.
fn overlong_index_context(k: &Kit, spec: &Overlong<'_>, system: Option<&Path>) -> P {
    let ctx = init_on(k, system, "the first init failed");
    if let Some(options) = spec.options {
        facade::set_options(ctx, options);
    }
    let first = spec
        .parse
        .map_or_else(|| vec![0, 1], |parse| first_key(ctx, parse));
    facade::fini(ctx);
    let rows: Vec<(Vec<u8>, Vec<u8>)> = (spec.rows)(&first)
        .into_iter()
        .map(|key| (key, Vec::new()))
        .collect();
    craft_tree(&k.user.join("user_pinyin_index.bin"), &rows);
    write_user_conf(k);
    let ctx = init_on(k, system, "the reopen failed");
    if let Some(options) = spec.options {
        facade::set_options(ctx, options);
    }
    ctx
}

/// `_craft_db(path, rows, 1, keep=True)`: adds `rows` to the existing tree DBM
/// at `path` in this cell's format, keeping its records.
fn craft_tree_keep(path: &Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    let store = DefaultStore::create(path).expect("open the DBM");
    store
        .write(|txn| {
            for (key, value) in rows {
                txn.put_raw(key, value)?;
            }
            Ok(())
        })
        .expect("write the DBM rows");
}

/// `system_overlong_index_context`: a private system index extended with a 17-word ni key.
fn system_overlong_index_context(k: &Kit, trailing: &[u8]) -> P {
    let ctx = init_on(k, None, "the first init failed");
    let first = first_key(ctx, b"ni");
    facade::fini(ctx);
    let system = private_system(k);
    let path = system.join("pinyin_index.bin");
    std::fs::remove_file(&path).expect("unlink the linked index");
    crate::fixtures::writable_copy(&k.data.join("pinyin_index.bin"), &path);
    craft_tree_keep(
        &path,
        &[([first.repeat(17), trailing.to_vec()].concat(), Vec::new())],
    );
    init_on(
        k,
        Some(&system),
        "the open on the private system dir failed",
    )
}

/// `{'ret': mask_out(ctx, 0xFFFFFFFF, 0)}`.
fn mask_all(ctx: P) -> J {
    obj! { "ret" => facade::mask_out(ctx, 0xFFFF_FFFF, 0) }
}

/// `{'ret': guess_candidates(0, 0)}` after parsing `parse` on a fresh instance.
fn guess_after(ctx: P, parse: &[u8]) -> J {
    let inst = facade::alloc_instance(ctx);
    assert!(facade::parse_more_full_pinyins(inst, c(parse).as_ptr()) > 0);
    obj! { "ret" => facade::guess_candidates(inst, 0, 0) }
}

/// One zhuyin candidate guess: its name, the call and its offset.
type Guess = (&'static str, fn(P, usize) -> bool, usize);

/// The zhuyin candidate guesses over a user index with a 17-word key.
fn zhuyin_guesses(ctx: P) -> J {
    let inst = facade::alloc_instance(ctx);
    let mut out = obj! { "parse" => facade::parse_more_chewings(inst, c("su3cl3").as_ptr()) };
    let guesses: [Guess; 2] = [
        (
            "guess_candidates_after_cursor",
            facade::guess_candidates_after_cursor,
            0,
        ),
        (
            "guess_candidates_before_cursor",
            facade::guess_candidates_before_cursor,
            6,
        ),
    ];
    for (name, guess, offset) in guesses {
        let ret = guess(inst, offset);
        let mut count = 0_u32;
        facade::get_n_candidate(inst, &raw mut count);
        let rows: Vec<Option<String>> = (0..count)
            .map(|index| {
                let mut cand: P = std::ptr::null_mut();
                facade::get_candidate(inst, index, &raw mut cand);
                let mut text: *const std::ffi::c_char = std::ptr::null();
                facade::get_candidate_string(inst, cand, &raw mut text);
                borrowed_text(text)
            })
            .collect();
        out.set(name, obj! { "ret" => ret, "rows" => rows });
    }
    out
}

/// The over-long index cases.
fn overlong_case(k: &mut Kit, id: &str) -> J {
    let ni = Some(&b"ni"[..]);
    match id {
        "abort-mask-out-overlong-index-key" | "abort-zhuyin-mask-out-overlong-index-key" => {
            mask_all(overlong_index_context(k, &Overlong::default(), None))
        }
        "abort-mask-out-overlong-index-key-later-unmappable"
        | "abort-zhuyin-mask-out-overlong-index-key-later-unmappable" => {
            let system = private_system(k);
            let ctx = overlong_index_context(k, &Overlong::default(), Some(&system));
            std::fs::remove_file(system.join("merged.bin")).expect("unlink merged.bin");
            mask_all(ctx)
        }
        "zhuyin-guess-candidates-overlong-index-key" => {
            zhuyin_guesses(overlong_index_context(k, &Overlong::default(), None))
        }
        "abort-guess-candidates-overlong-index-key" => {
            let spec = Overlong {
                parse: ni,
                ..Overlong::default()
            };
            guess_after(overlong_index_context(k, &spec, None), b"ni")
        }
        "abort-guess-candidates-overlong-odd-index-key" => {
            let spec = Overlong {
                parse: ni,
                rows: odd_rows,
                options: None,
            };
            guess_after(overlong_index_context(k, &spec, None), b"ni")
        }
        _ => overlong_rows_case(k, id),
    }
}

/// The over-long index cases with crafted row lists.
fn overlong_rows_case(k: &mut Kit, id: &str) -> J {
    let ni = Some(&b"ni"[..]);
    let rows_only = |rows| Overlong {
        rows,
        ..Overlong::default()
    };
    match id {
        "abort-mask-out-overlong-odd-index-key" => {
            mask_all(overlong_index_context(k, &rows_only(odd_rows), None))
        }
        "abort-mask-out-empty-index-key" | "abort-zhuyin-mask-out-empty-index-key" => {
            let spec = rows_only(|first| vec![Vec::new(), first.to_vec()]);
            mask_all(overlong_index_context(k, &spec, None))
        }
        "abort-mask-out-one-byte-index-key" => {
            let spec = rows_only(|first| vec![first.to_vec(), vec![7]]);
            mask_all(overlong_index_context(k, &spec, None))
        }
        "abort-guess-candidates-overlong-system-index-key" => {
            guess_after(system_overlong_index_context(k, b""), b"ni")
        }
        "abort-guess-candidates-overlong-odd-system-index-key" => {
            guess_after(system_overlong_index_context(k, b"\x07"), b"ni")
        }
        "guess-after-overlong-index-key-without-prefix-answers-true" => {
            let spec = Overlong {
                parse: ni,
                rows: |first| vec![first.repeat(17)],
                options: None,
            };
            guess_after(overlong_index_context(k, &spec, None), b"ni")
        }
        _ => overlong_guess_case(k, id),
    }
}

/// The over-long index cases that parse and guess past the crafted keys.
fn overlong_guess_case(k: &mut Kit, id: &str) -> J {
    let ni = Some(&b"ni"[..]);
    match id {
        "reparse-after-guess-ignores-overlong-index-key" => {
            let spec = Overlong {
                parse: ni,
                ..Overlong::default()
            };
            let ctx = overlong_index_context(k, &spec, None);
            let inst = facade::alloc_instance(ctx);
            assert!(facade::parse_more_full_pinyins(inst, c("hao").as_ptr()) > 0);
            let guessed = facade::guess_candidates(inst, 0, 0);
            let reparsed = facade::parse_more_full_pinyins(inst, c("ni").as_ptr());
            obj! { "guess_hao" => guessed, "reparse_ni" => reparsed }
        }
        "abort-guess-candidates-overlong-incomplete-index-key" => {
            let spec = Overlong {
                parse: Some(&b"n"[..]),
                rows: |first| {
                    let prefix = [first, b"\x00\x00"].concat();
                    vec![prefix.clone(), [prefix, b"\x01\x00".repeat(15)].concat()]
                },
                options: Some(8), // PINYIN_INCOMPLETE
            };
            guess_after(overlong_index_context(k, &spec, None), b"n'an")
        }
        other => panic!("unknown user_conf_overlong_index case {other}"),
    }
}

/// The case `id`'s result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    if id.starts_with("user-conf-marker-unwritable") {
        return unwritable_marker(k);
    }
    if let Some(edit) = id.strip_prefix("user-conf-920-") {
        let edit = edit.strip_suffix("-zhuyin").unwrap_or(edit);
        return user_conf_920_session(k, edit);
    }
    overlong_case(k, id)
}
