//! Writes a `bigram.db` through the Berkeley DB backend's user-bigram
//! seam so a reader that has no Rust in it can check the bytes.
//!
//! `tests/bdb_libpinyin_files.rs` proves the backend reads what libpinyin
//! wrote. This is the other direction: a file this backend *created*,
//! handed to `tools/bdb/hash-walk.c`, which drives libdb directly and
//! checks every `SingleGram` invariant. If that walk is clean, the file
//! is one libpinyin's own code can read.
//!
//! Usage: cargo run -p oxpinyin-store --features bdb --example \
//!            bdb_write_profile -- PATH

#[cfg(feature = "bdb")]
fn main() {
    use oxpinyin_store::WriteStore;

    let path = std::env::args()
        .nth(1)
        .expect("usage: bdb_write_profile PATH");
    let _ = std::fs::remove_file(&path);

    // A SingleGram chunk (`ngram.cpp:31-74`): a 4-byte native-endian
    // `total_freq`, then 8-byte `{token, freq}` items kept ascending by
    // token; the key is the raw 4 bytes of the previous token
    // (`ngram_bdb.cpp`). Tokens crossing 256 in the low and in a higher
    // byte, inserted out of order, so a writer that kept insertion order
    // produces a file the C walk rejects as unsorted.
    fn gram(total: u32, items: &[(u32, u32)]) -> Vec<u8> {
        let mut bytes = total.to_le_bytes().to_vec();
        let mut items: Vec<_> = items.to_vec();
        items.sort_by_key(|(token, _)| *token);
        for (token, freq) in items {
            bytes.extend_from_slice(&token.to_le_bytes());
            bytes.extend_from_slice(&freq.to_le_bytes());
        }
        bytes
    }
    let rows: Vec<(Vec<u8>, Vec<u8>)> = [
        (
            1_u32,
            gram(
                67,
                &[
                    (0x0100_05db_u32, 52_u32),
                    (0x0000_00ff, 7),
                    (0x0100_a271, 8),
                ],
            ),
        ),
        (0x0100_0001, gram(15, &[(0x0100_0002, 15)])),
        (0x0000_00ff, gram(23, &[(0x0000_0100, 23)])),
        (
            0x0300_1801,
            gram(96, &[(0x0100_05db, 52), (0x0300_18ff, 44)]),
        ),
    ]
    .into_iter()
    .map(|(prev, value)| (prev.to_le_bytes().to_vec(), value))
    .collect();

    oxpinyin_store::BdbStore::write_user_bigram(std::path::Path::new(&path), &rows)
        .expect("write the user bigram");
    println!("wrote {path}");
}

#[cfg(not(feature = "bdb"))]
fn main() {
    eprintln!("build with --features bdb");
    std::process::exit(2);
}
