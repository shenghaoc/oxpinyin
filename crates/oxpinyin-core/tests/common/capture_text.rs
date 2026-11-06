//! Field decoding for the `pinyin-capture-v1` text the frozen captures use.

/// Decodes one `pinyin-capture-v1` escaped field back to its bytes.
///
/// A trailing lone backslash or a malformed `\xNN` is preserved literally rather
/// than treated as fatal: a fixture the reader cannot fully decode must still be
/// reportable, not silently dropped.
pub fn unescape(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'\\' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        match bytes.get(index + 1) {
            Some(b'\\') => {
                out.push(b'\\');
                index += 2;
            }
            Some(b't') => {
                out.push(b'\t');
                index += 2;
            }
            Some(b'n') => {
                out.push(b'\n');
                index += 2;
            }
            Some(b'r') => {
                out.push(b'\r');
                index += 2;
            }
            Some(b'x') => {
                let hex = bytes.get(index + 2..index + 4).and_then(|pair| {
                    let text = core::str::from_utf8(pair).ok()?;
                    u8::from_str_radix(text, 16).ok()
                });
                if let Some(value) = hex {
                    out.push(value);
                    index += 4;
                } else {
                    out.push(b'\\');
                    index += 1;
                }
            }
            _ => {
                out.push(b'\\');
                index += 1;
            }
        }
    }
    out
}
