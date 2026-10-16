#![deny(unsafe_code)]
// Made by Claude
//! kona_ue - Kona inside Unreal. One Session = one Kona with its own globals; Unreal loads a .k script into it
//! once, then every frame sets a few globals, runs one line of K, and reads the answer lists back.

mod bridge;

use kona::errors::KonaError;
use kona::types::{K, KData};
use kona::verbs::dict::KTree;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Session {
    tree: Arc<KTree>,
    env: HashMap<String, K>,
    last: K,
    err: String,
}

impl Session {
    fn new() -> Self {
        Session { tree: Arc::new(KTree::new()), env: HashMap::new(), last: K::nil(), err: String::new() }
    }

    fn ok(&mut self, r: Result<K, KonaError>) -> bool {
        match r {
            Ok(k) => {
                self.last = k;
                true
            }
            Err(e) => {
                self.err = e.to_string();
                false
            }
        }
    }

    fn load(&mut self, path: &str) -> bool {
        let r = kona::load_and_run_script(path, &self.tree).map(|_| K::nil());
        self.ok(r)
    }

    fn run(&mut self, code: &str) -> bool {
        let r = kona::parser::parse(code).and_then(|w| kona::eval::eval_words(&w, &mut self.env, &self.tree));
        self.ok(r)
    }

    fn result_int(&self) -> i64 {
        match &*self.last {
            KData::IntAtom(i) => *i,
            _ => -1,
        }
    }

    fn result_text(&self) -> String {
        kona::io::format_k(&self.last)
    }

    fn error(&self) -> String {
        self.err.clone()
    }

    fn set_float(&mut self, name: &str, v: f64) {
        self.tree.assign(name.to_string(), K::float(v));
    }

    fn set_floats(&mut self, name: &str, v: &[f64]) {
        self.tree.assign(name.to_string(), K::float_list(v.to_vec()));
    }

    fn set_ints(&mut self, name: &str, v: &[i64]) {
        self.tree.assign(name.to_string(), K::int_list(v.to_vec()));
    }

    fn get_floats(&self, name: &str, out: &mut [f64]) -> i64 {
        match self.tree.lookup(name).as_deref() {
            Some(KData::FloatList(v)) => {
                let n = v.len().min(out.len());
                out[..n].copy_from_slice(&v[..n]);
                n as i64
            }
            _ => -1,
        }
    }

    fn get_ints(&self, name: &str, out: &mut [i64]) -> i64 {
        match self.tree.lookup(name).as_deref() {
            Some(KData::IntList(v)) => {
                let n = v.len().min(out.len());
                out[..n].copy_from_slice(&v[..n]);
                n as i64
            }
            _ => -1,
        }
    }

    fn get_int(&self, name: &str) -> i64 {
        match self.tree.lookup(name).as_deref() {
            Some(KData::IntAtom(i)) => *i,
            _ => -1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globals_round_trip() {
        let mut s = Session::new();
        assert!(s.run("a: 1.0 2.0 3.0"));
        s.set_floats("b", &[10.0, 20.0, 30.0]);
        assert!(s.run("c:: a+b"), "{}", s.err);
        let mut out = [0.0; 3];
        assert_eq!(s.get_floats("c", &mut out), 3);
        assert_eq!(out, [11.0, 22.0, 33.0]);
        assert!(s.run("+/!5"));
        assert_eq!(s.result_int(), 10);
    }
}
// Made by Claude
