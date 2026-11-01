//! Tool-diff helper: print a DBM file's logical rows, not its container
//! metadata. Not a pin utility and never installed.

use std::ffi::OsString;
use std::ops::Bound;

use oxpinyin_store::{DefaultStore, RawReadStore as _, ReadStore as _};

fn main() {
    let path = std::env::args_os().nth(1).expect("dbm file");
    let rows = rows(&path);
    for (key, value) in rows {
        println!("{} {}", hex(&key), hex(&value));
    }
}

fn rows(path: &OsString) -> Vec<(Vec<u8>, Vec<u8>)> {
    let path = std::path::Path::new(path);
    let mut rows = Vec::new();
    // A tkrzw TreeDBM is backed by a HashDBM: opening it as a hash can
    // succeed but yields internal pages, not the tree's logical records.
    if let Ok(store) = DefaultStore::open_read_only(path) {
        store
            .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
                rows.push((key.to_vec(), value.to_vec()));
                Ok(())
            })
            .expect("tree rows");
    } else {
        let store = DefaultStore::open_hash_read_only(path).expect("open DBM");
        for key in store.native_keys().expect("hash rows") {
            let mut value = Vec::new();
            assert!(store.get_raw(&key, &mut value).expect("hash value"));
            rows.push((key, value));
        }
    }
    rows.sort_unstable();
    rows
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
