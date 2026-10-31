//! `gen_deleted_ngram` (`utils/training/gen_deleted_ngram.cpp` at
//! libpinyin 074a2219).
//!
//! The `gen_ngram` loop over the deleted bigram: no unigram count and no
//! chunk save, `--deleted-bigram-file` instead of `--bigram-file`, and
//! `ENODATA` where the system trainer exits `ENOENT` on a refused chunk
//! load (`gen_deleted_ngram.cpp:72-73`).

use std::ffi::OsString;

/// `main` (`gen_deleted_ngram.cpp:46-119`).
pub fn main(args: impl IntoIterator<Item = OsString>) {
    super::gen_ngram::run(args, true);
}
