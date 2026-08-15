//! Tier 1 — cleaning-stage well-formedness invariants (no oracle).
//!
//! The cleaning stage has no libpinyin differential oracle: libpinyin's
//! corpus preparation was Peng Wu's private, undocumented pipeline, so
//! there is nothing to diff against. These tests instead assert the
//! committed sample is valid `ngseg` input — predominantly Han text with
//! standard Chinese punctuation (Latin names and digits pass through),
//! no residual markup syntax, simplified spot-check only, one sentence
//! per line, non-empty — and that the cleaner reproduces the sample from
//! the committed XML fixture bit-exactly. See
//! `docs/findings/corpus-pipeline.md`.

use std::io::Cursor;
use std::path::PathBuf;

use pinyin_corpus::{Options, clean};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn sample_path() -> PathBuf {
    repo_root().join("fixtures/w9/corpus-sample.txt")
}

fn xml_fixture_path() -> PathBuf {
    repo_root().join("fixtures/w9/corpus-xml.xml")
}

/// Markup syntax that must never survive into `ngseg` input.
const MARKUP_RESIDUE: &[&str] = &[
    "[[", "{{", "{|", "}}", "]]", "|}", "<ref", "<!--", "&amp;", "&#", "http", "==",
];

/// Unambiguous traditional characters: after t2s conversion none of
/// these may remain. This is a spot-check (the full check would need the
/// OpenCC dictionary), which is exactly what the task asks for.
const TRADITIONAL_SPOT_CHECK: &str = "國學體機發開關說讀話語東車長門間風電馬鳥魚龍點聽寫畫號員錢銀鐵錯鐘紙紅經網頁頂順個們來時會過還裡後頭實傳傷動單塊數對題樂識報務與島從麼見導動員壓歷史樹維類書氣無誤區認領選鐘變轉記統紹條嗎會場議隊縣衛機簡藝質變邊遠鄉鎮鄭勝利應蘇蘭嶼藝術音樂階段戰爭護衛義務讓";

fn read_sample() -> String {
    std::fs::read_to_string(sample_path()).expect("committed sample")
}

#[test]
fn cleaned_sample_is_valid_t1_input() {
    let text = read_sample();
    assert!(!text.is_empty(), "sample must not be empty");
    assert!(
        !text.contains('\r'),
        "ngseg strips one '\\n' only; no '\\r' may be present"
    );

    let lines: Vec<&str> = text.lines().collect();
    assert!(!lines.is_empty());
    for (index, line) in lines.iter().enumerate() {
        assert!(!line.is_empty(), "line {} is empty", index + 1);
        assert_eq!(
            line.trim(),
            *line,
            "line {} has leading/trailing whitespace",
            index + 1
        );
        assert!(
            line.chars().any(|ch| matches!(
                ch as u32,
                0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F
            )),
            "line {} has no Han character: {line:?}",
            index + 1
        );
        for marker in MARKUP_RESIDUE {
            assert!(
                !line.contains(marker),
                "line {} retains markup {marker:?}: {line:?}",
                index + 1
            );
        }
        let traditional: Vec<char> = line
            .chars()
            .filter(|ch| TRADITIONAL_SPOT_CHECK.contains(*ch))
            .collect();
        assert!(
            traditional.is_empty(),
            "line {} retains traditional characters {traditional:?}: {line:?}",
            index + 1
        );
    }
    assert!(
        text.ends_with('\n'),
        "sample must end with a newline like every T1 input line"
    );
}

#[test]
fn cleaner_reproduces_sample_from_xml_fixture_bit_exactly() {
    let xml = std::fs::read(xml_fixture_path()).expect("committed xml fixture");
    // CI runs without a converter; the fixture is all-simplified, so the
    // opencc stage is the identity on it (checked when opencc exists).
    let mut out = Vec::new();
    let stats = clean(Cursor::new(xml), &mut out, &Options::default())
        .expect("cleaner must accept the fixture");
    assert_eq!(stats.pages_read, 3, "fixture has three articles");
    assert_eq!(stats.pages_skipped, 0, "all three articles are prose");
    assert_eq!(stats.sentences, 16, "sentence count is pinned");
    assert_eq!(out, read_sample().into_bytes(), "cleaned bytes diverge");
}

/// Guards against the strip rules silently changing: the invariant test
/// above is the specification, and it must stay in sync with the sample.
#[test]
fn invariant_guards_are_not_silently_relaxed() {
    assert!(MARKUP_RESIDUE.contains(&"<ref"));
    assert!(MARKUP_RESIDUE.contains(&"{{"));
    assert!(TRADITIONAL_SPOT_CHECK.contains('國'));
    assert!(TRADITIONAL_SPOT_CHECK.contains('學'));
}

fn locate_opencc() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os("PINYIN_OPENCC") {
        let path = PathBuf::from(raw);
        return path.is_file().then_some(path);
    }
    let probe = std::process::Command::new("opencc")
        .arg("--version")
        .output()
        .ok()?;
    probe.status.success().then(|| PathBuf::from("opencc"))
}

/// trad → simp through the real `opencc` binary, when one exists. The
/// pipeline design shells out; this test pins the seam, not the OpenCC
/// dictionaries.
#[test]
fn converter_turns_traditional_into_simplified_when_opencc_present() {
    let Some(opencc) = locate_opencc() else {
        eprintln!("skipping: opencc binary not found (PINYIN_OPENCC / PATH)");
        return;
    };
    let converter = pinyin_corpus::Converter::new(opencc);
    let out = converter
        .convert("漢字。臺灣。國語。")
        .expect("opencc runs");
    assert_eq!(out, "汉字。台湾。国语。");
}

/// A smoke check that the exported `clean` stream shape is exactly what
/// T1's `Segmenter::segment_bytes` accepts: line-oriented UTF-8, `\n`
/// only. (The full T1 join is exercised by `tests/differential.rs`.)
#[test]
fn sample_feeds_the_segmenter_byte_shape() {
    let text = read_sample();
    let bytes = text.as_bytes();
    // Every line boundary is exactly one 0x0A; no other ASCII control.
    for &byte in bytes {
        assert!(byte != b'\r', "no carriage returns");
        assert!(byte >= 0x20 || byte == b'\n', "no stray control bytes");
    }
    // The final line is newline-terminated, matching ngseg's getline.
    assert!(bytes.ends_with(b"\n"));
}
