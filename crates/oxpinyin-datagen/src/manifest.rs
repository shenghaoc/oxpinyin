//! The datagen manifest: provenance and fingerprints of one compile run.
//!
//! CI and packagers publish this next to the produced tables so a
//! differential run can report exactly which source, producer, and backend
//! produced its data (`docs/findings/datagen-model20.md`).

use std::fs;
use std::path::Path;

use crate::write::Backend;
use crate::{DatagenError, fnv1a64};

/// Manifest file name, written into the compile output directory.
pub const MANIFEST_FILE: &str = "datagen-manifest.txt";

/// Manifest schema line.
pub const MANIFEST_SCHEMA: &str = "oxpinyin-datagen-manifest-v1";

/// One produced table's fingerprint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableRecord {
    /// Output file name (base + backend extension).
    pub file: String,
    /// Number of `(key, value)` rows.
    pub records: u64,
    /// FNV-1a 64 of the output file bytes.
    pub fnv1a64: u64,
}

/// A compile run's provenance record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Manifest {
    /// The backend the tables were written for.
    pub backend: Backend,
    /// The pinned model20 archive SHA-256 this compile consumed.
    pub model_sha256: String,
    /// The crate version that produced the tables.
    pub producer_version: String,
    /// Per-table fingerprints, in write order.
    pub tables: Vec<TableRecord>,
}

impl Manifest {
    /// Serializes the manifest as newline-delimited metadata and table fingerprint records.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let text = manifest.render();
    /// assert!(text.contains("table=users"));
    /// ```
    ///
    /// Returns the serialized manifest text.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(MANIFEST_SCHEMA);
        out.push('\n');
        out.push_str(&format!("pin_ref=model20-{}\n", self.model_sha256));
        out.push_str(&format!("backend={}\n", self.backend.extension()));
        out.push_str(&format!(
            "producer=oxpinyin-datagen@{}\n",
            self.producer_version
        ));
        for table in &self.tables {
            out.push_str(&format!(
                "table={} records={} fnv1a64={:016x}\n",
                table.file, table.records, table.fnv1a64
            ));
        }
        out
    }

    /// Records the fingerprint and row count of an already-written table file.
    ///
    /// # Arguments
    ///
    /// * `file` - The table filename to store in the record.
    /// * `path` - The path to the table file.
    /// * `records` - The number of rows in the table.
    ///
    /// # Errors
    ///
    /// Returns an error if the table file cannot be read.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn example() -> Result<(), DatagenError> {
    /// let record = Manifest::record_file("words.csv", std::path::Path::new("words.csv"), 42)?;
    /// assert_eq!(record.file, "words.csv");
    /// assert_eq!(record.records, 42);
    /// # Ok(())
    /// # }
    /// ```
    pub fn record_file(file: &str, path: &Path, records: u64) -> Result<TableRecord, DatagenError> {
        let bytes = fs::read(path)?;
        Ok(TableRecord {
            file: file.to_owned(),
            records,
            fnv1a64: fnv1a64(&bytes),
        })
    }

    /// Writes the rendered manifest to `datagen-manifest.txt` in `out_dir`.
    ///
    /// # Errors
    ///
    /// Returns a [`DatagenError`] if the manifest cannot be written.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use std::path::Path;
    /// # let manifest = todo!();
    /// manifest.write_to_dir(Path::new("output"))?;
    /// # Ok::<(), DatagenError>(())
    /// ```
    pub fn write_to_dir(&self, out_dir: &Path) -> Result<(), DatagenError> {
        fs::write(out_dir.join(MANIFEST_FILE), self.render())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_renders_every_field() {
        let manifest = Manifest {
            backend: Backend::Redb,
            model_sha256: "59c68e89".to_owned(),
            producer_version: "0.1.0".to_owned(),
            tables: vec![TableRecord {
                file: "pinyin_index.redb".to_owned(),
                records: 93_349,
                fnv1a64: 0x0123_4567_89ab_cdef,
            }],
        };
        let text = manifest.render();
        assert!(text.starts_with("oxpinyin-datagen-manifest-v1\n"));
        assert!(text.contains("pin_ref=model20-59c68e89\n"));
        assert!(text.contains("backend=redb\n"));
        assert!(text.contains("producer=oxpinyin-datagen@0.1.0\n"));
        assert!(text.contains("table=pinyin_index.redb records=93349 fnv1a64=0123456789abcdef\n"));
    }
}
