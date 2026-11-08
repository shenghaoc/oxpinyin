//! The library_reload family's case bodies: each answers the result document the old case returned.
//!
//! Phrase library unload and reload (inventory row 55, #805, #1175).
//!
//! The old cases drop the kit's cached instance without freeing it
//! (`k._inst = None`) while keeping its context, so these bodies keep their own
//! lazily opened context and instance in a [`Session`] with the kit's paths.

use std::collections::BTreeMap;
use std::ffi::c_char;
use std::path::PathBuf;

use crate::facade;
use crate::fixtures::{chunk, modify_header, write_log, write_user_conf};
use crate::readers::{
    c, candidate_at, candidate_text, candidate_type, final_step_sentences, garray_u16, guess_rows,
    lookup, n_candidate, owned_out, seeded, segment_out,
};
use crate::support::{J, Kit, P, UNTOUCHED, borrowed_text, path_c, take_text};

/// The oracle data's loaded libraries' totals, summed.
const FACADE_TOTAL: i64 = 51_051_831;
/// The phrases whose place in a candidate list the snapshots watch.
const RELOAD_WATCH: [&str; 2] = ["礙", "龘龘"];

/// The kit's cached context and instance as the old cases used them: each
/// opened on first use, the instance droppable without a free.
struct Session {
    data: PathBuf,
    user: PathBuf,
    ctx: P,
    inst: P,
}

impl Session {
    fn new(k: &Kit) -> Self {
        Self {
            data: k.data.clone(),
            user: k.user.clone(),
            ctx: std::ptr::null_mut(),
            inst: std::ptr::null_mut(),
        }
    }

    /// `k.ctx`: the context, initialised and asserted on first use.
    fn ctx(&mut self) -> P {
        if self.ctx.is_null() {
            let (data, user) = (path_c(&self.data), path_c(&self.user));
            self.ctx = facade::init(data.as_ptr(), user.as_ptr());
            assert!(!self.ctx.is_null(), "init failed");
        }
        self.ctx
    }

    /// `k.alloc()`: a fresh instance of the context.
    fn alloc(&mut self) -> P {
        let inst = facade::alloc_instance(self.ctx());
        assert!(!inst.is_null());
        inst
    }

    /// `k.inst`: the cached instance, allocated on first use.
    fn inst(&mut self) -> P {
        if self.inst.is_null() {
            self.inst = self.alloc();
        }
        self.inst
    }

    /// `k._inst = None`: forgets the instance without freeing it.
    fn drop_inst(&mut self) {
        self.inst = std::ptr::null_mut();
    }

    /// Frees the instance and finalises the context, forgetting both.
    fn close(&mut self) {
        facade::free_instance(self.inst());
        facade::fini(self.ctx);
        self.ctx = std::ptr::null_mut();
        self.inst = std::ptr::null_mut();
    }
}

/// Python's `hex(token)`.
fn hex_token(token: u32) -> String {
    format!("{token:#x}")
}

/// Bytes as lowercase hex.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `tokens_of` on the session's cached instance.
fn tokens_of(s: &mut Session, phrase: &str) -> Vec<u32> {
    lookup(s.inst(), phrase.as_bytes())
}

/// `unigram_of` on the session's cached instance.
fn unigram_of(s: &mut Session, token: u32) -> (bool, u32) {
    let mut freq = UNTOUCHED;
    let ret = facade::token_get_unigram_frequency(s.inst(), token, &raw mut freq);
    (ret, freq)
}

/// A GArray of two-byte keys as hex, `''` when empty.
fn keys_hex(keys: *mut glib_sys::GArray) -> String {
    let bytes: Vec<u8> = garray_u16(keys)
        .iter()
        .flat_map(|key| key.to_le_bytes())
        .collect();
    hex(&bytes)
}

/// `_reload_pron_keys`: the pronunciation count and each reading's keys as hex.
/// The arrays are not freed, as the old helper left them.
fn reload_pron_keys(s: &mut Session, token: u32) -> J {
    let inst = s.inst();
    let mut count = UNTOUCHED;
    let ok = facade::token_get_n_pronunciation(inst, token, &raw mut count);
    let mut out = vec![J::from(ok), J::from(count)];
    for nth in 0..if ok { count } else { 0 } {
        // SAFETY: a fresh GArray of two-byte elements, left allocated as the
        // old helper left it.
        let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
        facade::token_get_nth_pronunciation(inst, token, nth, keys.cast());
        out.push(J::from(keys_hex(keys)));
    }
    J::L(out)
}

/// `import_rows` into `library` on the session's context.
fn import_rows(s: &mut Session, library: u8, rows: &[(&str, &str, i32)]) -> Vec<bool> {
    let it = facade::begin_add_phrases(s.ctx(), library);
    let added = rows
        .iter()
        .map(|(phrase, reading, count)| {
            facade::iterator_add_phrase(it, c(phrase).as_ptr(), c(reading).as_ptr(), *count)
        })
        .collect();
    facade::end_add_phrases(it);
    added
}

/// The facade's parse of `text`: full pinyin, or chewing on zhuyin.
fn parse(inst: P, text: &[u8]) -> usize {
    if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c(text).as_ptr())
    } else {
        facade::parse_more_full_pinyins(inst, c(text).as_ptr())
    }
}

/// The facade's candidate guess at offset 0 (pinyin sort word 0x1e).
fn guess_at_start(inst: P) -> bool {
    if facade::ZHUYIN {
        facade::guess_candidates_after_cursor(inst, 0)
    } else {
        facade::guess_candidates(inst, 0, 0x1e)
    }
}

/// `_reload_candidates`: the count, the first three rows and the watched
/// phrases' places in a fresh guess's list.
fn reload_candidates(s: &mut Session, text: &[u8]) -> J {
    let inst = s.alloc();
    parse(inst, text);
    guess_at_start(inst);
    let mut count = 0_u32;
    facade::get_n_candidate(inst, &raw mut count);
    let (mut head, mut place) = (Vec::new(), BTreeMap::new());
    for index in 0..count {
        let mut cand: P = std::ptr::null_mut();
        facade::get_candidate(inst, index, &raw mut cand);
        let mut text_out: *const c_char = std::ptr::null();
        facade::get_candidate_string(inst, cand, &raw mut text_out);
        let mut kind = 0_i32;
        facade::get_candidate_type(inst, cand, &raw mut kind);
        let shown = borrowed_text(text_out).filter(|text| !text.is_empty());
        if index < 3 {
            head.push(J::from((shown.clone(), kind)));
        }
        if let Some(shown) = shown.filter(|text| RELOAD_WATCH.contains(&text.as_str())) {
            place.insert(shown, J::from((index, kind)));
        }
    }
    facade::free_instance(inst);
    J::from((count, head, place))
}

/// `_reload_snapshot`: tokens, unigrams, readings and candidates after dropping the instance.
fn reload_snapshot(s: &mut Session, phrases: &[&str], inputs: &[&[u8]]) -> J {
    s.drop_inst();
    let mut out = J::map();
    for phrase in phrases {
        let tokens = tokens_of(s, phrase);
        out.set(
            format!("tokens {phrase}"),
            tokens
                .iter()
                .map(|token| hex_token(*token))
                .collect::<Vec<_>>(),
        );
        for token in tokens {
            let tag = format!("{phrase} {}", hex_token(token));
            out.set(format!("unigram {tag}"), unigram_of(s, token));
            out.set(format!("readings {tag}"), reload_pron_keys(s, token));
        }
    }
    for text in inputs {
        let name = format!("candidates {}", String::from_utf8_lossy(text));
        out.set(name, reload_candidates(s, text));
    }
    out
}

/// `train_selected_top`: chooses the NORMAL candidate `wanted`, guesses and
/// trains result 0, on the session's cached instance.
fn train_selected_top(s: &mut Session, wanted: &str, token: u32) {
    let inst = s.inst();
    assert!(guess_at_start(inst));
    for index in 0..n_candidate(inst) {
        let cand = candidate_at(inst, index).expect("candidate");
        let text = candidate_text(inst, cand).expect("candidate text");
        if candidate_type(inst, cand) == 2 && text == wanted {
            let cursor = facade::choose_candidate(inst, 0, cand);
            assert!(cursor > 0, "normal phrase selection failed");
            unigram_of(s, token);
            assert!(
                facade::guess_sentence(inst),
                "training needs a guessed n-best result"
            );
            assert!(facade::train_top(inst), "training n-best result 0 failed");
            return;
        }
    }
    panic!("missing normal candidate: {wanted}");
}

/// One session change of [`reload_probe`].
enum Change {
    /// `_reload_train(phrase, token_library, reading)`.
    Train(&'static str, u32, &'static [u8]),
    /// `_reload_import(library, rows, phrases, inputs)`.
    Import(
        u8,
        (&'static str, &'static str, i32),
        &'static [&'static str],
        &'static [&'static [u8]],
    ),
}

impl Change {
    /// The watched phrases and inputs (`change(k, False)`).
    fn watched(&self) -> (Vec<&'static str>, Vec<&'static [u8]>) {
        match self {
            Change::Train(phrase, _, reading) => (vec![*phrase], vec![*reading]),
            Change::Import(_, _, phrases, inputs) => (phrases.to_vec(), inputs.to_vec()),
        }
    }

    /// Applies the change (`change(k, True)`).
    fn apply(&self, s: &mut Session) {
        match self {
            Change::Train(phrase, library, reading) => {
                assert_eq!(parse(s.inst(), reading), reading.len());
                let token = tokens_of(s, phrase)
                    .into_iter()
                    .find(|token| token >> 24 == *library)
                    .expect("a token of the library");
                train_selected_top(s, phrase, token);
            }
            Change::Import(library, row, _, _) => {
                import_rows(s, *library, &[*row]);
            }
        }
    }
}

/// Unloads and loads `library` again, dropping the instance after each, into `out`.
fn unload_load(s: &mut Session, library: u8, out: &mut J) {
    out.set("unload", facade::unload_phrase_library(s.ctx(), library));
    s.drop_inst();
    out.set("load", facade::load_phrase_library(s.ctx(), library));
    s.drop_inst();
}

/// `_reload_probe`: snapshots around a change and an unload/load pair.
fn reload_probe(k: &Kit, change: &Change, library: u8, save_first: bool) -> J {
    let mut s = Session::new(k);
    let (phrases, inputs) = change.watched();
    let mut out = obj! { "before" => reload_snapshot(&mut s, &phrases, &inputs) };
    change.apply(&mut s);
    out.set("changed", reload_snapshot(&mut s, &phrases, &inputs));
    if save_first {
        out.set("save", facade::save(s.ctx()));
    }
    unload_load(&mut s, library, &mut out);
    out.set("reloaded", reload_snapshot(&mut s, &phrases, &inputs));
    out
}

/// The change a `reload-*-<mode>-<name>` case makes, and its library.
fn reload_change(name: &str) -> (Change, u8) {
    let change = match (facade::ZHUYIN, name) {
        (false, "train") => Change::Train("礙", 2, b"ai4"),
        (false, "reading") => Change::Import(2, ("礙", "ai1", 50), &["礙"], &[b"ai1"]),
        (false, "new-item") => Change::Import(2, ("龘龘", "da3'da3", 50), &["龘龘"], &[]),
        (false, "train-library-1") => Change::Train("癫痫", 1, b"dian4xian4"),
        (true, "train") => Change::Train("礙", 2, b"94"),
        (true, "reading") => Change::Import(2, ("礙", "ㄞ", 50), &["礙"], &[b"9"]),
        (true, "new-item") => Change::Import(2, ("龘龘", "ㄉㄚˇ ㄉㄚˇ", 50), &["龘龘"], &[]),
        (true, "train-library-1") => Change::Train("癫痫", 1, b"2u04vu04"),
        (_, other) => panic!("unknown reload change {other}"),
    };
    let library = if name == "train-library-1" { 1 } else { 2 };
    (change, library)
}

/// One `_reload_user` probe's parameters.
struct UserReload {
    setup: Vec<(&'static str, &'static str, i32)>,
    change: Vec<(&'static str, &'static str, i32)>,
    inputs: Vec<&'static [u8]>,
    save_setup: bool,
    save_change: bool,
}

/// The `_RELOAD_USER` probe the case `name` (`item`, `item-saved`, ...) runs.
fn user_reload(name: &str) -> UserReload {
    let (first, second, input): (&str, &str, &[u8]) = if facade::ZHUYIN {
        ("ㄏㄠˇ ㄋㄧˇ", "ㄏㄠˋ ㄋㄧˇ", b"cl3su3")
    } else {
        ("hao3'ni3", "hao4'ni3", b"hao3ni3")
    };
    let item = vec![("好你", first, 50)];
    let more = vec![("好你", second, 50)];
    let (setup, change, inputs, save_setup, save_change) = match name {
        "item" => (vec![], item, vec![], false, false),
        "item-saved" => (vec![], item, vec![input], false, true),
        "reading" => (item, more, vec![input], true, false),
        "reading-saved" => (item, more, vec![input], true, true),
        other => panic!("unknown user reload {other}"),
    };
    UserReload {
        setup,
        change,
        inputs,
        save_setup,
        save_change,
    }
}

/// `_reload_user`: a user-library reload probe with independent saves.
fn reload_user(k: &Kit, probe: &UserReload) -> J {
    let mut s = Session::new(k);
    let phrases = ["好你"];
    if !probe.setup.is_empty() {
        import_rows(&mut s, 7, &probe.setup);
        if probe.save_setup {
            assert!(facade::save(s.ctx()));
        }
    }
    s.drop_inst();
    let mut out = obj! { "before" => reload_snapshot(&mut s, &phrases, &probe.inputs) };
    import_rows(&mut s, 7, &probe.change);
    out.set("changed", reload_snapshot(&mut s, &phrases, &probe.inputs));
    if probe.save_change {
        out.set("save", facade::save(s.ctx()));
    }
    unload_load(&mut s, 7, &mut out);
    out.set("reloaded", reload_snapshot(&mut s, &phrases, &probe.inputs));
    out
}

/// `_reload_reuse_setup`: an unsaved zhuyin user phrase, then its library reloaded.
fn reload_reuse_setup(s: &mut Session) -> J {
    let mut out = obj! { "first" => import_rows(s, 7, &[("好你", "ㄏㄠˇ ㄋㄧˇ", 50)]) };
    unload_load(s, 7, &mut out);
    out
}

/// The `reload-user-zhuyin-reuse-*` cases.
fn reuse_case(k: &Kit, name: &str) -> J {
    let mut s = Session::new(k);
    let mut out = reload_reuse_setup(&mut s);
    if name == "same" {
        let it = facade::begin_add_phrases(s.ctx(), 7);
        let ret =
            facade::iterator_add_phrase(it, c("好你").as_ptr(), c("ㄏㄠˇ ㄋㄧˇ").as_ptr(), 50);
        return obj! { "ret" => ret };
    }
    out.set(
        "again",
        import_rows(&mut s, 7, &[("你好", "ㄋㄧˇ ㄏㄠˇ", 50)]),
    );
    let inputs: [&[u8]; 3] = [b"cl3su3", b"su3cl3", b"cl3cl3"];
    let after = match name {
        "other" => reload_snapshot(&mut s, &["好你", "你好"], &inputs[..2]),
        "other-save-reopen" => {
            out.set("save", facade::save(s.ctx()));
            s.drop_inst();
            facade::fini(s.ctx());
            s.ctx = std::ptr::null_mut();
            reload_snapshot(&mut s, &["好你", "你好"], &inputs[..2])
        }
        "other-mask" => {
            out.set("mask", facade::mask_out(s.ctx(), 0xFF00_0000, 0x0700_0000));
            s.drop_inst();
            out.set(
                "third",
                import_rows(&mut s, 7, &[("好好", "ㄏㄠˇ ㄏㄠˇ", 50)]),
            );
            reload_snapshot(&mut s, &["好你", "你好", "好好"], &inputs)
        }
        other => panic!("unknown reuse case {other}"),
    };
    out.set("after", after);
    out
}

/// `_reload_dangling`: a candidate walk over an imported token after its library reload.
fn reload_dangling(k: &Kit) -> J {
    let mut s = Session::new(k);
    let (walk, reading): (&[u8], &str) = if facade::ZHUYIN {
        (b"283283", "ㄉㄚˇ ㄉㄚˇ")
    } else {
        (b"dada", "da3'da3")
    };
    import_rows(&mut s, 2, &[("龘龘", reading, 50)]);
    s.drop_inst();
    let token = tokens_of(&mut s, "龘龘")
        .into_iter()
        .find(|token| token >> 24 == 2)
        .expect("a library 2 token");
    facade::unload_phrase_library(s.ctx(), 2);
    s.drop_inst();
    facade::load_phrase_library(s.ctx(), 2);
    s.drop_inst();
    let tokens: Vec<String> = tokens_of(&mut s, "龘龘")
        .into_iter()
        .map(hex_token)
        .collect();
    obj! {
        "tokens" => tokens,
        "unigram" => unigram_of(&mut s, token),
        "readings" => reload_pron_keys(&mut s, token),
        "candidates" => reload_candidates(&mut s, walk),
    }
}

/// `sentence_out(k, inst, 0)`.
fn sentence_out(inst: P) -> J {
    let mut out = seeded();
    let ret = facade::get_sentence_at(inst, 0, &raw mut out);
    J::from((ret, owned_out(out)))
}

/// `_reload_total_window`'s `look`: four fresh-instance sentences and candidate lists.
fn look(s: &mut Session, tag: &str, out: &mut J) {
    s.drop_inst();
    for text in ["nihao", "women", "xian", "zhongguo"] {
        let inst = s.alloc();
        facade::parse_more_full_pinyins(inst, c(text).as_ptr());
        facade::guess_sentence(inst);
        out.set(format!("{tag} sentence {text}"), sentence_out(inst));
        let mut rows = guess_rows(inst, 0, 0x1e).rows;
        rows.truncate(12);
        out.set(format!("{tag} candidates {text}"), rows);
        facade::free_instance(inst);
    }
}

/// `_reload_total_window`: sentence and candidate order across import, unload and reload.
fn reload_total_window(k: &Kit) -> J {
    let mut s = Session::new(k);
    let mut out = J::map();
    look(&mut s, "loaded", &mut out);
    import_rows(
        &mut s,
        2,
        &[
            ("礙", "ai1", 2_000_000_000),
            ("龘龘", "da3'da3", 2_000_000_000),
        ],
    );
    look(&mut s, "imported", &mut out);
    out.set("unload", facade::unload_phrase_library(s.ctx(), 2));
    look(&mut s, "unloaded", &mut out);
    out.set("load", facade::load_phrase_library(s.ctx(), 2));
    look(&mut s, "reloaded", &mut out);
    out
}

/// `_facade_total_out` on the session's cached instance (pinyin).
fn facade_total_out(s: &mut Session, out: &mut J) {
    let inst = s.inst();
    out.set(
        "parsed",
        facade::parse_more_full_pinyins(inst, c("ni3'hao3").as_ptr()),
    );
    out.set("guessed", facade::guess_sentence(inst));
    out.set("sentences", final_step_sentences(inst));
    let mut candidates = guess_rows(inst, 0, 0x1e).rows;
    candidates.truncate(8);
    out.set("candidates", candidates);
    out.set("segment", segment_out(inst, &["你好"]));
}

/// A system library chunk's stored total, after the MemoryChunk header.
fn stored_library_total(k: &Kit, name: &str) -> i64 {
    let bytes = std::fs::read(k.data.join(name)).expect("a system library");
    i64::from(u32::from_le_bytes(
        bytes[8..12].try_into().expect("a total word"),
    ))
}

/// `mask_out_reload`: a saved, reopened large import masked by its exact token.
fn mask_out_reload(k: &Kit, compact: bool) -> J {
    let mut s = Session::new(k);
    let count = ((1_i64 << 32) + 1_900_001 - FACADE_TOTAL) / 3;
    assert!(stored_library_total(k, "gb_char.bin") + 3 * count <= 0xFFFF_FFFF);
    let count = i32::try_from(count).expect("an i32 count");
    let mut out = obj! { "added" => import_rows(&mut s, 1, &[("龘龘", "da3'da3", count)]) };
    out.set("saved", facade::save(s.ctx()));
    facade::fini(s.ctx());
    s.ctx = std::ptr::null_mut();
    s.drop_inst();
    let token = tokens_of(&mut s, "龘龘")
        .into_iter()
        .find(|token| token >> 24 == 1)
        .expect("a library 1 token");
    if compact {
        out.set("import", import_rows(&mut s, 7, &[("好你", "hao3'ni3", 2)]));
    }
    out.set("mask", facade::mask_out(s.ctx(), 0xFFFF_FFFF, token));
    s.drop_inst();
    facade_total_out(&mut s, &mut out);
    out
}

/// `_digest`: a user file's SHA-256, `None` when absent.
fn digest(s: &Session, name: &str) -> Option<String> {
    use sha2::Digest as _;
    let bytes = std::fs::read(s.user.join(name)).ok()?;
    Some(hex(&sha2::Sha256::digest(bytes)))
}

/// One `_unload_save` probe's parameters.
struct UnloadSave {
    library: u8,
    file: &'static str,
    saved: (&'static str, &'static str, i32),
    unsaved: (&'static str, &'static str, i32),
    phrases: &'static [&'static str],
    inputs: &'static [&'static [u8]],
}

/// The `unload-save-skips-*` case `name`'s parameters.
fn unload_save_params(name: &str) -> UnloadSave {
    match (facade::ZHUYIN, name) {
        (false, "library") => UnloadSave {
            library: 2,
            file: "gbk_char.dbin",
            saved: ("礙", "ai1", 50),
            unsaved: ("龘龘", "da3'da3", 50),
            phrases: &["礙", "龘龘"],
            inputs: &[b"ai1"],
        },
        (true, "library") => UnloadSave {
            library: 2,
            file: "gbk_char.dbin",
            saved: ("礙", "ㄞ", 50),
            unsaved: ("龘龘", "ㄉㄚˇ ㄉㄚˇ", 50),
            phrases: &["礙", "龘龘"],
            inputs: &[b"9"],
        },
        (true, "user-library") => UnloadSave {
            library: 7,
            file: "user.bin",
            saved: ("好你", "ㄏㄠˇ ㄋㄧˇ", 50),
            unsaved: ("好你", "ㄏㄠˋ ㄋㄧˇ", 50),
            phrases: &["好你"],
            inputs: &[b"cl3su3"],
        },
        (_, other) => panic!("unknown unload-save case {other}"),
    }
}

/// `_unload_save`: import and save, change both libraries, unload, save, load.
fn unload_save(k: &Kit, p: &UnloadSave) -> J {
    let mut s = Session::new(k);
    let other = if facade::ZHUYIN {
        ("癫痫", "ㄉㄧㄢˇ ㄒㄧㄢˋ", 50)
    } else {
        ("癫痫", "dian3'xian4", 50)
    };
    let mut out = obj! { "saved import" => import_rows(&mut s, p.library, &[p.saved]) };
    out.set("save", facade::save(s.ctx()));
    let saved = digest(&s, p.file);
    out.set("file saved", saved.is_some());
    out.set(
        "unsaved import",
        import_rows(&mut s, p.library, &[p.unsaved]),
    );
    out.set("other import", import_rows(&mut s, 1, &[other]));
    out.set("unload", facade::unload_phrase_library(s.ctx(), p.library));
    s.drop_inst();
    out.set("save unloaded", facade::save(s.ctx()));
    out.set("file unchanged", digest(&s, p.file) == saved);
    out.set("load", facade::load_phrase_library(s.ctx(), p.library));
    s.drop_inst();
    out.set("reloaded", reload_snapshot(&mut s, p.phrases, p.inputs));
    out
}

/// `_reload_unreadable`: library 2's saved log made unreadable before its reload.
fn reload_unreadable(k: &Kit, missing: bool) -> J {
    let mut s = Session::new(k);
    let (row, input): ((&str, &str, i32), &[u8]) = if facade::ZHUYIN {
        (("礙", "ㄞ", 50), b"9")
    } else {
        (("礙", "ai1", 50), b"ai1")
    };
    let (phrases, inputs) = (["礙"], [input]);
    let mut out = obj! { "import" => import_rows(&mut s, 2, &[row]) };
    out.set("save", facade::save(s.ctx()));
    s.drop_inst();
    out.set("saved", reload_snapshot(&mut s, &phrases, &inputs));
    out.set("unload", facade::unload_phrase_library(s.ctx(), 2));
    let log = s.user.join("gbk_char.dbin");
    if missing {
        std::fs::remove_file(&log).expect("remove the log");
    } else {
        std::fs::write(&log, b"abc").expect("shorten the log");
    }
    out.set("load", facade::load_phrase_library(s.ctx(), 2));
    s.drop_inst();
    out.set("reloaded", reload_snapshot(&mut s, &phrases, &inputs));
    out
}

/// `_persisted_item_getters`: text, unigram, count and each nth reading.
fn persisted_item_getters(s: &mut Session, token: u32) -> J {
    let inst = s.inst();
    let mut count = 0_u32;
    let ok = facade::token_get_n_pronunciation(inst, token, &raw mut count);
    let mut readings = Vec::new();
    for nth in 0..if ok { count } else { 0 } {
        // SAFETY: a fresh GArray of two-byte elements, freed below.
        let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
        let got = facade::token_get_nth_pronunciation(inst, token, nth, keys.cast());
        // SAFETY: `keys` is the live GArray allocated above.
        let len = unsafe { (*keys).len };
        readings.push(J::from((got, len, keys_hex(keys))));
        // SAFETY: the array was allocated above and is freed once.
        unsafe { glib_sys::g_array_free(keys, 1) };
    }
    let (mut length, mut text) = (0_u32, std::ptr::null_mut());
    let ret = facade::token_get_phrase(inst, token, &raw mut length, &raw mut text);
    let phrase = J::from((ret, length, take_text(text)));
    obj! {
        "phrase" => phrase,
        "unigram" => unigram_of(s, token),
        "count" => (ok, count),
        "readings" => readings,
    }
}

/// `_system_items`' total and largest item slot of a system chunk.
fn system_total_and_last_slot(k: &Kit, name: &str) -> (u32, u32) {
    let bytes = std::fs::read(k.data.join(name)).expect("a system library");
    let payload = &bytes[8..];
    let word =
        |offset: usize| u32::from_le_bytes(payload[offset..offset + 4].try_into().expect("a word"));
    let (total, index, content) = (word(0), word(4) as usize, word(8) as usize);
    let last = (0..(content - index - 1) / 4)
        .filter(|slot| word(index + slot * 4) != 0)
        .max()
        .expect("an item");
    (total, u32::try_from(last).expect("a slot"))
}

/// Plants the persisted empty item before init; answers its library, token and file.
fn plant_empty_item(k: &Kit, user_file: bool, frequencies: &[u32]) -> (u8, u32, &'static str) {
    write_user_conf(k);
    let mut body = vec![0, u8::try_from(frequencies.len()).expect("a count")];
    body.extend(37_u32.to_le_bytes());
    body.extend(frequencies.iter().flat_map(|value| value.to_le_bytes()));
    let (library, slot, name) = if user_file {
        let content = [vec![0; 8], body].concat();
        let end = u32::try_from(34 + content.len() + 1).expect("an offset");
        let mut payload: Vec<u8> = [91_u32, 17, 34, end]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        payload.extend(b"#");
        payload.extend([0; 12]);
        payload.extend(8_u32.to_le_bytes());
        payload.extend(b"#");
        payload.extend(content);
        payload.extend(b"#");
        std::fs::write(k.user.join("addon.bin"), chunk(&payload)).expect("write addon.bin");
        (5, 3, "addon.bin")
    } else {
        let (total, last) = system_total_and_last_slot(k, "gbk_char.bin");
        let slot = last + 3;
        let length = u16::try_from(body.len()).expect("a record length");
        let mut log = modify_header(0, total, total + 37);
        log.extend(1_u32.to_le_bytes());
        log.extend(slot.to_le_bytes());
        log.extend(length.to_le_bytes());
        log.extend(body);
        write_log(k, &log, "gbk_char.dbin");
        (2, slot, "gbk_char.dbin")
    };
    (library, (u32::from(library) << 24) | slot, name)
}

/// `_persisted_empty_reload`: a persisted empty item through load, unload,
/// reload, compaction, save and reopen.
fn persisted_empty_reload(k: &Kit, user_file: bool, frequencies: &[u32]) -> J {
    let (library, token, name) = plant_empty_item(k, user_file, frequencies);
    let mut s = Session::new(k);
    let mut out = obj! { "initial" => persisted_item_getters(&mut s, token) };
    facade::free_instance(s.inst());
    s.drop_inst();
    out.set("unload", facade::unload_phrase_library(s.ctx(), library));
    out.set("unloaded", persisted_item_getters(&mut s, token));
    facade::free_instance(s.inst());
    s.drop_inst();
    out.set("load", facade::load_phrase_library(s.ctx(), library));
    out.set("reloaded", persisted_item_getters(&mut s, token));
    assert!(import_rows(&mut s, 7, &[]).is_empty());
    out.set("compacted", persisted_item_getters(&mut s, token));
    out.set("save", facade::save(s.ctx()));
    let saved = std::fs::read(s.user.join(name)).expect("the saved file");
    out.set("saved", hex(&saved));
    out.set("after-save", persisted_item_getters(&mut s, token));
    s.close();
    out.set("reopened", persisted_item_getters(&mut s, token));
    out
}

/// The `reload-*` cases of the reload probes, the user library and its reuse.
fn reload_case(k: &mut Kit, rest: &str) -> J {
    let mode = if facade::ZHUYIN { "zhuyin" } else { "pinyin" };
    if let Some(name) = rest.strip_prefix(&format!("user-{mode}-reuse-")) {
        return reuse_case(k, name);
    }
    if let Some(name) = rest
        .strip_prefix(&format!("refused-user-{mode}-"))
        .or_else(|| rest.strip_prefix(&format!("user-{mode}-")))
    {
        return reload_user(k, &user_reload(name));
    }
    for (prefix, save_first) in [
        ("refused-", false),
        ("drops-session-", false),
        ("keeps-saved-", true),
    ] {
        if let Some(name) = rest
            .strip_prefix(prefix)
            .and_then(|name| name.strip_prefix(&format!("{mode}-")))
        {
            let (change, library) = reload_change(name);
            return reload_probe(k, &change, library, save_first);
        }
    }
    reload_misc(k, rest)
}

/// The remaining `reload-*` cases.
fn reload_misc(k: &mut Kit, rest: &str) -> J {
    let name = rest
        .strip_suffix("-pinyin")
        .or_else(|| rest.strip_suffix("-zhuyin"))
        .unwrap_or(rest);
    match name {
        "dangling-token-candidates" => reload_dangling(k),
        "total-while-unloaded" => reload_total_window(k),
        "unreadable-log-missing" => reload_unreadable(k, true),
        "unreadable-log-short" => reload_unreadable(k, false),
        "persisted-empty-add-plain" => persisted_empty_reload(k, false, &[]),
        "persisted-empty-add-pronunciations" => persisted_empty_reload(k, false, &[13, 5]),
        "persisted-empty-user-file" => persisted_empty_reload(k, true, &[13, 5]),
        other => panic!("unknown library_reload case reload-{other}"),
    }
}

/// The case `id`'s result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(rest) = id.strip_prefix("reload-") {
        return reload_case(k, rest);
    }
    if let Some(rest) = id.strip_prefix("unload-save-skips-") {
        let name = rest
            .strip_suffix("-pinyin")
            .or_else(|| rest.strip_suffix("-zhuyin"))
            .expect("a facade suffix");
        return unload_save(k, &unload_save_params(name));
    }
    match id {
        "mask-out-replaces-reload-offset" => mask_out_reload(k, false),
        "mask-out-after-compact-keeps-reload-offset" => mask_out_reload(k, true),
        other => panic!("unknown library_reload case {other}"),
    }
}
