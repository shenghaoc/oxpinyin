//! The utilities' input stream: `std::cin`, or the `std::ifstream` the
//! second remaining argument opens (`ngseg.cpp:105-143`,
//! `spseg.cpp:202-238`, `mergeseq.cpp:201-237`).
//!
//! The stream is opened after the output file and the argument-count
//! check, so a missing input reports `open file failed` and exits
//! `EINVAL` without reading `table.conf`. The reader is the boundary's
//! [`LineReader`], under `std::getline`'s contract.

use std::ffi::CStr;
use std::fs::File;
use std::io::{BufReader, StdinLock};

use oxpinyin_input::tool::{self, LineReader};

/// Either `std::cin` (tied: each read flushes `stdout`) or an
/// `std::ifstream` (untied).
pub(crate) enum Input {
    /// `std::istream * input = &std::cin;`
    Stdin(LineReader<StdinLock<'static>>),
    /// `input_file.open(argv[1])`, which succeeded.
    File(LineReader<BufReader<File>>),
}

impl Input {
    /// The remaining arguments after option parsing: `argv[0]` plus an
    /// optional input file. The caller has already refused more than two.
    pub(crate) fn open(args: &[Vec<u8>]) -> Self {
        let Some(name) = args.get(1) else {
            return Self::Stdin(LineReader::stdin());
        };
        let name = crate::segment::c_string(name).unwrap_or_default();
        let reader = LineReader::open(&name);
        if !reader.is_open() {
            // `perror("open file failed")` with the `errno` the failed open
            // left, then `exit(EINVAL)`.
            tool::perror(c"open file failed");
            tool::exit(tool::EINVAL);
        }
        Self::File(reader)
    }

    /// `std::getline(*input, linebuf)`.
    pub(crate) fn getline(&mut self) -> bool {
        match self {
            Self::Stdin(reader) => reader.getline(),
            Self::File(reader) => reader.getline(),
        }
    }

    /// `linebuf` as the `std::string`: its bytes, NUL included.
    pub(crate) fn line(&self) -> &[u8] {
        match self {
            Self::Stdin(reader) => reader.line(),
            Self::File(reader) => reader.line(),
        }
    }

    /// `static_cast<const gchar *>(linebuf)`: `c_str()`.
    pub(crate) fn c_str(&self) -> &CStr {
        match self {
            Self::Stdin(reader) => reader.c_str(),
            Self::File(reader) => reader.c_str(),
        }
    }
}
