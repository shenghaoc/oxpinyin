//! Build-time interning shared by the core and chewing table generators.

use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Default)]
pub struct StringPool {
    blob: String,
    spans: BTreeMap<String, (u16, u16)>,
}

impl StringPool {
    fn intern(&mut self, text: &str) -> (u16, u16) {
        if let Some(&span) = self.spans.get(text) {
            return span;
        }
        let offset = u16::try_from(self.blob.len()).expect("string blob offset fits u16");
        let length = u16::try_from(text.len()).expect("string length fits u16");
        self.blob.push_str(text);
        assert!(
            self.blob.len() <= usize::from(u16::MAX),
            "string blob fits u16"
        );
        self.spans.insert(text.to_owned(), (offset, length));
        (offset, length)
    }

    pub fn table<const N: usize>(
        &mut self,
        output: &mut String,
        name: &str,
        accessor: &str,
        rows: impl ExactSizeIterator<Item = [&'static str; N]>,
        runtime_path: &str,
    ) {
        writeln!(
            output,
            "static {name}: [[{runtime_path}::StringSpan; {N}]; {}] = [",
            rows.len()
        )
        .expect("write table declaration");
        for row in rows {
            write!(output, "[").expect("write row");
            for text in row {
                let (offset, length) = self.intern(text);
                write!(
                    output,
                    "{runtime_path}::StringSpan::new({offset}, {length}),"
                )
                .expect("write span");
            }
            writeln!(output, "],").expect("write row end");
        }
        writeln!(output, "];\npub(crate) const fn {accessor}() -> {runtime_path}::StringTable<{N}> {{ {runtime_path}::StringTable::new(STRING_BLOB, &{name}) }}\n")
            .expect("write table accessor");
    }

    pub fn finish(self, output: &mut String) {
        writeln!(output, "const STRING_BLOB: &str = {:?};", self.blob).expect("write string blob");
    }
}
