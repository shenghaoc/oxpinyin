use std::sync::{Mutex, MutexGuard};

/// Tracks the addresses issued by one C facade until it finalizes each context.
/// Each facade owns a separate registry (compatibility register rows 60 and 105).
#[derive(Default)]
pub struct LiveContexts {
    addresses: Mutex<Vec<usize>>,
}

impl LiveContexts {
    /// Creates an empty registry for the facade's static.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            addresses: Mutex::new(Vec::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<usize>> {
        self.addresses
            .lock()
            .unwrap_or_else(|_| std::process::abort())
    }

    /// Records an address once, even if it was already registered.
    pub fn register(&self, address: usize) {
        let mut live = self.lock();
        if !live.contains(&address) {
            live.push(address);
        }
    }

    /// Removes an address and reports whether it was present, under one lock.
    pub fn unregister(&self, address: usize) -> bool {
        let mut live = self.lock();
        let before = live.len();
        live.retain(|held| *held != address);
        if live.is_empty() {
            // The static outlives all contexts. Release the last allocation here.
            *live = Vec::new();
        }
        live.len() != before
    }

    /// Reports whether a registered context is still live.
    #[must_use]
    pub fn contains(&self, address: usize) -> bool {
        self.lock().contains(&address)
    }
}

#[cfg(test)]
mod tests;
