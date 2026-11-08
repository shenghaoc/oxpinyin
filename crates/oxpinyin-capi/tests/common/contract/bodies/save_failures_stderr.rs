//! The save_failures_stderr family's case bodies: each answers the result document the old case returned.
//!
//! Save failures and the pin's stderr (inventory row 59, #525, #545, #929):
//! blocked staging and final names, removed user directories, header writes
//! that fail, interrupted saves and reloads of a saved profile.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::facade;
use crate::fixtures::{
    TOKEN_REUSE_PHRASES, crafted_bigram_context, gram_bytes, hex_bytes, index_file_rows,
    token_reuse_reading, write_profile,
};
use crate::readers::{c, lookup};
use crate::support::{J, Kit, P, Raw};

/// The case body for `id`.
/// The body of a case whose reference holds the pin-written handoff profile.
pub fn case_with_profile(k: &mut Kit, id: &str, profile: &Raw) -> J {
    match id {
        "save-929-logical-cross-open-pinyin" | "save-929-logical-cross-open-zhuyin" => {
            save_929_logical_roundtrip(k, profile)
        }
        other => panic!("no profile case {other}"),
    }
}

/// The case body for `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(out) = save_929_case(k, id) {
        return out;
    }
    match id {
        "stderr-save-one-tmp-blocked" | "stderr-save-one-tmp-blocked-zhuyin" => {
            blocked_save(k, "user_pinyin_index.bin.tmp")
        }
        #[cfg(unix)]
        "abort-save-chunk-header-length" | "abort-zhuyin-save-chunk-header-length" => {
            save_header_dev_full(k)
        }
        #[cfg(unix)]
        "abort-save-chunk-header-checksum" | "abort-zhuyin-save-chunk-header-checksum" => {
            save_header_rlimit(k)
        }
        "stderr-save-dir-removed" | "stderr-save-dir-removed-zhuyin" => save_after_training(k),
        "stderr-fini-dir-removed" | "stderr-fini-dir-removed-zhuyin" => {
            let ctx = k.ctx();
            std::fs::remove_dir_all(&k.user).expect("remove the user directory");
            facade::fini(ctx);
            J::map()
        }
        "stderr-init-missing-system-dir" | "stderr-init-missing-system-dir-zhuyin" => {
            let ctx = k.init(Some(Path::new("/nonexistent-system-dir")), Some(&k.user));
            obj! { "ctx" => !ctx.is_null() }
        }
        "stderr-init-system-dir-without-table-conf" => {
            let empty = k.scratch.join("sys-X");
            std::fs::create_dir(&empty).expect("the empty system directory");
            let system = PathBuf::from(format!("{}//", empty.to_str().expect("UTF-8")));
            let ctx = k.init(Some(&system), Some(&k.user));
            obj! { "ctx" => !ctx.is_null() }
        }
        #[cfg(unix)]
        "stderr-non-utf8-user-dir" => non_utf8_user_dir(k),
        "stderr-unmodified-save-dir-removed" => {
            let ctx = k.ctx();
            std::fs::remove_dir_all(&k.user).expect("remove the user directory");
            obj! { "save" => facade::save(ctx) }
        }
        other => panic!("no save_failures_stderr case {other}"),
    }
}

/// The `save-929-*` cases, `None` for another case.
fn save_929_case(k: &mut Kit, id: &str) -> Option<J> {
    let suffix = if facade::ZHUYIN { "-zhuyin" } else { "-pinyin" };
    let stem = id.strip_prefix("save-929-")?.strip_suffix(suffix)?;
    Some(match stem {
        "rename-user.bin" => save_929_failure(k),
        "stale-bigram-tmp" => save_bigram_stale_tmp(k),
        other => panic!("no save_failures_stderr case {id}: {other}"),
    })
}

/// The facade's parse of `LIBRARY_FACADES`' text: `nihao` or `su3cl3`.
fn parse_text(inst: P) {
    if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c("su3cl3").as_ptr());
    } else {
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    }
}

/// A fresh instance of the cached context, parsed and guessed.
fn guessed(k: &mut Kit) -> P {
    let inst = k.alloc();
    parse_text(inst);
    facade::guess_sentence(inst);
    inst
}

/// `save_after_training`: train a guessed sentence, optionally remove the user
/// directory, then save the cached context.
fn save_after_training(k: &mut Kit) -> J {
    let ctx = k.ctx();
    let inst = guessed(k);
    let mut out = obj! { "train" => facade::train_top(inst) };
    std::fs::remove_dir_all(&k.user).expect("remove the user directory");
    out.set("save", facade::save(ctx));
    out
}

/// `_dirty_for_save`: parse, guess and train, answering the cached context.
fn dirty_for_save(k: &mut Kit) -> P {
    let ctx = k.ctx();
    let inst = guessed(k);
    facade::train_top(inst);
    ctx
}

/// `blocked_save`'s snapshot: each user entry by its sorted listing or contents.
fn snapshot(k: &Kit) -> BTreeMap<String, String> {
    use sha2::Digest as _;
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(&k.user).expect("user directory") {
        let entry = entry.expect("user entry");
        let path = entry.path();
        let name = crate::support::os_text(&entry.file_name());
        let view = if path.is_dir() {
            let mut names: Vec<String> = std::fs::read_dir(&path)
                .expect("a listed directory")
                .map(|inner| crate::support::os_text(&inner.expect("entry").file_name()))
                .collect();
            names.sort();
            format!("dir:{}", names.join(","))
        } else {
            hex_bytes(&sha2::Sha256::digest(
                std::fs::read(&path).expect("a user file"),
            ))
        };
        files.insert(name, view);
    }
    files
}

/// `blocked_save`: a second, modified save with `blocked` replaced by a
/// non-empty directory, both saves' answers and which entries it updated.
fn blocked_save(k: &mut Kit, blocked: &str) -> J {
    let ctx = k.ctx();
    let inst = guessed(k);
    facade::train_top(inst);
    let mut out = obj! { "first" => facade::save(ctx) };
    let before = snapshot(k);
    facade::train_top(inst);
    let path = k.user.join(blocked);
    if path.exists() {
        std::fs::rename(&path, k.user.join(format!("{blocked}.moved"))).expect("move aside");
    }
    std::fs::create_dir(&path).expect("the blocking directory");
    std::fs::write(path.join("keep"), "x").expect("keep");
    out.set("second", facade::save(ctx));
    let after = snapshot(k);
    let mut files = J::map();
    for (name, view) in &after {
        if name.ends_with(".moved") {
            continue;
        }
        let state = match before.get(name) {
            None => "new",
            Some(old) if old != view => "changed",
            Some(_) => "same",
        };
        files.set(name, state);
    }
    out.set("files", files);
    out
}

/// `save_bigram_stale_tmp`: a stale `user_bigram.db.tmp` before a second save,
/// then the final bigram's rows.
fn save_bigram_stale_tmp(k: &mut Kit) -> J {
    let ctx = dirty_for_save(k);
    let first = facade::save(ctx);
    std::fs::write(k.user.join("user_bigram.db.tmp"), b"stale staging bytes").expect("stale");
    let inst = guessed(k);
    facade::train_top(inst);
    let second = facade::save(ctx);
    obj! {
        "first" => first,
        "second" => second,
        "tmp_exists" => k.user.join("user_bigram.db.tmp").exists(),
        "rows" => index_file_rows(&k.user.join("user_bigram.db"), true),
    }
}

/// `save_929_failure`: block a final name, then check the soft failure and the
/// clean-save guard.
fn save_929_failure(k: &mut Kit) -> J {
    let blocked = "user.bin";
    let mut out = blocked_save(k, blocked);
    out.set("clean_save", facade::save(k.ctx()));
    out.set(
        "staged_retained",
        k.user.join(format!("{blocked}.tmp")).is_file(),
    );
    out.set(
        "destination_retained",
        std::fs::read_to_string(k.user.join(blocked).join("keep")).expect("keep"),
    );
    out
}

/// `save_header_dev_full`: `user.bin.tmp` linked to `/dev/full`, so the first
/// header word's write fails (`memory_chunk.h:543`). Unix only: `/dev/full`
/// (Linux) and symbolic links.
#[cfg(unix)]
fn save_header_dev_full(k: &mut Kit) -> J {
    let ctx = dirty_for_save(k);
    std::os::unix::fs::symlink("/dev/full", k.user.join("user.bin.tmp")).expect("link");
    obj! { "ret" => facade::save(ctx) }
}

/// `save_header_rlimit`: a four-byte `RLIMIT_FSIZE`, so the checksum word past
/// the length word is refused (`memory_chunk.h:547`). Unix only: the resource limit.
#[cfg(unix)]
fn save_header_rlimit(k: &mut Kit) -> J {
    let ctx = dirty_for_save(k);
    let limit = libc::rlimit {
        rlim_cur: 4,
        rlim_max: 4,
    };
    // SAFETY: ignores SIGXFSZ and lowers this child's own file-size limit, as the
    // old case did; nothing restores them.
    unsafe {
        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &raw const limit), 0);
    }
    obj! { "ret" => facade::save(ctx) }
}

/// The user directory with a `0xFF` byte after its `user-` prefix.
#[cfg(unix)]
fn non_utf8_user(k: &Kit) -> PathBuf {
    use std::os::unix::ffi::OsStrExt as _;
    let base = k.user.file_name().expect("a base name").as_bytes();
    let name = [&b"user-\xff"[..], &base[5..]].concat();
    k.scratch.join(std::ffi::OsStr::from_bytes(&name))
}

/// `stderr-non-utf8-user-dir`: train in a user directory whose name holds
/// `0xFF`, remove it, then save and finalise. Unix only: a byte-string
/// directory name.
#[cfg(unix)]
fn non_utf8_user_dir(k: &mut Kit) -> J {
    let user = non_utf8_user(k);
    std::fs::create_dir(&user).expect("the user directory");
    let ctx = k.init(Some(&k.data), Some(&user));
    let inst = facade::alloc_instance(ctx);
    facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    facade::guess_sentence(inst);
    let mut out = obj! { "train" => facade::train_top(inst) };
    std::fs::remove_dir_all(&user).expect("remove the user directory");
    out.set("save", facade::save(ctx));
    facade::fini(ctx);
    out
}

/// `save_929_import`: imports `TOKEN_REUSE_PHRASES[number]` into library 7.
fn save_929_import(ctx: P, number: usize, count: i32) {
    let it = facade::begin_add_phrases(ctx, 7);
    assert!(!it.is_null(), "the import iterator");
    let text = c(TOKEN_REUSE_PHRASES[number].0);
    let reading = c(token_reuse_reading(number));
    assert!(facade::iterator_add_phrase(
        it,
        text.as_ptr(),
        reading.as_ptr(),
        count
    ));
    facade::end_add_phrases(it);
}

/// `save_929_tokens`: the tokens of the first three phrases on a fresh instance.
fn save_929_tokens(ctx: P) -> Vec<Vec<u32>> {
    let inst = facade::alloc_instance(ctx);
    assert!(!inst.is_null(), "alloc_instance failed");
    let tokens = TOKEN_REUSE_PHRASES[..3]
        .iter()
        .map(|(text, _, _)| lookup(inst, text.as_bytes()))
        .collect();
    facade::free_instance(inst);
    tokens
}

/// `save_929_logical_roundtrip`'s reload half: the three user containers' rows,
/// the tokens, a count-zero re-import and a save that changes no record.
fn logical_reload(k: &mut Kit, bigram: &(Vec<u8>, Vec<u8>)) -> J {
    let rows = |k: &Kit| {
        let mut map = J::map();
        for name in [
            "user_pinyin_index.bin",
            "user_phrase_index.bin",
            "user_bigram.db",
        ] {
            map.set(
                name,
                index_file_rows(&k.user.join(name), name == "user_bigram.db"),
            );
        }
        map
    };
    let before = rows(k);
    assert_eq!(
        before.get("user_bigram.db"),
        Some(&J::from(vec![(hex_bytes(&bigram.0), hex_bytes(&bigram.1))]))
    );
    let ctx = k.ctx();
    let tokens = save_929_tokens(ctx);
    assert_eq!(tokens, [vec![0x0700_0001], vec![0x0700_0002], vec![]]);
    save_929_import(ctx, 0, 0);
    assert!(facade::save(ctx));
    let after = rows(k);
    assert_eq!(before, after, "facade reopen/save changed logical records");
    facade::fini(ctx);
    k.forget();
    obj! { "rows" => after, "tokens" => tokens }
}

/// `save_929_logical_roundtrip`: a donor profile in this side's handoff
/// directory (crafted bigram row, two saved imports and an unsaved one), then
/// its reload, as the old verify ran it. The pin's profile is then reloaded:
/// its rows and tokens must equal this side's reload, and that reload, with its
/// GLib records alone, is `cross_reload` (the old verify's fresh reload worker).
fn save_929_logical_roundtrip(k: &mut Kit, profile: &Raw) -> J {
    let own = k.scratch.join("handoff-subject-X");
    std::fs::create_dir(&own).expect("the handoff profile");
    k.user = own;
    let bigram = (
        0x0700_0001_u32.to_le_bytes().to_vec(),
        gram_bytes(37, &[(0x0700_0002, 37)]),
    );
    let ctx = crafted_bigram_context(k, std::slice::from_ref(&bigram));
    save_929_import(ctx, 0, 100);
    save_929_import(ctx, 1, 100);
    assert!(facade::save(ctx));
    save_929_import(ctx, 2, 100);
    facade::fini(ctx);
    let mut out = logical_reload(k, &bigram);
    let donor = k.scratch.join("handoff-pin-X");
    std::fs::create_dir(&donor).expect("the pin's handoff profile");
    write_profile(profile, &donor);
    k.user = donor;
    let seen = k.logs().len();
    let mut cross = logical_reload(k, &bigram);
    assert_eq!(cross, out, "cross-open changed logical records");
    let logs: Vec<J> = k.logs().into_iter().skip(seen).map(J::from).collect();
    cross.set("logs", logs);
    out.set("cross_reload", cross);
    out
}
