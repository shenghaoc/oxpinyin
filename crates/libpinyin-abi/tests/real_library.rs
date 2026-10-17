//! The subject loader against a real libpinyin and libzhuyin.
//!
//! The unit tests exercise the ownership and missing-symbol rules against an
//! in-process mock on every platform; this file is the check that the same
//! calls work against a library somebody built. It needs one, so it is ignored
//! by default and *fails* — it does not skip — when its inputs are absent:
//!
//! ```text
//! LIBPINYIN_ABI_PINYIN_SO=/path/to/libpinyin.so \
//! LIBPINYIN_ABI_SYSTEM_DIR=/path/to/data \
//! LIBPINYIN_ABI_ZHUYIN_SO=/path/to/libzhuyin.so   # optional
//! cargo test -p libpinyin-abi --test real_library -- --include-ignored --nocapture
//! ```
//!
//! The data directory must have been written by the backend the library under
//! test was built with. The test prints one stable line per observation, so two
//! libraries can be compared by running it once for each (one process per
//! library, as a differential should) and diffing the output.

use std::path::PathBuf;

use libpinyin_abi::option::SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY as SORT;
use libpinyin_abi::{AbiError, Facade, Subject, ZhuyinInput};

/// `IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE`.
const OPTIONS: u32 = 0x18a;
const PINYIN_INPUTS: [&[u8]; 6] = [
    b"nihao",
    b"zhongguo",
    b"xian",
    b"n",
    b"nihaoshijie",
    b"zhonghuarenmingongheguo",
];

fn required(name: &str) -> PathBuf {
    match std::env::var_os(name) {
        Some(value) => PathBuf::from(value),
        None => panic!("missing input: {name} (see the module documentation)"),
    }
}

/// A fresh, empty user directory removed on drop.
struct UserDir(PathBuf);

impl UserDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("libpinyin-abi-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create the user directory");
        Self(path)
    }
}

impl Drop for UserDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn show(texts: impl Iterator<Item = String>) -> String {
    texts.collect::<Vec<_>>().join("|")
}

#[test]
#[ignore = "needs a libpinyin build (LIBPINYIN_ABI_PINYIN_SO, LIBPINYIN_ABI_SYSTEM_DIR); run with --include-ignored"]
fn a_real_library_answers_through_the_subject() -> Result<(), AbiError> {
    let pinyin_so = required("LIBPINYIN_ABI_PINYIN_SO");
    let system = required("LIBPINYIN_ABI_SYSTEM_DIR");

    {
        let user = UserDir::new("pinyin");
        let subject = Subject::open(&pinyin_so, Facade::Pinyin)?;
        println!("missing={:?}", subject.missing_symbols());
        // A second default open in this process is refused.
        assert!(matches!(
            Subject::open(&pinyin_so, Facade::Pinyin),
            Err(AbiError::SubjectAlreadyOpen)
        ));
        let context = subject.open_context(&system, &user.0)?;
        context.set_options(OPTIONS)?;
        let mut session = context.pinyin_session()?;
        for input in PINYIN_INPUTS {
            let seen = session.observe(input, SORT, 10)?;
            assert!(seen.parsed_input_length <= input.len());
            println!(
                "pinyin\t{}\tparse={}\tparsed={}\tsentence={}\ttotal={}\t{}",
                String::from_utf8_lossy(input),
                seen.parse_return,
                seen.parsed_input_length,
                seen.sentence.as_deref().unwrap_or("-"),
                seen.candidate_total,
                show(seen.candidates.iter().map(|c| format!(
                    "{}#{}#{}",
                    c.text,
                    c.raw_type,
                    c.nbest_index.map_or_else(|| "-".to_owned(), |i| i.to_string())
                )))
            );
        }
    }

    if let Some(zhuyin_so) = std::env::var_os("LIBPINYIN_ABI_ZHUYIN_SO") {
        let user = UserDir::new("zhuyin");
        let subject = Subject::open(PathBuf::from(zhuyin_so), Facade::Zhuyin)?;
        println!("missing={:?}", subject.missing_symbols());
        let context = subject.open_context(&system, &user.0)?;
        let mut session = context.zhuyin_session()?;
        // A pinyin session on a zhuyin context is refused, not miscalled.
        assert!(matches!(
            context.pinyin_session(),
            Err(AbiError::FacadeMismatch { .. })
        ));
        for (kind, input) in [
            (ZhuyinInput::FullPinyin, &b"ni3hao3"[..]),
            (ZhuyinInput::FullPinyin, b"zhong1guo2"),
            (ZhuyinInput::Chewing, b"su3cl3"),
        ] {
            let seen = session.observe(kind, input, 10)?;
            assert!(seen.parsed_input_length <= input.len());
            println!(
                "zhuyin\t{kind:?}\t{}\tparse={}\tparsed={}\tsentence={}\ttotal={}\t{}",
                String::from_utf8_lossy(input),
                seen.parse_return,
                seen.parsed_input_length,
                seen.sentence.as_deref().unwrap_or("-"),
                seen.candidate_total,
                show(
                    seen.candidates
                        .iter()
                        .map(|c| format!("{}#{}", c.text, c.raw_type))
                )
            );
        }
    }
    Ok(())
}
