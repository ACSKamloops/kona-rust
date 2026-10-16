#![forbid(unsafe_code)]
// Made by Claude
//! door.rs - `_sys (lib; fn; args)`: one door out of Kona, the way Martin Richards' BCPL does it with
//! `sys(Sys_gl, fno, a1, a2, ...)`. A library number, a function number, plain values. Kona can only reach what
//! is compiled in behind the door - it never loads outside code by itself (that is what made `2:` unsafe).
//!
//!   library 1  GPU (gpu.rs)                 0 up   1 down   2 kernel   3 run   4 free   (same args as _gup ...)
//!   library 2  C++ game library, built into kona.exe (door_cpp.rs, needs `--features cpp`)
//!   library 3  the same C++ game library running next door as kgame_server.exe (door_net.rs, a 127.0.0.1 socket)
//!   library 4  Martin Richards' BCPL itself: his cintsys running as workers (door_bcpl.rs) - 0 numbers, 1 text
//!   library 0  the door itself: function 0 runs a list of door calls AT THE SAME TIME, one thread each
//!              (spread over the cores), and returns their results in order: _sys(0;0;((4;0;"kqueens 12");(2;1;v)))
//!
//! C++ functions (cpp/kgame_core.h): 0 echo  1 sum  2 move (pos;vel;dt)  3 world n  4 step (h;dt)  5 read h  6 free h
//! Args: a general list = one value per item, anything else = one value. Results: one value, or a general list.

use crate::errors::KonaError;
use crate::types::{K, KData};

pub const LIB_GPU: i64 = 1;
pub const LIB_CPP: i64 = 2;
pub const LIB_NEXT_DOOR: i64 = 3;
pub const LIB_BCPL: i64 = 4;
pub const LIB_DOOR: i64 = 0;

pub fn sys(x: &K) -> Result<K, KonaError> {
    let parts: Vec<K> = match &**x {
        KData::GeneralList(v) => v.clone(),
        KData::IntList(v) => v.iter().map(|&i| K::int(i)).collect(),
        _ => return Err(KonaError::Type),
    };
    if parts.len() != 3 {
        return Err(KonaError::Length);
    }
    let lib = int_of(&parts[0])?;
    let f = int_of(&parts[1])?;
    let args = &parts[2];
    match lib {
        LIB_GPU => match f {
            0 => crate::gpu::up(args),
            1 => crate::gpu::down(args),
            2 => crate::gpu::kernel(args),
            3 => crate::gpu::run(args),
            4 => crate::gpu::free(args),
            _ => Err(KonaError::Domain),
        },
        LIB_CPP => cpp_call(f, args),
        LIB_NEXT_DOOR => crate::door_net::call(f, args),
        LIB_BCPL => crate::door_bcpl::call(f, args),
        LIB_DOOR => match f {
            0 => all_at_once(args),
            _ => Err(KonaError::Domain),
        },
        _ => Err(KonaError::Domain),
    }
}

/// `_sys(0;0;calls)`: every call is its own (lib;fn;args); they run on separate threads, at most one per core
/// at a time, and the results come back in the same order. The first error wins.
fn all_at_once(calls: &K) -> Result<K, KonaError> {
    let list: Vec<K> = match &**calls {
        KData::GeneralList(v) => v.clone(),
        _ => return Err(KonaError::Type),
    };
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut results: Vec<Option<Result<K, KonaError>>> = (0..list.len()).map(|_| None).collect();
    let slots: Vec<std::sync::Mutex<&mut Option<Result<K, KonaError>>>> =
        results.iter_mut().map(std::sync::Mutex::new).collect();
    std::thread::scope(|s| {
        for _ in 0..cores.min(list.len()) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if i >= list.len() {
                    break;
                }
                let r = if matches!(&*list[i], KData::GeneralList(v) if v.len() == 3 && int_of(&v[0]) == Ok(LIB_DOOR)) {
                    Err(KonaError::Domain) // no door-in-door
                } else {
                    sys(&list[i])
                };
                if let Ok(mut slot) = slots[i].lock() {
                    **slot = Some(r);
                }
            });
        }
    });
    drop(slots);
    let mut out = Vec::with_capacity(results.len());
    for r in results {
        out.push(r.unwrap_or(Err(KonaError::System("call did not run".to_string())))?);
    }
    Ok(K::general_list(out))
}

#[cfg(feature = "cpp")]
fn cpp_call(f: i64, args: &K) -> Result<K, KonaError> {
    crate::door_cpp::call(f, args)
}

#[cfg(not(feature = "cpp"))]
fn cpp_call(_: i64, _: &K) -> Result<K, KonaError> {
    Err(KonaError::System("C++ door not built in (cargo build --release --features cpp)".to_string()))
}

fn int_of(k: &K) -> Result<i64, KonaError> {
    match &**k {
        KData::IntAtom(i) => Ok(*i),
        _ => Err(KonaError::Type),
    }
}

/// one value going out through the door, borrowed straight from the K data (no copy here)
pub enum Arg<'a> {
    Floats(&'a [f64], bool),
    Ints(&'a [i64], bool),
}

fn arg_of(k: &K) -> Result<Arg<'_>, KonaError> {
    match &**k {
        KData::FloatList(v) => Ok(Arg::Floats(v, false)),
        KData::IntList(v) => Ok(Arg::Ints(v, false)),
        KData::FloatAtom(f) => Ok(Arg::Floats(std::slice::from_ref(f), true)),
        KData::IntAtom(i) => Ok(Arg::Ints(std::slice::from_ref(i), true)),
        _ => Err(KonaError::Type),
    }
}

pub fn split(args: &K) -> Result<Vec<Arg<'_>>, KonaError> {
    match &**args {
        KData::GeneralList(v) => v.iter().map(arg_of).collect(),
        _ => Ok(vec![arg_of(args)?]),
    }
}

pub fn fn_number(f: i64) -> Result<i32, KonaError> {
    i32::try_from(f).map_err(|_| KonaError::Domain)
}

/// one value coming back
pub enum Out {
    Floats(Vec<f64>, bool),
    Ints(Vec<i64>, bool),
}

pub fn to_k(mut outs: Vec<Out>) -> K {
    let one = |o: Out| match o {
        Out::Floats(v, true) if v.len() == 1 => K::float(v[0]),
        Out::Ints(v, true) if v.len() == 1 => K::int(v[0]),
        Out::Floats(v, _) => K::float_list(v),
        Out::Ints(v, _) => K::int_list(v),
    };
    if outs.len() == 1 {
        if let Some(o) = outs.pop() {
            return one(o);
        }
    }
    K::general_list(outs.into_iter().map(one).collect())
}
// Made by Claude
