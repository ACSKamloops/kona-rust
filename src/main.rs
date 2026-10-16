#![forbid(unsafe_code)]
// main.rs - kona.exe: the REPL and the command line. Everything else lives in the library (lib.rs).

use kona::{handle_system_command, load_and_run_script};
use kona::types::K;
use kona::verbs::dict::KTree;
use std::collections::HashMap;
use std::sync::Arc;

fn count_braces(s: &str) -> i32 {
    kona::count_braces(s)
}

fn run_repl(ktree: Arc<KTree>) {
    use std::io::{self, Write};

    let mut env = HashMap::new();
    let mut accumulator = String::new();
    let mut brace_depth = 0;

    println!("Kona K3 Interpreter (Rust Port)");
    println!("Type \\ for help, \\\\ to exit.");

    loop {
        if brace_depth == 0 {
            print!("  ");
        } else {
            print!("  ");
        }
        let _ = io::stdout().flush();

        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            break;
        }

        if line.is_empty() {
            break;
        }

        let trimmed = line.trim();
        if brace_depth == 0 {
            if trimmed == "\\\\" {
                break;
            }
            if trimmed.starts_with("\\t ") {
                let code = trimmed["\\t ".len()..].trim();
                match kona::parser::parse(code) {
                    Ok(words) => {
                        let start = std::time::Instant::now();
                        match kona::eval::eval_words(&words, &mut env, &ktree) {
                            Ok(res) => {
                                let elapsed = start.elapsed();
                                if !res.is_nil() {
                                    println!("{}", kona::io::format_k(&res));
                                }
                                println!(
                                    "Time: {} ms ({} us)",
                                    elapsed.as_millis(),
                                    elapsed.as_micros()
                                );
                            }
                            Err(e) => {
                                eprintln!("{}", e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", e);
                    }
                }
                continue;
            }
            if trimmed.starts_with('\\') {
                handle_system_command(trimmed, &ktree);
                continue;
            }
        }

        accumulator.push_str(&line);
        brace_depth += count_braces(&line);

        if brace_depth <= 0 {
            let code = accumulator.trim();
            if !code.is_empty() {
                match kona::parser::parse(code) {
                    Ok(words) => match kona::eval::eval_words(&words, &mut env, &ktree) {
                        Ok(res) => {
                            if !res.is_nil() {
                                println!("{}", kona::io::format_k(&res));
                            }
                        }
                        Err(e) => {
                            eprintln!("{}", e);
                        }
                    },
                    Err(e) => {
                        eprintln!("{}", e);
                    }
                }
            }
            accumulator.clear();
            brace_depth = 0;
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ktree = std::sync::Arc::new(KTree::new());

    // Register actual program arguments _a in global KTree
    let k_args = K::symbol_list(
        args.iter()
            .map(|s| std::sync::Arc::from(s.as_str()))
            .collect(),
    );
    ktree.assign("_a".to_string(), k_args);

    let mut script_path = None;
    let mut http_port = None;
    let mut ipc_port = None;
    let mut execute_expr = None;
    let mut print_expr = None;

    let mut i = 1;
    while i < args.len() {
        if args[i] == "-h" && i + 1 < args.len() {
            if let Ok(port) = args[i + 1].parse::<u16>() {
                http_port = Some(port);
            } else {
                eprintln!("Invalid port number: {}", args[i + 1]);
                std::process::exit(1);
            }
            i += 2;
        } else if args[i] == "-i" && i + 1 < args.len() {
            // "-i 5931" or C Kona's "-i 127.0.0.1:5931" (the server always listens on 127.0.0.1 only)
            let port_text = args[i + 1].rsplit(':').next().unwrap_or("");
            if let Ok(port) = port_text.parse::<u16>() {
                ipc_port = Some(port);
            } else {
                eprintln!("Invalid port number: {}", args[i + 1]);
                std::process::exit(1);
            }
            i += 2;
        } else if args[i] == "-e" && i + 1 < args.len() {
            execute_expr = Some(&args[i + 1]);
            i += 2;
        } else if args[i] == "-x" && i + 1 < args.len() {
            print_expr = Some(&args[i + 1]);
            i += 2;
        } else {
            script_path = Some(&args[i]);
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
        match kona::parser::parse(code) {
            Ok(words) => match kona::eval::eval_words(&words, &mut env, &ktree) {
                Ok(res) => {
                    if !res.is_nil() {
                        println!("{}", kona::io::format_k(&res));
                    }
                }
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        std::process::exit(0);
    }

    if let Some(path) = script_path {
        if let Err(e) = load_and_run_script(path, &ktree) {
            eprintln!("Error executing script {}: {:?}", path, e);
            std::process::exit(1);
        }
    }

    if let Some(port) = http_port {
        kona::io::start_http_server(port, ktree);
    } else if let Some(port) = ipc_port {
        kona::io::start_ipc_server(port, ktree);
    } else if script_path.is_none() {
        run_repl(ktree);
    }
}
