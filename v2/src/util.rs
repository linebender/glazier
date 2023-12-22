use std::{
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering},
};

/// An thread-safe incrementing counter for generating unique ids.
///
/// The counter wraps on overflow overflow. If the [new] constructor
/// If this is possible for your application, and reuse would be undesirable,
/// use something else.
///
/// [new]: Counter::new
pub struct Counter(pub AtomicU64);

impl Counter {
    /// Create a new counter.
    pub const fn new() -> Counter {
        Counter(AtomicU64::new(1))
    }

    /// Creates a new counter with a given starting value.
    pub const fn new_with_initial_value(init: u64) -> Counter {
        Counter(AtomicU64::new(init))
    }

    pub const fn to_raw(self) -> AtomicU64 {
        self.0
    }

    /// Return the next value.
    ///
    /// This wraps on overflow
    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed)
    }

    /// Return the next value, as a `NonZeroU64`.
    ///
    /// If the next value would be zero, the counter is incremented again
    /// to get the next value
    pub fn next_nonzero(&self) -> NonZeroU64 {
        // If we increment and wrap reach zero, try again.
        // It is implausible that another 2^64-1 calls would be made between
        // the two, so we can safely unwrap
        NonZeroU64::new(self.next()).unwrap_or_else(|| NonZeroU64::new(self.next()).unwrap())
    }
}
