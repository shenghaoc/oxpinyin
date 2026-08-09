//! Pin verification for the source-built oracle prefix.
//!
//! `docs/findings/oracle-environment.md` states that W2-T3 and every S1b
//! parity run load only the shared object produced by
//! `tools/oracle/build-oracle.sh`. This module is the gate that enforces it.
//! It is pure: it reads the manifest the recipe writes and decides whether the
//! prefix is the frozen pin. No FFI, no feature gate, so the rule stays under
//! test on every platform.
//!
//! A distribution-provided libpinyin cannot satisfy [`EXPECTED_PIN_REF`], so it
//! can never be mistaken for the oracle.

use std::path::{Path, PathBuf};

use crate::OracleError;
pub use crate::pin_ref::{EXPECTED_PIN_REF, MANIFEST_FILE_NAME};

/// Data-payload manifest the build recipe writes into the prefix.
pub const DATA_MANIFEST_FILE_NAME: &str = "oracle-data.sha256";

/// Manifest schema this harness understands.
pub const MANIFEST_SCHEMA: &str = "pinyin-oracle-v1";

/// DBM backend the pin requires. The deprecated Berkeley DB backend is not used.
pub const EXPECTED_DBM: &str = "Tkrzw";

/// libpinyin release tag the pin requires.
pub const EXPECTED_LIBPINYIN_TAG: &str = "2.11.91";

/// Relative directory holding the generated model tables inside the prefix.
pub const DATA_SUBDIR: &str = "lib/libpinyin/data";

/// A parsed `oracle-pin.txt`.
///
/// Field order is preserved and repeated fields are rejected, so a manifest
/// cannot smuggle a second value for a field the harness checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PinManifest {
    fields: Vec<(String, String)>,
}

impl PinManifest {
    /// Parses manifest text.
    ///
    /// Lines are `key=value`. Blank lines are ignored. A line without `=` is
    /// ignored rather than fatal, so a future recipe may add commentary; a
    /// repeated key is fatal, because its value would be ambiguous.
    pub fn parse(text: &str) -> Result<Self, OracleError> {
        let mut fields: Vec<(String, String)> = Vec::new();

        for line in text.lines() {
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if fields.iter().any(|(existing, _)| existing == key) {
                return Err(OracleError::ManifestFieldRepeated {
                    field: key.to_owned(),
                });
            }
            fields.push((key.to_owned(), value.to_owned()));
        }

        Ok(Self { fields })
    }

    /// Reads and parses `oracle-pin.txt` from `prefix`.
    pub fn read_from_prefix(prefix: &Path) -> Result<Self, OracleError> {
        let path = prefix.join(MANIFEST_FILE_NAME);
        let bytes = std::fs::read(&path).map_err(|source| OracleError::ManifestUnreadable {
            path: path.clone(),
            source,
        })?;
        let text = String::from_utf8(bytes)
            .map_err(|_| OracleError::ManifestNotUtf8 { path: path.clone() })?;
        Self::parse(&text)
    }

    /// Returns the value of `field`, if present.
    #[must_use]
    pub fn field(&self, field: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(key, _)| key == field)
            .map(|(_, value)| value.as_str())
    }

    fn require(&self, field: &'static str) -> Result<&str, OracleError> {
        self.field(field)
            .ok_or(OracleError::ManifestFieldMissing { field })
    }

    fn require_eq(&self, field: &'static str, expected: &str) -> Result<(), OracleError> {
        let found = self.require(field)?;
        if found == expected {
            return Ok(());
        }
        Err(OracleError::ManifestFieldMismatch {
            field,
            expected: expected.to_owned(),
            found: found.to_owned(),
        })
    }

    /// Checks the manifest against the frozen pin.
    ///
    /// Verifies schema, pin ref, DBM backend and libpinyin tag, and requires
    /// the recorded artefact digests to be present. Digest *values* are
    /// reported for the log; re-hashing the payload is the build recipe's and
    /// `run-capture.sh`'s job and is not repeated here.
    pub fn verify(&self) -> Result<VerifiedPin, OracleError> {
        self.require_eq("schema", MANIFEST_SCHEMA)?;
        self.require_eq("pin_ref", EXPECTED_PIN_REF)?;
        self.require_eq("dbm", EXPECTED_DBM)?;
        self.require_eq("libpinyin_tag", EXPECTED_LIBPINYIN_TAG)?;

        Ok(VerifiedPin {
            pin_ref: self.require("pin_ref")?.to_owned(),
            libpinyin_commit: self.require("libpinyin_commit")?.to_owned(),
            model_sha256: self.require("model_sha256")?.to_owned(),
            header_sha256: self.require("header_sha256")?.to_owned(),
            shared_object_sha256: self.require("shared_object_sha256")?.to_owned(),
            data_manifest_sha256: self.require("data_manifest_sha256")?.to_owned(),
        })
    }
}

/// A prefix whose manifest matched the frozen pin.
///
/// Holding this value is the proof obligation for opening an oracle context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedPin {
    pin_ref: String,
    libpinyin_commit: String,
    model_sha256: String,
    header_sha256: String,
    shared_object_sha256: String,
    data_manifest_sha256: String,
}

impl VerifiedPin {
    /// Pin reference string stamped into every observation.
    #[must_use]
    pub fn pin_ref(&self) -> &str {
        &self.pin_ref
    }

    /// libpinyin source commit the prefix was built from.
    #[must_use]
    pub fn libpinyin_commit(&self) -> &str {
        &self.libpinyin_commit
    }

    /// SHA-256 of the model archive the prefix was built with.
    #[must_use]
    pub fn model_sha256(&self) -> &str {
        &self.model_sha256
    }

    /// SHA-256 of the installed public header the FFI is declared against.
    #[must_use]
    pub fn header_sha256(&self) -> &str {
        &self.header_sha256
    }

    /// SHA-256 of the pin-built shared object.
    #[must_use]
    pub fn shared_object_sha256(&self) -> &str {
        &self.shared_object_sha256
    }

    /// SHA-256 of the generated data-payload manifest.
    #[must_use]
    pub fn data_manifest_sha256(&self) -> &str {
        &self.data_manifest_sha256
    }
}

/// A prefix that carries a verified pin and the artefacts a run needs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OraclePrefix {
    root: PathBuf,
    data_dir: PathBuf,
    pin: VerifiedPin,
}

impl OraclePrefix {
    /// Verifies `root` as an oracle prefix.
    ///
    /// Requires a matching pin manifest, the data-payload manifest, and the
    /// generated data directory the oracle loads its tables from.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, OracleError> {
        let root = root.into();
        let pin = PinManifest::read_from_prefix(&root)?.verify()?;

        let data_manifest = root.join(DATA_MANIFEST_FILE_NAME);
        if !data_manifest.is_file() {
            return Err(OracleError::PrefixArtefactMissing {
                path: data_manifest,
            });
        }

        let data_dir = root.join(DATA_SUBDIR);
        if !data_dir.is_dir() {
            return Err(OracleError::PrefixArtefactMissing { path: data_dir });
        }

        Ok(Self {
            root,
            data_dir,
            pin,
        })
    }

    /// Root of the prefix.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// System data directory to hand to `pinyin_init`.
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The verified pin.
    #[must_use]
    pub fn pin(&self) -> &VerifiedPin {
        &self.pin
    }
}

#[cfg(test)]
mod tests {
    use super::{EXPECTED_PIN_REF, MANIFEST_SCHEMA, PinManifest};
    use crate::OracleError;

    fn manifest_text(pin_ref: &str, dbm: &str) -> String {
        format!(
            "schema={MANIFEST_SCHEMA}\n\
             pin_ref={pin_ref}\n\
             libpinyin_tag=2.11.91\n\
             libpinyin_commit=0c5e80e1200f84fab185d1c5bde458b770a0636c\n\
             ibus_libpinyin_tag=1.16.5\n\
             ibus_libpinyin_commit=2d2cdac0187101aa0cd7ac06694a8340721ddfbb\n\
             model_sha256=59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155\n\
             dbm={dbm}\n\
             header_sha256=aa\n\
             shared_object_sha256=bb\n\
             data_manifest_sha256=cc\n"
        )
    }

    #[test]
    fn frozen_pin_manifest_verifies() {
        let manifest = PinManifest::parse(&manifest_text(EXPECTED_PIN_REF, "Tkrzw"))
            .expect("well-formed manifest parses");
        let pin = manifest.verify().expect("frozen pin verifies");
        assert_eq!(pin.pin_ref(), EXPECTED_PIN_REF);
        assert_eq!(pin.header_sha256(), "aa");
        assert_eq!(pin.shared_object_sha256(), "bb");
        assert_eq!(pin.data_manifest_sha256(), "cc");
    }

    #[test]
    fn off_pin_reference_is_rejected() {
        let manifest = PinManifest::parse(&manifest_text("libpinyin-2.11.90-deadbeef", "Tkrzw"))
            .expect("well-formed manifest parses");
        assert!(matches!(
            manifest.verify(),
            Err(OracleError::ManifestFieldMismatch {
                field: "pin_ref",
                ..
            })
        ));
    }

    #[test]
    fn berkeley_db_backend_is_rejected() {
        let manifest = PinManifest::parse(&manifest_text(EXPECTED_PIN_REF, "BerkeleyDB"))
            .expect("well-formed manifest parses");
        assert!(matches!(
            manifest.verify(),
            Err(OracleError::ManifestFieldMismatch { field: "dbm", .. })
        ));
    }

    #[test]
    fn missing_field_is_reported_not_defaulted() {
        let manifest = PinManifest::parse("schema=pinyin-oracle-v1\n").expect("parses");
        assert!(matches!(
            manifest.verify(),
            Err(OracleError::ManifestFieldMissing { field: "pin_ref" })
        ));
    }

    #[test]
    fn repeated_field_is_ambiguous_and_rejected() {
        let text = format!("schema={MANIFEST_SCHEMA}\nschema=something-else\n");
        assert!(matches!(
            PinManifest::parse(&text),
            Err(OracleError::ManifestFieldRepeated { .. })
        ));
    }

    #[test]
    fn unknown_schema_is_rejected() {
        let text = manifest_text(EXPECTED_PIN_REF, "Tkrzw").replace(MANIFEST_SCHEMA, "v2");
        let manifest = PinManifest::parse(&text).expect("parses");
        assert!(matches!(
            manifest.verify(),
            Err(OracleError::ManifestFieldMismatch {
                field: "schema",
                ..
            })
        ));
    }

    #[test]
    fn parsing_never_panics_on_arbitrary_text() {
        for text in [
            "",
            "\n\n\n",
            "=",
            "=value",
            "key=",
            "no-separator",
            "a=b=c",
            "\u{feff}schema=pinyin-oracle-v1",
        ] {
            let _ = PinManifest::parse(text);
        }
    }
}
