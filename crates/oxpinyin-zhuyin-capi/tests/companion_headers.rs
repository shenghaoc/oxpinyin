//! The companion headers `novel_types.h` and `pinyin_custom2.h` are shipped
//! verbatim by BOTH `oxpinyin-capi` and `oxpinyin-zhuyin-capi` into the same
//! installed include subdirectory (`libpinyin-2.11.91/`), because each
//! library must be installable on its own and its public header must still
//! resolve. Two copies in one destination are only coherent while they are
//! byte-identical; this test is the tree-level gate on that invariant.
//! `tools/packaging/install.sh` repeats the check as a pre-install guard.
// Placed below the `//!` block, never above it: a crate-level `#![cfg]`
// that evaluates false discards the crate attributes that FOLLOW it, so a
// gate on line 1 takes these docs with it and `missing_docs` then fires on
// every non-Linux host. Same placement as the other cfg-gated test crates.
#![cfg(target_os = "linux")]

use std::fs;
use std::path::Path;

const COMPANION_HEADERS: &[&str] = &["novel_types.h", "pinyin_custom2.h"];

#[test]
fn companion_headers_are_byte_identical_across_crates() {
    let zhuyin_crate = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pinyin_crate = zhuyin_crate.join("..").join("oxpinyin-capi");
    for name in COMPANION_HEADERS {
        let zhuyin_path = zhuyin_crate.join(name);
        let pinyin_path = pinyin_crate.join(name);
        let zhuyin_bytes = fs::read(&zhuyin_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", zhuyin_path.display()));
        let pinyin_bytes = fs::read(&pinyin_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", pinyin_path.display()));
        assert!(
            zhuyin_bytes == pinyin_bytes,
            "companion header {name} differs between {} and {}; both crates install it \
             into the same include subdirectory, so the copies must be byte-identical",
            pinyin_path.display(),
            zhuyin_path.display()
        );
    }
}
