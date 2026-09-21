# Send+Sync mechanisms vs the C ABI

Date: 2026-09-21 · Status: audit (PR #503 follow-up; findings only)

The Python wrapper (`oxpinyin-python`) was the only in-tree consumer that
held a session behind `Arc<Mutex<_>>` and therefore needed
`RuntimeSession: Send` (`Arc<Mutex<T>>: Send + Sync` requires only
`T: Send`). PR #503 removed it. What remains is the
C ABI: `pinyin_context_t` / `zhuyin_context_t` own one `ContextCore`,
and every `pinyin_alloc_instance` / `zhuyin_alloc_instance` clones that
context's handles so the instance does not borrow the context and stays
alive past `pinyin_fini` (`crates/oxpinyin-capi/src/state.rs`).

libpinyin's public C surface makes no thread-safety promise. The
drop-in header (`crates/oxpinyin-capi/pinyin.h`; the zhuyin twin is the
same shape) documents opaque handles and `G_BEGIN_DECLS`, not concurrent
use. A consumer that shares a context or its instances across threads is
outside the ABI. This audit assesses the remaining sharing machinery
against that contract only.

The compile-time pins that still name `Send + Sync` are
`assert_send_sync` on `RuntimeDict` / `RuntimeLm` / `UserStore`
(`crates/oxpinyin-runtime/src/lib.rs`) and `sends_and_syncs` on
`RuntimeSession` / `Runtime` (`crates/oxpinyin-runtime/tests/assembly.rs`).
They prove the adapter handles *can* cross threads behind the caller's
own synchronization. They are not a C ABI guarantee.

This audit assumes libpinyin's own contract — callers serialise all
access to a context and its instances — so weakening the Row 3
`SeqCst` loads is a valid Stage-2 move only under that contract and
after the compile-time `Send + Sync` pins that keep the caller's races
sound are relaxed; until then, `SeqCst` stays.

## Mechanism table

| Mechanism | Where | What it shares | C ABI assessment |
| --- | --- | --- | --- |
| **Arc handles** | `RuntimeDict::system`, `RuntimeDict`/`RuntimeLm`/`UserStore` clones handed to each instance; `PunctTable`; `library_mask` / `library_epoch` arcs | Immutable (or atomically mutated) table and mask state across the context and every instance it allocates | Required even single-threaded: the instance must not borrow the context. `Arc` is an ownership/lifetime device. `Arc<T>` is `Send + Sync` iff `T: Send + Sync`; those traits on the handle come from the inner types, not from the wrapper, and they are not a promise that two threads may call into the same context. |
| **Mutex lookup cache and unigram overlay** | `RuntimeDict::user_lookup_cache` (`Arc<Mutex<Option<(u64, Arc<UserLookup>)>>>`); `RuntimeDict::unigram_overlay` (`Arc<Mutex<HashMap<u32, u64>>>`) | Mutable per-context user-phrase lookup (generation-stamped) and in-memory `add_unigram_frequency` deltas (gone at fini, matching `pinyin_save` flushing user data only) | Required so handle clones of one context see one cache and one overlay. A single-threaded C consumer still has several clones (context + instances). The `Mutex` serializes those clones; poison recovery (`into_inner`) is defensive. It is not a published concurrent-access API. |
| **SeqCst library-mask seqlock and epoch** | `RuntimeDict::library_mask` (`Arc<AtomicU32>`, bit set = unloaded); `RuntimeDict::library_epoch` (`Arc<AtomicU64>`). Load/unload bump the epoch before and after the mask flip; key-cost walks load epoch, mask, walk, epoch and discard a torn window. Comments require every mask read on that protocol to stay `SeqCst` so the total order keeps those operations in program order. Lookup/prefix paths also `SeqCst`-load the mask. The fast-path mask load in `cached_key_costs` (`crates/oxpinyin-runtime/src/lib.rs:1109`) is `Acquire`; it only gates the cached-stamp probe and sits outside the seqlock protocol. | GBK visibility (`pinyin_load_phrase_library` / `pinyin_unload_phrase_library`) across clones, including a session built while a flip is in flight | Stronger than the C contract. A single-threaded consumer never observes a torn window: load/unload and `pinyin_alloc_instance` are sequenced on one thread. The seqlock exists because clones share the mask and the compile-time `Send + Sync` pins allow a caller to race them. **This is the next-release item "ordered atomics off the steady cycle"** (`docs/findings/perf-cycle-ir-differential-2026-09-08.md`: arm64 pays `ldar`/release on the per-candidate path where x86-64 TSO makes acquire/release effectively free). Relaxing those `SeqCst` loads off the lookup walk is a Stage-2 measurement, not a C ABI break, provided the single-threaded sequenced contract is preserved. |
| **RwLock key-cost memo and AddonSet** | `Runtime::key_costs` (`RwLock<Option<(u32, Arc<[Cost]>)>>`, stamp = the mask the table was walked under); `RuntimeDict::addons` / `RuntimeLm::addons` (`Arc<RwLock<AddonSet>>`) | Lazy key-cost table (filled on first `new_session` that needs the fallback scorer; real-unigram models skip the walk entirely) and addon DBM load/unload vs lookup | Same clone-sharing reason as the Mutex row. The key-cost fast path takes a shared read lock so concurrent `new_session` would not contend; under the C ABI those calls are sequenced. Addon load/unload vs lookup needs *some* exclusive/shared split because clones share the set; `RwLock` is the `Send + Sync`-preserving stand-in, not a thread-safety promise to C. |
| **LiveOptions `Arc<Atomic*>` words** | `LiveOptions`: `incomplete`/`use_tone`/`force_tone` (`Arc<AtomicBool>`), `double_scheme`/`zhuyin_scheme`/`full_scheme` (`Arc<AtomicI32>`), `options` (`Arc<AtomicU32>`). Context `set_options` / `pinyin_set_*_scheme` store; instances load on parse. Every load and store is `Relaxed`. | Live option word and scheme discriminants so `set_options` on the context remasks instances already allocated | Exactly the C ABI shape: one context, many instances, sequenced calls. `Relaxed` matches that sequencing (no cross-thread publication). These atomics are not on the ordered-atomics next-release list. |

## What this is not

- Not a divergence. The mechanisms reproduce handle-sharing the C ABI
  already has (`pinyin_alloc_instance` clones from `pinyin_init`'s
  context). Recording them here does not argue a class-(a)/(b)/(c)
  exception.
- Not a code change. The `assert_send_sync` pins stay. Demoting
  constructors, dropping `Send + Sync`, or relaxing `SeqCst` is later
  work; the last of those is the next-release item named above.
- Not a thread-safety guarantee for embedders. Unmodified libpinyin
  consumers remain single-threaded against one context.

## Next-release pointer

Ordered atomics off the steady cycle: the `SeqCst` mask/epoch protocol
on the per-candidate lookup path. Evidence and the architecture spread
that motivated it live in
`docs/findings/perf-cycle-ir-differential-2026-09-08.md`. This audit is
the ownership/ABI half of that item — what the C contract actually
requires of those loads — so a later change can measure a weaker
ordering without rediscovering why the seqlock is there.
