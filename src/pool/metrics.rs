//! Optional diagnostics; normal builds use the original blocking mutex operation.
use std::sync::{LockResult, Mutex, MutexGuard};
#[cfg(feature = "pool-stats")]
use std::sync::{atomic::{AtomicU64, Ordering}, TryLockError};

#[derive(Clone, Copy, Debug, Default)]
pub struct PoolStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub lock_attempts: u64,
    pub lock_contentions: u64,
    /// Actual allocation retained, including capacity beyond vector length.
    pub retained_bytes: usize,
    /// The budget counter, reported separately to detect accounting mistakes.
    pub accounted_bytes: usize,
    pub buffers: usize,
}

#[cfg(feature = "pool-stats")]
static COUNTERS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];

#[inline]
pub(super) fn event(index: usize) {
    #[cfg(feature = "pool-stats")]
    { COUNTERS[index].fetch_add(1, Ordering::Relaxed); }
    #[cfg(not(feature = "pool-stats"))]
    let _ = index;
}

pub(super) fn counters() -> PoolStats {
    #[cfg(feature = "pool-stats")]
    { return PoolStats {
        hits: COUNTERS[0].load(Ordering::Relaxed),
        misses: COUNTERS[1].load(Ordering::Relaxed),
        evictions: COUNTERS[2].load(Ordering::Relaxed),
        lock_attempts: COUNTERS[3].load(Ordering::Relaxed),
        lock_contentions: COUNTERS[4].load(Ordering::Relaxed),
        ..PoolStats::default()
    }; }
    #[cfg(not(feature = "pool-stats"))]
    PoolStats::default()
}

#[inline]
pub(super) fn lock<T>(mutex: &Mutex<T>) -> LockResult<MutexGuard<'_, T>> {
    #[cfg(feature = "pool-stats")]
    {
        event(3);
        match mutex.try_lock() {
            Ok(guard) => Ok(guard),
            Err(TryLockError::Poisoned(error)) => Err(error),
            Err(TryLockError::WouldBlock) => { event(4); mutex.lock() }
        }
    }
    #[cfg(not(feature = "pool-stats"))]
    mutex.lock()
}
