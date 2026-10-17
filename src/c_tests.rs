#![cfg(test)]

use crate::errors::KonaError;
use crate::types::{K, KData};
use crate::verbs::dict::KTree;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

// 117 on 2026-10-15; 106 on 2026-10-16 after the semantic fixes (each-pair, -1#x, _in, bracket
// chains, verb+lambda, flip, first, x!0, ...). Cases where both sides error are counted apart.
const MAX_C_SUITE_FAILURES: usize = 95;
// Passes must not drop either: a change that makes both sides of many cases error would lower
// `failed` while moving cases into both_errored, and the ratchet above would not notice.
const MIN_C_SUITE_PASSED: usize = 1065;

fn parse_c_string(s: &str) -> Option<(String, usize)> {
    let mut chars = s.char_indices().peekable();
    let mut start_idx = None;
    while let Some(&(idx, c)) = chars.peek() {
        if c == '"' {
            start_idx = Some(idx);
            chars.next();
            break;
        }
        if !c.is_whitespace() {
            return None;
        }
        chars.next();
    }
    let _start = start_idx?;
    let mut content = String::new();
    let mut escaped = false;
    let mut end_idx = None;
    while let Some((idx, c)) = chars.next() {
        if escaped {
            match c {
                'n' => content.push('\n'),
                't' => content.push('\t'),
                'r' => content.push('\r'),
                'b' => content.push('\x08'),
                'f' => content.push('\x0c'),
                'v' => content.push('\x0b'),
                'a' => content.push('\x07'),
                '\\' => content.push('\\'),
                '"' => content.push('"'),
                '\'' => content.push('\''),
                '?' => content.push('?'),
                _ => {
                    content.push(c);
                }
            }
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            end_idx = Some(idx);
            break;
        } else {
            content.push(c);
        }
    }
    let end = end_idx?;
    Some((content, end + 1))
}

fn get_inside_parentheses<I>(chars: &mut std::iter::Peekable<I>) -> Option<String>
where
    I: Iterator<Item = (usize, char)>,
{
    let mut depth = 1;
    let mut inside = String::new();
    let mut in_str = false;
    let mut escaped = false;

    while let Some((_, c)) = chars.next() {
        if in_str {
            inside.push(c);
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
                inside.push(c);
            } else if c == '(' {
                depth += 1;
                inside.push(c);
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    return Some(inside);
                }
                inside.push(c);
            } else {
                inside.push(c);
            }
        }
    }
    None
}

fn split_by_top_level_comma(s: &str) -> Option<(String, String)> {
    let mut depth_paren = 0;
    let mut depth_bracket = 0;
    let mut depth_brace = 0;
    let mut in_str = false;
    let mut escaped = false;

    for (idx, c) in s.char_indices() {
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
            } else if c == '(' {
                depth_paren += 1;
            } else if c == ')' {
                depth_paren -= 1;
            } else if c == '[' {
                depth_bracket += 1;
            } else if c == ']' {
                depth_bracket -= 1;
            } else if c == '{' {
                depth_brace += 1;
            } else if c == '}' {
                depth_brace -= 1;
            } else if c == ',' && depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 {
                let arg1 = s[..idx].trim().to_string();
                let arg2 = s[idx + 1..].trim().to_string();
                return Some((arg1, arg2));
            }
        }
    }
    None
}

struct TestCase {
    lhs: String,
    rhs: String,
    line_num: usize,
    #[allow(dead_code)]
    is_quoted: bool,
}

fn parse_tests(content: &str) -> Vec<TestCase> {
    let mut results = Vec::new();
    let mut chars = content.char_indices().peekable();

    // Track current line number
    let mut line_num = 1;
    let mut last_idx = 0;

    while let Some(&(idx, c)) = chars.peek() {
        // Count lines
        while last_idx < idx {
            if content.as_bytes()[last_idx] == b'\n' {
                line_num += 1;
            }
            last_idx += 1;
        }

        if c == '#' {
            chars.next();
            while let Some((_, next_c)) = chars.next() {
                if next_c == '\n' || next_c == '\r' {
                    line_num += 1;
                    break;
                }
            }
            last_idx = last_idx.max(chars.peek().map(|&(i, _)| i).unwrap_or(content.len()));
            continue;
        }

        if c == '/' {
            chars.next();
            if let Some(&(_, '/')) = chars.peek() {
                chars.next();
                // Skip comment line
                while let Some((_, next_c)) = chars.next() {
                    if next_c == '\n' || next_c == '\r' {
                        line_num += 1;
                        last_idx =
                            last_idx.max(chars.peek().map(|&(i, _)| i).unwrap_or(content.len()));
                        break;
                    }
                }
            } else if let Some(&(_, '*')) = chars.peek() {
                chars.next();
                // Skip block comment
                while let Some((_, next_c)) = chars.next() {
                    if next_c == '\n' {
                        line_num += 1;
                    }
                    if next_c == '*' {
                        if let Some(&(_, '/')) = chars.peek() {
                            chars.next();
                            break;
                        }
                    }
                }
                last_idx = last_idx.max(chars.peek().map(|&(i, _)| i).unwrap_or(content.len()));
            }
            continue;
        }

        if content[idx..].starts_with("TC_(") {
            for _ in 0..4 {
                chars.next();
            }
            if let Some(inside) = get_inside_parentheses(&mut chars) {
                if let Some((arg1, len1)) = parse_c_string(&inside) {
                    let rest = &inside[len1..];
                    let rest_trimmed = rest.trim_start();
                    if rest_trimmed.starts_with(',') {
                        let rest2 = &rest_trimmed[1..];
                        if let Some((arg2, _)) = parse_c_string(rest2) {
                            results.push(TestCase {
                                lhs: arg1,
                                rhs: arg2,
                                line_num,
                                is_quoted: true,
                            });
                        }
                    }
                }
            }
            last_idx = last_idx.max(chars.peek().map(|&(i, _)| i).unwrap_or(content.len()));
            continue;
        } else if content[idx..].starts_with("TC(") {
            for _ in 0..3 {
                chars.next();
            }
            if let Some(inside) = get_inside_parentheses(&mut chars) {
                if let Some((arg1, arg2)) = split_by_top_level_comma(&inside) {
                    results.push(TestCase {
                        lhs: arg1.replace("\\\\", "\\"),
                        rhs: arg2.replace("\\\\", "\\"),
                        line_num,
                        is_quoted: false,
                    });
                }
            }
            last_idx = last_idx.max(chars.peek().map(|&(i, _)| i).unwrap_or(content.len()));
            continue;
        }

        if c == '"' {
            chars.next();
            while let Some((_, next_c)) = chars.next() {
                if next_c == '\n' {
                    line_num += 1;
                }
                if next_c == '\\' {
                    if let Some((_, escaped_c)) = chars.next() {
                        if escaped_c == '\n' {
                            line_num += 1;
                        }
                    }
                } else if next_c == '"' {
                    break;
                }
            }
            last_idx = last_idx.max(chars.peek().map(|&(i, _)| i).unwrap_or(content.len()));
            continue;
        }

        chars.next();
    }

    results
}

fn eval_k(code: &str, env: &mut HashMap<String, K>, ktree: &Arc<KTree>) -> Result<K, KonaError> {
    let words = crate::parser::parse(code)?;
    crate::eval::eval_words(&words, env, ktree)
}

fn values_match(a: &K, b: &K) -> bool {
    // Custom matching logic for K values
    match (&**a, &**b) {
        (KData::FloatAtom(x), KData::FloatAtom(y)) => {
            if x.is_nan() && y.is_nan() {
                true
            } else if x.is_infinite() && y.is_infinite() {
                x.is_sign_positive() == y.is_sign_positive()
            } else {
                // Tolerance comparison: tolerance is ~ 1e-11 in C Kona
                (x - y).abs() <= 1e-9
            }
        }
        (KData::FloatList(x), KData::FloatList(y)) => {
            if x.len() != y.len() {
                false
            } else {
                x.iter().zip(y.iter()).all(|(&xv, &yv)| {
                    if xv.is_nan() && yv.is_nan() {
                        true
                    } else if xv.is_infinite() && yv.is_infinite() {
                        xv.is_sign_positive() == yv.is_sign_positive()
                    } else {
                        (xv - yv).abs() <= 1e-9
                    }
                })
            }
        }
        (KData::GeneralList(x), KData::GeneralList(y)) => {
            if x.len() != y.len() {
                false
            } else {
                x.iter().zip(y.iter()).all(|(xv, yv)| values_match(xv, yv))
            }
        }
        _ => a == b,
    }
}

#[test]
fn test_c_suite_comparison() {
    let paths = [
        "src/tests.c",
        "../kona-master3/src/tests.c",
        "../kona-master/src/tests.c",
        "/home/juker/Desktop/kona-master3/src/tests.c",
    ];

    let mut tests_c_path = None;
    for p in &paths {
        if Path::new(p).exists() {
            tests_c_path = Some(*p);
            break;
        }
    }

    let path = match tests_c_path {
        Some(p) => p,
        None => {
            println!("tests.c not found. Skipping C suite comparison.");
            return;
        }
    };

    let content = fs::read_to_string(path).expect("Failed to read tests.c");
    let test_cases = parse_tests(&content);

    println!("Parsed {} test cases from {}", test_cases.len(), path);

    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut both_errored = 0;
    let mut fail_reports = Vec::new();

    for tc in test_cases {
        // Skip tests that are known to use C specific functionality or network, or marked skip
        if tc.lhs == "skip" || tc.rhs == "skip" {
            skipped += 1;
            continue;
        }
        if tc.lhs.contains("google.com") || tc.rhs.contains("google.com") {
            skipped += 1;
            continue;
        }
        // Skip DNS host lookup if it fails (localhost is fine, but network hosts might be blocked)
        if tc.lhs.contains("_host") && !tc.lhs.contains("localhost") {
            skipped += 1;
            continue;
        }

        eprintln!(
            "Running test at line {}: {} ~ {}",
            tc.line_num, tc.lhs, tc.rhs
        );
        use std::io::Write;
        std::io::stderr().flush().unwrap();
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        let res_lhs = eval_k(&tc.lhs, &mut env, &ktree);
        let res_rhs = eval_k(&tc.rhs, &mut env, &ktree);

        match (res_lhs, res_rhs) {
            (Ok(lhs_val), Ok(rhs_val)) => {
                if values_match(&lhs_val, &rhs_val) {
                    passed += 1;
                } else {
                    failed += 1;
                    let report = format!(
                        "Mismatch at line {}:\n  LHS code: {}\n  RHS code: {}\n  LHS val: {:?}\n  RHS val: {:?}",
                        tc.line_num, tc.lhs, tc.rhs, lhs_val, rhs_val
                    );
                    fail_reports.push(report);
                }
            }
            (Err(e_lhs), Ok(_)) => {
                failed += 1;
                let report = format!(
                    "LHS Error at line {}:\n  LHS code: {}\n  RHS code: {}\n  Error: {:?}",
                    tc.line_num, tc.lhs, tc.rhs, e_lhs
                );
                fail_reports.push(report);
            }
            (Ok(_), Err(e_rhs)) => {
                failed += 1;
                let report = format!(
                    "RHS Error at line {}:\n  LHS code: {}\n  RHS code: {}\n  Error: {:?}",
                    tc.line_num, tc.lhs, tc.rhs, e_rhs
                );
                fail_reports.push(report);
            }
            (Err(_e_lhs), Err(_e_rhs)) => {
                // Every case in tests.c passes in C Kona, so both sides erroring is not a pass; it is
                // counted on its own line (not in the ratchet) so a verb raising the wrong error, or a
                // regression that breaks both sides at once, stays visible
                both_errored += 1;
            }
        }
    }

    println!("Test Comparison Summary:");
    println!("  Total:   {}", passed + failed + skipped + both_errored);
    println!("  Passed:  {}", passed);
    println!("  Failed:  {}", failed);
    println!("  Both sides errored (not counted as pass): {}", both_errored);
    println!("  Skipped: {}", skipped);

    if passed < MIN_C_SUITE_PASSED {
        panic!(
            "C suite comparison regressed: {} passes is below the floor of {}",
            passed, MIN_C_SUITE_PASSED
        );
    }

    if failed > 0 {
        println!("\n--- Failed Cases ---");
        let report_limit = if std::env::var("KONA_ALL_FAILURES").is_ok() {
            fail_reports.len()
        } else {
            std::cmp::min(10, fail_reports.len())
        };
        for r in &fail_reports[..report_limit] {
            println!("{}", r);
            println!("--------------------");
        }
        if fail_reports.len() > 10 {
            println!("... and {} more failures", fail_reports.len() - 10);
        }
        if failed > MAX_C_SUITE_FAILURES {
            panic!(
                "C suite comparison regressed: {} mismatches exceeds max {}",
                failed, MAX_C_SUITE_FAILURES
            );
        }
        println!(
            "C suite comparison accepted: {} mismatches <= max {}",
            failed, MAX_C_SUITE_FAILURES
        );
    }
}
