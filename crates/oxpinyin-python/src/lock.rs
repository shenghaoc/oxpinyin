//! The engine lock policy, in one place.
//!
//! [`Engine`](crate) parks its session behind a `Mutex`, and a Rust `Mutex`
//! is *poisoned* once a thread panics while holding it. The two ways to
//! react — recover the guard and carry on, or refuse — are a policy choice,
//! not an implementation detail, so the choice is made here rather than at
//! each call site. Every entry point acquires through [`locked`], which is
//! what makes mutating and reading paths agree.
//!
//! **The policy: a poisoned lock is refused, never recovered.** A panic
//! under the lock means an operation stopped part-way through mutating the
//! session, so the invariants the decoder reads may no longer hold. Handing
//! that state to the next caller trades a loud failure for a quiet wrong
//! answer — candidates that look ordinary and are not. Refusing beats
//! guessing. `Engine` is `#[pyclass(frozen)]`, so there is no way to rebuild
//! the session in place: a refused engine stays refused, and the caller
//! opens a new one.
//!
//! This is a contingency, not a reachable path. Nothing in the oxpinyin
//! crates panics on any input (constitution §4), so no Python-level input
//! poisons this lock — which is also why refusing costs nothing in practice,
//! and why the tests below poison a mutex directly instead of driving the
//! binding.

use std::sync::{Mutex, MutexGuard};

/// A lock refused because some earlier operation panicked while holding it.
#[derive(Debug, Eq, PartialEq)]
pub struct Poisoned;

/// Acquires `mutex` under the module policy: poisoned means refused.
pub fn locked<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, Poisoned> {
    mutex.lock().map_err(|_| Poisoned)
}

#[cfg(test)]
mod tests {
    use super::{Poisoned, locked};
    use std::sync::{Arc, Mutex};

    /// Poisons `mutex` the only way a mutex can be poisoned: by panicking
    /// while its guard is held.
    fn poison(mutex: &Arc<Mutex<i32>>) {
        let mutex = Arc::clone(mutex);
        let outcome = std::thread::spawn(move || {
            let _guard = mutex.lock().expect("a fresh mutex is not poisoned");
            panic!("an operation failed while holding the engine lock");
        })
        .join();
        assert!(outcome.is_err(), "the helper thread must have panicked");
    }

    #[test]
    fn a_healthy_lock_is_granted() {
        let mutex = Mutex::new(7);
        assert_eq!(locked(&mutex).map(|guard| *guard), Ok(7));
    }

    #[test]
    fn a_poisoned_lock_is_refused_from_both_entry_shapes() {
        let inner = Arc::new(Mutex::new(0_i32));
        poison(&inner);

        // The mutating shape: `Engine::with_session` clones the `Arc` into a
        // closure and acquires inside it with the interpreter detached, so a
        // separate thread is the faithful stand-in.
        let mutator = {
            let inner = Arc::clone(&inner);
            std::thread::spawn(move || locked(&inner).map(|guard| *guard))
                .join()
                .expect("acquiring a poisoned lock must refuse, not panic")
        };
        // The reading shape: `Engine::guard` acquires straight off
        // `&self.inner` while still attached.
        let getter = locked(&inner).map(|guard| *guard);

        assert_eq!(mutator, Err(Poisoned));
        assert_eq!(getter, Err(Poisoned));
        assert_eq!(
            mutator, getter,
            "a mutator and a getter must see the same poisoned-lock outcome"
        );
    }

    #[test]
    fn refusal_is_permanent() {
        let inner = Arc::new(Mutex::new(0_i32));
        poison(&inner);
        for _ in 0..3 {
            assert_eq!(locked(&inner).map(|guard| *guard), Err(Poisoned));
        }
    }
}
