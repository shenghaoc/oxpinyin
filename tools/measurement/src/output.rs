//! Streaming JSON without owned intermediate values in a measured process.
use std::io::{self, Write};

fn short_escape(byte: u8) -> Option<&'static [u8]> {
    match byte {
        b'"' => Some(b"\\\""),
        b'\\' => Some(b"\\\\"),
        b'\n' => Some(b"\\n"),
        b'\r' => Some(b"\\r"),
        b'\t' => Some(b"\\t"),
        _ => None,
    }
}

pub(crate) fn string(out: &mut impl Write, text: &str) -> io::Result<()> {
    out.write_all(b"\"")?;
    for byte in text.bytes() {
        match short_escape(byte) {
            Some(escape) => out.write_all(escape)?,
            None if byte < 32 => write!(out, "\\u{byte:04x}")?,
            None => out.write_all(&[byte])?,
        }
    }
    out.write_all(b"\"")
}

pub(crate) fn nullable(out: &mut impl Write, value: Option<&str>) -> io::Result<()> {
    match value {
        Some(text) => string(out, text),
        None => out.write_all(b"null"),
    }
}

pub(crate) fn numbers(out: &mut impl Write, values: &[u64]) -> io::Result<()> {
    out.write_all(b"[")?;
    for (i, value) in values.iter().enumerate() {
        if i != 0 {
            out.write_all(b",")?;
        }
        write!(out, "{value}")?;
    }
    out.write_all(b"]")
}
