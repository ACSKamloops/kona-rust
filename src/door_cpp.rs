// The cxx bridge below writes the unsafe glue between Rust and C++ for us (cxx checks both sides when it builds).
// This is the ONE place in the port where unsafe is allowed, and only with `--features cpp`.
#![allow(unsafe_code)]
// Made by Claude
//! door_cpp.rs - library 2 of the `_sys` door: the C++ game library (cpp/kgame_core.cpp) built into kona.exe.
//! Values are copied once each way. A C++ exception comes back as a Kona error.

use crate::door::{self, Arg, Out};
use crate::errors::KonaError;
use crate::types::K;

#[cxx::bridge(namespace = "kgame")]
mod ffi {
    unsafe extern "C++" {
        include!("kona/cpp/kgame_cxx.h");

        type Args;
        type Outs;

        fn new_args() -> UniquePtr<Args>;
        fn add_floats(a: Pin<&mut Args>, v: &[f64], atom: bool);
        fn add_ints(a: Pin<&mut Args>, v: &[i64], atom: bool);
        fn call(fnum: i32, a: &Args) -> Result<UniquePtr<Outs>>;
        fn out_count(o: &Outs) -> usize;
        fn out_is_ints(o: &Outs, k: usize) -> bool;
        fn out_is_atom(o: &Outs, k: usize) -> bool;
        fn out_floats(o: &Outs, k: usize) -> &[f64];
        fn out_ints(o: &Outs, k: usize) -> &[i64];
    }
}

pub fn call(f: i64, args: &K) -> Result<K, KonaError> {
    let fnum = door::fn_number(f)?;
    let mut a = ffi::new_args();
    if a.is_null() {
        return Err(KonaError::System("C++ door: no memory".to_string()));
    }
    for v in door::split(args)? {
        match v {
            Arg::Floats(s, atom) => ffi::add_floats(a.pin_mut(), s, atom),
            Arg::Ints(s, atom) => ffi::add_ints(a.pin_mut(), s, atom),
        }
    }
    let o = ffi::call(fnum, &a).map_err(|e| KonaError::System(e.what().to_string()))?;
    if o.is_null() {
        return Err(KonaError::System("C++ door: no result".to_string()));
    }
    let outs = (0..ffi::out_count(&o))
        .map(|k| {
            let atom = ffi::out_is_atom(&o, k);
            if ffi::out_is_ints(&o, k) {
                Out::Ints(ffi::out_ints(&o, k).to_vec(), atom)
            } else {
                Out::Floats(ffi::out_floats(&o, k).to_vec(), atom)
            }
        })
        .collect();
    Ok(door::to_k(outs))
}
// Made by Claude
