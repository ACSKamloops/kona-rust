#![forbid(unsafe_code)]
// Made by Claude
//! door_bcpl.rs - library 4 of the `_sys` door: Martin Richards' BCPL (his Cintcode system, cintsys) running as
//! worker programs next to Kona. Each worker is one cintsys kept open; Kona types a CLI command into it and reads
//! what it prints. Several workers can run at once - that is how `_sys(0;0;calls)` spreads BCPL over the cores.
//!   _sys(4;0;"kqueens 12")   -> the whole numbers the command printed, as an int list (12 14200)
//!   _sys(4;1;"kqueens 12")   -> the printed text
//! Settings: KONA_BCPL = path to cintsys.exe (its folder is BCPLROOT),
//!           KONA_BCPL_WORKERS = most workers kept (default: number of cores).

use crate::errors::KonaError;
use crate::types::{K, KData};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

const DEFAULT_CINTSYS: &str = r"D:\kona-bcpl\src\BCPL\cintcode\bin\cintsys.exe";
const DONE: &str = "@@kona-done@@";

struct Worker {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

static IDLE: Mutex<Vec<Worker>> = Mutex::new(Vec::new());

fn bcpl_err(what: impl std::fmt::Display) -> KonaError {
    KonaError::System(format!("BCPL: {what}"))
}

fn max_workers() -> usize {
    std::env::var("KONA_BCPL_WORKERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4))
}

fn start_worker() -> Result<Worker, KonaError> {
    let exe = std::env::var("KONA_BCPL").unwrap_or_else(|_| DEFAULT_CINTSYS.to_string());
    let bin = std::path::Path::new(&exe).parent().ok_or_else(|| bcpl_err("bad cintsys path"))?;
    let root = bin.parent().ok_or_else(|| bcpl_err("bad cintsys path"))?;
    let r = root.to_string_lossy().replace('\\', "/");
    let mut child = Command::new(&exe)
        .arg("-q")
        .current_dir(root)
        .env("BCPLROOT", &r)
        .env("BCPLPATH", format!("{r}/cin"))
        .env("BCPLHDRS", format!("{r}/g"))
        .env("BCPLSCRIPTS", format!("{r}/s"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| bcpl_err(format!("could not start {exe}: {e}")))?;
    let input = child.stdin.take().ok_or_else(|| bcpl_err("no stdin"))?;
    let output = BufReader::new(child.stdout.take().ok_or_else(|| bcpl_err("no stdout"))?);
    let mut w = Worker { child, input, output };
    run_on(&mut w, "echo \"ready\"")?; // swallow the start-up banner
    Ok(w)
}

/// type one command into a worker, read everything it prints up to our end marker
fn run_on(w: &mut Worker, cmd: &str) -> Result<String, KonaError> {
    write!(w.input, "{cmd}\necho \"{DONE}\"\n").map_err(bcpl_err)?;
    w.input.flush().map_err(bcpl_err)?;
    let mut out = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        if w.output.read_line(&mut line).map_err(bcpl_err)? == 0 {
            return Err(bcpl_err("cintsys stopped"));
        }
        if let Some(i) = line.find(DONE) {
            out.push_str(&line[..i]);
            return Ok(out);
        }
        out.push_str(&line);
    }
}

fn run(cmd: &str) -> Result<String, KonaError> {
    if cmd.contains('\n') || cmd.contains('\r') || cmd.contains(DONE) {
        return Err(KonaError::Domain);
    }
    let taken = IDLE.lock().map_err(|_| bcpl_err("lock poisoned"))?.pop();
    let mut w = match taken {
        Some(w) => w,
        None => start_worker()?,
    };
    let r = run_on(&mut w, cmd);
    if r.is_ok() {
        let mut idle = IDLE.lock().map_err(|_| bcpl_err("lock poisoned"))?;
        if idle.len() < max_workers() {
            idle.push(w); // keep it for the next call; a broken or extra worker is dropped (killed)
        }
    }
    r
}

fn numbers(text: &str) -> Vec<i64> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '-'))
        .filter_map(|t| t.parse::<i64>().ok())
        .collect()
}

pub fn call(f: i64, args: &K) -> Result<K, KonaError> {
    let cmd = match &**args {
        KData::CharList(b) => String::from_utf8_lossy(b).into_owned(),
        KData::CharAtom(c) => c.to_string(),
        _ => return Err(KonaError::Type),
    };
    match f {
        0 => Ok(K::int_list(numbers(&run(&cmd)?))),
        1 => Ok(K::char_list(run(&cmd)?.into_bytes())),
        _ => Err(KonaError::Domain),
    }
}
// Made by Claude
