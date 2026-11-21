//! `pinyin_init`'s library loop (`pinyin.cpp:377-392`, `zhuyin.cpp:318-334`):
//! each default row is judged, then mapped, then its log merged or its user
//! file read, before the next row is looked at. The first fault ends the
//! loop, so nothing past that row is opened, mapped, read or reported.

use std::path::Path;

use crate::data::phrase_library::LibraryError;
use crate::data::{DefaultRowStep, TableConf, TableConfSite};

use crate::user::replay::JournalFault;
use crate::user::{SessionProfile, SystemOriginals};

/// Where the library loop stopped: the one record of every way a default row
/// ends the pin's init.
#[derive(Debug)]
pub enum LibraryLoopStop {
    /// A row the pin dies on before or instead of reading it
    /// (`pinyin.cpp:388`, `memory_chunk.h:434`, `:493`).
    Row(TableConfSite),
    /// The row's system chunk: unmappable (`pinyin.cpp:256`, then the empty
    /// chunk), a directory `MemoryChunk::mmap` asserts on
    /// (`memory_chunk.h:493`, register row 96), or a layout read past it.
    Library(LibraryError),
    /// The row's log reached a pin assertion (register rows 91 and 108).
    Journal(JournalFault),
}

impl std::fmt::Display for LibraryLoopStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Row(site) => write!(f, "{site:?}"),
            Self::Library(error) => error.fmt(f),
            Self::Journal(fault) => fault.fmt(f),
        }
    }
}

impl std::error::Error for LibraryLoopStop {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Row(_) => None,
            Self::Library(error) => Some(error),
            Self::Journal(fault) => Some(fault),
        }
    }
}

/// The default rows of one init, and the directory their chunks live in.
#[derive(Debug)]
pub struct LibraryLoopRows<'a> {
    system_dir: &'a Path,
    steps: Vec<(u8, DefaultRowStep<'a>)>,
}

impl<'a> LibraryLoopRows<'a> {
    /// `conf`'s default rows for an init over `system_dir` and `user_dir`.
    #[must_use]
    pub fn new(system_dir: &'a Path, conf: &'a TableConf, user_dir: Option<&Path>) -> Self {
        Self {
            system_dir,
            steps: conf.default_row_steps(user_dir),
        }
    }

    /// Rows a test states step by step.
    #[cfg(test)]
    pub(crate) const fn from_steps(
        system_dir: &'a Path,
        steps: Vec<(u8, DefaultRowStep<'a>)>,
    ) -> Self {
        Self { system_dir, steps }
    }

    pub(crate) const fn system_dir(&self) -> &'a Path {
        self.system_dir
    }

    pub(crate) fn steps(&self) -> &[(u8, DefaultRowStep<'a>)] {
        &self.steps
    }

    /// The loop for an init without a user profile: the rows are judged and
    /// the libraries mapped into `originals`.
    ///
    /// # Errors
    ///
    /// The loop's first fault.
    pub(crate) fn map_libraries(
        &self,
        originals: &mut SystemOriginals,
        unmapped: &mut dyn FnMut(&Path),
    ) -> Result<(), LibraryLoopStop> {
        run(&self.steps, self.system_dir, originals, None, unmapped)
    }
}

/// Walks `rows` in order and stops at the first fault.
pub(crate) fn run(
    rows: &[(u8, DefaultRowStep<'_>)],
    system_dir: &Path,
    originals: &mut SystemOriginals,
    mut session: Option<SessionProfile<'_>>,
    unmapped: &mut dyn FnMut(&Path),
) -> Result<(), LibraryLoopStop> {
    for &(index, step) in rows {
        match step {
            DefaultRowStep::Stop(site) => {
                if site == TableConfSite::NullSystemFile
                    && crate::data::phrase_library::null_chunk_writes_mmap_line(system_dir)
                {
                    unmapped(system_dir);
                }
                return Err(LibraryLoopStop::Row(site));
            }
            DefaultRowStep::System {
                file,
                null_user_file,
            } => {
                originals
                    .map_library(index, system_dir, file, unmapped)
                    .map_err(LibraryLoopStop::Library)?;
                if let Some(session) = &mut session
                    && let Some(live) = originals.take_live_library(index)
                {
                    session.profile.replayed.insert(index, live);
                }
                // The chunk maps first; the log's read of the directory is
                // what dies (`pinyin.cpp:268-271`, `memory_chunk.h:434`).
                if null_user_file {
                    return Err(LibraryLoopStop::Row(TableConfSite::NullUserFile));
                }
                if let Some(session) = &mut session {
                    session
                        .merge_log(originals, index)
                        .map_err(LibraryLoopStop::Journal)?;
                }
            }
            DefaultRowStep::User => {
                if let Some(session) = &mut session {
                    session.read_user_library(originals, index);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
