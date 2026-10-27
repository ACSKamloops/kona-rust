#![forbid(unsafe_code)]
//! Reuse numeric arrays after their last owner releases them.
//!
//! K's Drop implementation uses Arc::get_mut before transferring a vector here.
//! KONA_POOL_MB (default 1024) bounds retained *capacity* across BOTH numeric
//! types. The 48-buffer limit is also combined. Zero disables retention.
//! Small requests keep a direct allocation path. Callers overwrite every slot.

use std::sync::{Mutex, OnceLock};
mod metrics;
pub use metrics::PoolStats;

pub const MIN_LEN: usize = 1 << 12;
const MAX_BUFFERS: usize = 48;
const MAX_BYTES_DEFAULT: usize = 1 << 30;

fn parse_budget(value: Option<&str>) -> usize {
    value.and_then(|v| v.trim().parse::<usize>().ok())
        .and_then(|mb| mb.checked_mul(1 << 20))
        .unwrap_or(MAX_BYTES_DEFAULT)
}

struct Saved<T> {
    values: Vec<T>,
    age: u64,
}

#[doc(hidden)]
pub struct Shelf<T> {
    bufs: Vec<Saved<T>>,
}

/// One lock coordinates the shared budget and transfers ownership of buffers.
/// Numeric computation and resizing happen after this lock is released.
#[doc(hidden)]
pub struct Pool {
    floats: Shelf<f64>,
    integers: Shelf<i64>,
    bytes: usize,
    max_bytes: usize,
    age: u64,
}

impl Pool {
    fn new(max_bytes: usize) -> Self {
        Self { floats: Shelf { bufs: Vec::new() }, integers: Shelf { bufs: Vec::new() },
            bytes: 0, max_bytes, age: 0 }
    }

    fn buffers(&self) -> usize { self.floats.bufs.len() + self.integers.bufs.len() }

    fn evict_oldest(&mut self) {
        let float = self.floats.bufs.iter().enumerate().min_by_key(|(_, v)| v.age)
            .map(|(i, v)| (i, v.age));
        let int = self.integers.bufs.iter().enumerate().min_by_key(|(_, v)| v.age)
            .map(|(i, v)| (i, v.age));
        let bytes = match (float, int) {
            (Some((f, fa)), Some((_, ia))) if fa <= ia => self.floats.bufs.swap_remove(f).values.capacity() * size_of::<f64>(),
            (Some((f, _)), None) => self.floats.bufs.swap_remove(f).values.capacity() * size_of::<f64>(),
            (_, Some((i, _))) => self.integers.bufs.swap_remove(i).values.capacity() * size_of::<i64>(),
            (None, None) => unreachable!("cannot evict from an empty pool"),
        };
        self.bytes -= bytes;
        metrics::event(2);
    }
}

fn pool() -> &'static Mutex<Pool> {
    static POOL: OnceLock<Mutex<Pool>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(Pool::new(parse_budget(std::env::var("KONA_POOL_MB").ok().as_deref()))))
}

/// Numeric types supported by the internal output-buffer pool.
pub trait Pooled: Copy + Default + Send + Sync + 'static {
    #[doc(hidden)]
    fn shelf(pool: &mut Pool) -> &mut Shelf<Self>;
}
impl Pooled for f64 {
    fn shelf(pool: &mut Pool) -> &mut Shelf<Self> { &mut pool.floats }
}
impl Pooled for i64 {
    fn shelf(pool: &mut Pool) -> &mut Shelf<Self> { &mut pool.integers }
}

/// Diagnostic snapshot. Counters require pool-stats; retained bytes always use
/// capacity. Call at a quiescent point when comparing counter deltas.
pub fn stats() -> PoolStats {
    let mut stats = metrics::counters();
    if let Ok(p) = pool().lock() {
        stats.retained_bytes = p.floats.bufs.iter().map(|v| v.values.capacity() * size_of::<f64>()).sum::<usize>()
            + p.integers.bufs.iter().map(|v| v.values.capacity() * size_of::<i64>()).sum::<usize>();
        stats.accounted_bytes = p.bytes;
        stats.buffers = p.buffers();
    }
    stats
}

/// Exactly n initialized slots to overwrite, reusing the smallest allocation
/// with n <= capacity <= 2*n. No live K value can alias a retained vector.
pub fn take<T: Pooled>(n: usize) -> Vec<T> {
    if n >= MIN_LEN {
        if let Ok(mut p) = metrics::lock(pool()) {
            let shelf = T::shelf(&mut p);
            let best = shelf.bufs.iter().enumerate()
                .filter(|(_, v)| v.values.capacity() >= n && v.values.capacity() <= n.saturating_mul(2))
                .min_by_key(|(_, v)| v.values.capacity()).map(|(i, _)| i);
            if let Some(i) = best {
                // Keep the original swap-remove reuse order: shifting every
                // remaining entry can cycle through cold spare allocations.
                let mut values = shelf.bufs.swap_remove(i).values;
                p.bytes -= values.capacity() * size_of::<T>();
                drop(p);
                // A vector may have been truncated on its previous use. resize
                // safely restores initialized slots without growing its allocation.
                values.resize(n, T::default());
                metrics::event(0);
                return values;
            }
        }
    }
    metrics::event(1);
    vec![T::default(); n]
}

/// Transfer an unused vector into the pool, or free it when it cannot fit.
pub fn give<T: Pooled>(values: Vec<T>) {
    if values.capacity() < MIN_LEN { return; }
    let bytes = values.capacity() * size_of::<T>();
    if let Ok(mut p) = metrics::lock(pool()) {
        if bytes > p.max_bytes { return; }
        while p.buffers() >= MAX_BUFFERS || p.bytes > p.max_bytes - bytes {
            p.evict_oldest();
        }
        let age = p.age;
        p.age = p.age.saturating_add(1);
        p.bytes += bytes;
        T::shelf(&mut p).bufs.push(Saved { values, age });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_parsing_is_bounded() {
        assert_eq!(parse_budget(Some("0")), 0);
        assert_eq!(parse_budget(Some(" 2 ")), 2 << 20);
        assert_eq!(parse_budget(Some("invalid")), MAX_BYTES_DEFAULT);
        assert_eq!(parse_budget(Some(&usize::MAX.to_string())), MAX_BYTES_DEFAULT);
    }
}
