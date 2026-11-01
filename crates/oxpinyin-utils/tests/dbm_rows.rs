//! The comparator must read logical records from both container classes.
use oxpinyin_store::{DefaultStore, WriteStore as _, WriteTxn as _};

#[test]
fn tree_and_hash_rows_keep_keys_values_and_empty_values() {
    for hash in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("records.db");
        let store = if hash {
            DefaultStore::create_hash(&path).unwrap()
        } else {
            DefaultStore::create(&path).unwrap()
        };
        store
            .write(|txn| {
                txn.put_raw(&[2, 0], &[5, 6])?;
                txn.put_raw(&[1, 0], &[1, 2, 3, 4])?;
                txn.put_raw(&[3, 0], &[])
            })
            .unwrap();
        drop(store);
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_dbm_rows"))
            .arg(&path)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(output.stdout, b"0100 01020304\n0200 0506\n0300 \n");
    }
}
