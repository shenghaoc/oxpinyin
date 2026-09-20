//! The baked `@DATABASE_FORMAT@` names this build's backend — pinned.
//!
//! `LIBPINYIN_DATABASE_FORMAT`-less builds bake the token of the active
//! peer feature into `libpinyin.pc.in.baked`; a consumer's cmake probe
//! reads it (`build.rs` documents the chain). Nothing asserted the
//! mapping until this test: the missing `bdb` arm that PR #498 closes
//! shipped exactly because no gate compared the baked string against the
//! build's own token.
//!
//! The comparison target is `SystemVersions::for_this_build`, whose
//! `database_format` field *is* `oxpinyin_store::DEFAULT_STORE_DB_FORMAT`
//! — the same value the runtime writes into `user.conf`/`table.conf`.
//! Asserting against it (rather than against a literal) is what makes the
//! test non-vacuous: removing a feature arm makes the `.pc` disagree with
//! the token every other artefact of this build carries. The one backend
//! excluded is redb, whose spelling is oxpinyin-only by design — see the
//! `cfg` on the test.

// redb's `database format:` token is oxpinyin-only and deliberately
// unknown upstream (`user_files.rs`'s `UPSTREAM_DB_FORMATS` excludes both
// spellings, and its test pins `"Redb"` as known to no upstream build), so
// the `.pc` reports an unknown format to a consumer's probe whichever
// spelling it carries — and the backend is removed in the next stack
// member, so pinning its `.pc` spelling buys nothing. #501 drops this
// `cfg` when redb goes.
#[cfg(not(feature = "redb"))]
#[test]
fn baked_pc_database_format_matches_the_compiled_backend() {
    // The parse lives in the test body rather than in a file-scope helper:
    // under the redb gate below there is no test left to use a helper, and
    // this crate's lanes carry `RUSTFLAGS: -D warnings`, so a dead
    // `fn` would be a hard error on the one lane the gate excludes.
    fn database_format_from_pc(pc: &str) -> &str {
        pc.lines()
            .find_map(|line| line.strip_prefix("database_format="))
            .unwrap_or_else(|| panic!("the baked .pc has no database_format line:\n{pc}"))
    }

    let path = env!("OXPINYIN_PC_BAKED");
    let pc = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {path}: {e}; the build script should have baked it"));
    let versions = oxpinyin_data::user_files::SystemVersions::for_this_build(0, 0);
    assert_eq!(
        database_format_from_pc(&pc),
        versions.database_format,
        "the .pc a packager installs must name the backend this build carries \
         (see docs/findings/compatibility-policy.md on the same-backend claim)",
    );
}
