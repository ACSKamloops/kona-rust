#![forbid(unsafe_code)]
// Made by Claude
//! kwire.rs - C Kona's own byte format for -i messages, so C Kona tools (kipc.py, the training watcher,
//! the bullet hell) can talk to this Kona unchanged. Every layout below was read off real C Kona `_bd`
//! output on 2026-10-15 (`_ic _bd x` for each kind of value):
//!
//!   header (16 bytes): 1 (little-endian), 6 zero bytes, d (0 one-way / 1 needs an answer / 2 answer),
//!                      then the body size as an 8-byte int
//!   int 1,v | float 2,v | char 3,c + 7 zero bytes | symbol 4,name\0 | nil 6,0     (each code 8 bytes)
//!   ints -1,n,v.. | floats -2,n,v.. | chars -3,n,bytes\0 padded to 8 | symbols -4,n,name\0name\0.. padded
//!   general list 0,n,items.. | dictionary 5,n, then each entry is the 3-item list (name; value; nil)
//!
//! Symbols copy a C Kona quirk on purpose: it WRITES a symbol as max(8, len+1) bytes (no padding past
//! that) but READS one as len+1 padded to 8. Doing the same keeps both sides reading each other right.
//! An answer to a d=1 message is (0; result), or (1; "error text") when the handler failed.

use crate::errors::KonaError;
use crate::types::{K, KData};
use std::sync::Arc;

fn pad8(n: usize) -> usize {
    n.div_ceil(8) * 8
}

fn put_i(out: &mut Vec<u8>, v: i64) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// whole message: header + body
pub fn message(d: u8, k: &K) -> Result<Vec<u8>, KonaError> {
    let mut body = Vec::new();
    encode(k, &mut body)?;
    let mut out = Vec::with_capacity(16 + body.len());
    out.push(1);
    out.extend_from_slice(&[0; 6]);
    out.push(d);
    put_i(&mut out, body.len() as i64);
    out.extend_from_slice(&body);
    Ok(out)
}

pub fn encode(k: &K, out: &mut Vec<u8>) -> Result<(), KonaError> {
    match &**k {
        KData::IntAtom(v) => {
            put_i(out, 1);
            put_i(out, *v);
        }
        KData::FloatAtom(v) => {
            put_i(out, 2);
            out.extend_from_slice(&v.to_le_bytes());
        }
        KData::CharAtom(c) => {
            put_i(out, 3);
            let mut b = [0u8; 8];
            b[0] = *c as u8;
            out.extend_from_slice(&b);
        }
        KData::SymbolAtom(s) => {
            put_i(out, 4);
            let start = out.len();
            out.extend_from_slice(s.as_bytes());
            out.push(0);
            while out.len() - start < 8 {
                out.push(0); // max(8, len+1): short names padded to 8, long ones not padded (as C Kona)
            }
        }
        KData::Nil => {
            put_i(out, 6);
            put_i(out, 0);
        }
        KData::IntList(v) => {
            put_i(out, -1);
            put_i(out, v.len() as i64);
            for x in v {
                put_i(out, *x);
            }
        }
        KData::FloatList(v) => {
            put_i(out, -2);
            put_i(out, v.len() as i64);
            for x in v {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        KData::CharList(v) => {
            put_i(out, -3);
            put_i(out, v.len() as i64);
            let start = out.len();
            out.extend_from_slice(v);
            out.push(0);
            out.resize(start + pad8(v.len() + 1), 0);
        }
        KData::SymbolList(v) => {
            put_i(out, -4);
            put_i(out, v.len() as i64);
            let start = out.len();
            for s in v {
                out.extend_from_slice(s.as_bytes());
                out.push(0);
            }
            let used = out.len() - start;
            out.resize(start + pad8(used), 0);
        }
        KData::GeneralList(v) => {
            put_i(out, 0);
            put_i(out, v.len() as i64);
            for x in v {
                encode(x, out)?;
            }
        }
        KData::Dictionary { keys, values } => {
            let ks = crate::verbs::math::to_vec_of_atoms(keys)?;
            let vs = crate::verbs::math::to_vec_of_atoms(values)?;
            put_i(out, 5);
            put_i(out, ks.len() as i64);
            for (kk, vv) in ks.iter().zip(vs.iter()) {
                encode(&K::general_list(vec![kk.clone(), vv.clone(), K::nil()]), out)?;
            }
        }
        _ => return Err(KonaError::Type), // functions and hash lists don't travel
    }
    Ok(())
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], KonaError> {
        let end = self.i.checked_add(n).filter(|&e| e <= self.b.len()).ok_or_else(|| {
            KonaError::Parse("message ends early".to_string())
        })?;
        let s = &self.b[self.i..end];
        self.i = end;
        Ok(s)
    }
    fn int(&mut self) -> Result<i64, KonaError> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().map_err(|_| KonaError::Parse("int".into()))?))
    }
    fn count(&mut self) -> Result<usize, KonaError> {
        let n = self.int()?;
        // a count can't be bigger than the bytes left - stops a bad message from asking for huge memory
        if n < 0 || n as usize > self.b.len() {
            return Err(KonaError::Parse("bad count".to_string()));
        }
        Ok(n as usize)
    }
    fn name(&mut self) -> Result<Arc<str>, KonaError> {
        let rest = &self.b[self.i..];
        let z = rest.iter().position(|&c| c == 0).ok_or_else(|| KonaError::Parse("symbol".into()))?;
        let s = String::from_utf8_lossy(&rest[..z]).into_owned();
        self.take(pad8(z + 1))?; // C Kona reads symbols padded to 8
        Ok(Arc::from(s))
    }
    fn value(&mut self, depth: usize) -> Result<K, KonaError> {
        if depth > 64 {
            return Err(KonaError::Parse("nested too deep".to_string()));
        }
        let t = self.int()?;
        Ok(match t {
            1 => K::int(self.int()?),
            2 => K::float(f64::from_le_bytes(self.take(8)?.try_into().map_err(|_| KonaError::Parse("float".into()))?)),
            3 => K::char(self.take(8)?[0] as char),
            4 => K::symbol(self.name()?),
            6 => {
                self.int()?;
                K::nil()
            }
            -1 => {
                let n = self.count()?;
                let mut v = Vec::with_capacity(n.min(self.b.len() / 8));
                for _ in 0..n {
                    v.push(self.int()?);
                }
                K::int_list(v)
            }
            -2 => {
                let n = self.count()?;
                let raw = self.take(n * 8)?;
                K::float_list_raw(raw.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap_or([0; 8]))).collect())
            }
            -3 => {
                let n = self.count()?;
                let raw = self.take(pad8(n + 1))?;
                K::char_list(raw[..n].to_vec())
            }
            -4 => {
                let n = self.count()?;
                let start = self.i;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    let rest = &self.b[self.i..];
                    let z = rest.iter().position(|&c| c == 0).ok_or_else(|| KonaError::Parse("symbols".into()))?;
                    v.push(Arc::from(String::from_utf8_lossy(&rest[..z]).into_owned()));
                    self.take(z + 1)?;
                }
                let used = self.i - start;
                self.take(pad8(used) - used)?;
                K::symbol_list(v)
            }
            0 => {
                let n = self.count()?;
                let mut v = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    v.push(self.value(depth + 1)?);
                }
                crate::verbs::math::normalize_list(v)
            }
            5 => {
                let n = self.count()?;
                let (mut ks, mut vs) = (Vec::with_capacity(n.min(1024)), Vec::with_capacity(n.min(1024)));
                for _ in 0..n {
                    let e = self.value(depth + 1)?;
                    let parts = crate::verbs::math::to_vec_of_atoms(&e)?;
                    match (parts.first().map(|p| &**p), parts.get(1)) {
                        (Some(KData::SymbolAtom(s)), Some(val)) => {
                            ks.push(s.clone());
                            vs.push(val.clone());
                        }
                        _ => return Err(KonaError::Parse("dictionary entry".to_string())),
                    }
                }
                K::dictionary(K::symbol_list(ks), K::general_list(vs))
            }
            _ => return Err(KonaError::Parse(format!("unknown type {}", t))),
        })
    }
}

pub fn decode(body: &[u8]) -> Result<K, KonaError> {
    Reader { b: body, i: 0 }.value(0)
}

/// a C Kona header starts 1,0,0,0,0 - this port's own format starts with its message type and a non-zero
/// payload length, so the first 5 bytes tell the two apart
pub fn is_c_header(first5: &[u8; 5]) -> bool {
    first5[0] == 1 && first5[1..5].iter().all(|&b| b == 0)
}

/// short error text like C Kona's ("type", "length", ...)
pub fn error_text(e: &KonaError) -> String {
    match e {
        KonaError::Value(m) | KonaError::Parse(m) | KonaError::System(m) | KonaError::File(m) => m.clone(),
        other => format!("{:?}", other).to_lowercase(),
    }
}
// Made by Claude
