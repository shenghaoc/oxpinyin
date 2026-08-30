//! Backend-selection contract for `oxpinyin-datagen`.
//!
//! Three invariants:
//!   1. `Backend::DEFAULT` — the constant `Options::default()` pulls from —
//!      is `Backend::KyotoCabinet`, matching `oxpinyin_store::DefaultStore`
//!      under the workspace's default feature set.
//!   2. The default backend's file extension matches
//!      `oxpinyin_store::DEFAULT_STORE_EXT`, so `oxpinyin-datagen compile`
//!      writes files an oxpinyin-runtime built with the same features can
//!      open. This closes the "datagen default and runtime default can
//!      silently diverge" gap.
//!   3. Each of the four peer backends (KC, redb, LMDB, tkrzw) knows its
//!      own extension and can be selected explicitly on the command line —
//!      the peer set is not just a compile-time chain, it is the actual
//!      producer set.

use oxpinyin_datagen::write::Backend;

#[test]
fn default_is_kyoto_cabinet() {
    // Written as an equality against the KC variant, not `matches!`, so
    // an accidental slide back to `Backend::Redb` fails with a clear
    // diagnostic naming both variants.
    assert_eq!(Backend::DEFAULT, Backend::KyotoCabinet);
}

/// The datagen default's on-disk extension must match `oxpinyin-store`'s
/// `DEFAULT_STORE_EXT` — otherwise `oxpinyin-datagen compile` writes files
/// the runtime built with the same features cannot open by name.
///
/// Guarded on the KC feature: an explicit-redb build
/// (`--no-default-features --features redb`) resolves the store's
/// `DEFAULT_STORE_EXT` to `redb` and picks a different backend at the CLI,
/// so the equality only holds for the KC-default configuration.
#[cfg(feature = "kyotocabinet")]
#[test]
fn default_backend_extension_matches_store_default() {
    assert_eq!(Backend::DEFAULT.extension(), "kct");
    assert_eq!(
        Backend::DEFAULT.extension(),
        oxpinyin_store::DEFAULT_STORE_EXT
    );
}

/// Every peer backend the CLI can name reports the extension the
/// runtime uses for that peer. Any mismatch here would let
/// `oxpinyin-datagen compile --backend <peer>` write files a runtime
/// built with that peer refuses to open by name.
#[test]
fn peer_backends_report_their_expected_extensions() {
    assert_eq!(Backend::KyotoCabinet.extension(), "kct");
    assert_eq!(Backend::Redb.extension(), "redb");
    assert_eq!(Backend::Lmdb.extension(), "lmdb");
    assert_eq!(Backend::Tkrzw.extension(), "tkt");
}

/// The `--backend` argument parser accepts each of the four peer names
/// spelled the way the CLI's usage line advertises. A silent rename or
/// omission here would break a documented invocation.
#[test]
fn parse_accepts_each_peer_backend_name() {
    assert_eq!(
        Backend::parse("kyotocabinet").expect("kyotocabinet parses"),
        Backend::KyotoCabinet
    );
    assert_eq!(Backend::parse("redb").expect("redb parses"), Backend::Redb);
    assert_eq!(Backend::parse("lmdb").expect("lmdb parses"), Backend::Lmdb);
    assert_eq!(
        Backend::parse("tkrzw").expect("tkrzw parses"),
        Backend::Tkrzw
    );
    // A non-peer name is rejected, not silently mapped to the default.
    assert!(Backend::parse("bogus").is_err());
}
