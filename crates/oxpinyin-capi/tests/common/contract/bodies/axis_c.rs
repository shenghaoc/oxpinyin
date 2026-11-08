//! Axis C: the option word, the input encodings and the zhuyin layouts (inventory
//! row 66, #1175, #1620).
//!
//! The inputs are the frozen axis C definition's, captured at pin 074a2219.
//! The old harness forked one child per observation of an options or layout case. Here
//! the case is already one child process, and its option and layout observations share
//! the case's one context: each input gets a fresh instance, freed after the input
//! ([`observe`]). This is the scoped exception to a fresh context per input, for these
//! read-only observations only. The context has the same configuration at every input,
//! and no input changes it for the next: `pinyin_set_options` writes only the option
//! word (`pinyin.cpp:1299-1301`), `pinyin_alloc_instance` creates fresh instance state
//! (`:1311-1333`), `FullPinyinParser2::parse` resets its working steps before it
//! rebuilds them (`pinyin_parser2.cpp:227`), and the sentence and candidate calls made
//! here do not write the shared context. The encoding cases and the abort schemes keep
//! their own paths.

use std::ffi::{CString, c_char};

use crate::facade;
use crate::readers::{candidate_rows, export_phrases, garray_u32};
use crate::support::{
    J, Kit, P, StderrCapture, UNTOUCHED, libpinyin_export, logs_j, messages_j, take_text,
};

/// The texts each option word is observed on (`option_inputs`).
const OPTION_INPUTS: &[&str] = &[
    "a",
    "kuaige",
    "saihanpianjing",
    "jizuizhuan",
    "yaosouyongninoqiangtiaominyuanken",
    "asai'touha'pai'caixu",
    "mi'w",
    "nic",
    "lenglan",
    "cuntiechuang",
    "gongnenjiaooshuoqiangpangsourigengei",
    "huanchong'ranzhuai'liuyue'shugeng",
    "naoxion",
    "liangniejue",
    "tinNgtingdia",
    "",
];

/// The encoding probes' named inputs (`encoding_inputs`), as bytes.
const ENCODING_INPUTS: &[(&str, &[u8])] = &[
    ("empty", b""),
    ("pinyin", b"nihao"),
    ("tone", b"ni3hao3"),
    ("nfc", "n\u{1d0}h\u{1ce}o".as_bytes()),
    ("hanzi", "你好".as_bytes()),
    ("bopomofo", "ㄋㄧˇ ㄏㄠˇ".as_bytes()),
    ("fullwidth", "ｎｉｈａｏ".as_bytes()),
    ("ff-middle", b"ni\xffhao"),
    ("nul-middle", b"ni\x00hao"),
];

/// The encoding operations as the case names spell them.
const ENCODING_OPERATIONS: &[&str] = &[
    "parse_more_full_pinyins",
    "parse_more_double_pinyins",
    "parse_more_chewings",
    "lookup_tokens",
    "phrase_segment",
    "remember_user_input",
    "import-phrase",
    "import-reading",
];

/// The bytes each layout maps through the chewing keyboard (`layout_ascii`).
const LAYOUT_ASCII: std::ops::RangeInclusive<u8> = 32..=126;

/// The texts each layout parses as chewings (`layout_inputs`).
const LAYOUT_INPUTS: &[&str] = &[
    "", "1", "18", "su3", "su3cl3", "su3u3", "su3u3u3", "vu0", "zhang", "zhan", "n", "u", "wu",
    "yi", "nin", "3",
];

/// Runs the case `id` and answers its result document.
///
/// The kept test design (#1616's method): the option words are the defined bits that
/// are read, the undefined bits 0 and 20, and the 32-bit edges. Pairs: 0x18
/// (`pinyin_phrase2.h:86`) and 0x60 (`zhuyin_parser2.cpp:373`) are read on one line.
/// The 14 combined bits reach one shared matrix, so the pin combines them later and
/// each pair stays: the parser's segmentation (`pinyin_parser2.cpp:49-53`), the fuzzy
/// pass's final step reading the columns its initial step and the tables appended
/// (`phonetic_key_matrix.cpp:291-292`), a phrase's possibility summed over key paths
/// across columns (`:534-584`), and candidate ranking (`pinyin.cpp:1845`). The other
/// bits move no outcome on these inputs, alone or paired.
///
/// Layouts: every scheme of the setter's switch in each facade with word 0
/// (`pinyin.cpp:1166-1187`, `zhuyin.cpp:713-734`), and one case per distinct outcome
/// with word 0x60 (the parser reads the word and the scheme's tone table together,
/// `zhuyin_parser2.cpp:379-380`). 0xe0000070 sets the zhuyin corrections the parser
/// reads against an item's flags (`zhuyin_parser2.cpp:56`). Scheme 7 and scheme 0 are
/// the two abort sites per facade: the fall-through to `zhuyin_parser2.cpp:295` and the
/// setter's default (`pinyin.cpp:1188`, `zhuyin.cpp:735`).
pub fn case(k: &mut Kit, id: &str) -> J {
    let mut result = if let Some(rest) = id.strip_prefix("axis-c-options-") {
        let (_, word) = rest.rsplit_once('-').expect("an option word");
        options(k, hex_word(word))
    } else if let Some(rest) = id.strip_prefix("axis-c-encoding-") {
        let (_, rest) = rest.split_once('-').expect("a mode");
        let operation = ENCODING_OPERATIONS
            .iter()
            .copied()
            .find(|operation| rest.starts_with(&format!("{operation}-")))
            .expect("a known encoding operation");
        let name = &rest[operation.len() + 1..];
        let text = ENCODING_INPUTS
            .iter()
            .find(|(input, _)| *input == name)
            .map(|(_, bytes)| *bytes)
            .expect("a known encoding input");
        encoding(k, operation, text)
    } else if let Some(rest) = id.strip_prefix("axis-c-layout-") {
        let mut parts = rest.split('-').skip(1);
        let scheme = parts.next().expect("a scheme");
        let word = parts.next().expect("a word");
        layout(k, scheme.parse().expect("a scheme number"), hex_word(word))
    } else {
        panic!("not an axis C case: {id}")
    };
    // The old worker added the GLib messages to every axis C document.
    result.set("log_messages", messages_j(k));
    result
}

fn hex_word(text: &str) -> u32 {
    u32::from_str_radix(text, 16).expect("a hex word")
}

/// The bytes as the C string ctypes passed for a `bytes` argument: the C side reads
/// up to the first NUL.
fn c_bytes(bytes: &[u8]) -> CString {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    CString::new(&bytes[..end]).expect("no NUL before the end")
}

/// A parser entry point by its facade name.
fn parser(name: &str) -> fn(P, *const c_char) -> usize {
    match name {
        "parse_more_full_pinyins" => facade::parse_more_full_pinyins,
        "parse_more_double_pinyins" => facade::parse_more_double_pinyins,
        "parse_more_chewings" => facade::parse_more_chewings,
        other => panic!("not a parser: {other}"),
    }
}

/// `axis_c_options`: sets the option word on the kit's context, then observes each
/// full-pinyin input on a fresh instance of that context, the word set again.
fn options(k: &mut Kit, word: u32) -> J {
    let set_options = facade::set_options(k.ctx(), word);
    let mut observations = J::map();
    for (index, text) in OPTION_INPUTS.iter().enumerate() {
        let observation = observe(k, word, |child| {
            surface(child.alloc(), "parse_more_full_pinyins", text.as_bytes())
        });
        observations.set(index.to_string(), observation);
    }
    obj! { "set_options" => set_options, "observations" => observations }
}

/// `axis_c_layout`: sets the option word on the kit's context, then maps each byte
/// through the chewing keyboard and parses each text as chewings, each observation on a
/// fresh instance of the kit's context with the word and this facade's scheme set again.
/// The scheme setter aborts for schemes 0 and 7, before any input: those cases make that
/// call once, on the kit's context.
fn layout(k: &mut Kit, scheme: i32, word: u32) -> J {
    facade::set_options(k.ctx(), word);
    if scheme == 0 || scheme == 7 {
        set_scheme(k.ctx(), scheme);
        unreachable!("the scheme setter returned for scheme {scheme}");
    }
    let mut observations = J::map();
    for byte in LAYOUT_ASCII {
        let observation = observe(k, word, |child| {
            configured(child, scheme, |child| keyboard(child.alloc(), byte))
        });
        observations.set(format!("ascii-{byte}"), observation);
    }
    for (index, text) in LAYOUT_INPUTS.iter().enumerate() {
        let observation = observe(k, word, |child| {
            configured(child, scheme, |child| {
                surface(child.alloc(), "parse_more_chewings", text.as_bytes())
            })
        });
        observations.set(format!("input-{index}"), observation);
    }
    obj! { "observations" => observations }
}

/// This facade's scheme setter: `set_zhuyin_scheme` on pinyin, `set_chewing_scheme`
/// on zhuyin.
fn set_scheme(ctx: P, scheme: i32) -> bool {
    if facade::ZHUYIN {
        facade::set_chewing_scheme(ctx, scheme)
    } else {
        facade::set_zhuyin_scheme(ctx, scheme)
    }
}

/// `configured`: sets the scheme on the observation's context and observes only when
/// the setter succeeds.
fn configured(child: &mut Child, scheme: i32, observation: impl FnOnce(&mut Child) -> J) -> J {
    let ret = set_scheme(child.ctx, scheme);
    let observation = if ret { observation(child) } else { J::Null };
    obj! { "set_scheme" => ret, "observation" => observation }
}

/// The case's context and the instances one observation allocated.
struct Child {
    ctx: P,
    instances: Vec<P>,
}

impl Child {
    /// A fresh instance of the observation's context.
    fn alloc(&mut self) -> P {
        let inst = facade::alloc_instance(self.ctx);
        assert!(!inst.is_null(), "alloc_instance failed");
        self.instances.push(inst);
        inst
    }
}

/// `axis_c_isolated`: one observation as the forked child answered it,
/// `{"exit": 0, "result": {...}, "stderr": ...}`.
///
/// The fork's copy of the parent's context is the kit's context, opened once per case,
/// with the same option word set again. The observation's own stderr is captured from
/// then until its result is read. The result carries the GLib records of the
/// observation alone, and the kit holds the parent's records (none) again afterwards.
/// The observation's instances are freed before the next input; the context stays.
fn observe(k: &mut Kit, word: u32, body: impl FnOnce(&mut Child) -> J) -> J {
    assert!(
        k.logs().is_empty(),
        "an axis C parent holds no GLib records before its observations"
    );
    let ctx = k.ctx();
    facade::set_options(ctx, word);
    k.clear_logs();
    let mut child = Child {
        ctx,
        instances: Vec::new(),
    };
    let capture = StderrCapture::start();
    let mut result = body(&mut child);
    result.set("logs", logs_j(k));
    result.set("log_messages", messages_j(k));
    let stderr = capture.finish();
    k.clear_logs();
    for inst in child.instances {
        facade::free_instance(inst);
    }
    assert!(
        k.logs().is_empty(),
        "an observation's teardown logged {:?}",
        k.messages()
    );
    obj! { "exit" => 0, "result" => result, "stderr" => stderr }
}

/// `axis_c_surface`: parse, lengths, sentence rank zero and the complete candidate
/// list. The Python's `candidate_multiset` (a sorted copy of `candidates`) is not part
/// of the kept observation.
fn surface(inst: P, parse: &str, text: &[u8]) -> J {
    let consumed = parser(parse)(inst, c_bytes(text).as_ptr());
    let mut out = obj! {
        "consumed" => consumed,
        "parsed" => facade::get_parsed_input_length(inst),
    };
    let guessed = facade::guess_sentence(inst);
    out.set("guess_sentence", guessed);
    if guessed {
        out.set("sentence", sentence(inst));
    }
    out.set(
        "guess_candidates",
        if facade::ZHUYIN {
            facade::guess_candidates_after_cursor(inst, 0)
        } else {
            facade::guess_candidates(inst, 0, 0x1e)
        },
    );
    let mut count = UNTOUCHED;
    out.set(
        "get_n_candidate",
        facade::get_n_candidate(inst, &raw mut count),
    );
    assert_ne!(count, UNTOUCHED, "candidate count untouched");
    out.set("n", count);
    out.set("candidates", candidate_rows(inst));
    out
}

/// `contract_abi.sentence_out(k, inst, 0)` / `zhuyin_sentence_out`: the flag and the
/// text, `None` or `"untouched"`.
fn sentence(inst: P) -> J {
    let mut out = UNTOUCHED as usize as *mut c_char;
    let ret = if facade::ZHUYIN {
        facade::get_sentence_only(inst, &raw mut out)
    } else {
        facade::get_sentence_at(inst, 0, &raw mut out)
    };
    let text = if out as usize == UNTOUCHED as usize {
        J::from("untouched")
    } else {
        J::from(take_text(out))
    };
    J::L(vec![ret.into(), text])
}

/// `axis_c_keyboard`: one byte through the chewing keyboard: the flag and the symbol
/// list, `None` or `"untouched"`; the vector is released.
fn keyboard(inst: P, byte: u8) -> J {
    let mut out = UNTOUCHED as usize as *mut *mut c_char;
    let ret = facade::in_chewing_keyboard(inst, byte as c_char, &raw mut out);
    let symbols = if out as usize == UNTOUCHED as usize {
        J::from("untouched")
    } else if out.is_null() {
        J::Null
    } else {
        let mut symbols = Vec::new();
        // SAFETY: a non-NULL answer is a NULL-terminated vector of NUL-terminated
        // strings the caller owns and frees once with `g_strfreev`.
        unsafe {
            let mut index = 0;
            while !(*out.add(index)).is_null() {
                let text = std::ffi::CStr::from_ptr(*out.add(index)).to_bytes();
                symbols.push(String::from_utf8(text.to_vec()).expect("UTF-8 symbol"));
                index += 1;
            }
            glib_sys::g_strfreev(out);
        }
        J::from(symbols)
    };
    obj! { "ret" => ret, "symbols" => symbols }
}

/// `axis_c_lookup`: `lookup_tokens` of the bytes into a fresh four-byte GArray: the
/// flag and the raw token values.
fn lookup(inst: P, text: &[u8]) -> J {
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    let ret = facade::lookup_tokens(inst, c_bytes(text).as_ptr(), array.cast());
    let tokens = garray_u32(array);
    // SAFETY: the array was allocated above and is freed once.
    unsafe { glib_sys::g_array_free(array, 1) };
    obj! { "ret" => ret, "tokens" => tokens }
}

/// `axis_c_encoding`: the bytes through one encoding-sensitive operation, the option
/// word 0x28 set first.
fn encoding(k: &mut Kit, operation: &str, text: &[u8]) -> J {
    facade::set_options(k.ctx(), 0x28);
    match operation {
        "lookup_tokens" => lookup(k.inst(), text),
        "phrase_segment" => segment(k.inst(), text),
        "remember_user_input" => {
            let inst = k.inst();
            let parsed = facade::parse_more_full_pinyins(inst, c_bytes(b"nihao").as_ptr());
            let ret = facade::remember_user_input(inst, c_bytes(text).as_ptr(), 1);
            obj! { "parsed" => parsed, "ret" => ret, "rows" => export_phrases(k) }
        }
        "import-phrase" | "import-reading" => import(k, operation, text),
        parse => surface(k.alloc(), parse, text),
    }
}

/// `phrase_segment` and the phrase count. On the preregistered surrogate inputs (#751)
/// the pin returns false with six null slots and no GLib record, so the token outputs
/// and two reads past the end are read out too, the token walk bounded by the input's
/// byte length.
fn segment(inst: P, text: &[u8]) -> J {
    let ret = facade::phrase_segment(inst, c_bytes(text).as_ptr());
    let mut count = UNTOUCHED;
    let got = facade::get_n_phrase(inst, &raw mut count);
    obj! { "ret" => ret, "get_n_phrase" => got, "count" => count }
}

/// `axis_c_import`: imports the phrase or reading bytes into library 7 with count 1
/// and a fixed counterpart, looks the phrase up, saves and closes the context, then
/// re-opens the saved profile through libpinyin's export iterator in a separate
/// process (libzhuyin has none, so either facade's profile goes through libpinyin).
fn import(k: &mut Kit, operation: &str, text: &[u8]) -> J {
    let phrase: &[u8] = if operation == "import-phrase" {
        text
    } else {
        "你好".as_bytes()
    };
    let reading: &[u8] = if operation == "import-reading" {
        text
    } else if facade::ZHUYIN {
        "ㄋㄧˇ ㄏㄠˇ".as_bytes()
    } else {
        b"ni'hao"
    };
    let ctx = k.ctx();
    let it = facade::begin_add_phrases(ctx, 7);
    assert!(!it.is_null(), "missing import iterator");
    let added =
        facade::iterator_add_phrase(it, c_bytes(phrase).as_ptr(), c_bytes(reading).as_ptr(), 1);
    facade::end_add_phrases(it);
    let lookup = lookup(k.inst(), phrase);
    let saved = facade::save(ctx);
    // As the Python did, the context is finalised with its instance still allocated.
    facade::fini(ctx);
    k.forget();
    let export = libpinyin_export(&k.data, &k.user);
    obj! {
        "added" => added,
        "lookup" => lookup,
        "saved" => saved,
        "reopen_stderr" => export.stderr,
        "export" => export.document,
    }
}
