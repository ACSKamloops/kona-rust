#![forbid(unsafe_code)]
// Made by Claude
//! door_net.rs - library 3 of the `_sys` door: the C++ game library running next door as kgame_server.exe.
//! Plain std sockets, no unsafe. One connection kept open; address 127.0.0.1:5990 or the KONA_DOOR3 setting.
//! Message layout is written down in cpp/kgame_server.cpp. If the other program dies, Kona just gets an error
//! and reconnects on the next call.

use crate::door::{self, Arg, Out};
use crate::errors::KonaError;
use crate::types::K;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;

static CONN: Mutex<Option<TcpStream>> = Mutex::new(None);
const MAX_ITEMS: i64 = 1 << 31;

fn addr() -> String {
    std::env::var("KONA_DOOR3").unwrap_or_else(|_| "127.0.0.1:5990".to_string())
}

pub fn call(f: i64, args: &K) -> Result<K, KonaError> {
    let fnum = door::fn_number(f)?;
    let vals = door::split(args)?;
    let size: usize = vals
        .iter()
        .map(|v| 12 + 8 * match v {
            Arg::Floats(s, _) => s.len(),
            Arg::Ints(s, _) => s.len(),
        })
        .sum();
    let mut msg = Vec::with_capacity(8 + size);
    msg.extend_from_slice(&fnum.to_le_bytes());
    msg.extend_from_slice(&(vals.len() as i32).to_le_bytes());
    for v in &vals {
        match v {
            Arg::Floats(s, atom) => {
                msg.extend_from_slice(&(if *atom { 2i32 } else { 0 }).to_le_bytes());
                msg.extend_from_slice(&(s.len() as i64).to_le_bytes());
                for x in s.iter() {
                    msg.extend_from_slice(&x.to_le_bytes());
                }
            }
            Arg::Ints(s, atom) => {
                msg.extend_from_slice(&(if *atom { 3i32 } else { 1 }).to_le_bytes());
                msg.extend_from_slice(&(s.len() as i64).to_le_bytes());
                for x in s.iter() {
                    msg.extend_from_slice(&x.to_le_bytes());
                }
            }
        }
    }
    let mut conn = CONN.lock().map_err(|_| KonaError::System("door 3 lock poisoned".to_string()))?;
    if conn.is_none() {
        let s = TcpStream::connect(addr())
            .map_err(|e| KonaError::System(format!("next-door C++ not answering at {}: {e}", addr())))?;
        s.set_nodelay(true).map_err(net)?;
        *conn = Some(s);
    }
    let Some(s) = conn.as_mut() else {
        return Err(KonaError::System("door 3 not connected".to_string()));
    };
    match exchange(s, &msg) {
        Ok(Ok(outs)) => Ok(door::to_k(outs)),
        Ok(Err(text)) => Err(KonaError::System(text)),
        Err(e) => {
            *conn = None; // broken connection: drop it, the next call reconnects
            Err(e)
        }
    }
}

fn net(e: std::io::Error) -> KonaError {
    KonaError::System(format!("next-door C++: {e}"))
}

fn read_i32(s: &mut TcpStream) -> Result<i32, KonaError> {
    let mut b = [0u8; 4];
    s.read_exact(&mut b).map_err(net)?;
    Ok(i32::from_le_bytes(b))
}

fn read_i64(s: &mut TcpStream) -> Result<i64, KonaError> {
    let mut b = [0u8; 8];
    s.read_exact(&mut b).map_err(net)?;
    Ok(i64::from_le_bytes(b))
}

fn read_n(s: &mut TcpStream, n: i64) -> Result<Vec<u8>, KonaError> {
    if !(0..MAX_ITEMS * 8).contains(&n) {
        return Err(KonaError::System("next-door C++ sent a bad length".to_string()));
    }
    let mut b = vec![0u8; n as usize];
    s.read_exact(&mut b).map_err(net)?;
    Ok(b)
}

/// Ok(Ok(results)), Ok(Err(the C++ library's error text)), or Err(the connection broke)
fn exchange(s: &mut TcpStream, msg: &[u8]) -> Result<Result<Vec<Out>, String>, KonaError> {
    s.write_all(msg).map_err(net)?;
    if read_i32(s)? != 0 {
        let n = read_i64(s)?;
        let text = read_n(s, n)?;
        return Ok(Err(String::from_utf8_lossy(&text).into_owned()));
    }
    let count = read_i32(s)?;
    let mut outs = Vec::with_capacity(count.max(0) as usize);
    for _ in 0..count {
        let tag = read_i32(s)?;
        let n = read_i64(s)?;
        if !(0..MAX_ITEMS).contains(&n) {
            return Err(KonaError::System("next-door C++ sent a bad length".to_string()));
        }
        let raw = read_n(s, n * 8)?;
        let atom = tag & 2 != 0;
        let words = raw.chunks_exact(8).map(|c| {
            let mut w = [0u8; 8];
            w.copy_from_slice(c);
            w
        });
        outs.push(if tag & 1 != 0 {
            Out::Ints(words.map(i64::from_le_bytes).collect(), atom)
        } else {
            Out::Floats(words.map(f64::from_le_bytes).collect(), atom)
        });
    }
    Ok(Ok(outs))
}
// Made by Claude
