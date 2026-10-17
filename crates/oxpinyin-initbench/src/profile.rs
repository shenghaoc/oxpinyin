//! Profile fixtures: user directories whose contents are fixed, named by
//! digest, and handed to every measured process as a private copy.
//!
//! A fixture is a directory
//!
//! ```text
//! <root>/user/             the user directory exactly as the library left it
//! <root>/profile.manifest  one line per file: size, SHA-256, relative path
//! <root>/witness.txt       optional: inputs whose answers show the profile loaded
//! ```
//!
//! Each measured process gets its own copy of `user/`, so no sample's learning
//! or journal writes reach the next one, and the fixture is checked against its
//! manifest before the run and again after it. An empty fixture (a fresh,
//! writable, empty user directory) is a fixture whose manifest lists no files.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::digest::{Sha256, sha256_file};
use crate::error::BenchError;

/// First line of a manifest file.
pub const MANIFEST_HEADER: &str = "oxpinyin-startup-profile-v1";

/// The user directory's name inside a fixture.
pub const USER_SUBDIR: &str = "user";
/// The manifest's file name inside a fixture.
pub const MANIFEST_FILE: &str = "profile.manifest";
/// The witness list's file name inside a fixture.
pub const WITNESS_FILE: &str = "witness.txt";

/// One file of a profile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileEntry {
    /// Size in bytes.
    pub size: u64,
    /// Lowercase hexadecimal SHA-256.
    pub sha256: String,
}

/// The files of a user directory, by relative path.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Manifest {
    /// Relative path (`/`-separated) to entry.
    pub files: BTreeMap<String, FileEntry>,
}

impl Manifest {
    /// Hashes every regular file under `dir`.
    ///
    /// # Errors
    ///
    /// I/O failure, or an entry that is neither a regular file nor a
    /// directory (a symlink or device is not content a copy can preserve).
    pub fn of_directory(dir: &Path) -> Result<Self, BenchError> {
        let mut manifest = Self::default();
        walk(dir, dir, &mut manifest)?;
        Ok(manifest)
    }

    /// A single digest naming the whole profile: the SHA-256 of the manifest
    /// text.
    #[must_use]
    pub fn identity(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.to_text().as_bytes());
        hasher.finish_hex()
    }

    /// Total bytes in the profile.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.files.values().map(|entry| entry.size).sum()
    }

    /// The manifest file's text.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut text = format!("{MANIFEST_HEADER}\n");
        for (path, entry) in &self.files {
            text.push_str(&format!("file\t{}\t{}\t{path}\n", entry.size, entry.sha256));
        }
        text
    }

    /// Parses [`Self::to_text`]; strict about every field.
    ///
    /// # Errors
    ///
    /// [`BenchError::Invalid`] naming the first bad line.
    pub fn parse(text: &str) -> Result<Self, BenchError> {
        let mut lines = text.lines();
        if lines.next() != Some(MANIFEST_HEADER) {
            return Err(BenchError::invalid(format!(
                "profile manifest does not start with {MANIFEST_HEADER}"
            )));
        }
        let mut manifest = Self::default();
        for (number, line) in lines.enumerate() {
            let fields: Vec<&str> = line.splitn(4, '\t').collect();
            let [kind, size, sha256, path] = fields[..] else {
                return Err(BenchError::invalid(format!(
                    "profile manifest line {} is not four tab-separated fields",
                    number + 2
                )));
            };
            let valid_digest = sha256.len() == 64
                && sha256
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
            let size = size.parse::<u64>().ok();
            let (true, true, Some(size)) = (kind == "file", valid_digest, size) else {
                return Err(BenchError::invalid(format!(
                    "profile manifest line {} is malformed",
                    number + 2
                )));
            };
            if path.is_empty()
                || path.starts_with('/')
                || path.split('/').any(|p| p == ".." || p.is_empty())
            {
                return Err(BenchError::invalid(format!(
                    "profile manifest line {} has an unsafe path",
                    number + 2
                )));
            }
            let entry = FileEntry {
                size,
                sha256: sha256.to_owned(),
            };
            if manifest.files.insert(path.to_owned(), entry).is_some() {
                return Err(BenchError::invalid(format!(
                    "profile manifest lists {path} twice"
                )));
            }
        }
        Ok(manifest)
    }
}

fn walk(root: &Path, dir: &Path, manifest: &mut Manifest) -> Result<(), BenchError> {
    let mut entries = fs::read_dir(dir)
        .map_err(|e| BenchError::io(format!("reading {}", dir.display()), e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| BenchError::io(format!("reading {}", dir.display()), e))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|e| BenchError::io(format!("inspecting {}", path.display()), e))?;
        if kind.is_dir() {
            walk(root, &path, manifest)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| BenchError::invalid("a profile file escaped its directory"))?;
            let name = relative
                .components()
                .map(|c| c.as_os_str().to_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| BenchError::invalid("a profile file name is not UTF-8"))?
                .join("/");
            let size = entry
                .metadata()
                .map_err(|e| BenchError::io(format!("inspecting {}", path.display()), e))?
                .len();
            let sha256 = sha256_file(&path)
                .map_err(|e| BenchError::io(format!("hashing {}", path.display()), e))?;
            manifest.files.insert(name, FileEntry { size, sha256 });
        } else {
            return Err(BenchError::invalid(format!(
                "{} is neither a regular file nor a directory",
                path.display()
            )));
        }
    }
    Ok(())
}

/// Copies a directory tree (regular files and directories only).
///
/// # Errors
///
/// I/O failure, or a special file.
pub fn copy_tree(source: &Path, destination: &Path) -> Result<(), BenchError> {
    fs::create_dir_all(destination)
        .map_err(|e| BenchError::io(format!("creating {}", destination.display()), e))?;
    for entry in fs::read_dir(source)
        .map_err(|e| BenchError::io(format!("reading {}", source.display()), e))?
    {
        let entry =
            entry.map_err(|e| BenchError::io(format!("reading {}", source.display()), e))?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let kind = entry
            .file_type()
            .map_err(|e| BenchError::io(format!("inspecting {}", from.display()), e))?;
        if kind.is_dir() {
            copy_tree(&from, &to)?;
        } else if kind.is_file() {
            fs::copy(&from, &to)
                .map_err(|e| BenchError::io(format!("copying {}", from.display()), e))?;
        } else {
            return Err(BenchError::invalid(format!(
                "{} is neither a regular file nor a directory",
                from.display()
            )));
        }
    }
    Ok(())
}

/// A profile fixture whose contents have been checked against its manifest.
#[derive(Clone, Debug)]
pub struct Fixture {
    /// The fixture's name (its directory's name).
    pub name: String,
    /// The fixture directory.
    pub root: PathBuf,
    /// What the manifest says the user directory holds.
    pub manifest: Manifest,
    /// Witness inputs, one per line of `witness.txt` (empty if absent).
    pub witness: Vec<Vec<u8>>,
}

impl Fixture {
    /// Opens `root` and verifies `user/` against `profile.manifest`.
    ///
    /// # Errors
    ///
    /// A missing manifest or user directory, or any difference between the
    /// manifest and the directory: a fixture that does not match its manifest
    /// is not a fixture.
    pub fn open(root: &Path) -> Result<Self, BenchError> {
        let name = root
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| BenchError::invalid("a fixture directory needs a UTF-8 name"))?
            .to_owned();
        let text = fs::read_to_string(root.join(MANIFEST_FILE))
            .map_err(|e| BenchError::io(format!("reading the manifest of {name}"), e))?;
        let manifest = Manifest::parse(&text)?;
        let fixture = Self {
            name,
            root: root.to_path_buf(),
            manifest,
            witness: read_witness(&root.join(WITNESS_FILE))?,
        };
        fixture.verify()?;
        Ok(fixture)
    }

    /// The user directory inside the fixture.
    #[must_use]
    pub fn user_dir(&self) -> PathBuf {
        self.root.join(USER_SUBDIR)
    }

    /// Recomputes the user directory's manifest and compares.
    ///
    /// # Errors
    ///
    /// The directory differs from the manifest (named in the message).
    pub fn verify(&self) -> Result<(), BenchError> {
        let actual = Manifest::of_directory(&self.user_dir())?;
        if actual == self.manifest {
            Ok(())
        } else {
            Err(BenchError::invalid(format!(
                "fixture {} no longer matches its manifest (expected identity {}, found {})",
                self.name,
                self.manifest.identity(),
                actual.identity()
            )))
        }
    }

    /// Restores the user directory to exactly `destination` (replacing anything
    /// there), verified equal to the manifest. The verification phase uses one
    /// fixed path for every subject so that diagnostics naming the path are
    /// comparable.
    ///
    /// # Errors
    ///
    /// I/O failure or a copy that differs from the manifest.
    pub fn restore_to(&self, destination: &Path) -> Result<(), BenchError> {
        if destination.exists() {
            fs::remove_dir_all(destination)
                .map_err(|e| BenchError::io(format!("clearing {}", destination.display()), e))?;
        }
        copy_tree(&self.user_dir(), destination)?;
        if Manifest::of_directory(destination)? != self.manifest {
            return Err(BenchError::invalid(format!(
                "the restored copy of {} differs from its manifest",
                self.name
            )));
        }
        Ok(())
    }

    /// Makes a private working copy of the user directory under `parent`,
    /// verified equal to the fixture before it is handed out.
    ///
    /// # Errors
    ///
    /// I/O failure or a copy that differs from the manifest.
    pub fn working_copy(&self, parent: &Path) -> Result<WorkingCopy, BenchError> {
        let directory = WorkingCopy::create(parent)?;
        copy_tree(&self.user_dir(), directory.path())?;
        if Manifest::of_directory(directory.path())? != self.manifest {
            return Err(BenchError::invalid(format!(
                "the working copy of {} differs from its manifest",
                self.name
            )));
        }
        Ok(directory)
    }
}

fn read_witness(path: &Path) -> Result<Vec<Vec<u8>>, BenchError> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(<[u8]>::to_vec)
            .collect()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(BenchError::io(format!("reading {}", path.display()), error)),
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A uniquely named directory removed when dropped.
#[derive(Debug)]
pub struct WorkingCopy {
    path: PathBuf,
}

impl WorkingCopy {
    /// Creates a fresh, empty directory under `parent`.
    ///
    /// # Errors
    ///
    /// I/O failure.
    pub fn create(parent: &Path) -> Result<Self, BenchError> {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!("startup-work-{}-{unique}", std::process::id()));
        fs::create_dir_all(parent)
            .map_err(|e| BenchError::io(format!("creating {}", parent.display()), e))?;
        fs::create_dir(&path)
            .map_err(|e| BenchError::io(format!("creating {}", path.display()), e))?;
        Ok(Self { path })
    }

    /// The directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for WorkingCopy {
    fn drop(&mut self) {
        // Best effort: a leftover temporary directory is not a measurement
        // result, and a failed removal must not mask the real outcome.
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> WorkingCopy {
        WorkingCopy::create(&std::env::temp_dir().join(format!("startup-profile-test-{tag}")))
            .unwrap()
    }

    fn write_fixture(root: &Path, files: &[(&str, &[u8])]) {
        let user = root.join(USER_SUBDIR);
        fs::create_dir_all(&user).unwrap();
        for (name, bytes) in files {
            let path = user.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let manifest = Manifest::of_directory(&user).unwrap();
        fs::write(root.join(MANIFEST_FILE), manifest.to_text()).unwrap();
    }

    #[test]
    fn manifest_text_round_trips() {
        let scratch = scratch("roundtrip");
        write_fixture(
            scratch.path(),
            &[("a.db", b"alpha"), ("sub/b.log", b"beta")],
        );
        let manifest = Manifest::of_directory(&scratch.path().join(USER_SUBDIR)).unwrap();
        assert_eq!(manifest.files.len(), 2);
        assert_eq!(Manifest::parse(&manifest.to_text()).unwrap(), manifest);
        assert_eq!(manifest.total_bytes(), 9);
    }

    #[test]
    fn parse_rejects_malformed_manifests() {
        let digest = "0".repeat(64);
        for bad in [
            String::new(),
            "wrong-header\n".to_owned(),
            format!("{MANIFEST_HEADER}\nfile\t1\t{digest}\n"),
            format!("{MANIFEST_HEADER}\nfile\tx\t{digest}\ta\n"),
            format!("{MANIFEST_HEADER}\nfile\t1\tXYZ\ta\n"),
            format!("{MANIFEST_HEADER}\ndir\t1\t{digest}\ta\n"),
            format!("{MANIFEST_HEADER}\nfile\t1\t{digest}\t../escape\n"),
            format!("{MANIFEST_HEADER}\nfile\t1\t{digest}\t/abs\n"),
            format!("{MANIFEST_HEADER}\nfile\t1\t{digest}\ta\nfile\t1\t{digest}\ta\n"),
        ] {
            assert!(Manifest::parse(&bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn an_empty_profile_has_a_stable_identity() {
        let empty = Manifest::default();
        assert_eq!(
            empty.identity(),
            Manifest::parse(&empty.to_text()).unwrap().identity()
        );
        assert_eq!(empty.total_bytes(), 0);
    }

    #[test]
    fn fixture_opens_and_a_working_copy_is_independent() {
        let root = scratch("independent");
        write_fixture(
            root.path(),
            &[("user.db", b"learned"), ("log.1", b"journal")],
        );
        let fixture = Fixture::open(root.path()).unwrap();
        let parent = scratch("independent-work");
        let copy = fixture.working_copy(parent.path()).unwrap();
        // The library would write here; the fixture must not notice.
        fs::write(copy.path().join("user.db"), b"mutated by a sample").unwrap();
        fs::write(copy.path().join("__db.001"), b"region").unwrap();
        fixture.verify().unwrap();
        let second = fixture.working_copy(parent.path()).unwrap();
        assert_ne!(copy.path(), second.path());
        assert_eq!(fs::read(second.path().join("user.db")).unwrap(), b"learned");
    }

    #[test]
    fn a_modified_fixture_is_refused() {
        let root = scratch("modified");
        write_fixture(root.path(), &[("user.db", b"learned")]);
        let fixture = Fixture::open(root.path()).unwrap();
        fs::write(root.path().join(USER_SUBDIR).join("user.db"), b"tampered").unwrap();
        assert!(fixture.verify().is_err());
        assert!(Fixture::open(root.path()).is_err());
    }

    #[test]
    fn working_copy_is_removed_on_drop() {
        let parent = scratch("cleanup");
        let path = {
            let copy = WorkingCopy::create(parent.path()).unwrap();
            fs::write(copy.path().join("x"), b"y").unwrap();
            copy.path().to_path_buf()
        };
        assert!(!path.exists());
    }

    #[test]
    fn witness_lines_are_read_and_blank_lines_dropped() {
        let root = scratch("witness");
        write_fixture(root.path(), &[]);
        fs::write(root.path().join(WITNESS_FILE), b"nihao\n\nzhongguo\n").unwrap();
        let fixture = Fixture::open(root.path()).unwrap();
        assert_eq!(
            fixture.witness,
            vec![b"nihao".to_vec(), b"zhongguo".to_vec()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_in_a_profile_is_refused() {
        let root = scratch("symlink");
        let user = root.path().join(USER_SUBDIR);
        fs::create_dir_all(&user).unwrap();
        std::os::unix::fs::symlink("/etc/hostname", user.join("link")).unwrap();
        assert!(Manifest::of_directory(&user).is_err());
    }
}
