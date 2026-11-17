//! Init and save: reports, refusals, conf formats and copy faults.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

/// A stale `user_phrase_index.bin.tmp` directory: the save renames it over
/// an absent or empty-directory index; over a live file the rename fails, the
/// tmp stays and the live file is kept, with one stderr line (#542). On
/// Windows `g_rename` cannot replace a directory with a directory, so the
/// empty-directory round keeps its tmp too.
#[test]
fn a_stale_index_tmp_directory_is_renamed_unless_a_live_file_holds_the_name() {
    let run = run_in_child(
        "boundary_tests::init_and_save::a_stale_index_tmp_directory_is_renamed_unless_a_live_file_holds_the_name",
        |root| {
            let marker: &[u8] = b"unchanged temporary directory";
            let empty_directory_moves = cfg!(not(windows));
            for (round, (destination, moved)) in [
                ("absent", true),
                ("empty-directory", empty_directory_moves),
                ("live-file", false),
            ]
            .into_iter()
            .enumerate()
            {
                let user = root.join(format!("round-{round}"));
                std::fs::create_dir(&user).expect("a fresh user directory");
                let (context, instance) = open_with_user(&user);
                let train = |instance| {
                    unsafe { zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()) };
                    assert!(unsafe { zhuyin_guess_sentence(instance) });
                    assert!(unsafe { zhuyin_train(instance) });
                };
                train(instance);
                let live = user.join("user_phrase_index.bin");
                let mut before = None;
                if destination == "live-file" {
                    assert!(unsafe { zhuyin_save(context) });
                    before = Some(std::fs::read(&live).expect("the live index"));
                    train(instance);
                } else if destination == "empty-directory" {
                    std::fs::create_dir(&live).expect("an empty destination directory");
                }
                let tmp = user.join("user_phrase_index.bin.tmp");
                std::fs::create_dir(&tmp).expect("the stale temporary directory");
                std::fs::write(tmp.join("stale-marker"), marker).expect("the stale marker");
                assert!(unsafe { zhuyin_save(context) }, "{destination}");
                assert_eq!(tmp.exists(), !moved, "{destination}");
                assert_eq!(
                    std::fs::read(live.join("stale-marker")).ok().as_deref(),
                    moved.then_some(marker),
                    "{destination}"
                );
                if let Some(before) = before {
                    assert_eq!(std::fs::read(&live).expect("the live index"), before);
                }
                close(context, instance);
            }
        },
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    let renames = stderr.matches("user_phrase_index.bin failed.").count();
    assert_eq!(renames, if cfg!(windows) { 2 } else { 1 }, "{stderr}");
}
