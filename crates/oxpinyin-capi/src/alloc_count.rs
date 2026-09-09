//! Allocation counting for the keystroke-cycle differentials.
//!
//! Gated behind the non-default `alloc-count` cargo feature and absent
//! from every default build: with the feature off this module does not
//! exist, no `#[global_allocator]` is declared, and none of the accessor
//! symbols are exported. `tools/abi/check-exports.sh` and a plain
//! `nm -D` on a default artifact are the checks.
//!
//! Two different questions are asked of this counter, and they want
//! different statistics.
//!
//! **Instruction differential (call count and requested bytes).**
//! Callgrind links `vg_replace_malloc.c` into every tool, so client
//! `malloc`/`free` are intercepted and the Ir the profile attributes to
//! allocation is valgrind's replacement allocator, not glibc's — an
//! underestimate of the real cost. Callgrind's *call counts* are
//! unaffected and remain the primary allocation figure, because they are
//! produced symmetrically for both engines from one instrument. This
//! counter adds what the profile cannot give: the requested byte volume,
//! and an independent check on the Rust side's call count.
//!
//! **RSS attribution (live and peak-live bytes).** Cumulative count and
//! cumulative bytes say nothing about resident memory: a site that
//! allocates thirty-nine small vectors in sequence and drops each holds
//! no resident memory at all, while a site holding the same thirty-nine
//! alive at once raises the allocator's high-water mark by their sum.
//! `oxpinyin_alloc_live_bytes` and `oxpinyin_alloc_peak_live_bytes` are
//! the RSS-relevant pair; peak-live is the one that bounds the heap the
//! allocator must have obtained from the kernel.
//!
//! Scope, stated so a reading of these numbers is not overclaimed: this
//! is Rust's `GlobalAlloc` traffic only. Allocations the process makes
//! through C — glib inside the C ABI marshalling, and everything the
//! backend library does — do not pass through here and are invisible to
//! it. The `/proc` and `mallinfo2` readings in `tools/bisection/bisect.c`
//! are what covers the whole process.
//!
//! The counters are `Relaxed` atomics. The harness reads them from one
//! thread around a region that runs on one thread, so no ordering
//! stronger than atomicity is needed, and `Relaxed` keeps the counter
//! from adding barriers to the very path being measured. `LIVE_BYTES` is
//! therefore exact only when read with no other thread allocating; under
//! concurrent allocation it is a sum of independent relaxed updates,
//! which is what the single-threaded measurement region wants and all it
//! claims.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

static ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE_BYTES: AtomicU64 = AtomicU64::new(0);
static PEAK_LIVE_BYTES: AtomicU64 = AtomicU64::new(0);

/// Add `bytes` to the live total and raise the peak to match.
///
/// `fetch_add` returns the previous value, so the new live total is
/// computed here rather than re-read: a re-read could observe another
/// thread's update and record a peak this call did not reach.
fn live_add(bytes: u64) {
    let live = LIVE_BYTES.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK_LIVE_BYTES.fetch_max(live, Ordering::Relaxed);
}

/// A `System` forwarder that counts allocating calls, requested bytes,
/// currently-live bytes and the high-water mark of the live total.
///
/// `realloc` counts as one call and adds the new size to the cumulative
/// byte total, matching the "allocations per cycle" figure the
/// instruction differential reports; against the *live* total it posts
/// only the difference, so a grow-in-place `Vec` does not inflate the
/// live figure by the whole buffer each time it doubles.
///
/// `dealloc` is not counted as a call — the cumulative pair keeps its
/// existing meaning — but it does subtract from the live total, which is
/// what makes the live and peak figures mean anything.
pub struct CountingAlloc;

// SAFETY: every method forwards its unmodified `Layout` and pointer
// arguments to `System`, which is a correct `GlobalAlloc`, and returns
// what `System` returned. The counters are separate atomics that never
// read or write through the allocated pointers, so nothing about the
// allocator contract — validity, alignment, or the pairing of allocation
// with deallocation — is changed by the wrapper.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: `layout` is forwarded unchanged from our caller, which
        // upholds `GlobalAlloc::alloc`'s contract.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
            live_add(layout.size() as u64);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: `layout` is forwarded unchanged from our caller, which
        // upholds `GlobalAlloc::alloc_zeroed`'s contract.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
            live_add(layout.size() as u64);
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `ptr`, `layout` and `new_size` are forwarded unchanged
        // from our caller, which upholds `GlobalAlloc::realloc`'s
        // contract — `ptr` came from this same allocator under `layout`.
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
            // A failed realloc leaves the old block live, so the live
            // total must only move when the call succeeded.
            if new_size >= layout.size() {
                live_add((new_size - layout.size()) as u64);
            } else {
                LIVE_BYTES.fetch_sub((layout.size() - new_size) as u64, Ordering::Relaxed);
            }
        }
        new_ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size() as u64, Ordering::Relaxed);
        // SAFETY: `ptr` and `layout` are forwarded unchanged from our
        // caller, which upholds `GlobalAlloc::dealloc`'s contract — `ptr`
        // came from this same allocator under `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// Allocating calls (`alloc`, `alloc_zeroed`, `realloc`) since process start.
#[unsafe(no_mangle)]
pub extern "C" fn oxpinyin_alloc_count() -> u64 {
    ALLOC_CALLS.load(Ordering::Relaxed)
}

/// Bytes requested by those calls since process start.
#[unsafe(no_mangle)]
pub extern "C" fn oxpinyin_alloc_bytes() -> u64 {
    ALLOC_BYTES.load(Ordering::Relaxed)
}

/// Bytes currently held in live Rust allocations.
///
/// Requested sizes, not allocator-rounded sizes: this is a lower bound on
/// what the heap must hold, never the resident cost itself.
#[unsafe(no_mangle)]
pub extern "C" fn oxpinyin_alloc_live_bytes() -> u64 {
    LIVE_BYTES.load(Ordering::Relaxed)
}

/// The high-water mark of [`oxpinyin_alloc_live_bytes`].
///
/// This is the RSS-relevant statistic: the allocator must at some point
/// have held at least this much, and glibc does not readily return it to
/// the kernel afterwards.
#[unsafe(no_mangle)]
pub extern "C" fn oxpinyin_alloc_peak_live_bytes() -> u64 {
    PEAK_LIVE_BYTES.load(Ordering::Relaxed)
}

/// Drop the recorded peak back to the current live total.
///
/// Lets a harness bound the peak to one region — one keystroke cycle, say
/// — instead of carrying initialization's high-water mark forever. It is
/// not a reset of the cumulative counters, which stay monotonic.
///
/// `tools/bisection/bisect.c` calls this in `rss-diag` mode after reading
/// the post-initialization counters and immediately before the cycle
/// loop, so the `alloc_cycle` peak describes the cycles alone. Without
/// that call the reported peak is the larger of the two regions, which
/// happens to be the cycle peak on this workload but is not guaranteed
/// to be on another.
#[unsafe(no_mangle)]
pub extern "C" fn oxpinyin_alloc_reset_peak() {
    PEAK_LIVE_BYTES.store(LIVE_BYTES.load(Ordering::Relaxed), Ordering::Relaxed);
}
