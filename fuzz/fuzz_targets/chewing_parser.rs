//! Hostile bytes through the eight parseable Zhuyin keyboards
//! (`docs/testing/upstream-test-coverage.md`, "Fuzz harnesses"):
//! the `parser` target exercises `FullPinyinParser` only, so the chewing
//! path below the ABI was unproven at the byte level against
//! constitution rule 4 (nothing panics on any input).
//!
//! The law per `(layout, use_tone, allow_incomplete)` is the
//! `test_parser2` hard invariant lifted to the scheme seam (the same
//! one `src/scheme/tests.rs` pins for curated
//! inputs): the parse is total, deterministic, and every key carries an
//! ordered, non-overlapping span inside the input with `consumed`
//! reaching at least past the last span. A finding is any abort, panic,
//! or sanitizer report, or a violated invariant here.

use oxpinyin::__internal::parser::ZhuyinParser;
use oxpinyin::OptionBits;
use oxpinyin::facade::ZhuyinScheme;

/// Every zhuyin keyboard except the `StandardDvorak` abort slot (upstream's
/// setter aborts on it; `ZhuyinParser` reports `false` and keeps the current
/// scheme, so there is no parser surface to fuzz there).
const SCHEMES: &[ZhuyinScheme] = &[
    ZhuyinScheme::Standard,
    ZhuyinScheme::Hsu,
    ZhuyinScheme::Ibm,
    ZhuyinScheme::Ginyieh,
    ZhuyinScheme::Eten,
    ZhuyinScheme::Eten26,
    ZhuyinScheme::HsuDvorak,
    ZhuyinScheme::DachenCp26,
];

/// Inputs per child: AFL++'s suggested start
/// (`instrumentation/README.persistent_mode.md`), which bounds a child's
/// lifetime. `AFL_FUZZER_LOOPCOUNT` overrides it.
const LOOP_COUNT: usize = 1000;

fn main() {
    oxpinyin_fuzz::fuzz(LOOP_COUNT, &[], run);
}

/// Runs one input.
fn run(data: &[u8]) {
    for scheme in SCHEMES {
        for use_tone in [false, true] {
            for allow_incomplete in [false, true] {
                let parser = ZhuyinParser::with_scheme(*scheme);
                let mut options = OptionBits::empty();
                options.set(OptionBits::USE_TONE, use_tone);
                options.set(OptionBits::ZHUYIN_INCOMPLETE, allow_incomplete);
                let first = parser.parse_with_options(data, options);
                let second = parser.parse_with_options(data, options);
                assert_eq!(first, second, "{scheme:?}: parse must be deterministic");

                // The test_parser2 span law, verbatim from the curated test.
                let mut cursor = 0;
                for key in first.keys() {
                    assert!(
                        key.start() >= cursor,
                        "{scheme:?}: key span {}..{} overlaps or regresses (cursor {cursor})",
                        key.start(),
                        key.end()
                    );
                    assert!(key.end() > key.start(), "{scheme:?}: empty key span");
                    assert!(
                        key.end() <= data.len(),
                        "{scheme:?}: key span {}..{} runs past the input",
                        key.start(),
                        key.end()
                    );
                    assert!(
                        key.tone() <= 5,
                        "{scheme:?}: tone {} is outside the 1..=5 / zero-tone range",
                        key.tone()
                    );
                    cursor = key.end();
                }
                assert!(
                    first.consumed() >= cursor,
                    "{scheme:?}: consumed {} but spans only reach {cursor}",
                    first.consumed()
                );
                assert!(
                    first.consumed() <= data.len(),
                    "{scheme:?}: consumed {} of a {}-byte input",
                    first.consumed(),
                    data.len()
                );
            }
        }
    }
}
