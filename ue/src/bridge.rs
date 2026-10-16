// The cxx bridge below writes the unsafe glue between Unreal's C++ and Rust for us (cxx checks both sides when it
// builds). Like door_cpp.rs in the port, this is the ONE place in kona_ue where unsafe is allowed.
#![allow(unsafe_code)]
// Made by Claude
//! bridge.rs - what Unreal's C++ can call. Everything is plain values: text, numbers, and number lists that
//! Kona copies straight into Unreal's own arrays (no extra lists made on the Unreal side).

use crate::Session;

#[cxx::bridge(namespace = "kona")]
mod ffi {
    extern "Rust" {
        type Session;

        /// a fresh Kona with its own globals
        fn new_session() -> Box<Session>;
        /// \l a .k script (its globals and functions stay in this session)
        fn load(self: &mut Session, path: &str) -> bool;
        /// run K text, keep the result (true = it ran, false = see error())
        fn run(self: &mut Session, code: &str) -> bool;
        /// the last result as an int (when it was one) / as K text
        fn result_int(self: &Session) -> i64;
        fn result_text(self: &Session) -> String;
        /// what went wrong last
        fn error(self: &Session) -> String;
        /// set a global to a float atom / float list / int list
        fn set_float(self: &mut Session, name: &str, v: f64);
        fn set_floats(self: &mut Session, name: &str, v: &[f64]);
        fn set_ints(self: &mut Session, name: &str, v: &[i64]);
        /// copy a global float list / int list into out; returns how many, or -1 (missing or wrong type)
        fn get_floats(self: &Session, name: &str, out: &mut [f64]) -> i64;
        fn get_ints(self: &Session, name: &str, out: &mut [i64]) -> i64;
        /// a global int atom (or -1)
        fn get_int(self: &Session, name: &str) -> i64;
    }
}

fn new_session() -> Box<Session> {
    Box::new(Session::new())
}
// Made by Claude
