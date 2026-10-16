#![forbid(unsafe_code)]
// Made by Claude
//! pool.rs - reuse big number lists instead of asking Windows for fresh memory on every operation.
//!
//! Every K operation on a big list makes a new list. Fresh memory from the OS has to be faulted in and zeroed
//! page by page; measured 2026-10-15 (1M adds, one thread): fresh list 2.10 ms vs reused list 1.36 ms. C Kona
//! avoids that with its own memory pools; this is the same idea for the Rust port, in safe Rust:
//!   - when the last holder of a big FloatList / IntList lets go (Drop for K in types.rs, using Arc::get_mut,
//!     which only succeeds when nobody else holds it), its Vec goes on the shelf here;
//!   - lanes.rs takes its output lists from the shelf (best fit) and overwrites every slot.
//! Only lists of 4,096+ numbers are kept (was 65,536 - lowering it took a 10k-particle 3D frame from 1.06 to
//! 0.61 ms and 30k from 2.35 to 1.45 ms), at most 48 lists and 1 GB in total; anything else is freed as usual.

use std::sync::Mutex;

pub const MIN_LEN: usize = 1 << 12;
const MAX_BUFFERS: usize = 48;
const MAX_BYTES_DEFAULT: usize = 1 << 30;

/// most spare memory the shelf may hold: KONA_POOL_MB if set, else 1 GB
fn max_bytes() -> usize {
    static MB: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *MB.get_or_init(|| {
        std::env::var("KONA_POOL_MB").ok().and_then(|v| v.trim().parse::<usize>().ok()).map(|m| m << 20).unwrap_or(MAX_BYTES_DEFAULT)
    })
}

pub struct Shelf<T> {
    bufs: Vec<Vec<T>>,
    bytes: usize,
}

static F64: Mutex<Shelf<f64>> = Mutex::new(Shelf { bufs: Vec::new(), bytes: 0 });
static I64: Mutex<Shelf<i64>> = Mutex::new(Shelf { bufs: Vec::new(), bytes: 0 });

/// number types that have a shelf
pub trait Pooled: Copy + Default + Send + Sync + 'static {
    fn shelf() -> &'static Mutex<Shelf<Self>>;
}
impl Pooled for f64 {
    fn shelf() -> &'static Mutex<Shelf<f64>> {
        &F64
    }
}
impl Pooled for i64 {
    fn shelf() -> &'static Mutex<Shelf<i64>> {
        &I64
    }
}

/// A list of exactly n numbers to OVERWRITE: from the shelf when one fits (its old contents are still in it -
/// the caller must write every slot), else a fresh zeroed one.
pub fn take<T: Pooled>(n: usize) -> Vec<T> {
    if n >= MIN_LEN {
        if let Ok(mut s) = T::shelf().lock() {
            // smallest spare list that is long enough, but not more than twice as long (no big waste)
            let best = s
                .bufs
                .iter()
                .enumerate()
                .filter(|(_, b)| b.len() >= n && b.len() <= 2 * n)
                .min_by_key(|(_, b)| b.len())
                .map(|(i, _)| i);
            if let Some(i) = best {
                let mut v = s.bufs.swap_remove(i);
                s.bytes -= v.len() * std::mem::size_of::<T>();
                v.truncate(n);
                return v;
            }
        }
    }
    vec![T::default(); n]
}

/// Put a list that nobody uses any more on the shelf (or free it if it's small or the shelf is full).
pub fn give<T: Pooled>(v: Vec<T>) {
    if v.len() < MIN_LEN {
        return;
    }
    let bytes = v.len() * std::mem::size_of::<T>();
    if bytes > max_bytes() {
        return;
    }
    if let Ok(mut s) = T::shelf().lock() {
        // shelf full: drop the OLDEST spare lists to make room, so the shelf always holds what the program is
        // using now. (Without this, 48 lists left over from a smaller run - e.g. 100k after a 100k game -
        // blocked every 1M list from being kept, and 1M frames ran at fresh-memory speed: 303 vs 195 ms.)
        while !s.bufs.is_empty() && (s.bufs.len() >= MAX_BUFFERS || s.bytes + bytes > max_bytes()) {
            let old = s.bufs.remove(0);
            s.bytes -= old.len() * std::mem::size_of::<T>();
        }
        s.bytes += bytes;
        s.bufs.push(v);
    }
}
// Made by Claude
