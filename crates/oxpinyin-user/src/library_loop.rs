//! `pinyin_init`'s library loop (`pinyin.cpp:377-392`, `zhuyin.cpp:318-334`):
//! each default row is judged, then mapped, then its log merged or its user
//! file read, before the next row is looked at. The first fault ends the
//! loop, so nothing past that row is opened, mapped, read or reported.

use std::path::Path;

use oxpinyin_data::phrase_library::LibraryError;
use oxpinyin_data::table_info::{DefaultRowStep, FileName, TableConf, TableConfSite};

use crate::persistence::{MapSteps, SystemOriginals};
use crate::replay::JournalFault;

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
    pub fn map_libraries(&self, originals: &mut SystemOriginals) -> Result<(), LibraryLoopStop> {
        run(
            &self.steps,
            &mut MapSteps {
                system_dir: self.system_dir,
                originals,
            },
        )
    }
}

/// What the loop does at a row, injected so a test can record and fault it.
pub(crate) trait LoopSteps {
    /// Maps a `SYSTEM_FILE` row's chunk.
    fn map(&mut self, index: u8, file: &FileName) -> Result<(), LibraryError>;
    /// Merges a `SYSTEM_FILE` row's log.
    fn merge_log(&mut self, index: u8) -> Result<(), JournalFault>;
    /// Reads a `USER_FILE` row's library.
    fn read_user_library(&mut self, index: u8);
}

/// Walks `rows` in order and stops at the first fault.
pub(crate) fn run(
    rows: &[(u8, DefaultRowStep<'_>)],
    steps: &mut impl LoopSteps,
) -> Result<(), LibraryLoopStop> {
    for &(index, step) in rows {
        match step {
            DefaultRowStep::Stop(site) => return Err(LibraryLoopStop::Row(site)),
            DefaultRowStep::System {
                file,
                null_user_file,
            } => {
                steps.map(index, file).map_err(LibraryLoopStop::Library)?;
                // The chunk maps first; the log's read of the directory is
                // what dies (`pinyin.cpp:268-271`, `memory_chunk.h:434`).
                if null_user_file {
                    return Err(LibraryLoopStop::Row(TableConfSite::NullUserFile));
                }
                steps.merge_log(index).map_err(LibraryLoopStop::Journal)?;
            }
            DefaultRowStep::User => steps.read_user_library(index),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use oxpinyin_data::chunk_format::build_memory_chunk;
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};
    use oxpinyin_data::phrase_library::RowFault;
    use oxpinyin_data::user_files::{SystemVersions, UserFileLayout, UserTableInfo};

    use super::*;
    use crate::persistence::{
        LoadedProfile, SessionSteps, UserConfLaw, begin_session_profile, check_format,
        system_originals,
    };

    const LAWS: [UserConfLaw; 2] = [UserConfLaw::Pinyin, UserConfLaw::Zhuyin];

    /// Row 1 to 3 and 5 are system libraries, row 4 a user library.
    const CONF: &str = "binary format version:7\nmodel data version:14\n\
        lambda parameter:0.312699\n\nsource table format:pinyin\n\
        database format:BerkeleyDB\n\n\
        default RESERVED NULL NULL NULL NOT_USED\n\
        default GB_DICTIONARY a.table a.bin a.dbin SYSTEM_FILE\n\
        default GBK_DICTIONARY b.table b.bin b.dbin SYSTEM_FILE\n\
        default OPENGRAM_DICTIONARY c.table c.bin c.dbin SYSTEM_FILE\n\
        default MERGED_DICTIONARY NULL NULL u.bin USER_FILE\n\
        default ADDON_DICTIONARY d.table d.bin d.dbin SYSTEM_FILE\n";

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Call {
        Map(u8),
        Log(u8),
        User(u8),
    }
    use Call::{Log, Map, User};

    /// A `MODIFY_HEADER` with a token, which `next_record` asserts on
    /// (`phrase_index_logger.h:202`, register row 91), framed as a chunk.
    fn bad_log() -> Vec<u8> {
        let mut payload = 4_u32.to_le_bytes().to_vec();
        payload.extend_from_slice(&0x0100_0001_u32.to_le_bytes());
        payload.extend_from_slice(&[4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        build_memory_chunk(&payload).expect("frame the log")
    }

    /// Records each step, optionally faulting a map, then runs the real one.
    struct Traced<'a> {
        inner: SessionSteps<'a>,
        trace: Vec<Call>,
        fault: Option<(u8, fn() -> LibraryError)>,
    }

    impl LoopSteps for Traced<'_> {
        fn map(&mut self, index: u8, file: &FileName) -> Result<(), LibraryError> {
            self.trace.push(Map(index));
            match self.fault {
                Some((at, fault)) if at == index => Err(fault()),
                _ => self.inner.map(index, file),
            }
        }

        fn merge_log(&mut self, index: u8) -> Result<(), JournalFault> {
            self.trace.push(Log(index));
            self.inner.merge_log(index)
        }

        fn read_user_library(&mut self, index: u8) {
            self.trace.push(User(index));
            self.inner.read_user_library(index);
        }
    }

    struct Fixture {
        _root: tempfile::TempDir,
        system: PathBuf,
        user: PathBuf,
    }

    impl Fixture {
        /// Every library present and well formed, the profile conforming.
        fn new(conf: &str) -> Self {
            let root = tempfile::tempdir().expect("scratch");
            let system = root.path().join("system");
            let user = root.path().join("user");
            std::fs::create_dir_all(&system).expect("system dir");
            std::fs::create_dir_all(&user).expect("user dir");
            std::fs::write(system.join("table.conf"), conf).expect("table.conf");
            let empty: &[(u32, ChunkItem)] = &[];
            let chunk = build_chunk(empty).expect("empty chunk");
            for file in ["a.bin", "b.bin", "c.bin", "d.bin"] {
                std::fs::write(system.join(file), &chunk).expect("library");
            }
            let this = Self {
                _root: root,
                system,
                user,
            };
            let versions = SystemVersions::from_conf(&this.conf());
            std::fs::write(
                this.user.join("user.conf"),
                UserTableInfo::conform_to(&versions).to_text(),
            )
            .expect("user.conf");
            this
        }

        fn conf(&self) -> TableConf {
            TableConf::load_from_dir(&self.system).expect("table.conf loads")
        }

        fn bad_log(&self, file: &str) {
            std::fs::write(self.user.join(file), bad_log()).expect("log");
        }

        fn absent(&self, file: &str) {
            std::fs::remove_file(self.system.join(file)).expect("unlink");
        }

        fn unmappable(&self, file: &str) {
            std::fs::write(self.system.join(file), b"abc").expect("short chunk");
        }

        /// The session load over `rows`, with each step recorded.
        fn run(
            &self,
            law: UserConfLaw,
            rows: &[(u8, DefaultRowStep<'_>)],
            fault: Option<(u8, fn() -> LibraryError)>,
        ) -> Run {
            let conf = self.conf();
            let layout = UserFileLayout::from_conf(&conf);
            let versions = SystemVersions::from_conf(&conf);
            let check = check_format(&self.user, &versions, &layout, law).expect("check_format");
            let mut originals = system_originals(
                oxpinyin_data::phrase_libraries::PhraseLibraries::empty(),
                layout,
            );
            let mut profile = begin_session_profile(&self.user, &check).expect("the user tables");
            let mut steps = Traced {
                inner: SessionSteps {
                    user_dir: &self.user,
                    system_dir: &self.system,
                    originals: &mut originals,
                    profile: &mut profile,
                    conform: check.conform,
                },
                trace: Vec::new(),
                fault,
            };
            let stop = run(rows, &mut steps);
            let trace = steps.trace;
            Run {
                trace,
                stop,
                unmapped: originals
                    .phrase_libraries()
                    .unmapped()
                    .map(Path::to_path_buf)
                    .collect(),
                loaded: (0..16)
                    .filter(|&nibble| originals.phrase_libraries().is_loaded(nibble))
                    .collect(),
                profile,
            }
        }
    }

    struct Run {
        trace: Vec<Call>,
        stop: Result<(), LibraryLoopStop>,
        unmapped: Vec<PathBuf>,
        loaded: Vec<u8>,
        profile: LoadedProfile,
    }

    fn assert_journal(stop: &Result<(), LibraryLoopStop>, library: u8) {
        assert!(
            matches!(stop, Err(LibraryLoopStop::Journal(fault)) if fault.library() == library),
            "{stop:?}"
        );
    }

    fn format_fault() -> LibraryError {
        LibraryError::Format("separator outside the payload".into())
    }

    fn directory_fault() -> LibraryError {
        LibraryError::Row(RowFault::ChunkHeaderRead)
    }

    /// a later chunk the pin never opens cannot stop library
    /// 1's replay, and writes no line (`pinyin.cpp:276` before `:256`).
    #[test]
    fn a_bad_log_aborts_before_a_later_library_that_cannot_map() {
        for law in LAWS {
            let fixture = Fixture::new(CONF);
            fixture.bad_log("a.dbin");
            fixture.absent("b.bin");
            fixture.unmappable("c.bin");
            let conf = fixture.conf();
            let run = fixture.run(law, &conf.default_row_steps(None), None);
            assert_journal(&run.stop, 1);
            assert_eq!(run.trace, [Map(1), Log(1)], "{law:?}");
            assert!(run.unmapped.is_empty(), "{law:?}: {:?}", run.unmapped);
        }
    }

    /// the same for a layout the pin reads past, which writes
    /// no line and answers NULL when the loop does reach it.
    #[test]
    fn a_bad_log_aborts_before_a_later_library_with_a_bad_layout() {
        for law in LAWS {
            let fixture = Fixture::new(CONF);
            let conf = fixture.conf();
            let reached = fixture.run(law, &conf.default_row_steps(None), Some((3, format_fault)));
            assert!(
                matches!(
                    reached.stop,
                    Err(LibraryLoopStop::Library(LibraryError::Format(_)))
                ),
                "{law:?}: {:?}",
                reached.stop
            );
            assert_eq!(
                reached.trace,
                [Map(1), Log(1), Map(2), Log(2), Map(3)],
                "{law:?}"
            );

            fixture.bad_log("a.dbin");
            let run = fixture.run(law, &conf.default_row_steps(None), Some((3, format_fault)));
            assert_journal(&run.stop, 1);
            assert_eq!(run.trace, [Map(1), Log(1)], "{law:?}");
        }
    }

    /// a fatal row is judged where the loop reaches it, after
    /// the logs before it. Row 3 is each of the three sites; the absent
    /// library 2 between them writes no line (`pinyin.cpp:388`,
    /// `memory_chunk.h:434`, `:493`).
    #[test]
    fn a_bad_log_aborts_before_a_later_fatal_row() {
        let dictionary = CONF.replace("c.dbin SYSTEM_FILE", "c.dbin DICTIONARY");
        let null_system = CONF.replace("c.table c.bin", "c.table NULL");
        let null_user = CONF.replace("c.dbin SYSTEM_FILE", "NULL SYSTEM_FILE");
        for law in LAWS {
            for (text, site) in [
                (&dictionary, TableConfSite::DefaultDictionary),
                (&null_system, TableConfSite::NullSystemFile),
                (&null_user, TableConfSite::NullUserFile),
            ] {
                let fixture = Fixture::new(text);
                fixture.absent("b.bin");
                let conf = fixture.conf();
                // Whether the user directory reads as a chunk depends on the
                // filesystem, so the null user file is set here.
                let rows: Vec<_> = conf
                    .default_row_steps(None)
                    .into_iter()
                    .map(|(index, step)| match step {
                        DefaultRowStep::System { file, .. }
                            if index == 3 && site == TableConfSite::NullUserFile =>
                        {
                            (
                                index,
                                DefaultRowStep::System {
                                    file,
                                    null_user_file: true,
                                },
                            )
                        }
                        other => (index, other),
                    })
                    .collect();

                let reached = fixture.run(law, &rows, None);
                assert!(
                    matches!(reached.stop, Err(LibraryLoopStop::Row(got)) if got == site),
                    "{law:?} {site:?}: {:?}",
                    reached.stop
                );
                assert_eq!(reached.unmapped, [fixture.system.join("b.bin")], "{law:?}");

                fixture.bad_log("a.dbin");
                let run = fixture.run(law, &rows, None);
                assert_journal(&run.stop, 1);
                assert_eq!(run.trace, [Map(1), Log(1)], "{law:?} {site:?}");
                assert!(run.unmapped.is_empty(), "{law:?} {site:?}");
            }
        }
    }

    /// the loop's own list holds the absent library when a
    /// later chunk stops it, so its line precedes the later one's
    /// (`pinyin.cpp:256`; register row 76's prefix).
    #[test]
    fn an_absent_library_keeps_its_line_before_a_later_unmappable_one() {
        for law in LAWS {
            let fixture = Fixture::new(CONF);
            fixture.absent("a.bin");
            fixture.unmappable("c.bin");
            let conf = fixture.conf();
            let run = fixture.run(law, &conf.default_row_steps(None), None);
            let Err(LibraryLoopStop::Library(error)) = &run.stop else {
                panic!("{law:?}: {:?}", run.stop);
            };
            assert_eq!(
                error.unmappable_path(),
                Some(fixture.system.join("c.bin").as_path()),
                "{law:?}"
            );
            assert_eq!(run.unmapped, [fixture.system.join("a.bin")], "{law:?}");
        }
    }

    /// nothing after the faulting log is mapped or read, a
    /// later well-formed library and user file included.
    #[test]
    fn the_loop_touches_nothing_after_the_faulting_log() {
        for law in LAWS {
            let fixture = Fixture::new(CONF);
            fixture.bad_log("a.dbin");
            let empty: &[(u32, ChunkItem)] = &[];
            std::fs::write(
                fixture.user.join("u.bin"),
                build_chunk(empty).expect("empty chunk"),
            )
            .expect("user library");
            let conf = fixture.conf();
            let run = fixture.run(law, &conf.default_row_steps(None), None);
            assert_journal(&run.stop, 1);
            assert_eq!(run.trace, [Map(1), Log(1)], "{law:?}");
            assert_eq!(run.loaded, [1], "{law:?}");
            assert!(run.profile.user_libraries.is_empty(), "{law:?}");
            assert!(
                !run.profile.loaded.library_totals.contains_key(&4),
                "{law:?}"
            );
        }
    }

    /// The whole order: each library before its log and before the next row,
    /// the user library at its own place.
    #[test]
    fn each_library_maps_before_its_log_and_before_the_next() {
        for law in LAWS {
            let fixture = Fixture::new(CONF);
            let conf = fixture.conf();
            let run = fixture.run(law, &conf.default_row_steps(None), None);
            assert!(run.stop.is_ok(), "{law:?}: {:?}", run.stop);
            assert_eq!(
                run.trace,
                [
                    Map(1),
                    Log(1),
                    Map(2),
                    Log(2),
                    Map(3),
                    Log(3),
                    User(4),
                    Map(5),
                    Log(5)
                ],
                "{law:?}"
            );
            assert_eq!(run.loaded, [1, 2, 3, 5], "{law:?}");
        }
    }

    /// Control: the pin dies on library 3's empty chunk before its merge
    /// (`pinyin.cpp:264`), so its bad log is never read, while the logs
    /// before it were.
    #[test]
    fn a_library_that_cannot_map_never_reads_its_log() {
        for law in LAWS {
            let fixture = Fixture::new(CONF);
            fixture.unmappable("c.bin");
            fixture.bad_log("c.dbin");
            let conf = fixture.conf();
            let run = fixture.run(law, &conf.default_row_steps(None), None);
            assert!(
                matches!(&run.stop, Err(LibraryLoopStop::Library(error)) if error.unmappable_path().is_some()),
                "{law:?}: {:?}",
                run.stop
            );
            assert_eq!(
                run.trace,
                [Map(1), Log(1), Map(2), Log(2), Map(3)],
                "{law:?}"
            );
        }
    }

    /// Control: an absent library stays unloaded and the loop goes on to its
    /// log, as row 76's class (b) answer does, with its line kept.
    #[test]
    fn an_absent_library_keeps_its_line_and_the_fault_of_its_log() {
        for law in LAWS {
            let fixture = Fixture::new(CONF);
            fixture.absent("b.bin");
            fixture.bad_log("b.dbin");
            let conf = fixture.conf();
            let run = fixture.run(law, &conf.default_row_steps(None), None);
            assert_journal(&run.stop, 2);
            assert_eq!(run.unmapped, [fixture.system.join("b.bin")], "{law:?}");
            assert_eq!(run.trace, [Map(1), Log(1), Map(2), Log(2)], "{law:?}");
        }
    }

    /// Register row 96 on every filesystem: the logs before the directory
    /// merge first, and an earlier bad log is the fault reached.
    #[test]
    fn a_directory_library_stops_the_loop_after_the_earlier_logs() {
        for law in LAWS {
            let fixture = Fixture::new(CONF);
            fixture.bad_log("c.dbin");
            let conf = fixture.conf();
            let run = fixture.run(
                law,
                &conf.default_row_steps(None),
                Some((2, directory_fault)),
            );
            assert!(
                matches!(
                    run.stop,
                    Err(LibraryLoopStop::Library(LibraryError::Row(
                        RowFault::ChunkHeaderRead
                    )))
                ),
                "{law:?}: {:?}",
                run.stop
            );
            assert_eq!(run.trace, [Map(1), Log(1), Map(2)], "{law:?}");

            fixture.bad_log("a.dbin");
            let run = fixture.run(
                law,
                &conf.default_row_steps(None),
                Some((2, directory_fault)),
            );
            assert_journal(&run.stop, 1);
        }
    }

    /// Without a user profile the loop still judges and maps in order.
    #[test]
    fn the_loop_without_a_profile_stops_at_the_first_fault() {
        let fixture = Fixture::new(CONF);
        fixture.absent("a.bin");
        fixture.unmappable("b.bin");
        let conf = fixture.conf();
        let rows = LibraryLoopRows::new(&fixture.system, &conf, None);
        let mut originals = system_originals(
            oxpinyin_data::phrase_libraries::PhraseLibraries::empty(),
            UserFileLayout::from_conf(&conf),
        );
        let stop = rows.map_libraries(&mut originals);
        assert!(
            matches!(&stop, Err(LibraryLoopStop::Library(error)) if error.unmappable_path().is_some()),
            "{stop:?}"
        );
        assert_eq!(
            originals.phrase_libraries().unmapped().collect::<Vec<_>>(),
            [fixture.system.join("a.bin")]
        );
        assert!(!originals.phrase_libraries().is_loaded(3));
    }
}
