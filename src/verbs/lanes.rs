#![forbid(unsafe_code)]
// Made by Claude
//! lanes.rs - fast path for arithmetic on flat number lists (the "width" work: one verb over a whole list).
//!
//! The general path (math.rs eval_dyadic) unpacks every element into its own K atom, applies the verb to
//! one pair at a time and packs the result back up - millions of small allocations for a 1M list. Here a
//! flat list stays a plain slice, the verb runs straight over it, and big lists are split across CPU cores
//! with scoped threads (std::thread::scope - safe Rust, no unsafe, no extra crates).
//!
//! Answers are the same as the general path: whole numbers use the same checked_*_k overflow rules,
//! ints meet floats through i2f, and < > = use the same tolerant compare fc. Anything not handled here
//! (int %, ^, !, chars, symbols, nested lists, length mismatches, empty lists) returns None and the
//! general path does it exactly as before.

use crate::errors::KonaError;
use crate::pool::{self, Pooled};
use crate::types::{fc, i2f, K, KData};
use std::sync::OnceLock;

/// When to use more than one core (measured 2026-10-15, Ryzen 5 3600, after the pool + straight loops):
/// std::thread::scope starts fresh OS threads every operation, so threads only pay when each one gets real
/// work. Heavy maths (_cos _sin _exp _log, map1): 4M cos 44 -> 9 ms on 12 threads, so split from 64k numbers,
/// 32k per thread. Cheap maths (+ - * % & | < > =, map2): RAM-bound - 4M add 5 vs 6 ms, and a 1M 3D frame got
/// SLOWER on 12 threads (185 -> 215-246 ms, ~100 operations = ~1200 thread starts) - so only from 2M numbers,
/// at least 512k per thread.
const PAR_MIN: usize = 1 << 16;
const PAR_CHUNK: usize = 1 << 15;
const CHEAP_PAR_MIN: usize = 1 << 21;
const CHEAP_PAR_CHUNK: usize = 1 << 19;

/// How many threads to use: KONA_THREADS if set (KONA_THREADS=1 turns parallelism off), else every core.
pub fn cores() -> usize {
    static CORES: OnceLock<usize> = OnceLock::new();
    *CORES.get_or_init(|| {
        std::env::var("KONA_THREADS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|&n| n >= 1)
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1))
    })
}

/// threads for heavy maths (map1)
fn workers(n: usize) -> usize {
    if n < PAR_MIN { 1 } else { cores().min(n / PAR_CHUNK).max(1) }
}

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name).ok().and_then(|v| v.trim().parse::<usize>().ok()).filter(|&n| n >= 1)
}

/// threads for cheap maths (map2). KONA_CHEAP_MIN / KONA_CHEAP_CHUNK (numbers) override the defaults, so the
/// thresholds can be measured on a machine without rebuilding.
fn cheap_workers(n: usize) -> usize {
    static CFG: OnceLock<(usize, usize)> = OnceLock::new();
    let (min, chunk) = *CFG.get_or_init(|| {
        (env_usize("KONA_CHEAP_MIN").unwrap_or(CHEAP_PAR_MIN), env_usize("KONA_CHEAP_CHUNK").unwrap_or(CHEAP_PAR_CHUNK))
    });
    if n < min { 1 } else { cores().min(n / chunk).max(1) }
}

/// One side of a dyadic verb: a single number used for every position, or a whole list.
#[derive(Clone, Copy)]
enum Lane<'a, T: Copy> {
    One(T),
    Many(&'a [T]),
}

impl<T: Copy> Lane<'_, T> {
    #[inline]
    fn at(&self, i: usize) -> T {
        match self {
            Lane::One(v) => *v,
            Lane::Many(s) => s[i],
        }
    }
}

/// out[j] = f(a[start+j], b[start+j]) for one chunk of the output. "One number or a list?" is answered ONCE
/// here, outside the loops, so each loop is a straight run over slices the compiler can turn into SIMD
/// (2026-10-15 check: asking it for every element cost 17% on a 1M add).
fn fill2<A: Copy, B: Copy, R: Copy>(out: &mut [R], start: usize, a: Lane<'_, A>, b: Lane<'_, B>, f: &impl Fn(A, B) -> R) {
    let end = start + out.len();
    match (a, b) {
        (Lane::Many(x), Lane::Many(y)) => {
            for ((o, &p), &q) in out.iter_mut().zip(&x[start..end]).zip(&y[start..end]) {
                *o = f(p, q);
            }
        }
        (Lane::Many(x), Lane::One(q)) => {
            for (o, &p) in out.iter_mut().zip(&x[start..end]) {
                *o = f(p, q);
            }
        }
        (Lane::One(p), Lane::Many(y)) => {
            for (o, &q) in out.iter_mut().zip(&y[start..end]) {
                *o = f(p, q);
            }
        }
        (Lane::One(p), Lane::One(q)) => out.fill(f(p, q)),
    }
}

/// n results of f over two sides. The output list comes from the pool (pool.rs: reused memory, measured
/// 2.10 -> 1.36 ms on a 1M add) and big lists are split one chunk per core with scoped threads
/// (chunks_mut gives every thread its own piece of the output - no locks, no unsafe).
fn map2<A, B, R, F>(n: usize, a: Lane<'_, A>, b: Lane<'_, B>, f: F) -> Vec<R>
where
    A: Copy + Send + Sync,
    B: Copy + Send + Sync,
    R: Pooled,
    F: Fn(A, B) -> R + Sync,
{
    let mut out = pool::take::<R>(n);
    let w = cheap_workers(n);
    if w == 1 {
        fill2(&mut out, 0, a, b, &f);
        return out;
    }
    let chunk = n.div_ceil(w);
    std::thread::scope(|s| {
        for (c, part) in out.chunks_mut(chunk).enumerate() {
            let f = &f;
            s.spawn(move || fill2(part, c * chunk, a, b, f));
        }
    });
    out
}

/// One-list version for the monadic maths (_cos _sin _exp _log ...): same pool, same chunks.
pub fn map1<A, R, F>(xs: &[A], f: F) -> Vec<R>
where
    A: Copy + Sync,
    R: Pooled,
    F: Fn(A) -> R + Sync,
{
    let n = xs.len();
    let mut out = pool::take::<R>(n);
    let run = |part: &mut [R], start: usize| {
        let len = part.len();
        for (o, &v) in part.iter_mut().zip(&xs[start..start + len]) {
            *o = f(v);
        }
    };
    let w = workers(n);
    if w == 1 {
        run(&mut out, 0);
        return out;
    }
    let chunk = n.div_ceil(w);
    std::thread::scope(|s| {
        for (c, part) in out.chunks_mut(chunk).enumerate() {
            let run = &run;
            s.spawn(move || run(part, c * chunk));
        }
    });
    out
}

/// A copy of a flat list into pooled memory (for amend: one copy, then the new values go in).
fn copied<T: Pooled>(t: &[T]) -> Vec<T> {
    let mut v = pool::take::<T>(t.len());
    v.copy_from_slice(t);
    v
}

#[derive(Clone, Copy)]
enum Num<'a> {
    I(Lane<'a, i64>),
    F(Lane<'a, f64>),
}

/// (side, Some(length) for a list / None for an atom), only for plain numbers
fn num(k: &K) -> Option<(Num<'_>, Option<usize>)> {
    match &**k {
        KData::IntAtom(v) => Some((Num::I(Lane::One(*v)), None)),
        KData::FloatAtom(v) => Some((Num::F(Lane::One(*v)), None)),
        KData::IntList(v) => Some((Num::I(Lane::Many(v)), Some(v.len()))),
        KData::FloatList(v) => Some((Num::F(Lane::Many(v)), Some(v.len()))),
        _ => None,
    }
}

/// Run `$e` with $a, $b as floats over both sides. Each side may hold ints or floats (ints become floats
/// through i2f, like the general path); macro_rules! writes out the four type combinations so every one
/// gets its own straight, typed loop instead of a per-element type check.
macro_rules! as_floats {
    ($n:expr, $x:expr, $y:expr, |$a:ident, $b:ident| $e:expr) => {
        match ($x, $y) {
            (Num::F(p), Num::F(q)) => map2($n, p, q, |$a: f64, $b: f64| $e),
            (Num::I(p), Num::F(q)) => map2($n, p, q, |a0: i64, $b: f64| {
                let $a = i2f(a0);
                $e
            }),
            (Num::F(p), Num::I(q)) => map2($n, p, q, |$a: f64, b0: i64| {
                let $b = i2f(b0);
                $e
            }),
            (Num::I(p), Num::I(q)) => map2($n, p, q, |a0: i64, b0: i64| {
                let $a = i2f(a0);
                let $b = i2f(b0);
                $e
            }),
        }
    };
}

/// < > = as 0/1 ints: the tolerant compare fc on the float values, same as compare_numeric
fn compares(verb: &str, n: usize, x: Num<'_>, y: Num<'_>) -> Option<K> {
    let r: Vec<i64> = match verb {
        "<" => as_floats!(n, x, y, |a, b| (fc(a, b) < 0) as i64),
        ">" => as_floats!(n, x, y, |a, b| (fc(a, b) > 0) as i64),
        "=" => as_floats!(n, x, y, |a, b| (fc(a, b) == 0) as i64),
        _ => return None,
    };
    Some(K::int_list(r))
}

/// Some(answer) if this verb on these two values can be done on flat lanes, None to use the general path.
pub fn dyadic(a: &K, b: &K, verb: &str) -> Result<Option<K>, KonaError> {
    if verb == "!" {
        return modulo(a, b); // only list ! number; everything else (rotate etc.) stays on the general path
    }
    let (Some((x, xn)), Some((y, yn))) = (num(a), num(b)) else {
        return Ok(None);
    };
    let n = match (xn, yn) {
        (Some(p), Some(q)) if p == q => p,
        (Some(p), None) | (None, Some(p)) => p,
        _ => return Ok(None), // two atoms, or lengths that differ: general path
    };
    if n == 0 {
        return Ok(None); // empty lists keep their special result types in the general path
    }
    use crate::verbs::math::{checked_add_k, checked_mul_k, checked_sub_k};
    let out = match (x, y) {
        (Num::I(p), Num::I(q)) => match verb {
            "+" => Some(K::int_list(map2(n, p, q, checked_add_k))),
            "-" => Some(K::int_list(map2(n, p, q, checked_sub_k))),
            "*" => Some(K::int_list(map2(n, p, q, checked_mul_k))),
            "&" => Some(K::int_list(map2(n, p, q, |a: i64, b: i64| a.min(b)))),
            "|" => Some(K::int_list(map2(n, p, q, |a: i64, b: i64| a.max(b)))),
            _ => compares(verb, n, x, y),
        },
        _ => match verb {
            "+" => Some(K::float_list_raw(as_floats!(n, x, y, |a, b| a + b))),
            "-" => Some(K::float_list_raw(as_floats!(n, x, y, |a, b| a - b))),
            "*" => Some(K::float_list_raw(as_floats!(n, x, y, |a, b| a * b))),
            "%" => Some(K::float_list_raw(as_floats!(n, x, y, |a, b| a / b))),
            "&" => Some(K::float_list_raw(as_floats!(n, x, y, |a, b| a.min(b)))),
            "|" => Some(K::float_list_raw(as_floats!(n, x, y, |a, b| a.max(b)))),
            _ => compares(verb, n, x, y),
        },
    };
    Ok(out)
}

/// list@i for a flat number list and int index/indices: pick straight out of the list instead of unpacking
/// all of it first. Same rules as list::at - negative counts from the end, out of range = Index error.
/// Empty index lists go to the general path (it decides the empty result's type).
pub fn index(x: &K, y: &K) -> Result<Option<K>, KonaError> {
    fn pos(i: i64, len: usize) -> Result<usize, KonaError> {
        let w = if i < 0 { i + len as i64 } else { i };
        if w < 0 || w >= len as i64 { Err(KonaError::Index) } else { Ok(w as usize) }
    }
    match (&**x, &**y) {
        (KData::FloatList(v), KData::IntAtom(i)) => Ok(Some(K::float(v[pos(*i, v.len())?]))),
        (KData::IntList(v), KData::IntAtom(i)) => Ok(Some(K::int(v[pos(*i, v.len())?]))),
        (KData::FloatList(v), KData::IntList(ix)) if !ix.is_empty() => {
            let out: Result<Vec<f64>, KonaError> = ix.iter().map(|&i| Ok(v[pos(i, v.len())?])).collect();
            Ok(Some(K::float_list_raw(out?)))
        }
        (KData::IntList(v), KData::IntList(ix)) if !ix.is_empty() => {
            let out: Result<Vec<i64>, KonaError> = ix.iter().map(|&i| Ok(v[pos(i, v.len())?])).collect();
            Ok(Some(K::int_list(out?)))
        }
        _ => Ok(None),
    }
}

/// list!n (remainder) for a flat number list and one number, with the general path's k_mod rules
fn modulo(x: &K, y: &K) -> Result<Option<K>, KonaError> {
    use crate::verbs::math::{k_mod_float, k_mod_int};
    Ok(match (&**x, &**y) {
        (KData::IntList(v), KData::IntAtom(m)) if !v.is_empty() => {
            Some(K::int_list(v.iter().map(|&a| k_mod_int(a, *m)).collect::<Result<_, _>>()?))
        }
        (KData::FloatList(v), KData::FloatAtom(m)) if !v.is_empty() => {
            Some(K::float_list_raw(v.iter().map(|&a| k_mod_float(a, *m)).collect::<Result<_, _>>()?))
        }
        _ => None,
    })
}

/// f/ x for + * & | over a flat number list: one pass, left to right, same order as the general fold
/// (so float sums round exactly the same way). None = not a case handled here.
pub fn fold(verb: &str, x: &K) -> Option<K> {
    use crate::verbs::math::{checked_add_k, checked_mul_k};
    match &**x {
        KData::FloatList(v) if v.len() >= 2 => {
            let (first, rest) = (v[0], &v[1..]);
            let r = match verb {
                "+" => rest.iter().fold(first, |a, &b| a + b),
                "*" => rest.iter().fold(first, |a, &b| a * b),
                "&" => rest.iter().fold(first, |a, &b| a.min(b)),
                "|" => rest.iter().fold(first, |a, &b| a.max(b)),
                _ => return None,
            };
            Some(K::float(r))
        }
        KData::IntList(v) if v.len() >= 2 => {
            let (first, rest) = (v[0], &v[1..]);
            let r = match verb {
                "+" => rest.iter().fold(first, |a, &b| checked_add_k(a, b)),
                "*" => rest.iter().fold(first, |a, &b| checked_mul_k(a, b)),
                "&" => rest.iter().fold(first, |a, &b| a.min(b)),
                "|" => rest.iter().fold(first, |a, &b| a.max(b)),
                _ => return None,
            };
            Some(K::int(r))
        }
        _ => None,
    }
}

/// @[a; i; :; v] on a flat number list: copy the list once and drop the new values in. Same type only
/// (floats into a float list, ints into an int list) - anything else, or v of a different length than i,
/// goes to the general path. Indexes wrap from the end like the general path; out of range = Index error.
pub fn amend_assign(a: &K, i: &K, v: &K) -> Result<Option<K>, KonaError> {
    fn put<T: Copy>(dst: &mut [T], idx: &[i64], vals: Lane<'_, T>) -> Result<(), KonaError> {
        let len = dst.len() as i64;
        for (p, &ix) in idx.iter().enumerate() {
            let w = if ix < 0 { ix + len } else { ix };
            if w < 0 || w >= len {
                return Err(KonaError::Index);
            }
            dst[w as usize] = vals.at(p);
        }
        Ok(())
    }
    let one;
    let idx: &[i64] = match &**i {
        KData::IntList(ix) => ix,
        KData::IntAtom(ix) => {
            one = [*ix];
            &one
        }
        _ => return Ok(None),
    };
    let fits = |n: usize| n == idx.len();
    match (&**a, &**v) {
        (KData::FloatList(t), KData::FloatAtom(x)) => {
            let mut out = copied(t);
            put(&mut out, idx, Lane::One(*x))?;
            Ok(Some(K::float_list_raw(out)))
        }
        (KData::FloatList(t), KData::FloatList(xs)) if fits(xs.len()) && i.is_list() => {
            let mut out = copied(t);
            put(&mut out, idx, Lane::Many(xs))?;
            Ok(Some(K::float_list_raw(out)))
        }
        (KData::IntList(t), KData::IntAtom(x)) => {
            let mut out = copied(t);
            put(&mut out, idx, Lane::One(*x))?;
            Ok(Some(K::int_list(out)))
        }
        (KData::IntList(t), KData::IntList(xs)) if fits(xs.len()) && i.is_list() => {
            let mut out = copied(t);
            put(&mut out, idx, Lane::Many(xs))?;
            Ok(Some(K::int_list(out)))
        }
        _ => Ok(None),
    }
}
// Made by Claude
