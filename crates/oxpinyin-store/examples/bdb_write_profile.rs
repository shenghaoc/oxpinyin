//! Writes a `bigram.db` through the Berkeley DB backend so a reader that
//! has no Rust in it can check the bytes.
//!
//! The round-trip test proves `encode(decode(x)) == x` over records
//! libpinyin wrote. This is the other direction: a file this backend
//! *created*, handed to `tools/bdb/hash-walk.c`, which drives libdb
//! directly and checks every `SingleGram` invariant. If that walk is
//! clean, the file is one libpinyin's own code can read.
//!
//! Usage: cargo run -p oxpinyin-store --features bdb --example \
//!            bdb_write_profile -- PATH

#[cfg(feature = "bdb")]
fn main() {
    use oxpinyin_store::{BigramDb, SingleGram};

    let path = std::env::args()
        .nth(1)
        .expect("usage: bdb_write_profile PATH");
    let _ = std::fs::remove_file(&path);
    let db = BigramDb::open(std::path::Path::new(&path), false).expect("create");

    // Tokens crossing 256 in the low and in a higher byte, inserted out of
    // order, so a writer that kept insertion order produces a file the
    // walk rejects as unsorted.
    for prev in [1_u32, 0x0100_0001, 0x0000_00ff, 0x0300_1801] {
        let mut gram = SingleGram::new();
        let mut total = 0_u32;
        for (offset, freq) in [
            (0x0100_a271_u32, 52_u32),
            (0x0000_00ff, 7),
            (0x0100_05db, 8),
        ] {
            gram.set_freq(offset.wrapping_add(prev & 0xff), freq);
            total += freq;
        }
        gram.set_total(total);
        db.put(prev, &gram).expect("put");
    }
    db.sync().expect("sync");
    println!("wrote {path}");
}

#[cfg(not(feature = "bdb"))]
fn main() {
    eprintln!("build with --features bdb");
    std::process::exit(2);
}
