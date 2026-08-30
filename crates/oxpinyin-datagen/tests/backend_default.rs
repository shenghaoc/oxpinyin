//! The datagen default backend cannot silently diverge from
//! `oxpinyin_store::DefaultStore`.
//!
//! Two axes are pinned:
//!   1. `Backend::DEFAULT` (the constant the binary's `Options::default()`
//!      pulls from) is `Backend::KyotoCabinet` — the workspace's native
//!      default (mirroring the DBM the reference libpinyin builds against
//!      on the primary target distros).
//!   2. The default backend's file extension matches the store's
//!      `DEFAULT_STORE_EXT` under the compiled-in feature set, so a
//!      `oxpinyin-datagen compile ...` writes files an oxpinyin-runtime
//!      built with the same features can open.

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
