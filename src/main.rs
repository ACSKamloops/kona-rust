#![forbid(unsafe_code)]
// main.rs - kona.exe: the REPL and the command line. Everything else lives in the library (lib.rs).

use kona::{handle_system_command, is_assignment_statement, load_and_run_script};
use kona::types::K;
use kona::verbs::dict::KTree;
use std::collections::HashMap;
use std::sync::Arc;

fn count_braces(s: &str) -> i32 {
    kona::count_braces(s)
}

/// Parse, run and show one line the way the prompt does: errors to stderr, nothing for nil or
/// for an assignment (C Kona prints nothing for a:1 2 3). Returns false on an error.
fn run_line(code: &str, env: &mut HashMap<String, K>, ktree: &Arc<KTree>) -> bool {
    let words = match kona::parser::parse(code) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("{}", e);
            return false;
        }
    };
    match kona::eval::eval_words(&words, env, ktree) {
        Ok(res) => {
            if !res.is_nil() && !is_assignment_statement(&words) {
                println!("{}", kona::io::format_k(&res));
            }
            true
        }
        Err(e) => {
            eprintln!("{}", e);
            false
        }
    }
}

fn run_repl(ktree: Arc<KTree>) {
    use std::io::{self, Write};

    let mut env = HashMap::new();
    let mut accumulator = String::new();
    let mut brace_depth = 0;

    println!("Kona K3 Interpreter (Rust Port)");
    println!("Type \\ for help, \\\\ to exit.");

    loop {
        print!("  ");
        let _ = io::stdout().flush();

        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() || line.is_empty() {
            break;
        }

        let trimmed = line.trim();
        if brace_depth == 0 {
            if trimmed == "\\\\" {
                break;
            }
            if trimmed.starts_with('\\') {
                handle_system_command(trimmed, &ktree, &mut env);
                continue;
            }
        }

        accumulator.push_str(&line);
        brace_depth += count_braces(&line);

        if brace_depth <= 0 {
            let code = accumulator.trim();
            if !code.is_empty() {
                run_line(code, &mut env, &ktree);
            }
            accumulator.clear();
            brace_depth = 0;
        }
    }
}

fn usage_exit(msg: &str) -> ! {
    eprintln!("{}", msg);
    eprintln!("usage: kona [-h port | -i [host:]port] [-e expr | -x expr] [script.k ...]");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ktree = std::sync::Arc::new(KTree::new());

    // _a: the command line as a list of strings, as C Kona has it (("kona";"-i";"5932") ...)
    let k_args = K::general_list(
        args.iter()
            .map(|s| K::char_list(s.as_bytes().to_vec()))
            .collect(),
    );
    ktree.assign("_a".to_string(), k_args);

    // every script named on the command line runs, in order (C Kona loads each; only the last used to)
    let mut script_paths: Vec<&String> = Vec::new();
    let mut http_port = None;
    let mut ipc_port = None;
    let mut execute_expr = None;
    let mut print_expr = None;

    let mut i = 1;
    while i < args.len() {
        let flag = args[i].as_str();
        if matches!(flag, "-h" | "-i" | "-e" | "-x") {
            if i + 1 >= args.len() {
                usage_exit(&format!("{} needs a value", flag));
            }
            let value = &args[i + 1];
            match flag {
                "-h" => match value.parse::<u16>() {
                    Ok(port) => http_port = Some(port),
                    Err(_) => usage_exit(&format!("Invalid port number: {}", value)),
                },
                "-i" => {
                    // "-i 5931" or C Kona's "-i 127.0.0.1:5931" (the server always listens on 127.0.0.1 only)
                    let port_text = value.rsplit(':').next().unwrap_or("");
                    match port_text.parse::<u16>() {
                        Ok(port) => ipc_port = Some(port),
                        Err(_) => usage_exit(&format!("Invalid port number: {}", value)),
                    }
                }
                "-e" => execute_expr = Some(value),
                _ => print_expr = Some(value),
            }
            i += 2;
        } else {
            script_paths.push(&args[i]);
            i += 1;
        }
    }
    let port_val = if let Some(port) = http_port {
        K::int(port as i64)
    } else if let Some(port) = ipc_port {
        K::int(port as i64)
    } else {
        K::int(0)
    };
    ktree.assign(".k.port".to_string(), port_val);

    if let Some(code) = execute_expr {
        let mut env = HashMap::new();
        match kona::parser::parse(code) {
            Ok(words) => {
                if let Err(e) = kona::eval::eval_words(&words, &mut env, &ktree) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        std::process::exit(0);
    }

    if let Some(code) = print_expr {
        let mut env = HashMap::new();
        let ok = run_line(code, &mut env, &ktree);
        std::process::exit(if ok { 0 } else { 1 });
    }

    for path in &script_paths {
        if let Err(e) = load_and_run_script(path, &ktree) {
            eprintln!("Error executing script {}: {:?}", path, e);
            std::process::exit(1);
        }
    }

    if let Some(port) = http_port {
        kona::io::start_http_server(port, ktree);
    } else if let Some(port) = ipc_port {
        kona::io::start_ipc_server(port, ktree);
    } else if script_paths.is_empty() {
        run_repl(ktree);
    }
}
