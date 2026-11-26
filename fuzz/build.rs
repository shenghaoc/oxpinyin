//! Rebuilds the fuzz package when `instrument-denylist.txt` changes. The
//! recipe passes the file to the AFL++ LLVM plugins as `AFL_LLVM_DENYLIST`
//! (tools/dev/fuzz.py), and Cargo does not track a file that only an
//! environment variable names.

fn main() {
    println!("cargo::rerun-if-changed=instrument-denylist.txt");
}
