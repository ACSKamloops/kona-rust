#![forbid(unsafe_code)]

use crate::errors::KonaError;
use crate::eval::apply_func;
use crate::types::{K, KData, KFunc};
use crate::verbs::dict::KTree;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

pub static PRINT_PRECISION: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(7);

/// A client that sends part of a request and stops used to hold a thread (and its pre-allocated
/// body) forever. HTTP requests are short-lived; an IPC client (the game) keeps one connection
/// open and may be idle for any length of time, so IPC only times out in the middle of a message.
const HTTP_READ_TIMEOUT_SECS: u64 = 30;
const IPC_MESSAGE_TIMEOUT_SECS: u64 = 30;
/// Connection threads run user K handlers (.m.g .m.s .m.h), so they get the same deep stack as the
/// main interpreter thread rather than the 2 MB default; the reservation is virtual.
const CONNECTION_STACK: usize = 256 << 20;
/// Longest HTTP request head read before answering 400
const HTTP_MAX_HEAD: usize = 64 * 1024;

pub fn active_sockets() -> K {
    if let Ok(reg) = socket_registry().lock() {
        let mut keys: Vec<i64> = reg.keys().cloned().collect();
        keys.sort();
        K::int_list(keys)
    } else {
        K::int_list(vec![])
    }
}

/// Formats a K object to its string representation.
/// C Kona prints floats with %.{p}g (\p is significant digits, 7 by default): 0.000123456 stays
/// 0.000123456, 123.456789 is 123.4568 and the big ones are 8.507059e+37. Rust's {:.p} is decimal
/// places, which cut small numbers short and printed 38-digit integers. (Made by Claude)
pub fn fmt_g(f: f64, p: usize) -> String {
    // \p 0 means every digit in C Kona (PPMAX = 19 significant digits), and nothing above that
    let p = if p == 0 || p > 19 { 19 } else { p };
    if f == 0.0 {
        return if f.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    // round to p significant digits first, then read the exponent off the rounded number
    let sci = format!("{:.*e}", p - 1, f);
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if exp < -4 || exp >= p as i32 {
        let m = trim_fraction_zeros(mantissa);
        format!("{}e{}{:02}", m, if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        trim_fraction_zeros(&format!("{:.*}", decimals, f))
    }
}

fn trim_fraction_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let t = s.trim_end_matches('0');
    t.strip_suffix('.').unwrap_or(t).to_string()
}

/// A float the way the prompt shows it; with_point adds .0 to a whole number (the atom, and the
/// last item of a list: 1 2 3.0)
fn fmt_float(f: f64, with_point: bool) -> String {
    if f.is_nan() {
        return "0n".to_string();
    }
    if f.is_infinite() {
        // K3 spells infinity 0i (0w is k4)
        return if f.is_sign_positive() { "0i" } else { "-0i" }.to_string();
    }
    let p = PRINT_PRECISION.load(std::sync::atomic::Ordering::Relaxed);
    let mut s = fmt_g(f, p);
    if with_point && !s.contains('.') && !s.contains('e') {
        s.push_str(".0");
    }
    s
}

/// An int with K3's null and infinities spelled out: 0N 0I -0I
pub fn fmt_int(i: i64) -> String {
    if i == i64::MIN {
        "0N".to_string()
    } else if i == i64::MAX {
        "0I".to_string()
    } else if i == -i64::MAX {
        "-0I".to_string()
    } else {
        i.to_string()
    }
}

/// A K string the way C Kona prints it: quotes and backslashes escaped, \n \t \r for those
/// controls, \ooo (octal) for every other control or non-ASCII byte. Before this, "a\"b" printed
/// as "a"b", a newline printed raw, and a non-UTF-8 string printed as char_list(len=N).
fn k_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{:03o}", b)),
        }
    }
    out.push('"');
    out
}

pub fn format_k(k: &K) -> String {
    match &**k {
        KData::HashList { .. } => ".(..)".to_string(),
        KData::Nil => "".to_string(),
        KData::IntAtom(i) => fmt_int(*i),
        KData::FloatAtom(f) => fmt_float(*f, true),
        KData::CharAtom(c) => k_string(&[*c as u32 as u8]),
        KData::SymbolAtom(s) => {
            format!("`{}", s)
        }
        KData::IntList(v) => {
            if v.is_empty() {
                "!0".to_string()
            } else if v.len() == 1 {
                format!(",{}", fmt_int(v[0]))
            } else {
                v.iter()
                    .map(|x| fmt_int(*x))
                    .collect::<Vec<String>>()
                    .join(" ")
            }
        }
        KData::FloatList(v) => {
            if v.is_empty() {
                "0#0.0".to_string()
            } else {
                let mut formatted = Vec::with_capacity(v.len());
                for (idx, &x) in v.iter().enumerate() {
                    let is_last = idx == v.len() - 1;
                    let s = if x.is_nan() {
                        "0n".to_string()
                    } else {
                        fmt_float(x, is_last)
                    };
                    formatted.push(s);
                }
                if v.len() == 1 {
                    format!(",{}", formatted[0])
                } else {
                    formatted.join(" ")
                }
            }
        }
        // a one-item list prints with a leading comma, as C Kona does, so ,"c" ,`a and ,1 2 3 are
        // told apart from the atom "c", the atom `a and the list 1 2 3 (they used to print the same)
        KData::CharList(v) => {
            if v.len() == 1 {
                format!(",{}", k_string(v))
            } else {
                k_string(v)
            }
        }
        KData::SymbolList(v) => {
            if v.is_empty() {
                "0#`".to_string()
            } else if v.len() == 1 {
                format!(",`{}", v[0])
            } else {
                // `a `b `c with spaces, the K3 spelling C Kona prints
                v.iter()
                    .map(|s| format!("`{}", s))
                    .collect::<Vec<String>>()
                    .join(" ")
            }
        }
        KData::GeneralList(v) => {
            if v.is_empty() {
                "()".to_string()
            } else if v.len() == 1 {
                format!(",{}", format_k(&v[0]))
            } else {
                let items: Vec<String> = v.iter().map(|item| format_k(item)).collect();
                format!("({})", items.join(";"))
            }
        }
        KData::Dictionary { keys, values } => {
            // K3's spelling, which C Kona prints and both interpreters read back: .((`a;1);(`b;2)).
            // The k4 form `a`b!1 2 that used to print here is not valid C Kona input.
            let ks = crate::verbs::math::to_vec_of_atoms(keys).unwrap_or_default();
            let vs = crate::verbs::math::to_vec_of_atoms(values).unwrap_or_default();
            let entries: Vec<String> = ks
                .iter()
                .zip(vs.iter())
                .map(|(k, v)| format!("({};{})", format_k(k), format_k(v)))
                .collect();
            match entries.len() {
                0 => ".()".to_string(),
                1 => format!(".,{}", entries[0]),
                _ => format!(".({})", entries.join(";")),
            }
        }
        KData::Function(f) => match f {
            KFunc::Primitive(name) => name.clone(),
            KFunc::Adverb { name, operand } => {
                if let Some(op) = operand {
                    format!("{}{}", format_k(op), name)
                } else {
                    name.clone()
                }
            }
            KFunc::Projection { func, args } => {
                let arg_strs: Vec<String> = args
                    .iter()
                    .map(|a| a.as_ref().map(|x| format_k(x)).unwrap_or_default())
                    .collect();
                format!("{}[{}]", format_k(func), arg_strs.join(";"))
            }
            KFunc::UserDefined { body, .. } => {
                format!("{{{}}}", body)
            }
            KFunc::Composition { f, g } => {
                format!("{}{}", format_k(f), format_k(g))
            }
        },
    }
}

/// Extracts a clean file path string from a K symbol or string.
fn get_file_path(k: &K) -> Result<String, KonaError> {
    let path = match &**k {
        KData::SymbolAtom(s) => s.as_ref(),
        KData::CharList(v) => {
            std::str::from_utf8(v).map_err(|_| KonaError::File("Invalid UTF-8 path".to_string()))?
        }
        _ => return Err(KonaError::Type),
    };

    let clean_path = path.strip_prefix(':').unwrap_or(path);
    if clean_path.contains("..") {
        return Err(KonaError::File(
            "Directory traversal not allowed".to_string(),
        ));
    }

    Ok(clean_path.to_string())
}

/// Monadic 0: - Read text file line-by-line. Returns a general list of char vectors (strings).
pub fn read_text_file(x: &K) -> Result<K, KonaError> {
    let path = get_file_path(x)?;
    let content = if path.is_empty() {
        let mut buffer = String::new();
        std::io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|e| KonaError::File(format!("Failed to read stdin: {}", e)))?;
        buffer
    } else {
        std::fs::read_to_string(&path)
            .map_err(|e| KonaError::File(format!("Failed to read file {}: {}", path, e)))?
    };
    let mut lines = Vec::new();
    for line in content.lines() {
        lines.push(K::char_list(line.as_bytes().to_vec()));
    }
    Ok(K::general_list(lines))
}

/// Print a K value to stdout.
fn print_to_stdout(k: &K) -> Result<(), KonaError> {
    let mut stdout = std::io::stdout();
    match &**k {
        KData::CharList(bytes) => {
            stdout
                .write_all(bytes)
                .map_err(|e| KonaError::System(e.to_string()))?;
            stdout
                .write_all(b"\n")
                .map_err(|e| KonaError::System(e.to_string()))?;
        }
        KData::GeneralList(list) => {
            for item in list {
                match &**item {
                    KData::CharList(bytes) => {
                        stdout
                            .write_all(bytes)
                            .map_err(|e| KonaError::System(e.to_string()))?;
                        stdout
                            .write_all(b"\n")
                            .map_err(|e| KonaError::System(e.to_string()))?;
                    }
                    _ => {
                        writeln!(stdout, "{}", format_k(item))
                            .map_err(|e| KonaError::System(e.to_string()))?;
                    }
                }
            }
        }
        _ => {
            writeln!(stdout, "{}", format_k(k)).map_err(|e| KonaError::System(e.to_string()))?;
        }
    }
    stdout
        .flush()
        .map_err(|e| KonaError::System(e.to_string()))?;
    Ok(())
}

/// Dyadic 0: - Write list of char vectors to a text file or print to stdout if empty symbol.
pub fn write_text_file(a: &K, b: &K) -> Result<K, KonaError> {
    let is_empty_symbol = match &**a {
        KData::SymbolAtom(s) => s.is_empty(),
        KData::CharList(v) => v.is_empty(),
        _ => false,
    };

    if is_empty_symbol {
        print_to_stdout(b)?;
        return Ok(b.clone());
    }

    let path = get_file_path(a)?;
    let mut content = Vec::new();

    match &**b {
        KData::CharList(bytes) => {
            content.extend_from_slice(bytes);
        }
        KData::GeneralList(list) => {
            for (i, item) in list.iter().enumerate() {
                if i > 0 {
                    content.push(b'\n');
                }
                match &**item {
                    KData::CharList(bytes) => {
                        content.extend_from_slice(bytes);
                    }
                    _ => {
                        content.extend_from_slice(format_k(item).as_bytes());
                    }
                }
            }
        }
        _ => {
            content.extend_from_slice(format_k(b).as_bytes());
        }
    }

    std::fs::write(&path, content)
        .map_err(|e| KonaError::File(format!("Failed to write file {}: {}", path, e)))?;

    Ok(b.clone())
}

// --- SAFE BOUNDS-CHECKED BINARY SERIALIZATION/DESERIALIZATION ---

pub fn serialize_k(k: &K) -> Vec<u8> {
    let mut buf = Vec::new();
    serialize_k_to(k, &mut buf);
    buf
}

fn serialize_string(s: &str, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
}

fn serialize_k_to(k: &K, buf: &mut Vec<u8>) {
    match &**k {
        KData::Nil => {
            buf.push(0);
        }
        KData::IntAtom(i) => {
            buf.push(1);
            buf.extend_from_slice(&i.to_le_bytes());
        }
        KData::FloatAtom(f) => {
            buf.push(2);
            buf.extend_from_slice(&f.to_le_bytes());
        }
        KData::CharAtom(c) => {
            buf.push(3);
            buf.extend_from_slice(&(*c as u32).to_le_bytes());
        }
        KData::SymbolAtom(s) => {
            buf.push(4);
            serialize_string(s, buf);
        }
        KData::IntList(v) => {
            buf.push(5);
            buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
            for &i in v {
                buf.extend_from_slice(&i.to_le_bytes());
            }
        }
        KData::FloatList(v) => {
            buf.push(6);
            buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
            for &f in v {
                buf.extend_from_slice(&f.to_le_bytes());
            }
        }
        KData::CharList(v) => {
            buf.push(7);
            buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
            buf.extend_from_slice(v);
        }
        KData::SymbolList(v) => {
            buf.push(8);
            buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
            for s in v {
                serialize_string(s, buf);
            }
        }
        KData::GeneralList(v) => {
            buf.push(9);
            buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
            for item in v {
                serialize_k_to(item, buf);
            }
        }
        KData::Dictionary { keys, values } => {
            buf.push(10);
            serialize_k_to(keys, buf);
            serialize_k_to(values, buf);
        }
        KData::Function(f) => {
            buf.push(11);
            match f {
                KFunc::Primitive(name) => {
                    buf.push(12);
                    serialize_string(name, buf);
                }
                KFunc::Adverb { name, operand } => {
                    buf.push(13);
                    serialize_string(name, buf);
                    match operand {
                        None => buf.push(0),
                        Some(op) => {
                            buf.push(1);
                            serialize_k_to(op, buf);
                        }
                    }
                }
                KFunc::Projection { func, args } => {
                    buf.push(14);
                    serialize_k_to(func, buf);
                    buf.extend_from_slice(&(args.len() as u32).to_le_bytes());
                    for arg in args {
                        match arg {
                            None => buf.push(0),
                            Some(a) => {
                                buf.push(1);
                                serialize_k_to(a, buf);
                            }
                        }
                    }
                }
                KFunc::UserDefined { body, args, .. } => {
                    buf.push(15);
                    serialize_string(body, buf);
                    buf.extend_from_slice(&(args.len() as u32).to_le_bytes());
                    for arg in args {
                        serialize_string(arg, buf);
                    }
                }
                KFunc::Composition { f, g } => {
                    buf.push(16);
                    serialize_k_to(f, buf);
                    serialize_k_to(g, buf);
                }
            }
        }
        KData::HashList { indices, .. } => {
            buf.push(17);
            buf.extend_from_slice(&(indices.len() as u32).to_le_bytes());
            for &i in indices {
                buf.extend_from_slice(&i.to_le_bytes());
            }
        }
    }
}

pub struct KDeserializer<'a> {
    data: &'a [u8],
    offset: usize,
    depth: usize,
}

/// Nesting allowed in a binary value (1: files and -i messages): a general list inside a general list
/// costs 6 bytes per level, so without a cap a 6 MB message recursed a million deep and overflowed the
/// thread's stack. 4096 levels read back anything this port writes in practice (a first cap of 64
/// refused files it had written itself) and cost well under 1 MB of stack.
const MAX_BINARY_DEPTH: usize = 10_000;

impl<'a> KDeserializer<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0, depth: 0 }
    }

    pub fn read_k(&mut self) -> Result<K, KonaError> {
        if self.depth >= MAX_BINARY_DEPTH {
            return Err(KonaError::Parse("Binary value nested too deep".to_string()));
        }
        self.depth += 1;
        let res = self.read_k_inner();
        self.depth -= 1;
        res
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.offset)
    }

    fn validate_capacity(&self, len: usize, elem_size: usize) -> Result<(), KonaError> {
        const MAX_ALLOC_LIMIT: usize = 100_000_000;
        if len > MAX_ALLOC_LIMIT {
            return Err(KonaError::Parse("Header size exceeds limit".to_string()));
        }
        if len > self.remaining() {
            return Err(KonaError::Parse("Corrupt binary file".to_string()));
        }
        let required_bytes = len
            .checked_mul(elem_size)
            .ok_or_else(|| KonaError::Parse("Length calculation overflow".to_string()))?;
        if self.remaining() < required_bytes {
            return Err(KonaError::Parse("Corrupt binary file".to_string()));
        }
        Ok(())
    }

    fn read_u8(&mut self) -> Result<u8, KonaError> {
        if self.remaining() < 1 {
            return Err(KonaError::Parse("Corrupt binary file".to_string()));
        }
        let val = self.data[self.offset];
        self.offset += 1;
        Ok(val)
    }

    fn read_u32(&mut self) -> Result<u32, KonaError> {
        if self.remaining() < 4 {
            return Err(KonaError::Parse("Corrupt binary file".to_string()));
        }
        let bytes = &self.data[self.offset..self.offset + 4];
        let val = u32::from_le_bytes(bytes.try_into().unwrap());
        self.offset += 4;
        Ok(val)
    }

    fn read_i64(&mut self) -> Result<i64, KonaError> {
        if self.remaining() < 8 {
            return Err(KonaError::Parse("Corrupt binary file".to_string()));
        }
        let bytes = &self.data[self.offset..self.offset + 8];
        let val = i64::from_le_bytes(bytes.try_into().unwrap());
        self.offset += 8;
        Ok(val)
    }

    fn read_f64(&mut self) -> Result<f64, KonaError> {
        if self.remaining() < 8 {
            return Err(KonaError::Parse("Corrupt binary file".to_string()));
        }
        let bytes = &self.data[self.offset..self.offset + 8];
        let val = f64::from_le_bytes(bytes.try_into().unwrap());
        self.offset += 8;
        Ok(val)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], KonaError> {
        if self.remaining() < len {
            return Err(KonaError::Parse("Corrupt binary file".to_string()));
        }
        let res = &self.data[self.offset..self.offset + len];
        self.offset += len;
        Ok(res)
    }

    fn read_string(&mut self) -> Result<String, KonaError> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_bytes(len)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| KonaError::Parse("Corrupt binary file".to_string()))
    }

    fn read_k_inner(&mut self) -> Result<K, KonaError> {
        let tag = self.read_u8()?;
        match tag {
            0 => Ok(K::nil()),
            1 => {
                let val = self.read_i64()?;
                Ok(K::int(val))
            }
            2 => {
                let val = self.read_f64()?;
                Ok(K::float(val))
            }
            3 => {
                let val = self.read_u32()?;
                let c = char::from_u32(val)
                    .ok_or_else(|| KonaError::Parse("Corrupt binary file".to_string()))?;
                Ok(K::char(c))
            }
            4 => {
                let s = self.read_string()?;
                Ok(K::symbol(s))
            }
            5 => {
                let len = self.read_u32()? as usize;
                self.validate_capacity(len, 8)?;
                let mut v = Vec::with_capacity(len);
                for _ in 0..len {
                    v.push(self.read_i64()?);
                }
                Ok(K::int_list(v))
            }
            6 => {
                let len = self.read_u32()? as usize;
                self.validate_capacity(len, 8)?;
                let mut v = Vec::with_capacity(len);
                for _ in 0..len {
                    v.push(self.read_f64()?);
                }
                Ok(K::float_list(v))
            }
            7 => {
                let len = self.read_u32()? as usize;
                self.validate_capacity(len, 1)?;
                let bytes = self.read_bytes(len)?;
                Ok(K::char_list(bytes.to_vec()))
            }
            8 => {
                let len = self.read_u32()? as usize;
                self.validate_capacity(len, 4)?;
                let mut v = Vec::with_capacity(len);
                for _ in 0..len {
                    let s = self.read_string()?;
                    v.push(Arc::from(s.as_str()));
                }
                Ok(K::symbol_list(v))
            }
            9 => {
                let len = self.read_u32()? as usize;
                self.validate_capacity(len, 1)?;
                let mut v = Vec::with_capacity(len);
                for _ in 0..len {
                    v.push(self.read_k()?);
                }
                Ok(K::general_list(v))
            }
            10 => {
                let keys = self.read_k()?;
                let values = self.read_k()?;
                Ok(K::dictionary(keys, values))
            }
            11 => {
                let f_tag = self.read_u8()?;
                match f_tag {
                    12 => {
                        let name = self.read_string()?;
                        Ok(K::function(KFunc::Primitive(name)))
                    }
                    13 => {
                        let name = self.read_string()?;
                        let opt_tag = self.read_u8()?;
                        let operand = if opt_tag == 1 {
                            Some(self.read_k()?)
                        } else if opt_tag == 0 {
                            None
                        } else {
                            return Err(KonaError::Parse("Corrupt binary file".to_string()));
                        };
                        Ok(K::function(KFunc::Adverb { name, operand }))
                    }
                    14 => {
                        let func = self.read_k()?;
                        let len = self.read_u32()? as usize;
                        self.validate_capacity(len, 1)?;
                        let mut args = Vec::with_capacity(len);
                        for _ in 0..len {
                            let opt_tag = self.read_u8()?;
                            let arg = if opt_tag == 1 {
                                Some(self.read_k()?)
                            } else if opt_tag == 0 {
                                None
                            } else {
                                return Err(KonaError::Parse("Corrupt binary file".to_string()));
                            };
                            args.push(arg);
                        }
                        Ok(K::function(KFunc::Projection { func, args }))
                    }
                    15 => {
                        let body = self.read_string()?;
                        let len = self.read_u32()? as usize;
                        self.validate_capacity(len, 4)?;
                        let mut args = Vec::with_capacity(len);
                        for _ in 0..len {
                            args.push(self.read_string()?);
                        }
                        Ok(K::function(KFunc::UserDefined {
                            body,
                            args,
                            code: None,
                            env: None,
                        }))
                    }
                    16 => {
                        let f = self.read_k()?;
                        let g = self.read_k()?;
                        Ok(K::function(KFunc::Composition { f, g }))
                    }
                    _ => Err(KonaError::Parse("Corrupt binary file".to_string())),
                }
            }
            17 => {
                let len = self.read_u32()? as usize;
                self.validate_capacity(len, 8)?;
                let mut indices = Vec::with_capacity(len);
                for _ in 0..len {
                    indices.push(self.read_i64()?);
                }
                Ok(K::hash_list(indices, std::collections::HashMap::new()))
            }
            _ => Err(KonaError::Parse("Corrupt binary file".to_string())),
        }
    }
}

/// Monadic 1: - Read a K object from a binary file.
pub fn read_binary_file(x: &K) -> Result<K, KonaError> {
    let path = get_file_path(x)?;
    let data = std::fs::read(&path)
        .map_err(|e| KonaError::File(format!("Failed to read file {}: {}", path, e)))?;
    let mut deserializer = KDeserializer::new(&data);
    let k = deserializer.read_k()?;
    if deserializer.remaining() > 0 {
        return Err(KonaError::Parse("Corrupt binary file".to_string()));
    }
    Ok(k)
}

/// Dyadic 1: - Write a K object to a binary file.
pub fn write_binary_file(a: &K, b: &K) -> Result<K, KonaError> {
    let path = get_file_path(a)?;
    let serialized = serialize_k(b);
    std::fs::write(&path, serialized)
        .map_err(|e| KonaError::File(format!("Failed to write file {}: {}", path, e)))?;
    Ok(b.clone())
}

/// Monadic 2: - Load dynamic C library. Feature is disabled.
pub fn load_c_library(_x: &K) -> Result<K, KonaError> {
    Err(KonaError::FeatureDisabled(
        "Dynamic C library loading is disabled for safety".to_string(),
    ))
}

// --- SERVERS ---

pub fn handle_http_connection(mut stream: TcpStream, ktree: Arc<KTree>) -> Result<(), KonaError> {
    // read until the end of the request head (a blank line): one read() can return any prefix of
    // the request, and a request line split across two segments used to be answered with a 400
    let mut head: Vec<u8> = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let bytes_read = stream
            .read(&mut buffer)
            .map_err(|e| KonaError::System(format!("Socket read error: {}", e)))?;
        if bytes_read == 0 {
            break;
        }
        head.extend_from_slice(&buffer[..bytes_read]);
        // a bare LF LF ends the head too (curl and many test clients send that)
        if head.windows(4).any(|w| w == b"\r\n\r\n")
            || head.windows(2).any(|w| w == b"\n\n")
            || head.len() >= HTTP_MAX_HEAD
        {
            break;
        }
    }
    if head.is_empty() {
        return Ok(());
    }

    let request_str = String::from_utf8_lossy(&head);
    let first_line = request_str.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        let response = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream
            .write_all(response.as_bytes())
            .map_err(|e| KonaError::System(format!("Socket write error: {}", e)))?;
        return Ok(());
    }
    if parts[0] != "GET" {
        let response = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream
            .write_all(response.as_bytes())
            .map_err(|e| KonaError::System(format!("Socket write error: {}", e)))?;
        return Ok(());
    }

    let path = parts[1];
    if let Some(handler) = crate::eval::resolve_dotted_name(".m.h", &HashMap::new(), &ktree) {
        let path_k = K::char_list(first_line.as_bytes().to_vec());
        let local_env = HashMap::new();
        match apply_func(&handler, &[path_k], &local_env, &ktree) {
            Ok(res) => {
                let response_bytes = match &*res {
                    KData::CharList(bytes) => bytes.clone(),
                    _ => format_k(&res).into_bytes(),
                };
                stream
                    .write_all(&response_bytes)
                    .map_err(|e| KonaError::System(format!("Socket write error: {}", e)))?;
            }
            Err(e) => {
                let err_msg = format!(
                    "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nError evaluating handler: {:?}",
                    e
                );
                stream
                    .write_all(err_msg.as_bytes())
                    .map_err(|e| KonaError::System(format!("Socket write error: {}", e)))?;
            }
        }
    } else {
        let response = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\nHandler .m.h not defined for path: {}",
            35 + path.len(),
            path
        );
        stream
            .write_all(response.as_bytes())
            .map_err(|e| KonaError::System(format!("Socket write error: {}", e)))?;
    }
    stream
        .flush()
        .map_err(|e| KonaError::System(format!("Socket flush error: {}", e)))?;
    Ok(())
}

pub fn start_http_server(port: u16, ktree: Arc<KTree>) {
    let address = format!("127.0.0.1:{}", port);
    let listener = match TcpListener::bind(&address) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind HTTP server to {}: {}", address, e);
            return;
        }
    };
    println!("HTTP server listening on http://{}", address);

    for stream_res in listener.incoming() {
        let stream = match stream_res {
            Ok(s) => s,
            Err(e) => {
                eprintln!("HTTP accept error: {}", e);
                continue;
            }
        };
        let ktree_clone = ktree.clone();
        // an idle or half-sent request no longer pins a thread forever
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(HTTP_READ_TIMEOUT_SECS)));
        let spawned = std::thread::Builder::new()
            .stack_size(CONNECTION_STACK)
            .spawn(move || {
                if let Err(e) = handle_http_connection(stream, ktree_clone) {
                    eprintln!("Error handling HTTP connection: {:?}", e);
                }
            });
        if let Err(e) = spawned {
            eprintln!("Could not start an HTTP connection thread: {}", e);
        }
    }
}

pub fn read_ipc_message(stream: &mut TcpStream) -> Result<(u8, K), KonaError> {
    let mut header = [0; 5];
    stream
        .read_exact(&mut header)
        .map_err(|e| KonaError::System(format!("Failed to read IPC header: {}", e)))?;
    read_ipc_body(stream, header)
}

/// the rest of a message in this port's own format, once its 5-byte header has been read
fn read_ipc_body(stream: &mut TcpStream, header: [u8; 5]) -> Result<(u8, K), KonaError> {
    let msg_type = header[0];
    let payload_len = u32::from_le_bytes(header[1..5].try_into().unwrap()) as usize;

    if payload_len > 100 * 1024 * 1024 {
        return Err(KonaError::Parse("IPC message too large".to_string()));
    }

    let mut payload = vec![0; payload_len];
    stream
        .read_exact(&mut payload)
        .map_err(|e| KonaError::System(format!("Failed to read IPC payload: {}", e)))?;

    let mut deserializer = KDeserializer::new(&payload);
    let k = deserializer.read_k()?;
    Ok((msg_type, k))
}

pub fn write_ipc_message(stream: &mut TcpStream, msg_type: u8, k: &K) -> Result<(), KonaError> {
    let serialized = serialize_k(k);
    let payload_len = serialized.len() as u32;
    let mut header = [0; 5];
    header[0] = msg_type;
    header[1..5].copy_from_slice(&payload_len.to_le_bytes());

    stream
        .write_all(&header)
        .map_err(|e| KonaError::System(format!("Failed to write IPC header: {}", e)))?;
    stream
        .write_all(&serialized)
        .map_err(|e| KonaError::System(format!("Failed to write IPC payload: {}", e)))?;
    stream
        .flush()
        .map_err(|e| KonaError::System(format!("Failed to flush IPC stream: {}", e)))?;
    Ok(())
}

/// One message in C Kona's byte format (kwire.rs): d=1 runs .m.g and answers (0; result) or (1; "error"),
/// d=0 runs .m.s and answers nothing - the same as C Kona, so kipc.py and other C Kona tools work here.
/// (Made by Claude)
fn handle_c_message(stream: &mut TcpStream, first5: [u8; 5], ktree: &Arc<KTree>) -> Result<(), KonaError> {
    let mut rest = [0u8; 11];
    stream
        .read_exact(&mut rest)
        .map_err(|e| KonaError::System(format!("Failed to read IPC header: {}", e)))?;
    let d = rest[2]; // header byte 7
    let size = i64::from_le_bytes(rest[3..11].try_into().unwrap_or([0; 8]));
    // same 100 MB cap as the port's own format: the body is allocated before it arrives, and the old
    // bound (2 GB) let one 16-byte header reserve 2 GB per connection
    if !(0..=100 * 1024 * 1024).contains(&size) {
        return Err(KonaError::Parse("IPC message too large".to_string()));
    }
    let _ = first5;
    let mut body = vec![0u8; size as usize];
    stream
        .read_exact(&mut body)
        .map_err(|e| KonaError::System(format!("Failed to read IPC payload: {}", e)))?;
    let handler_name = if d == 1 { ".m.g" } else { ".m.s" };
    let result = crate::kwire::decode(&body).and_then(|k| {
        match crate::eval::resolve_dotted_name(handler_name, &HashMap::new(), ktree) {
            Some(handler) => apply_func(&handler, &[k], &HashMap::new(), ktree),
            None => Err(KonaError::Value(format!("no {} handler", handler_name))),
        }
    });
    if d != 1 {
        return Ok(()); // one-way message: no answer
    }
    let answer = match result {
        Ok(v) => K::general_list(vec![K::int(0), v]),
        Err(e) => K::general_list(vec![K::int(1), K::char_list(crate::kwire::error_text(&e).into_bytes())]),
    };
    let bytes = match crate::kwire::message(2, &answer) {
        Ok(b) => b,
        Err(e) => crate::kwire::message(2, &K::general_list(vec![
            K::int(1),
            K::char_list(format!("answer can't be sent: {}", crate::kwire::error_text(&e)).into_bytes()),
        ]))?,
    };
    stream
        .write_all(&bytes)
        .map_err(|e| KonaError::System(format!("Failed to write IPC payload: {}", e)))?;
    stream
        .flush()
        .map_err(|e| KonaError::System(format!("Failed to flush IPC stream: {}", e)))?;
    Ok(())
}

pub fn handle_ipc_connection(mut stream: TcpStream, ktree: Arc<KTree>) -> Result<(), KonaError> {
    let _ = stream.set_nodelay(true);
    loop {
        // idle between messages for as long as the client likes (C Kona never drops a connection);
        // once a header has started, the rest of the message has IPC_MESSAGE_TIMEOUT_SECS to arrive
        let _ = stream.set_read_timeout(None);
        let mut first5 = [0u8; 5];
        if stream.read_exact(&mut first5).is_err() {
            return Ok(());
        }
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(
            IPC_MESSAGE_TIMEOUT_SECS,
        )));
        if crate::kwire::is_c_header(&first5) {
            handle_c_message(&mut stream, first5, &ktree)?;
            continue;
        }
        let (msg_type, k) = match read_ipc_body(&mut stream, first5) {
            Ok(res) => res,
            Err(_) => {
                return Ok(());
            }
        };

        match msg_type {
            0 => {
                if let Some(handler) =
                    crate::eval::resolve_dotted_name(".m.s", &HashMap::new(), &ktree)
                {
                    let local_env = HashMap::new();
                    let _ = apply_func(&handler, &[k], &local_env, &ktree);
                } else {
                    // Remove eval_words entirely for security
                }
            }
            1 => {
                let response = if let Some(handler) =
                    crate::eval::resolve_dotted_name(".m.g", &HashMap::new(), &ktree)
                {
                    let local_env = HashMap::new();
                    match apply_func(&handler, &[k], &local_env, &ktree) {
                        Ok(res) => res,
                        Err(e) => K::symbol(format!("Error: {:?}", e)),
                    }
                } else {
                    // Return an error instead of executing arbitrary code
                    K::symbol("UnsupportedSyncPayload")
                };
                write_ipc_message(&mut stream, 2, &response)?;
            }
            _ => {
                return Err(KonaError::System(format!(
                    "Unrecognized IPC message type: {}",
                    msg_type
                )));
            }
        }
    }
}

pub fn start_ipc_server(port: u16, ktree: Arc<KTree>) {
    let address = format!("127.0.0.1:{}", port);
    let listener = match TcpListener::bind(&address) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind IPC server to {}: {}", address, e);
            return;
        }
    };
    println!("IPC server listening on port {}", port);

    for stream_res in listener.incoming() {
        let stream = match stream_res {
            Ok(s) => s,
            Err(e) => {
                eprintln!("IPC accept error: {}", e);
                continue;
            }
        };
        let ktree_clone = ktree.clone();
        let spawned = std::thread::Builder::new()
            .stack_size(CONNECTION_STACK)
            .spawn(move || {
                if let Err(e) = handle_ipc_connection(stream, ktree_clone) {
                    eprintln!("Error handling IPC connection: {:?}", e);
                }
            });
        if let Err(e) = spawned {
            // out of threads or address space: say so instead of silently dropping the client
            eprintln!("Could not start an IPC connection thread: {}", e);
        }
    }
}

fn socket_registry() -> &'static std::sync::Mutex<HashMap<i64, TcpStream>> {
    static REGISTRY: std::sync::OnceLock<std::sync::Mutex<HashMap<i64, TcpStream>>> =
        std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn next_socket_id() -> &'static std::sync::Mutex<i64> {
    static NEXT_ID: std::sync::OnceLock<std::sync::Mutex<i64>> = std::sync::OnceLock::new();
    NEXT_ID.get_or_init(|| std::sync::Mutex::new(3))
}

pub fn open_or_close_socket(x: &K) -> Result<K, KonaError> {
    use crate::verbs::math::to_vec_of_atoms;
    match &**x {
        KData::IntAtom(fd) => {
            let mut reg = socket_registry().lock().unwrap();
            if reg.remove(fd).is_some() {
                Ok(K::nil())
            } else {
                Err(KonaError::Index)
            }
        }
        KData::GeneralList(_) | KData::SymbolList(_) => {
            let parts = to_vec_of_atoms(x)?;
            if parts.len() != 2 {
                return Err(KonaError::Length);
            }
            let host = match &*parts[0] {
                KData::SymbolAtom(s) => s.as_ref(),
                KData::CharList(v) => std::str::from_utf8(v).map_err(|_| KonaError::Type)?,
                _ => return Err(KonaError::Type),
            };
            let port = parts[1].as_int().ok_or(KonaError::Type)?;

            let host_str = if host.is_empty() { "127.0.0.1" } else { host };
            let addr = format!("{}:{}", host_str, port);

            let stream = TcpStream::connect(&addr)
                .map_err(|e| KonaError::System(format!("Connection failed to {}: {}", addr, e)))?;

            stream
                .set_nodelay(true)
                .map_err(|e| KonaError::System(format!("Failed to set TCP_NODELAY: {}", e)))?;

            let mut reg = socket_registry().lock().unwrap();
            let mut next_id_guard = next_socket_id().lock().unwrap();
            let id = *next_id_guard;
            *next_id_guard += 1;

            reg.insert(id, stream);
            Ok(K::int(id))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn send_async(a: &K, b: &K) -> Result<K, KonaError> {
    match &**a {
        KData::IntAtom(fd) => {
            let mut reg = socket_registry().lock().unwrap();
            if let Some(stream) = reg.get_mut(fd) {
                write_ipc_message(stream, 0, b)?;
                Ok(b.clone())
            } else {
                Err(KonaError::Index)
            }
        }
        _ => Err(KonaError::Type),
    }
}

pub fn send_sync(a: &K, b: &K) -> Result<K, KonaError> {
    use crate::verbs::math::to_vec_of_atoms;
    match &**a {
        KData::IntAtom(fd) => {
            let mut reg = socket_registry().lock().unwrap();
            if let Some(stream) = reg.get_mut(fd) {
                write_ipc_message(stream, 1, b)?;
                let (_msg_type, response) = read_ipc_message(stream)?;
                Ok(response)
            } else {
                Err(KonaError::Index)
            }
        }
        KData::GeneralList(_) | KData::SymbolList(_) => {
            let parts = to_vec_of_atoms(a)?;
            if parts.len() != 2 {
                return Err(KonaError::Length);
            }
            let host = match &*parts[0] {
                KData::SymbolAtom(s) => s.as_ref(),
                KData::CharList(v) => std::str::from_utf8(v).map_err(|_| KonaError::Type)?,
                _ => return Err(KonaError::Type),
            };
            let port = parts[1].as_int().ok_or(KonaError::Type)?;

            let host_str = if host.is_empty() { "127.0.0.1" } else { host };
            let addr = format!("{}:{}", host_str, port);

            let mut stream = TcpStream::connect(&addr)
                .map_err(|e| KonaError::System(format!("Connection failed to {}: {}", addr, e)))?;

            let req_bytes = match &**b {
                KData::CharList(bytes) => bytes.clone(),
                _ => format_k(b).into_bytes(),
            };

            stream
                .write_all(&req_bytes)
                .map_err(|e| KonaError::System(format!("Failed to write HTTP request: {}", e)))?;
            stream
                .flush()
                .map_err(|e| KonaError::System(format!("Failed to flush HTTP request: {}", e)))?;

            let mut resp_bytes = Vec::new();
            stream
                .read_to_end(&mut resp_bytes)
                .map_err(|e| KonaError::System(format!("Failed to read HTTP response: {}", e)))?;

            Ok(K::char_list(resp_bytes))
        }
        _ => Err(KonaError::Type),
    }
}

/// Monadic 6: - Read raw byte string from a file. Returns a CharList.
pub fn read_raw_file(x: &K) -> Result<K, KonaError> {
    let path = get_file_path(x)?;
    let bytes = std::fs::read(&path)
        .map_err(|e| KonaError::File(format!("Failed to read file {}: {}", path, e)))?;
    Ok(K::char_list(bytes))
}

/// Dyadic 6: - Write raw byte string to a file.
pub fn write_raw_file(a: &K, b: &K) -> Result<K, KonaError> {
    let mut append = false;

    match &**a {
        KData::GeneralList(v) => {
            if v.len() == 1 {
                append = true;
            }
        }
        KData::SymbolList(v) => {
            if v.len() == 1 {
                append = true;
            }
        }
        _ => {}
    }

    let path = if append {
        match &**a {
            KData::GeneralList(v) => get_file_path(&v[0])?,
            KData::SymbolList(v) => {
                let p = v[0].as_ref();
                let clean_p = p.strip_prefix(':').unwrap_or(p);
                clean_p.to_string()
            }
            _ => get_file_path(a)?,
        }
    } else {
        get_file_path(a)?
    };

    let temp_buf;
    let bytes = match &**b {
        KData::CharList(v) => v.as_slice(),
        KData::CharAtom(c) => {
            temp_buf = vec![*c as u8];
            temp_buf.as_slice()
        }
        _ => return Err(KonaError::Type),
    };

    if path.is_empty() {
        use std::io::Write;
        std::io::stdout()
            .write_all(bytes)
            .map_err(|e| KonaError::System(format!("Stdout write error: {}", e)))?;
        std::io::stdout()
            .flush()
            .map_err(|e| KonaError::System(format!("Stdout flush error: {}", e)))?;
    } else {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .append(append)
            .truncate(!append)
            .open(&path)
            .map_err(|e| KonaError::File(format!("Failed to open file {}: {}", path, e)))?;
        file.write_all(bytes)
            .map_err(|e| KonaError::File(format!("Failed to write to file {}: {}", path, e)))?;
    }

    Ok(K::nil())
}

/// Dyadic 5: - Append a K object to a binary file.
pub fn append_binary_file(a: &K, b: &K) -> Result<K, KonaError> {
    let path = get_file_path(a)?;
    if !std::path::Path::new(&path).exists() {
        return write_binary_file(a, b);
    }

    use std::io::{Read, Seek, SeekFrom, Write};
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| KonaError::File(format!("Failed to open file {}: {}", path, e)))?;

    let mut header = [0u8; 5];
    file.read_exact(&mut header)
        .map_err(|e| KonaError::File(format!("Failed to read file header: {}", e)))?;

    let tag = header[0];
    let len = u32::from_le_bytes(header[1..5].try_into().unwrap());

    if tag < 5 || tag > 9 {
        return Err(KonaError::Type);
    }

    let mut append_bytes = Vec::new();
    let append_count = match tag {
        5 => {
            // IntList
            match &**b {
                KData::IntAtom(val) => {
                    append_bytes.extend_from_slice(&val.to_le_bytes());
                    1
                }
                KData::IntList(v) => {
                    for &val in v {
                        append_bytes.extend_from_slice(&val.to_le_bytes());
                    }
                    v.len() as u32
                }
                _ => return Err(KonaError::Type),
            }
        }
        6 => {
            // FloatList
            match &**b {
                KData::FloatAtom(val) => {
                    append_bytes.extend_from_slice(&val.to_le_bytes());
                    1
                }
                KData::FloatList(v) => {
                    for &val in v {
                        append_bytes.extend_from_slice(&val.to_le_bytes());
                    }
                    v.len() as u32
                }
                _ => return Err(KonaError::Type),
            }
        }
        7 => {
            // CharList
            match &**b {
                KData::CharAtom(val) => {
                    append_bytes.push(*val as u8);
                    1
                }
                KData::CharList(v) => {
                    append_bytes.extend_from_slice(v);
                    v.len() as u32
                }
                _ => return Err(KonaError::Type),
            }
        }
        8 => {
            // SymbolList
            match &**b {
                KData::SymbolAtom(val) => {
                    serialize_string(val, &mut append_bytes);
                    1
                }
                KData::SymbolList(v) => {
                    for val in v {
                        serialize_string(val, &mut append_bytes);
                    }
                    v.len() as u32
                }
                _ => return Err(KonaError::Type),
            }
        }
        9 => {
            // GeneralList
            match &**b {
                KData::GeneralList(v) => {
                    for val in v {
                        serialize_k_to(val, &mut append_bytes);
                    }
                    v.len() as u32
                }
                _ => {
                    serialize_k_to(b, &mut append_bytes);
                    1
                }
            }
        }
        _ => return Err(KonaError::Type),
    };

    let new_len = len + append_count;
    file.seek(SeekFrom::Start(1))
        .map_err(|e| KonaError::File(format!("Failed to seek header: {}", e)))?;
    file.write_all(&new_len.to_le_bytes())
        .map_err(|e| KonaError::File(format!("Failed to write header length: {}", e)))?;

    file.seek(SeekFrom::End(0))
        .map_err(|e| KonaError::File(format!("Failed to seek to end: {}", e)))?;
    file.write_all(&append_bytes)
        .map_err(|e| KonaError::File(format!("Failed to append data: {}", e)))?;

    Ok(K::int(new_len as i64))
}
