# Transient libpinyin user sessions

`oxpinyin-user` is an internal crate without a stability promise. It adds:

```rust
UserStore::open_transient(
    originals: BTreeMap<u8, SystemLibrary>,
    versions: SystemVersions,
    law: UserConfLaw,
) -> Result<Self, UserStoreError>

store.has_user_directory() -> bool
```

The constructor owns in-memory indexes and a native in-memory bigram store,
seeded with system library originals. It has no persistence destination and
performs no profile reads or writes. Import, lookup, remember, mask and
frequency mutations remain visible in that session. A separate context starts
with separate state. The directory accessor distinguishes this session from
an ordinary libpinyin profile, including a non-NULL empty path.

Facade train and save refuse for a transient session before changing training
state, locale or dirty state. Pinyin's failed empty-filename init/fini marker
diagnostics match libpinyin 2.11.92; Zhuyin has no fini marker write.
The existing open_libpinyin signature and all C ABI signatures are unchanged.
