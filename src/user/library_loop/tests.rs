use std::path::PathBuf;

use crate::data::chunk_format::build_memory_chunk;
use crate::data::chunk_write::{ChunkItem, build_chunk};
use crate::data::user_files::{SystemVersions, UserFileLayout, UserTableInfo};

use super::*;
use crate::user::persistence::{
    LoadedPinyinIndex, LoadedProfile, UserConfLaw, check_format, system_originals,
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

/// A `MODIFY_HEADER` with a token, which `next_record` asserts on
/// (`phrase_index_logger.h:202`, register row 91), framed as a chunk.
fn bad_log() -> Vec<u8> {
    let mut payload = 4_u32.to_le_bytes().to_vec();
    payload.extend_from_slice(&0x0100_0001_u32.to_le_bytes());
    payload.extend_from_slice(&[4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    build_memory_chunk(&payload).expect("frame the log")
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

    /// The session load over `rows`, using the production loop.
    fn run(&self, law: UserConfLaw, rows: &[(u8, DefaultRowStep<'_>)]) -> Run {
        let conf = self.conf();
        let layout = UserFileLayout::from_conf(&conf);
        let versions = SystemVersions::from_conf(&conf);
        let check = check_format(&self.user, &versions, &layout, law).expect("check_format");
        let mut originals = system_originals(
            crate::data::phrase_libraries::PhraseLibraries::empty(),
            layout,
        );
        let mut profile = LoadedPinyinIndex::open(Some((&self.user, check)))
            .unwrap()
            .load_phrase_table()
            .unwrap();
        let mut unmapped = Vec::new();
        let stop = run(
            rows,
            &self.system,
            &mut originals,
            Some(SessionProfile {
                user_dir: &self.user,
                profile: &mut profile,
                conform: check.conform,
            }),
            &mut |path| unmapped.push(path.to_owned()),
        );
        Run {
            stop,
            unmapped,
            loaded: (0..16)
                .filter(|&nibble| originals.phrase_libraries().is_loaded(nibble))
                .collect(),
            profile,
        }
    }
}

struct Run {
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
        let run = fixture.run(law, &conf.default_row_steps(None));
        assert_journal(&run.stop, 1);
        assert!(run.unmapped.is_empty(), "{law:?}: {:?}", run.unmapped);
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
                    other @ (DefaultRowStep::Stop(_)
                    | DefaultRowStep::System { .. }
                    | DefaultRowStep::User) => (index, other),
                })
                .collect();

            let reached = fixture.run(law, &rows);
            assert!(
                matches!(reached.stop, Err(LibraryLoopStop::Row(got)) if got == site),
                "{law:?} {site:?}: {:?}",
                reached.stop
            );
            let mut expected = vec![fixture.system.join("b.bin")];
            if site == TableConfSite::NullSystemFile
                && crate::data::phrase_library::null_chunk_writes_mmap_line(&fixture.system)
            {
                expected.push(fixture.system.clone());
            }
            assert_eq!(reached.unmapped, expected, "{law:?}");

            fixture.bad_log("a.dbin");
            let run = fixture.run(law, &rows);
            assert_journal(&run.stop, 1);
            assert!(run.unmapped.is_empty(), "{law:?} {site:?}");
        }
    }
}

/// An absent library reports before the later chunk that stops the loop
/// (`pinyin.cpp:256`; register row 76's prefix).
#[test]
fn an_absent_library_keeps_its_line_before_a_later_unmappable_one() {
    for law in LAWS {
        let fixture = Fixture::new(CONF);
        fixture.absent("a.bin");
        fixture.unmappable("c.bin");
        let conf = fixture.conf();
        let run = fixture.run(law, &conf.default_row_steps(None));
        let Err(LibraryLoopStop::Library(error)) = &run.stop else {
            panic!("{law:?}: {:?}", run.stop);
        };
        assert_eq!(
            error.unmappable_path(),
            Some(fixture.system.join("c.bin").as_path()),
            "{law:?}"
        );
        assert_eq!(
            run.unmapped,
            [fixture.system.join("a.bin"), fixture.system.join("c.bin")],
            "{law:?}"
        );
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
        let run = fixture.run(law, &conf.default_row_steps(None));
        assert_journal(&run.stop, 1);
        assert_eq!(run.loaded, [1], "{law:?}");
        assert!(run.profile.user_libraries.is_empty(), "{law:?}");
        assert!(
            !run.profile.loaded.library_totals.contains_key(&4),
            "{law:?}"
        );
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
        let run = fixture.run(law, &conf.default_row_steps(None));
        assert!(
            matches!(&run.stop, Err(LibraryLoopStop::Library(error)) if error.unmappable_path().is_some()),
            "{law:?}: {:?}",
            run.stop
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
        let run = fixture.run(law, &conf.default_row_steps(None));
        assert_journal(&run.stop, 2);
        assert_eq!(run.unmapped, [fixture.system.join("b.bin")], "{law:?}");
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
        crate::data::phrase_libraries::PhraseLibraries::empty(),
        UserFileLayout::from_conf(&conf),
    );
    let mut unmapped = Vec::new();
    let stop = rows.map_libraries(&mut originals, &mut |path| unmapped.push(path.to_owned()));
    assert!(
        matches!(&stop, Err(LibraryLoopStop::Library(error)) if error.unmappable_path().is_some()),
        "{stop:?}"
    );
    assert_eq!(
        unmapped,
        [fixture.system.join("a.bin"), fixture.system.join("b.bin")]
    );
    assert!(!originals.phrase_libraries().is_loaded(3));
}
