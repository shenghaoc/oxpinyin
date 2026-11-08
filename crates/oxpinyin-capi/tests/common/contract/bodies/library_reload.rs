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
use crate::readers::{
    c, candidate_at, candidate_text, candidate_type, garray_u16, guess_rows, lookup, n_candidate,
    owned_out, seeded,
};
use crate::support::{J, Kit, P, UNTOUCHED, borrowed_text, path_c};
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
fn reload_change() -> (Change, u8) {
    let change = if facade::ZHUYIN {
        Change::Train("礙", 2, b"94")
    } else {
        Change::Train("礙", 2, b"ai4")
    };
    (change, 2)
}
/// `_reload_reuse_setup`: an unsaved zhuyin user phrase, then its library reloaded.
fn reload_reuse_setup(s: &mut Session) -> J {
    let mut out = obj! { "first" => import_rows(s, 7, &[("好你", "ㄏㄠˇ ㄋㄧˇ", 50)]) };
    unload_load(s, 7, &mut out);
    out
}

/// The `reload-user-zhuyin-reuse-*` cases.
fn reuse_case(k: &Kit) -> J {
    let mut s = Session::new(k);
    let mut out = reload_reuse_setup(&mut s);
    let it = facade::begin_add_phrases(s.ctx(), 7);
    let ret = facade::iterator_add_phrase(it, c("好你").as_ptr(), c("ㄏㄠˇ ㄋㄧˇ").as_ptr(), 50);
    out.set("ret", ret);
    out
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
fn unload_save_params() -> UnloadSave {
    if facade::ZHUYIN {
        UnloadSave {
            library: 2,
            file: "gbk_char.dbin",
            saved: ("礙", "ㄞ", 50),
            unsaved: ("龘龘", "ㄉㄚˇ ㄉㄚˇ", 50),
            phrases: &["礙", "龘龘"],
            inputs: &[b"9"],
        }
    } else {
        UnloadSave {
            library: 2,
            file: "gbk_char.dbin",
            saved: ("礙", "ai1", 50),
            unsaved: ("龘龘", "da3'da3", 50),
            phrases: &["礙", "龘龘"],
            inputs: &[b"ai1"],
        }
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
fn reload_unreadable(k: &Kit) -> J {
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
    std::fs::write(&log, b"abc").expect("shorten the log");
    out.set("load", facade::load_phrase_library(s.ctx(), 2));
    s.drop_inst();
    out.set("reloaded", reload_snapshot(&mut s, &phrases, &inputs));
    out
}
/// The `reload-*` cases of the reload probes, the user library and its reuse.
fn reload_case(k: &mut Kit, rest: &str) -> J {
    let mode = if facade::ZHUYIN { "zhuyin" } else { "pinyin" };
    if rest.strip_prefix(&format!("user-{mode}-reuse-")).is_some() {
        return reuse_case(k);
    }
    for (prefix, save_first) in [("drops-session-", false), ("keeps-saved-", true)] {
        if rest
            .strip_prefix(prefix)
            .and_then(|name| name.strip_prefix(&format!("{mode}-")))
            .is_some()
        {
            let (change, library) = reload_change();
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
        "total-while-unloaded" => reload_total_window(k),
        "unreadable-log-short" => reload_unreadable(k),
        other => panic!("unknown library_reload case reload-{other}"),
    }
}

/// The case `id`'s result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(rest) = id.strip_prefix("reload-") {
        return reload_case(k, rest);
    }
    if let Some(rest) = id.strip_prefix("unload-save-skips-") {
        rest.strip_suffix("-pinyin")
            .or_else(|| rest.strip_suffix("-zhuyin"))
            .expect("a facade suffix");
        return unload_save(k, &unload_save_params());
    }
    panic!("unknown library_reload case {id}")
}
