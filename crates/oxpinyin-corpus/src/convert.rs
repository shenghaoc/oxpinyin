//! Traditional → simplified conversion by shelling out to `opencc`.
//!
//! Deliberately no new Cargo dependency: `OpenCC` is the standard
//! permissively-licensed (Apache-2.0) converter, and calling the
//! standard `opencc` binary keeps the corpus front-end dependency-free
//! and portable. The runtime requirement is one documented executable;
//! `--no-convert` skips the stage entirely when a corpus is already
//! simplified.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;

use crate::error::CorpusError;

/// Config name passed to `opencc --config`. The `t2s` preset ships with
/// every `OpenCC` distribution.
pub const DEFAULT_CONFIG: &str = "t2s";

/// An `opencc` converter process factory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Converter {
    binary: PathBuf,
    config: String,
}

impl Converter {
    /// The `opencc` binary (resolved on `PATH` by the caller or the
    /// `--opencc` flag) with the [`DEFAULT_CONFIG`] preset.
    #[must_use]
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            config: DEFAULT_CONFIG.to_owned(),
        }
    }

    /// Overrides the config name (e.g. `t2s.json`).
    #[must_use]
    pub fn with_config(mut self, config: impl Into<String>) -> Self {
        self.config = config.into();
        self
    }

    /// The converter binary path.
    #[must_use]
    pub const fn binary(&self) -> &PathBuf {
        &self.binary
    }

    /// The config name.
    #[must_use]
    pub fn config(&self) -> &str {
        &self.config
    }

    /// Converts one page's cleaned text. The child reads stdin and writes
    /// stdout; stderr is inherited (never piped), so a chatty converter
    /// cannot deadlock against a full pipe buffer before `wait`.
    ///
    /// # Errors
    ///
    /// [`CorpusError::Convert`] when the binary cannot be spawned or
    /// exits nonzero.
    pub fn convert(&self, text: &str) -> Result<String, CorpusError> {
        let mut child = Command::new(&self.binary)
            .arg("-c")
            .arg(&self.config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| CorpusError::Convert {
                detail: format!("{}: {error}", self.binary.display()),
            })?;

        let mut stdin = child.stdin.take().ok_or_else(|| CorpusError::Convert {
            detail: "converter stdin unavailable".into(),
        })?;
        // Pipe the input on a thread so a large page cannot deadlock
        // against a full pipe buffer.
        let input = text.to_owned();
        let writer = thread::spawn(move || -> Result<(), CorpusError> {
            stdin.write_all(input.as_bytes()).map_err(CorpusError::Io)?;
            drop(stdin);
            Ok(())
        });

        let mut stdout = Vec::new();
        if let Some(mut pipe) = child.stdout.take() {
            pipe.read_to_end(&mut stdout).map_err(CorpusError::Io)?;
        }
        writer.join().map_err(|_| CorpusError::Convert {
            detail: "converter writer thread panicked".into(),
        })??;

        let status = child.wait().map_err(CorpusError::Io)?;
        if !status.success() {
            // stderr was inherited, so the converter's own diagnostics
            // already reached the terminal; report the exit status.
            return Err(CorpusError::Convert {
                detail: format!(
                    "{} -c {} exited {status}",
                    self.binary.display(),
                    self.config,
                ),
            });
        }
        String::from_utf8(stdout).map_err(|error| CorpusError::Convert {
            detail: format!("converter emitted invalid utf-8: {error}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Converter, DEFAULT_CONFIG};

    #[test]
    fn defaults_use_the_t2s_preset() {
        let converter = Converter::new("opencc");
        assert_eq!(converter.config(), DEFAULT_CONFIG);
        assert_eq!(converter.binary().as_os_str(), "opencc");
    }
}
