#![cfg_attr(not(feature = "cpp"), forbid(unsafe_code))]
// with the C++ door built in, unsafe is still refused everywhere except the cxx bridge in door_cpp.rs
#![cfg_attr(feature = "cpp", deny(unsafe_code))]
// lib.rs - Kona as a library (split out of main.rs 2026-10-15 so other programs, like the Unreal bench, can run K).
// kona.exe (main.rs) is the same as before: it calls into this library.

pub mod adverbs;
pub mod errors;
pub mod eval;
pub mod io;

pub static STEP_SCRIPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub mod parser;
pub mod types;
pub mod kwire;
pub mod pool;
pub mod gpu;
pub mod door;
pub mod door_net;
pub mod door_bcpl;
#[cfg(feature = "cpp")]
pub mod door_cpp;
pub mod verbs;

#[cfg(test)]
pub mod c_tests;
#[cfg(test)]
pub mod tests;

use crate::errors::KonaError;
use crate::types::K;
use crate::verbs::dict::KTree;
use std::collections::HashMap;
use std::sync::Arc;

pub fn count_braces(s: &str) -> i32 {
    let mut count = 0;
    let mut in_str = false;
    let mut escaped = false;
    let mut prev_char: Option<char> = None;
    for c in s.chars() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
        } else {
            if c == '"' {
                in_str = true;
            } else if c == '/' {
                let preceded_by_whitespace = match prev_char {
                    None => true,
                    Some(pc) => pc == ' ' || pc == '\t' || pc == '\n' || pc == '\r',
                };
                if preceded_by_whitespace {
                    break;
                }
            } else if c == '{' || c == '[' || c == '(' {
                count += 1;
            } else if c == '}' || c == ']' || c == ')' {
                count -= 1;
            }
        }
        prev_char = Some(c);
    }
    count
}

/// A script name as typed, or name.k if that is what exists (C Kona's \l: "load script f or f.k").
/// The prompt's \l used to take the name literally while a script's \l tried .k; now both go here.
pub fn resolve_script_path(name: &str) -> String {
    if std::path::Path::new(name).exists() {
        return name.to_string();
    }
    let with_k = format!("{}.k", name);
    if std::path::Path::new(&with_k).exists() {
        with_k
    } else {
        name.to_string()
    }
}

thread_local! {
    static LOAD_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
/// Nested \l loads before it is an error (C Kona stops at 124); a.k loading a.k overflowed the stack
const MAX_LOAD_DEPTH: usize = 124;

/// Does this line's result stay unprinted at the prompt? C Kona shows nothing for an assignment
/// (a:1 2 3, a::x, a+:1, a[0]:9); the last statement on the line decides. (Made by Claude)
pub fn is_assignment_statement(words: &[crate::parser::Word]) -> bool {
    use crate::parser::Word;
    let start = words
        .iter()
        .rposition(|w| matches!(w, Word::Verb(v) if v == ";"))
        .map(|i| i + 1)
        .unwrap_or(0);
    let assigns = |w: &Word| matches!(w, Word::Verb(v) if v.ends_with(':'));
    match &words[start..] {
        [Word::LocalAssign(_) | Word::GlobalAssign(_), ..] => true,
        [Word::Name(_), v, ..] if assigns(v) => true,
        [Word::Name(_), Word::Bracket(_), v, ..] if assigns(v) => true,
        _ => false,
    }
}

pub fn load_and_run_script(path: &str, ktree: &Arc<KTree>) -> Result<(), KonaError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| KonaError::File(format!("Failed to read script {}: {}", path, e)))?;

    let depth = LOAD_DEPTH.with(|d| d.get());
    if depth >= MAX_LOAD_DEPTH {
        return Err(KonaError::Limit);
    }
    LOAD_DEPTH.with(|d| d.set(depth + 1));
    // \d inside a script is the script's own: C Kona's load saves the directory and puts it back
    let saved_d = ktree.lookup("_d");
    let res = run_script_lines(&content, ktree);
    ktree.assign(
        "_d".to_string(),
        saved_d.unwrap_or_else(|| K::symbol(".k".to_string())),
    );
    LOAD_DEPTH.with(|d| d.set(depth));
    res
}

fn run_script_lines(content: &str, ktree: &Arc<KTree>) -> Result<(), KonaError> {
    let mut env = HashMap::new();
    let mut accumulator = String::new();
    let mut brace_depth = 0;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }

        if STEP_SCRIPT.load(std::sync::atomic::Ordering::Relaxed) {
            println!("step> {}", line);
        }

        if brace_depth == 0 && trimmed.starts_with('\\') {
            // \\ ends the script, not the process: a C Kona script ends with \\ to leave the prompt
            // it drops into after loading, and a library host (the Unreal bridge) has to survive it
            if trimmed == "\\\\" {
                return Ok(());
            }
            handle_system_command(trimmed, ktree, &mut env);
            continue;
        }

        accumulator.push_str(line);
        accumulator.push('\n');

        brace_depth += count_braces(line);
        if brace_depth <= 0 {
            let code = accumulator.trim();
            if !code.is_empty() {
                let words = crate::parser::parse(code)?;
                let _ = crate::eval::eval_words(&words, &mut env, ktree)?;
            }
            accumulator.clear();
            brace_depth = 0;
        }
    }

    if !accumulator.trim().is_empty() {
        let words = crate::parser::parse(&accumulator)?;
        let _ = crate::eval::eval_words(&words, &mut env, ktree)?;
    }

    Ok(())
}

/// A backslash command from the prompt, a script or `. "\\..."`. env is the caller's local
/// variables, so \t sees them.
pub fn handle_system_command(cmd: &str, ktree: &Arc<KTree>, env: &mut HashMap<String, K>) {
    let trimmed = cmd.trim();
    if trimmed == "\\\\" {
        std::process::exit(0);
    }
    if trimmed == "\\" || trimmed == "\\?" || trimmed == "\\help" {
        println!("System commands:");
        println!("  \\\\         - Exit the interpreter");
        println!("  \\+         - Show primitive verbs list and help");
        println!("  \\'         - Show adverbs help");
        println!("  \\v         - List all defined variables");
        println!("  \\l file.k  - Load and run script file (file or file.k)");
        println!("  \\t expr    - Time execution of expression");
        println!("  \\p [n]     - Show/set print precision");
        println!("  \\r [seed]  - Show/set random seed");
        println!("  \\s file.k  - Step-through run script file");
        println!("  \\d [ns]    - Show/set namespace directory");
        return;
    }
    if trimmed == "\\+" {
        println!("Primitive Verbs:");
        println!("  +  add/flip      -  subtract/negate  *  multiply/first  %  divide/reciprocal");
        println!("  &  min/where     |  max/reverse      <  less/grade up   >  more/grade down");
        println!("  =  equal/group   ~  match/not        ,  join/enlist     #  take/count");
        println!("  _  drop/cut      !  key/enumerate    @  at/type         .  value/dot");
        println!("  0: read/write text    1: read/write binary    2: (C library loading is disabled in this port)");
        println!("  _sys (lib;fn;args)    the door: 1 GPU, 2 C++, 3 C++ server, 4 BCPL, 0 = several at once");
        return;
    }
    if trimmed == "\\'" || trimmed == "\\/" {
        println!("Adverbs:");
        println!("  '   each         /   over           \\   scan");
        println!("  /:  each-right   \\:  each-left      ':  each-pair");
        return;
    }
    if trimmed == "\\v" {
        let mut vars = ktree.keys();
        vars.sort();
        if vars.is_empty() {
            println!("No global variables defined.");
        } else {
            println!("Defined global variables:");
            for var in vars {
                println!("  {}", var);
            }
        }
        return;
    }
    if let Some(code) = trimmed.strip_prefix("\\t ") {
        // the time only, as C Kona's \t: printing the result too dumped 4M numbers per bench line
        match crate::parser::parse(code.trim()) {
            Ok(words) => {
                let start = std::time::Instant::now();
                match crate::eval::eval_words(&words, env, ktree) {
                    Ok(_) => {
                        let elapsed = start.elapsed();
                        println!(
                            "Time: {} ms ({} us)",
                            elapsed.as_millis(),
                            elapsed.as_micros()
                        );
                    }
                    Err(e) => eprintln!("{}", e),
                }
            }
            Err(e) => eprintln!("{}", e),
        }
        return;
    }
    if let Some(name) = trimmed.strip_prefix("\\l ") {
        let path = resolve_script_path(name.trim());
        if let Err(e) = load_and_run_script(&path, ktree) {
            eprintln!("Error loading script {}: {:?}", path, e);
        }
        return;
    }
    if let Some(name) = trimmed.strip_prefix("\\s ") {
        let path = resolve_script_path(name.trim());
        STEP_SCRIPT.store(true, std::sync::atomic::Ordering::Relaxed);
        let res = load_and_run_script(&path, ktree);
        STEP_SCRIPT.store(false, std::sync::atomic::Ordering::Relaxed);
        if let Err(e) = res {
            eprintln!("Error stepping script {}: {:?}", path, e);
        }
        return;
    }
    if trimmed.starts_with("\\d ") || trimmed == "\\d" {
        if trimmed == "\\d" {
            let current_d = ktree
                .lookup("_d")
                .and_then(|k| k.as_symbol().map(|s| s.to_string()))
                .unwrap_or_else(|| ".k".to_string());
            println!("{}", current_d);
        } else {
            let ns = trimmed["\\d ".len()..].trim();
            let ns_sym = if ns.is_empty() {
                ".k".to_string()
            } else if ns.starts_with('.') {
                ns.to_string()
            } else {
                format!(".{}", ns)
            };
            ktree.assign("_d".to_string(), K::symbol(ns_sym));
        }
        return;
    }
    if trimmed.starts_with("\\p ") || trimmed == "\\p" {
        if trimmed == "\\p" {
            let p = crate::io::PRINT_PRECISION.load(std::sync::atomic::Ordering::Relaxed);
            println!("{}", p);
        } else {
            let arg = trimmed["\\p ".len()..].trim();
            if let Ok(p) = arg.parse::<usize>() {
                crate::io::PRINT_PRECISION.store(p, std::sync::atomic::Ordering::Relaxed);
                ktree.assign("_p".to_string(), K::int(p as i64));
            } else {
                eprintln!("Invalid precision: {}", arg);
            }
        }
        return;
    }
    if trimmed.starts_with("\\r ") || trimmed == "\\r" {
        if trimmed == "\\r" {
            let seed = crate::verbs::math::get_prng_seed();
            println!("{}", seed);
        } else {
            let arg = trimmed["\\r ".len()..].trim();
            if let Ok(seed) = arg.parse::<u64>() {
                crate::verbs::math::set_prng_seed(seed);
            } else {
                eprintln!("Invalid seed: {}", arg);
            }
        }
        return;
    }

    println!("Unknown system command: {}. Type \\ for help.", trimmed);
}
