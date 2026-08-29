#![no_main]
//! Hostile bytes through the libpinyin-format table decoder — audit F-3's
//! class (`docs/safety/oxpinyin-audit.md`): `ContentTable::load` must
//! return a value on every input (no panic, no allocator abort) and stay
//! deterministic. The committed regression test pins the clamp; this
//! target hunts the rest of the header/record state space.

use libfuzzer_sys::fuzz_target;
use oxpinyin_data::ContentTable;

fuzz_target!(|data: &[u8]| {
    let first = ContentTable::load(data);
    let second = ContentTable::load(data);
    match (first, second) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a.len(), b.len(), "decode must be deterministic");
            assert_eq!(a.magic(), b.magic());
            assert_eq!(a.version(), b.version());
            for index in 0..a.len() {
                assert_eq!(
                    a.get(index).is_some(),
                    b.get(index).is_some(),
                    "record walk must be deterministic"
                );
            }
        }
        (Err(_), Err(_)) => {}
        _ => panic!("decode must be deterministic"),
    }
});
