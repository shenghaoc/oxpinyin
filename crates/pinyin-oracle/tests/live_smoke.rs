//! W2-T1 acceptance: convert one phrase through the real pinned libpinyin.
//!
//! Requires the `oracle-ffi` feature and a prefix built by
//! `tools/oracle/build-oracle.sh`:
//!
//! ```bash
//! bash tools/oracle/build-oracle.sh --prefix "$HOME/.local/opt/pinyin-oracle" --jobs "$(nproc)"
//!
//! PKG_CONFIG_PATH=$HOME/.local/opt/pinyin-oracle/lib/pkgconfig \
//! LD_LIBRARY_PATH=$HOME/.local/opt/pinyin-oracle/lib \
//! cargo test -p pinyin-oracle --features oracle-ffi
//! ```
//!
//! The assertions are not invented: they are the values frozen in
//! `fixtures/foundation/f-a.txt` for the same inputs under the same flag word.
//! A live run that disagrees means either the FFI is wrong or the prefix is not
//! the pin, and both must fail loudly.
#![cfg(feature = "oracle-ffi")]

use pinyin_oracle::observation::OracleCompleteness;
use pinyin_oracle::{
    EXPECTED_PIN_REF, Oracle, OracleError, OracleFlags, OraclePrefix, OracleSegment,
};

/// Opens the located oracle, or explains why the run cannot proceed.
fn oracle() -> Oracle {
    let prefix = match OraclePrefix::locate() {
        Ok(prefix) => prefix,
        Err(error) => panic!(
            "{error}\n\nBuild the oracle first:\n  \
             bash tools/oracle/build-oracle.sh \
             --prefix \"$HOME/.local/opt/pinyin-oracle\" --jobs \"$(nproc)\""
        ),
    };
    Oracle::open_with_temp_user_dir(prefix).expect("pin-verified prefix opens a context")
}

/// `(syllable, begin, end, complete?)` for each segment, for terse comparison.
fn path_of(segments: &[OracleSegment]) -> Vec<(&str, u16, u16, bool)> {
    segments
        .iter()
        .map(|segment| match segment {
            OracleSegment::Syllable {
                syllable,
                begin,
                end,
                completeness,
            } => (
                syllable.as_str(),
                *begin,
                *end,
                *completeness == OracleCompleteness::Complete,
            ),
            other => panic!("unexpected sentinel segment: {other:?}"),
        })
        .collect()
}

#[test]
fn converts_nihao_through_the_real_libpinyin() {
    let mut oracle = oracle();
    let observed = oracle
        .observe(b"nihao", OracleFlags::DEFAULT)
        .expect("nihao is observable");

    observed
        .check_self_consistent()
        .expect("oracle is self-consistent");

    // Frozen in fixtures/foundation/f-a.txt, case=valid-multiple.
    assert_eq!(observed.pin_ref, EXPECTED_PIN_REF);
    assert_eq!(observed.parse_return, 5);
    assert_eq!(observed.parsed_input_length, 5);
    assert_eq!(observed.remainder, b"");
    assert!(observed.fully_consumed());
    assert_eq!(
        path_of(&observed.segments),
        vec![("ni", 0, 2, true), ("hao", 2, 5, true)]
    );

    // The conversion itself: the phrase, not just a segmentation.
    assert_eq!(
        observed.candidates.first().map(String::as_str),
        Some("你好")
    );
    assert_eq!(observed.candidate_total, 126);
    assert_eq!(
        observed.candidates,
        ["你好", "你", "尼", "呢", "泥", "妮", "拟", "逆", "倪", "腻"]
    );
}

#[test]
fn reproduces_further_frozen_f_a_records() {
    let mut oracle = oracle();

    // case=valid-single
    let single = oracle.observe(b"ni", OracleFlags::DEFAULT).expect("ni");
    assert_eq!(path_of(&single.segments), vec![("ni", 0, 2, true)]);
    assert_eq!(single.candidate_total, 125);

    // case=valid-phrase
    let phrase = oracle
        .observe(b"zhongguoren", OracleFlags::DEFAULT)
        .expect("zhongguoren");
    assert_eq!(
        path_of(&phrase.segments),
        vec![
            ("zhong", 0, 5, true),
            ("guo", 5, 8, true),
            ("ren", 8, 11, true)
        ]
    );
    assert_eq!(
        phrase.candidates.first().map(String::as_str),
        Some("中国人")
    );

    // case=ambiguous-fangan — the oracle selects [fan, gan], not the greedy
    // [fang, an]. This is the tie-swap example W2-T5 classifies.
    let fangan = oracle
        .observe(b"fangan", OracleFlags::DEFAULT)
        .expect("fangan");
    assert_eq!(
        path_of(&fangan.segments),
        vec![("fan", 0, 3, true), ("gan", 3, 6, true)]
    );

    // case=incomplete-nih — a partial tail, and the F-E-01 neighbourhood.
    let nih = oracle.observe(b"nih", OracleFlags::DEFAULT).expect("nih");
    assert_eq!(
        path_of(&nih.segments),
        vec![("ni", 0, 2, true), ("h", 2, 3, false)]
    );

    // case=apostrophe-chang-an — apostrophe is a hard boundary and leaves a gap.
    let chang_an = oracle
        .observe(b"chang'an", OracleFlags::DEFAULT)
        .expect("chang'an");
    assert_eq!(
        path_of(&chang_an.segments),
        vec![("chang", 0, 5, true), ("an", 6, 8, true)]
    );
}

#[test]
fn junk_and_empty_input_are_reported_not_fatal() {
    let mut oracle = oracle();

    // case=junk-prefix
    let junk = oracle.observe(b"!ni", OracleFlags::DEFAULT).expect("!ni");
    assert_eq!(junk.parsed_input_length, 0);
    assert!(junk.segments.is_empty());
    assert_eq!(junk.candidate_total, 0);
    assert_eq!(junk.remainder, b"!ni");

    // case=junk-middle
    let middle = oracle
        .observe(b"ni!hao", OracleFlags::DEFAULT)
        .expect("ni!hao");
    assert_eq!(path_of(&middle.segments), vec![("ni", 0, 2, true)]);
    assert_eq!(middle.remainder, b"!hao");

    // case=empty
    let empty = oracle.observe(b"", OracleFlags::DEFAULT).expect("empty");
    assert_eq!(empty.parsed_input_length, 0);
    assert!(empty.segments.is_empty());
    assert!(empty.fully_consumed());

    // case=very-long-junk-4096
    let long = vec![b'!'; 4096];
    let observed = oracle
        .observe(&long, OracleFlags::DEFAULT)
        .expect("4096 junk bytes");
    assert_eq!(observed.parsed_input_length, 0);
    assert_eq!(observed.remainder.len(), 4096);
}

#[test]
fn session_reset_agrees_with_a_fresh_instance() {
    // Determinism across the two instance strategies. If reset left residue,
    // corpus-scale runs would silently diverge from the frozen protocol.
    let inputs: &[&[u8]] = &[b"nihao", b"xian", b"fangan", b"nih", b"xi'an", b"!ni", b""];

    let mut oracle = oracle();

    let fresh: Vec<_> = inputs
        .iter()
        .map(|input| {
            oracle
                .observe(input, OracleFlags::DEFAULT)
                .expect("fresh instance observation")
        })
        .collect();

    let mut session = oracle
        .session(OracleFlags::DEFAULT)
        .expect("session allocates");
    let reused: Vec<_> = inputs
        .iter()
        .map(|input| session.observe(input).expect("reset observation"))
        .collect();

    assert_eq!(fresh, reused);
}

#[test]
fn interior_nul_is_rejected_without_reaching_the_oracle() {
    let mut oracle = oracle();
    assert!(matches!(
        oracle.observe(b"ni\0hao", OracleFlags::DEFAULT),
        Err(OracleError::InteriorNul { .. })
    ));
}

#[test]
fn dynamic_adjust_cannot_be_requested_of_a_live_context() {
    let mut oracle = oracle();
    let forbidden = OracleFlags::new(pinyin_oracle::flags::CAPTURE_DEFAULT | 0x200);
    assert!(matches!(
        forbidden,
        Err(OracleError::DynamicAdjustRejected { .. })
    ));
    // The legal profile still works afterwards.
    assert!(oracle.observe(b"ni", OracleFlags::DEFAULT).is_ok());
}

#[test]
fn located_prefix_is_on_pin_and_reports_its_digests() {
    let prefix = OraclePrefix::locate().expect("oracle prefix is present");
    let pin = prefix.pin();
    assert_eq!(pin.pin_ref(), EXPECTED_PIN_REF);
    assert_eq!(
        pin.header_sha256(),
        "e1138482d06766163608406fe1083539b21ff8c44ea04f329f3db0c78a312d47",
        "the FFI in src/ffi.rs is declared against this header"
    );
    assert_eq!(
        pin.libpinyin_commit(),
        "0c5e80e1200f84fab185d1c5bde458b770a0636c"
    );
    assert!(prefix.data_dir().is_dir());
}

#[test]
fn fresh_user_directory_is_created_and_removed() {
    let path = {
        let oracle = oracle();
        let path = oracle.user_dir().to_path_buf();
        assert!(path.is_dir(), "user directory exists while open");
        path
    };
    assert!(!path.exists(), "user directory is removed on drop");
}
