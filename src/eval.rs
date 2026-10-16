#![forbid(unsafe_code)]

use crate::errors::KonaError;
use crate::parser::Word;
use crate::types::{K, KData, KFunc};
use crate::verbs::dict::KTree;
use std::collections::HashMap;
use std::sync::Arc;

thread_local! {
    pub static PENDING_ERROR: std::cell::Cell<Option<crate::errors::KonaError>> = std::cell::Cell::new(None);
    pub static IGNORE_PENDING_ERROR: std::cell::Cell<bool> = std::cell::Cell::new(false);
    static RECURSION_DEPTH: std::cell::Cell<usize> = std::cell::Cell::new(0);
    static RECENT_LOCALS: std::cell::RefCell<std::collections::HashSet<String>> = std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Nested evaluations (eval_words_flow and apply_func frames together) before a limit error. A
/// recursive K function costs about three per level, so this is ~3300 levels; C Kona manages 400
/// and stack-errors at 2000. Measured to fit an 8 MB stack with room to spare (the Linux/macOS
/// main thread; build.rs reserves the same for the Windows exe). A host embedding the library
/// (ue/) must give its calling thread that much, or lower this.
const MAX_DEPTH: usize = 10_000;

struct DepthGuard;
impl DepthGuard {
    fn new() -> Result<Self, KonaError> {
        RECURSION_DEPTH.with(|d| {
            let val = d.get();
            if val >= MAX_DEPTH {
                Err(KonaError::Limit)
            } else {
                d.set(val + 1);
                Ok(DepthGuard)
            }
        })
    }
}
impl Drop for DepthGuard {
    fn drop(&mut self) {
        RECURSION_DEPTH.with(|d| d.set(d.get() - 1));
    }
}

fn strip_param_list(body: &str) -> &str {
    let trimmed = body.trim_start();
    if trimmed.starts_with('[') {
        let mut depth = 0;
        let mut in_str = false;
        let mut escaped = false;
        for (i, c) in trimmed.char_indices() {
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
                } else if c == '[' {
                    depth += 1;
                } else if c == ']' {
                    depth -= 1;
                    if depth == 0 {
                        return &trimmed[i + 1..];
                    }
                }
            }
        }
    }
    body
}

/// Check K conditional truth. Empty vectors are type errors in K condition slots.
fn is_truthy(k: &K) -> Result<bool, KonaError> {
    match &**k {
        KData::IntAtom(i) => Ok(*i != 0),
        KData::FloatAtom(f) => Ok(*f != 0.0),
        KData::CharAtom(c) => Ok(*c != '\0'),
        KData::SymbolAtom(s) => Ok(!s.is_empty()),
        KData::IntList(v) => {
            if v.is_empty() {
                Err(KonaError::Type)
            } else {
                Ok(v.iter().any(|&x| x != 0))
            }
        }
        KData::FloatList(v) => {
            if v.is_empty() {
                Err(KonaError::Type)
            } else {
                Ok(v.iter().any(|&x| x != 0.0))
            }
        }
        KData::CharList(v) => {
            if v.is_empty() {
                Err(KonaError::Type)
            } else {
                Ok(v.iter().any(|&x| x != 0))
            }
        }
        KData::SymbolList(v) => {
            if v.is_empty() {
                Err(KonaError::Type)
            } else {
                Ok(v.iter().any(|s| !s.is_empty()))
            }
        }
        KData::GeneralList(v) => {
            if v.is_empty() {
                return Err(KonaError::Type);
            }
            let mut any = false;
            for item in v {
                any |= is_truthy(item)?;
            }
            Ok(any)
        }
        KData::HashList { indices, .. } => {
            if indices.is_empty() {
                Err(KonaError::Type)
            } else {
                Ok(indices.iter().any(|&x| x != 0))
            }
        }
        KData::Dictionary { keys, .. } => Ok(!keys.is_empty()),
        KData::Nil => Ok(false),
        KData::Function(_) => Ok(true),
    }
}

enum EvalFlow {
    Value(K),
    Return(K),
}

impl EvalFlow {
    fn into_value(self) -> K {
        match self {
            EvalFlow::Value(k) | EvalFlow::Return(k) => k,
        }
    }
}

fn eval_paren_flow(
    exprs: &[Vec<Word>],
    env: &mut HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<EvalFlow, KonaError> {
    let inserted = if !env.contains_key("__allow_undefined_nil") {
        env.insert("__allow_undefined_nil".to_string(), K::nil());
        true
    } else {
        false
    };
    let res = if exprs.is_empty() {
        Ok(EvalFlow::Value(K::general_list(vec![])))
    } else if exprs.len() == 1 {
        if exprs[0].is_empty() {
            Ok(EvalFlow::Value(K::general_list(vec![])))
        } else {
            eval_words_flow(&exprs[0], env, ktree)
        }
    } else {
        let mut elements = vec![None; exprs.len()];
        let mut err = None;
        for idx in (0..exprs.len()).rev() {
            let expr = &exprs[idx];
            if expr.is_empty() {
                elements[idx] = Some(K::nil());
                continue;
            }

            match eval_words_flow(expr, env, ktree) {
                Ok(EvalFlow::Value(k)) => elements[idx] = Some(k),
                Ok(EvalFlow::Return(k)) => {
                    return Ok(EvalFlow::Return(k));
                }
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = err {
            Err(e)
        } else {
            Ok(EvalFlow::Value(crate::verbs::math::normalize_list(
                elements.into_iter().flatten().collect(),
            )))
        }
    };
    if inserted {
        env.remove("__allow_undefined_nil");
    }
    res
}

fn is_namespace(ktree: &Arc<KTree>, ns: &str) -> bool {
    if ns == ".k" {
        return true;
    }
    if !ns.starts_with('.') || ns.is_empty() {
        return false;
    }
    let prefix = format!("{}.", ns);
    ktree.keys().iter().any(|k| k.starts_with(&prefix))
}

fn get_namespace_dict(ktree: &Arc<KTree>, ns: &str) -> K {
    let prefix = format!("{}.", ns);
    let keys_vec: Vec<String> = ktree
        .keys()
        .into_iter()
        .filter(|k| {
            if !k.starts_with(&prefix) {
                return false;
            }
            let suffix = &k[prefix.len()..];
            !suffix.contains('.') && !suffix.starts_with('_')
        })
        .collect();

    let mut symbols = Vec::new();
    let mut vals = Vec::new();
    let mut sorted_keys = keys_vec;
    sorted_keys.sort();

    for k in sorted_keys {
        if let Some(v) = ktree.lookup(&k) {
            let name_part = &k[prefix.len()..];
            symbols.push(Arc::from(name_part));
            vals.push(v);
        }
    }
    let k_keys = K::symbol_list(symbols);
    let k_vals = crate::verbs::math::normalize_list(vals);
    K::dictionary(k_keys, k_vals)
}

pub fn resolve_dotted_name(name: &str, env: &HashMap<String, K>, ktree: &Arc<KTree>) -> Option<K> {
    if let Some(val) = env.get(name) {
        return Some(val.clone());
    }
    if let Some(val) = ktree.lookup(name) {
        return Some(val);
    }
    if name.contains('.') && !name.starts_with('_') {
        let parts: Vec<&str> = name.split('.').collect();
        for i in (1..parts.len()).rev() {
            let prefix = parts[..i].join(".");
            if let Some(mut val) = env.get(&prefix).cloned().or_else(|| ktree.lookup(&prefix)) {
                let mut success = true;
                for part in &parts[i..] {
                    if let KData::Dictionary { .. } = &*val {
                        let sym = K::symbol(*part);
                        if let Ok(item) = crate::verbs::list::at(&val, &sym) {
                            val = item;
                        } else {
                            success = false;
                            break;
                        }
                    } else {
                        success = false;
                        break;
                    }
                }
                if success {
                    return Some(val);
                }
            }
        }
    }
    None
}

fn assign_var(
    name: String,
    val: K,
    env: &mut HashMap<String, K>,
    ktree: &Arc<KTree>,
    force_global: bool,
) -> Result<(), KonaError> {
    if name.contains('.') && !name.starts_with('_') {
        let parts: Vec<&str> = name.split('.').collect();
        // Check if prefix already exists
        let prefix_found = false;
        for i in (1..parts.len()).rev() {
            let prefix = parts[..i].join(".");
            if let Some(target) = env.get(&prefix).cloned().or_else(|| ktree.lookup(&prefix)) {
                if matches!(*target, KData::Dictionary { .. }) {
                    let path: Vec<K> = parts[i..].iter().map(|p| K::symbol(*p)).collect();
                    let assign_func = K::function(KFunc::Primitive(":".to_string()));
                    let updated =
                        apply_amend_deep(&target, &path, &assign_func, Some(&val), env, ktree)?;
                    let write_local = if force_global {
                        env.contains_key("_local") && env.contains_key(&prefix)
                    } else {
                        env.contains_key("_local")
                    };
                    if write_local {
                        env.insert(prefix, updated);
                    } else {
                        ktree.assign(prefix, updated);
                    }
                    return Ok(());
                } else {
                    return Err(KonaError::Type);
                }
            }
        }

        // If no prefix was found at all, create the root prefix as a dictionary
        if !prefix_found {
            let start_idx = if name.starts_with('.') { 2 } else { 1 };
            if parts.len() >= start_idx {
                let root_prefix = parts[..start_idx].join(".");
                let target = K::dictionary(K::symbol_list(vec![]), K::general_list(vec![]));
                let path: Vec<K> = parts[start_idx..].iter().map(|p| K::symbol(*p)).collect();
                let assign_func = K::function(KFunc::Primitive(":".to_string()));
                let updated =
                    apply_amend_deep(&target, &path, &assign_func, Some(&val), env, ktree)?;

                let write_local = if force_global {
                    env.contains_key("_local") && env.contains_key(&root_prefix)
                } else {
                    env.contains_key("_local")
                };
                if write_local {
                    env.insert(root_prefix, updated);
                } else {
                    ktree.assign(root_prefix, updated);
                }
                return Ok(());
            }
        }
    }
    let write_local = if force_global {
        env.contains_key("_local") && env.contains_key(&name)
    } else {
        env.contains_key("_local")
    };
    if write_local {
        env.insert(name, val);
    } else {
        ktree.assign(name, val);
    }
    Ok(())
}

fn get_assignment_return_value(target: &K, index: &K, val: &K) -> K {
    let is_nil = index.is_nil();
    let n_opt = if is_nil {
        target.len().ok()
    } else if index.is_list() {
        index.len().ok()
    } else {
        None
    };

    if let Some(n) = n_opt {
        if val.is_list() && val.len().ok() == Some(n) {
            val.clone()
        } else {
            K::general_list(vec![val.clone(); n])
        }
    } else {
        val.clone()
    }
}

fn register_immediate_assignments(
    words: &[Word],
    env: &mut HashMap<String, K>,
    ktree: &Arc<KTree>,
) {
    let is_local = env.contains_key("_local");
    for w in words {
        match w {
            Word::LocalAssign(name) => {
                if !name.starts_with('_') && !name.starts_with('.') {
                    if !is_local {
                        if ktree.lookup(name).is_none() {
                            ktree.assign(name.clone(), K::nil());
                        }
                    } else {
                        if !env.contains_key(name) {
                            env.insert(name.clone(), K::nil());
                        }
                    }
                }
            }
            Word::GlobalAssign(name) => {
                if !name.starts_with('_') && !name.starts_with('.') {
                    if ktree.lookup(name).is_none() {
                        ktree.assign(name.clone(), K::nil());
                    }
                }
            }
            _ => {}
        }
    }
}

/// Look up a variable in the local environment or the global KTree.
fn lookup_var(name: &str, env: &HashMap<String, K>, ktree: &Arc<KTree>) -> Result<K, KonaError> {
    if let Some(val) = resolve_dotted_name(name, env, ktree) {
        Ok(val)
    } else {
        match name {
            "_t" => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                let seconds = now.as_secs() as i64;
                let k_epoch_offset = 2_051_222_400;
                Ok(K::int(seconds - k_epoch_offset))
            }
            "_T" => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                let seconds = now.as_secs() as i64;
                let subsec_nanos = now.subsec_nanos() as f64;
                let k_epoch_offset = 2_051_222_400;
                let seconds_since_k = seconds - k_epoch_offset;
                let total_seconds = (seconds_since_k as f64) + (subsec_nanos / 1_000_000_000.0);
                Ok(K::float(total_seconds / 86400.0))
            }
            "_h" => {
                let host = std::env::var("HOSTNAME")
                    .or_else(|_| std::env::var("HOST"))
                    .or_else(|_| std::env::var("COMPUTERNAME"))
                    .or_else(|_| std::fs::read_to_string("/proc/sys/kernel/hostname"))
                    .or_else(|_| std::fs::read_to_string("/etc/hostname"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| "localhost".to_string());
                Ok(K::symbol(host))
            }
            "_u" => {
                let user = std::env::var("USER")
                    .or_else(|_| std::env::var("LOGNAME"))
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_else(|_| "user".to_string());
                Ok(K::symbol(user))
            }
            "_n" => Ok(K::nil()),
            "_k" => Ok(K::char_list(b"2026.05.25".to_vec())),
            "_s" => Ok(K::int_list(vec![0, 0, 0, 0, 0])),
            "_w" => Ok(K::int_list(vec![])),
            "_c" => Ok(K::int(0)),
            "_v" | "_i" | "_m" => Ok(K::nil()),
            "_f" => Ok(env.get("_f").cloned().unwrap_or_else(K::nil)),
            "_d" | "_a" | "_p" => {
                if let Some(val) = ktree.lookup(name) {
                    Ok(val)
                } else {
                    match name {
                        "_d" => Ok(K::symbol(".k")),
                        "_a" => Ok(K::symbol_list(vec![])),
                        "_p" => Ok(K::int(7)),
                        _ => unreachable!(),
                    }
                }
            }
            ".k" => Ok(get_namespace_dict(ktree, ".k")),
            _ => {
                if let Some(val) = ktree.lookup(name) {
                    Ok(val)
                } else if is_namespace(ktree, name) {
                    Ok(get_namespace_dict(ktree, name))
                } else if env.contains_key("_local") {
                    Ok(K::nil())
                } else if env.contains_key("__allow_undefined_nil") {
                    if !name.starts_with('_') && !name.starts_with('.') {
                        ktree.assign(name.to_string(), K::nil());
                    }
                    Ok(K::nil())
                } else {
                    let is_recent_local = RECENT_LOCALS.with(|rl| rl.borrow().contains(name));
                    if is_recent_local {
                        if !name.starts_with('_') && !name.starts_with('.') {
                            ktree.assign(name.to_string(), K::nil());
                        }
                        Ok(K::nil())
                    } else {
                        Err(KonaError::Value(format!("Undefined variable: {}", name)))
                    }
                }
            }
        }
    }
}

/// Apply a verb (primitive) to left and right arguments.
fn apply_verb(
    name: &str,
    left: Option<&K>,
    right: &K,
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    if name == ":" {
        Ok(right.clone())
    } else if name.ends_with(':')
        && name.len() > 1
        && !matches!(name, "0:" | "1:" | "2:" | "3:" | "4:" | "5:" | "6:")
    {
        if left.is_some() {
            Err(KonaError::Valence)
        } else {
            let base_name = &name[..name.len() - 1];
            apply_verb(base_name, None, right, env, ktree)
        }
    } else if let Some(left_val) = left {
        if name == "@" {
            if let KData::Dictionary { keys, values } = &**left_val {
                if let KData::CharList(v) = &**right {
                    let s = std::str::from_utf8(v).map_err(|_| KonaError::Type)?;
                    let words = crate::parser::parse(s)?;
                    let mut eval_env = env.clone();
                    let keys_vec = crate::verbs::math::to_vec_of_atoms(keys)?;
                    let vals_vec = crate::verbs::math::to_vec_of_atoms(values)?;
                    for (k, val) in keys_vec.iter().zip(vals_vec.iter()) {
                        if let KData::SymbolAtom(sym) = &**k {
                            eval_env.insert(sym.to_string(), val.clone());
                        }
                    }
                    return eval_words(&words, &mut eval_env, ktree);
                }
            }
        }
        if name == "@" && matches!(**left_val, KData::Function(_)) {
            return apply_func(left_val, &[right.clone()], env, ktree);
        }
        if name == "." && matches!(**left_val, KData::Function(_)) {
            let args_vec = crate::verbs::math::to_vec_of_atoms(right)?;
            return apply_func(left_val, &args_vec, env, ktree);
        }
        if (name == "@" || name == ".") && matches!(**left_val, KData::SymbolAtom(_)) {
            if let KData::SymbolAtom(sym) = &**left_val {
                if let Some(target) = env.get(sym.as_ref()).cloned().or_else(|| ktree.lookup(sym)) {
                    return crate::verbs::dispatch_dyadic(name, &target, right);
                }
                return Ok(right.clone());
            }
        }
        crate::verbs::dispatch_dyadic(name, left_val, right)
    } else {
        crate::verbs::dispatch_monadic(name, right, ktree)
    }
}

/// Evaluate bracket conditional application: :[cond1;ret1;cond2;ret2;...;ret_else]
fn eval_conditional_flow(
    exprs: &[Vec<Word>],
    env: &mut HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<EvalFlow, KonaError> {
    let mut idx = 0;
    while idx + 1 < exprs.len() {
        let cond_val = match eval_words_flow(&exprs[idx], env, ktree)? {
            EvalFlow::Value(k) => k,
            ret @ EvalFlow::Return(_) => return Ok(ret),
        };
        if is_truthy(&cond_val)? {
            return eval_words_flow(&exprs[idx + 1], env, ktree);
        }
        idx += 2;
    }
    if idx < exprs.len() {
        return eval_words_flow(&exprs[idx], env, ktree);
    }
    Ok(EvalFlow::Value(K::nil()))
}

/// Evaluates the rightmost expression in the slice, returning the result and the remaining slice.
fn eval_rightmost_flow<'a>(
    words: &'a [Word],
    env: &mut HashMap<String, K>,
    ktree: &Arc<KTree>,
    is_operand: bool,
) -> Result<(EvalFlow, &'a [Word]), KonaError> {
    if words.is_empty() {
        return Ok((EvalFlow::Value(K::nil()), words));
    }

    let len = words.len();
    let last = &words[len - 1];

    match last {
        Word::Noun(k) => Ok((EvalFlow::Value(k.clone()), &words[..len - 1])),
        Word::Name(name) => {
            let val = lookup_var(name, env, ktree)?;
            Ok((EvalFlow::Value(val), &words[..len - 1]))
        }
        Word::Verb(v) => {
            if v == "\\\\" {
                Ok((EvalFlow::Value(K::nil()), &words[..len - 1]))
            } else {
                Ok((
                    EvalFlow::Value(K::function(KFunc::primitive(v.clone()))),
                    &words[..len - 1],
                ))
            }
        }
        Word::Adverb(name) => {
            if len < 2 {
                return Err(KonaError::Parse("Adverb missing operand".to_string()));
            }
            let mut is_projection = false;
            if !is_operand && len >= 3 {
                if let Word::Verb(_) = &words[len - 2] {
                    let left_word = &words[len - 3];
                    if matches!(
                        left_word,
                        Word::Noun(_)
                            | Word::Name(_)
                            | Word::Paren(_)
                            | Word::Bracket(_)
                            | Word::Brace { .. }
                    ) {
                        is_projection = true;
                    }
                }
            }

            if is_projection {
                if let Word::Verb(v_name) = &words[len - 2] {
                    let (left_flow, remaining) =
                        eval_rightmost_flow(&words[..len - 2], env, ktree, is_operand)?;
                    let left_val = match left_flow {
                        EvalFlow::Value(k) => k,
                        ret @ EvalFlow::Return(_) => return Ok((ret, remaining)),
                    };
                    let proj = K::function(KFunc::Projection {
                        func: K::function(KFunc::Primitive(v_name.clone())),
                        args: vec![Some(left_val), None],
                    });
                    let adverb_func = K::function(KFunc::adverb(name.clone(), Some(proj)));
                    Ok((EvalFlow::Value(adverb_func), remaining))
                } else {
                    unreachable!()
                }
            } else {
                let (operand, remaining) =
                    eval_rightmost_flow(&words[..len - 1], env, ktree, true)?;
                let operand = match operand {
                    EvalFlow::Value(k) => k,
                    ret @ EvalFlow::Return(_) => return Ok((ret, remaining)),
                };
                let adverb_func = K::function(KFunc::adverb(name.clone(), Some(operand)));
                Ok((EvalFlow::Value(adverb_func), remaining))
            }
        }
        Word::Paren(exprs) => {
            let val = eval_paren_flow(exprs, env, ktree)?;
            Ok((val, &words[..len - 1]))
        }
        Word::Bracket(exprs) => {
            if len < 2 {
                return Err(KonaError::Parse(
                    "Bracket missing left function/list/dict".to_string(),
                ));
            }
            let func_word = &words[len - 2];

            if let Word::Verb(name) = func_word {
                if name == ":" {
                    let cond_val = eval_conditional_flow(exprs, env, ktree)?;
                    return Ok((cond_val, &words[..len - 2]));
                }
            }

            if let Word::GlobalAssign(name) = func_word {
                let cond_flow = eval_conditional_flow(exprs, env, ktree)?;
                let cond_val = match &cond_flow {
                    EvalFlow::Value(k) => k.clone(),
                    EvalFlow::Return(k) => k.clone(),
                };
                assign_var(name.clone(), cond_val, env, ktree, true)?;
                return Ok((cond_flow, &words[..len - 2]));
            }

            if let Word::LocalAssign(name) = func_word {
                let cond_flow = eval_conditional_flow(exprs, env, ktree)?;
                let cond_val = match &cond_flow {
                    EvalFlow::Value(k) => k.clone(),
                    EvalFlow::Return(k) => k.clone(),
                };
                assign_var(name.clone(), cond_val, env, ktree, false)?;
                return Ok((cond_flow, &words[..len - 2]));
            }

            if let Word::Name(name) = func_word {
                if name == "while" {
                    if exprs.is_empty() {
                        return Err(KonaError::Valence);
                    }
                    let cond_expr = &exprs[0];
                    loop {
                        let cond_val = match eval_words_flow(cond_expr, env, ktree)? {
                            EvalFlow::Value(k) => k,
                            ret @ EvalFlow::Return(_) => return Ok((ret, &words[..len - 2])),
                        };
                        if !is_truthy(&cond_val)? {
                            break;
                        }
                        for expr in &exprs[1..] {
                            if let ret @ EvalFlow::Return(_) = eval_words_flow(expr, env, ktree)? {
                                return Ok((ret, &words[..len - 2]));
                            }
                        }
                    }
                    return Ok((EvalFlow::Value(K::nil()), &words[..len - 2]));
                } else if name == "do" {
                    if exprs.len() < 2 {
                        return Err(KonaError::Valence);
                    }
                    let n_val = match eval_words_flow(&exprs[0], env, ktree)? {
                        EvalFlow::Value(k) => k,
                        ret @ EvalFlow::Return(_) => return Ok((ret, &words[..len - 2])),
                    };
                    let n = match &*n_val {
                        KData::IntAtom(i) => *i,
                        KData::IntList(v) if v.len() == 1 => v[0],
                        _ => return Err(KonaError::Type),
                    };
                    for _ in 0..n {
                        for expr in &exprs[1..] {
                            if let ret @ EvalFlow::Return(_) = eval_words_flow(expr, env, ktree)? {
                                return Ok((ret, &words[..len - 2]));
                            }
                        }
                    }
                    return Ok((EvalFlow::Value(K::nil()), &words[..len - 2]));
                } else if name == "if" {
                    if exprs.len() < 2 {
                        return Err(KonaError::Valence);
                    }
                    let cond_val = match eval_words_flow(&exprs[0], env, ktree)? {
                        EvalFlow::Value(k) => k,
                        ret @ EvalFlow::Return(_) => return Ok((ret, &words[..len - 2])),
                    };
                    if is_truthy(&cond_val)? {
                        for expr in &exprs[1..] {
                            if let ret @ EvalFlow::Return(_) = eval_words_flow(expr, env, ktree)? {
                                return Ok((ret, &words[..len - 2]));
                            }
                        }
                    }
                    return Ok((EvalFlow::Value(K::nil()), &words[..len - 2]));
                }
            }

            if let Word::Verb(name) = func_word {
                if (name == "@" || name == ".")
                    && (exprs.len() == 3 || exprs.len() == 4)
                    && !exprs.iter().any(|expr| expr.is_empty())
                {
                    let is_deep = name == ".";
                    let a_word = &exprs[0];
                    let mut is_var = false;
                    let mut var_name = String::new();
                    // Kona's \. help: .[a;..] returns the changed copy and leaves a alone; only .[`a;..]
                    // (a symbol) updates a in place. A bare name used to be written back too - inside a
                    // function that made a local copy that hid the global from later a:: (Made by Claude)
                    let mut write_back = false;

                    if a_word.len() == 1 {
                        if let Word::Name(n) = &a_word[0] {
                            is_var = true;
                            var_name = n.clone();
                        }
                    }

                    let mut is_trap = false;
                    let mut func_to_trap = K::nil();
                    let mut target_val = K::nil();
                    // Telling the trap .[f;x;:] from the amend @[x;i;f] needs exprs[0] and exprs[2]
                    // evaluated; those values are kept and reused below, so a side effect in either
                    // slot (c+:1, 0:, _draw) runs once. They used to be evaluated a second time.
                    let mut pre_a: Option<K> = None;
                    let mut pre_f: Option<K> = None;

                    if exprs.len() == 3 {
                        let f_val = match eval_words_flow(&exprs[0], env, ktree)? {
                            EvalFlow::Value(k) => k,
                            ret @ EvalFlow::Return(_) => return Ok((ret, &words[..len - 2])),
                        };
                        let e_val = match eval_words_flow(&exprs[2], env, ktree)? {
                            EvalFlow::Value(k) => k,
                            ret @ EvalFlow::Return(_) => return Ok((ret, &words[..len - 2])),
                        };
                        if is_colon_function(&e_val) || matches!(&*f_val, KData::Function(_)) {
                            is_trap = true;
                            func_to_trap = f_val;
                        } else {
                            pre_a = Some(f_val);
                        }
                        pre_f = Some(e_val);
                    }

                    if !is_trap {
                        if is_var {
                            target_val = lookup_var(&var_name, env, ktree)?;
                        } else {
                            let a_val = match pre_a.take() {
                                Some(v) => v,
                                None => match eval_words_flow(&exprs[0], env, ktree)? {
                                    EvalFlow::Value(k) => k,
                                    ret @ EvalFlow::Return(_) => {
                                        return Ok((ret, &words[..len - 2]));
                                    }
                                },
                            };
                            if let KData::SymbolAtom(s) = &*a_val {
                                write_back = true;
                                var_name = s.to_string();
                                target_val = lookup_var(&var_name, env, ktree)?;
                            } else {
                                target_val = a_val;
                            }
                        }
                    }

                    if is_trap {
                        let e_val = pre_f.take().unwrap_or_else(K::nil);
                        let res_val =
                            apply_trap(&func_to_trap, &exprs[1], e_val, is_deep, env, ktree)?;
                        return Ok((EvalFlow::Value(res_val), &words[..len - 2]));
                    }

                    let i_val = match eval_words_flow(&exprs[1], env, ktree)? {
                        EvalFlow::Value(k) => k,
                        ret @ EvalFlow::Return(_) => return Ok((ret, &words[..len - 2])),
                    };
                    let f_val = match pre_f.take() {
                        Some(v) => v,
                        None => match eval_words_flow(&exprs[2], env, ktree)? {
                            EvalFlow::Value(k) => k,
                            ret @ EvalFlow::Return(_) => return Ok((ret, &words[..len - 2])),
                        },
                    };
                    let v_val = if exprs.len() == 4 {
                        Some(match eval_words_flow(&exprs[3], env, ktree)? {
                            EvalFlow::Value(k) => k,
                            ret @ EvalFlow::Return(_) => return Ok((ret, &words[..len - 2])),
                        })
                    } else {
                        None
                    };

                    let res_val = if is_deep {
                        let path_vec = crate::verbs::math::to_vec_of_atoms(&i_val)?;
                        apply_amend_deep(
                            &target_val,
                            &path_vec,
                            &f_val,
                            v_val.as_ref(),
                            env,
                            ktree,
                        )?
                    } else {
                        apply_amend(&target_val, &i_val, &f_val, v_val.as_ref(), env, ktree)?
                    };

                    if write_back {
                        // `v names the K-tree variable, so the write goes to the global even inside a
                        // function (force_global=false made a local shadow and the global never changed)
                        assign_var(var_name.clone(), res_val.clone(), env, ktree, true)?;
                        return Ok((EvalFlow::Value(res_val), &words[..len - 2]));
                    } else {
                        return Ok((EvalFlow::Value(res_val), &words[..len - 2]));
                    }
                }
            }

            // The bracket's arguments go first, then what is to its left: K is right-to-left across
            // a bracket chain as well, so in z[a][a][a:1] the a:1 runs before z[a][a] is indexed
            // (tests.c expects 8; evaluating the left side first gave 2). An undefined name in an
            // argument is held until the function is known, because f[ghost] on a non-list f reads
            // as nil and creates the global (a probed Kona quirk) while on a list it is the error.
            let mut arg_vals = Vec::new();
            let mut undefined: Vec<(usize, String)> = Vec::new();
            if exprs.len() == 1 && exprs[0].is_empty() {
                arg_vals.push(Some(K::nil()));
            } else {
                arg_vals = vec![None; exprs.len()];
                for idx in (0..exprs.len()).rev() {
                    let expr = &exprs[idx];
                    if expr.is_empty() {
                        continue;
                    }
                    match eval_words_flow(expr, env, ktree) {
                        Ok(EvalFlow::Value(k)) => arg_vals[idx] = Some(k),
                        Ok(ret @ EvalFlow::Return(_)) => return Ok((ret, &words[..len - 1])),
                        Err(KonaError::Value(msg)) if msg.starts_with("Undefined variable: ") => {
                            undefined.push((idx, msg["Undefined variable: ".len()..].to_string()));
                        }
                        Err(e) => return Err(e),
                    }
                }
            }

            let (func_val, remaining_left) =
                eval_rightmost_flow(&words[..len - 1], env, ktree, false)?;
            let func_val = match func_val {
                EvalFlow::Value(k) => k,
                EvalFlow::Return(k) if k.is_function() => k,
                ret @ EvalFlow::Return(_) => return Ok((ret, remaining_left)),
            };

            if !undefined.is_empty() && func_val.is_list() {
                let (_, name) = undefined.remove(0);
                return Err(KonaError::Value(format!("Undefined variable: {}", name)));
            }
            for (idx, var_name) in undefined {
                if !var_name.starts_with('_') && !var_name.starts_with('.') {
                    if !env.contains_key("_local") {
                        if ktree.lookup(&var_name).is_none() {
                            ktree.assign(var_name, K::nil());
                        }
                    } else if !env.contains_key(&var_name) {
                        env.insert(var_name, K::nil());
                    }
                }
                arg_vals[idx] = Some(K::nil());
            }

            let res = apply_func_brackets(&func_val, arg_vals, env, ktree)?;
            Ok((EvalFlow::Value(res), remaining_left))
        }
        Word::Brace { body, args, code } => {
            let captured = if env.contains_key("_local") {
                Some(env.clone())
            } else {
                None
            };
            let func = K::function(KFunc::UserDefined {
                body: body.clone(),
                args: args.clone(),
                code: Some(std::sync::Arc::new(code.clone())),
                env: captured,
            });
            Ok((EvalFlow::Value(func), &words[..len - 1]))
        }
        Word::LocalAssign(_) | Word::GlobalAssign(_) => Err(KonaError::Parse(
            "Unexpected assignment operator in expression".to_string(),
        )),
    }
}

/// Evaluates a sequence of Words right-to-left.
pub fn eval_words(
    words: &[Word],
    env: &mut HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    register_immediate_assignments(words, env, ktree);
    Ok(eval_words_flow(words, env, ktree)?.into_value())
}

fn eval_words_flow(
    words: &[Word],
    env: &mut HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<EvalFlow, KonaError> {
    let _guard = DepthGuard::new()?;
    if words.is_empty() {
        return Ok(EvalFlow::Value(K::nil()));
    }

    // Split on semicolons at the top level
    let mut has_semicolon = false;
    for w in words {
        if let Word::Verb(name) = w {
            if name == ";" {
                has_semicolon = true;
                break;
            }
        }
    }

    #[allow(unused_assignments)]
    if has_semicolon {
        let mut last_val = K::nil();
        let mut start = 0;
        for i in 0..words.len() {
            if let Word::Verb(name) = &words[i] {
                if name == ";" {
                    let stmt = &words[start..i];
                    if !stmt.is_empty() {
                        match eval_words_flow(stmt, env, ktree)? {
                            EvalFlow::Value(k) => last_val = k,
                            ret @ EvalFlow::Return(_) => return Ok(ret),
                        }
                    } else {
                        last_val = K::nil();
                    }
                    start = i + 1;
                }
            }
        }
        let last_stmt = &words[start..];
        if !last_stmt.is_empty() {
            match eval_words_flow(last_stmt, env, ktree)? {
                EvalFlow::Value(k) => last_val = k,
                ret @ EvalFlow::Return(_) => return Ok(ret),
            }
        } else {
            last_val = K::nil();
        }
        return Ok(EvalFlow::Value(last_val));
    }

    register_immediate_assignments(words, env, ktree);

    let mut remaining = words;
    let mut accum: Option<K> = None;
    let mut accum_is_composable = false;

    while !remaining.is_empty() {
        let len = remaining.len();
        let last = &remaining[len - 1];

        match last {
            Word::LocalAssign(name) => {
                let val = accum.clone().ok_or_else(|| {
                    KonaError::Parse("Missing right-hand side for assignment".to_string())
                })?;
                assign_var(name.clone(), val.clone(), env, ktree, false)?;
                accum = Some(val);
                accum_is_composable = false;
                remaining = &remaining[..len - 1];
            }
            Word::GlobalAssign(name) => {
                let val = accum.clone().ok_or_else(|| {
                    KonaError::Parse("Missing right-hand side for assignment".to_string())
                })?;
                assign_var(name.clone(), val.clone(), env, ktree, true)?;
                accum = Some(val);
                accum_is_composable = false;
                remaining = &remaining[..len - 1];
            }
            Word::Verb(name) => {
                if accum.is_none() {
                    let is_dyadic = if len > 1 {
                        let left = &remaining[len - 2];
                        matches!(
                            left,
                            Word::Noun(_)
                                | Word::Name(_)
                                | Word::Paren(_)
                                | Word::Bracket(_)
                                | Word::Brace { .. }
                        )
                    } else {
                        false
                    };

                    if is_dyadic {
                        let (left_arg, next_remaining) =
                            eval_rightmost_flow(&remaining[..len - 1], env, ktree, false)?;
                        let left_arg = match left_arg {
                            EvalFlow::Value(k) => k,
                            ret @ EvalFlow::Return(_) => return Ok(ret),
                        };
                        let proj = K::function(KFunc::Projection {
                            func: K::function(KFunc::Primitive(name.clone())),
                            args: vec![Some(left_arg), None],
                        });
                        accum = Some(proj);
                        accum_is_composable = true;
                        remaining = next_remaining;
                    } else {
                        if name == "\\\\" {
                            accum = Some(K::nil());
                        } else {
                            accum = Some(K::function(KFunc::Primitive(name.clone())));
                        }
                        accum_is_composable = true;
                        remaining = &remaining[..len - 1];
                    }
                    continue;
                }

                let right_arg = accum.unwrap();
                let is_assign = name == ":";
                let is_inplace_assign = name.ends_with(':')
                    && name.len() > 1
                    && !matches!(
                        name.as_str(),
                        "0:" | "1:" | "2:" | "3:" | "4:" | "5:" | "6:"
                    );

                if is_assign || is_inplace_assign {
                    let op_opt = if is_inplace_assign {
                        Some(name[..name.len() - 1].to_string())
                    } else {
                        None
                    };

                    // Pattern A: Simple in-place assignment (e.g. i+:1)
                    if is_inplace_assign && len >= 2 {
                        if let Word::Name(var_name) = &remaining[len - 2] {
                            let op = op_opt.as_ref().unwrap();
                            let curr_val = lookup_var(var_name, env, ktree)?;
                            let new_val = apply_verb(op, Some(&curr_val), &right_arg, env, ktree)?;
                            assign_var(var_name.clone(), new_val.clone(), env, ktree, true)?;
                            accum = Some(new_val);
                            accum_is_composable = false;
                            remaining = &remaining[..len - 2];
                            continue;
                        }
                    }

                    // Pattern B: Indexed/In-place Indexed assignment (e.g. x[1]:9 or x[1]+:9)
                    let idx = len - 1;
                    let mut num_brackets = 0;
                    while idx > num_brackets {
                        if let Word::Bracket(_) = &remaining[idx - 1 - num_brackets] {
                            num_brackets += 1;
                        } else {
                            break;
                        }
                    }

                    if num_brackets > 0 {
                        let is_valid = num_brackets == 1
                            && idx > num_brackets
                            && matches!(&remaining[idx - 1 - num_brackets], Word::Name(_));

                        if !is_valid {
                            return Err(KonaError::Parse("Invalid assignment target".to_string()));
                        }

                        let var_name = match &remaining[idx - 1 - num_brackets] {
                            Word::Name(n) => n,
                            _ => unreachable!(),
                        };

                        let mut bracket_exprs = Vec::new();
                        for i in 0..num_brackets {
                            if let Word::Bracket(exprs) = &remaining[idx - num_brackets + i] {
                                bracket_exprs.push(exprs.clone());
                            }
                        }

                        let mut path_vals = Vec::new();
                        for exprs in bracket_exprs {
                            for expr in exprs {
                                if expr.is_empty() {
                                    path_vals.push(K::nil());
                                } else {
                                    path_vals.push(match eval_words_flow(&expr, env, ktree)? {
                                        EvalFlow::Value(k) => k,
                                        ret @ EvalFlow::Return(_) => return Ok(ret),
                                    });
                                }
                            }
                        }

                        let target_val = lookup_var(var_name, env, ktree)?;
                        let f_val = K::function(crate::types::KFunc::Primitive(
                            op_opt.unwrap_or_else(|| ":".to_string()),
                        ));
                        let new_val = if path_vals.len() == 1 {
                            apply_amend(
                                &target_val,
                                &path_vals[0],
                                &f_val,
                                Some(&right_arg),
                                env,
                                ktree,
                            )?
                        } else {
                            apply_amend_deep(
                                &target_val,
                                &path_vals,
                                &f_val,
                                Some(&right_arg),
                                env,
                                ktree,
                            )?
                        };

                        assign_var(var_name.clone(), new_val.clone(), env, ktree, true)?;
                        let ret_val = if is_colon_function(&f_val) {
                            let sub_target = if path_vals.len() > 1 {
                                apply_index_chain(
                                    &target_val,
                                    &path_vals[..path_vals.len() - 1],
                                    env,
                                    ktree,
                                )
                                .unwrap_or_else(|_| target_val.clone())
                            } else {
                                target_val.clone()
                            };
                            let last_idx = path_vals.last().unwrap();
                            get_assignment_return_value(&sub_target, last_idx, &right_arg)
                        } else {
                            crate::verbs::list::at(&new_val, &path_vals[0])?
                        };
                        accum = Some(ret_val);
                        accum_is_composable = false;
                        remaining = &remaining[..idx - 1 - num_brackets];
                        continue;
                    }

                    if name == ":" {
                        if len > 1 {
                            let _ = eval_words_flow(&remaining[..len - 1], env, ktree)?;
                        }
                        return Ok(EvalFlow::Return(right_arg));
                    }
                }

                let is_dyadic = if len > 1 && !is_inplace_assign {
                    let left = &remaining[len - 2];
                    matches!(
                        left,
                        Word::Noun(_)
                            | Word::Name(_)
                            | Word::Paren(_)
                            | Word::Bracket(_)
                            | Word::Brace { .. }
                    )
                } else {
                    false
                };

                if is_dyadic {
                    let (left_arg, next_remaining) =
                        eval_rightmost_flow(&remaining[..len - 1], env, ktree, false)?;
                    let left_arg = match left_arg {
                        EvalFlow::Value(k) => k,
                        ret @ EvalFlow::Return(_) => return Ok(ret),
                    };
                    // a verb next to a primitive/adverb function composes ((|+\) 1 1 is 2 1); next to
                    // a lambda it applies, the lambda being an atom (C Kona: @{x+y} is 1, #{x} is 1)
                    let res = if right_arg.is_composable_func()
                        && !is_lambda(&right_arg)
                        && accum_is_composable
                        && !left_arg.is_composable_func()
                    {
                        let proj_func = K::function(KFunc::projection(
                            K::function(KFunc::Primitive(name.to_string())),
                            vec![Some(left_arg.clone()), None],
                        ));
                        K::function(KFunc::composition(proj_func, right_arg.clone()))
                    } else {
                        apply_verb(name, Some(&left_arg), &right_arg, env, ktree)?
                    };
                    accum = Some(res);
                    accum_is_composable = right_arg.is_composable_func() && accum_is_composable;
                    remaining = next_remaining;
                } else {
                    let res = if right_arg.is_composable_func()
                        && !is_lambda(&right_arg)
                        && accum_is_composable
                    {
                        let f = K::function(KFunc::Primitive(name.to_string()));
                        K::function(KFunc::composition(f, right_arg.clone()))
                    } else {
                        apply_verb(name, None, &right_arg, env, ktree)?
                    };
                    accum = Some(res);
                    accum_is_composable = right_arg.is_composable_func() && accum_is_composable;
                    remaining = &remaining[..len - 1];
                }
            }
            _ => {
                if let (Some(acc_arg), Word::Bracket(exprs)) = (accum.clone(), last) {
                    let func_word = remaining.get(len - 2);
                    if matches!(
                        func_word,
                        Some(Word::Verb(_))
                            | Some(Word::Name(_))
                            | Some(Word::Brace { .. })
                            | Some(Word::Adverb(_))
                    ) {
                        let (func_val, remaining_left) =
                            eval_rightmost_flow(&remaining[..len - 1], env, ktree, false)?;
                        let func_val = match func_val {
                            EvalFlow::Value(k) => k,
                            EvalFlow::Return(k) if k.is_function() => k,
                            ret @ EvalFlow::Return(_) => return Ok(ret),
                        };

                        if let KData::Function(kfunc) = &*func_val {
                            let mut arg_vals = Vec::new();
                            if exprs.len() == 1 && exprs[0].is_empty() {
                                arg_vals.push(Some(K::nil()));
                            } else {
                                arg_vals = vec![None; exprs.len()];
                                for idx in (0..exprs.len()).rev() {
                                    let expr = &exprs[idx];
                                    if expr.is_empty() {
                                        arg_vals[idx] = None;
                                    } else {
                                        let res_flow = match eval_words_flow(expr, env, ktree) {
                                            Ok(flow) => flow,
                                            Err(KonaError::Value(msg))
                                                if msg.starts_with("Undefined variable: ") =>
                                            {
                                                let var_name = &msg["Undefined variable: ".len()..];
                                                if !var_name.starts_with('_')
                                                    && !var_name.starts_with('.')
                                                {
                                                    let is_local = env.contains_key("_local");
                                                    if !is_local {
                                                        if ktree.lookup(var_name).is_none() {
                                                            ktree.assign(
                                                                var_name.to_string(),
                                                                K::nil(),
                                                            );
                                                        }
                                                    } else {
                                                        if !env.contains_key(var_name) {
                                                            env.insert(
                                                                var_name.to_string(),
                                                                K::nil(),
                                                            );
                                                        }
                                                    }
                                                }
                                                EvalFlow::Value(K::nil())
                                            }
                                            Err(e) => return Err(e),
                                        };
                                        match res_flow {
                                            EvalFlow::Value(k) => arg_vals[idx] = Some(k),
                                            ret @ EvalFlow::Return(_) => return Ok(ret),
                                        }
                                    }
                                }
                            }

                            let has_holes = arg_vals.iter().any(|a| a.is_none());
                            let valence = kfunc.valence();
                            if has_holes {
                                let projected =
                                    apply_func_brackets(&func_val, arg_vals, env, ktree)?;
                                let res = apply_func(&projected, &[acc_arg], env, ktree)?;
                                accum = Some(res);
                                accum_is_composable = false;
                                remaining = remaining_left;
                                continue;
                            } else if arg_vals.len() < valence {
                                arg_vals.push(Some(acc_arg));
                                let res = apply_func_brackets(&func_val, arg_vals, env, ktree)?;
                                accum = Some(res);
                                accum_is_composable = false;
                                remaining = remaining_left;
                                continue;
                            }
                        }
                    }
                }

                let (val, next_remaining) =
                    eval_rightmost_flow(remaining, env, ktree, accum.is_some())?;
                let val = match val {
                    EvalFlow::Value(k) => k,
                    ret @ EvalFlow::Return(_) => return Ok(ret),
                };
                let val_is_composable = if next_remaining.len() < remaining.len() {
                    let first_evaluated_word = &remaining[next_remaining.len()];
                    matches!(
                        first_evaluated_word,
                        Word::Verb(_) | Word::Adverb(_) | Word::Brace { .. }
                    )
                } else {
                    false
                };

                if let Some(acc) = accum {
                    let is_left_noun = if !next_remaining.is_empty() {
                        let left_word = &next_remaining[next_remaining.len() - 1];
                        match left_word {
                            // a name holding a function is not a left argument: f g 3 is f[g[3]]
                            // (C Kona: 7), only a name holding data makes `a g 3` infix
                            Word::Name(n) => !lookup_var(n, env, ktree)
                                .map(|v| v.is_function())
                                .unwrap_or(false),
                            Word::Noun(_) | Word::Paren(_) | Word::Bracket(_) | Word::Brace { .. } => {
                                true
                            }
                            _ => false,
                        }
                    } else {
                        false
                    };

                    if is_left_noun {
                        let (left_val, final_remaining) =
                            eval_rightmost_flow(next_remaining, env, ktree, false)?;
                        let left_val = match left_val {
                            EvalFlow::Value(k) => k,
                            ret @ EvalFlow::Return(_) => return Ok(ret),
                        };
                        accum = Some(apply_func(&val, &[left_val, acc], env, ktree)?);
                        accum_is_composable = false;
                        remaining = final_remaining;
                    } else {
                        let is_acc_func = acc.is_composable_func();
                        let is_val_func = val.is_composable_func();
                        let res = if is_acc_func && is_val_func {
                            K::function(KFunc::composition(val.clone(), acc.clone()))
                        } else {
                            apply_func(&val, &[acc], env, ktree)?
                        };
                        accum = Some(res);
                        accum_is_composable = is_acc_func && is_val_func;
                        remaining = next_remaining;
                    }
                } else {
                    accum = Some(val);
                    accum_is_composable = val_is_composable;
                    remaining = next_remaining;
                }
            }
        }
    }

    accum
        .map(EvalFlow::Value)
        .ok_or_else(|| KonaError::Parse("Empty expression".to_string()))
}

fn find_locals(words: &[Word]) -> std::collections::HashSet<String> {
    let mut locals = std::collections::HashSet::new();
    fn traverse(words: &[Word], locals: &mut std::collections::HashSet<String>) {
        for w in words {
            match w {
                Word::LocalAssign(name) => {
                    locals.insert(name.clone());
                }
                Word::Paren(exprs) => {
                    for expr in exprs {
                        traverse(expr, locals);
                    }
                }
                Word::Bracket(exprs) => {
                    for expr in exprs {
                        traverse(expr, locals);
                    }
                }
                _ => {}
            }
        }
    }
    traverse(words, &mut locals);
    locals
}

/// Applies a function to fully-specified positional arguments.
pub fn apply_func(
    func: &K,
    args: &[K],
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    let _guard = DepthGuard::new()?;
    if let KData::SymbolAtom(sym) = &**func {
        let resolved = lookup_var(sym.as_ref(), env, ktree)?;
        return apply_func(&resolved, args, env, ktree);
    }
    match &**func {
        KData::Function(kfunc) => match kfunc {
            KFunc::Primitive(name) => {
                // @[x;i;f;v] and the 3-argument @[x;i;f] are amends whichever way @ was reached
                // (.[@;(1 2 3;0;-:)] used to be a valence error); the 3-argument trap .[f;x;:] with a
                // function first stays on the syntactic path
                let three_arg_amend = args.len() == 3
                    && !args[0].is_function()
                    && !is_colon_function(&args[2]);
                if (name == "@" || name == ".") && (args.len() == 4 || three_arg_amend) {
                    apply_amend_primitive(name, args, env, ktree)
                } else if name == "_ssr" && args.len() == 3 {
                    crate::verbs::ssr_verb_triadic(&args[0], &args[1], &args[2])
                } else if args.len() == 1 {
                    apply_verb(name, None, &args[0], env, ktree)
                } else if args.len() == 2 {
                    apply_verb(name, Some(&args[0]), &args[1], env, ktree)
                } else {
                    Err(KonaError::Valence)
                }
            }
            KFunc::Adverb { name, operand } => {
                if args.len() > func.valence() {
                    return Err(KonaError::Valence);
                }
                let op = operand
                    .as_ref()
                    .ok_or_else(|| KonaError::Value("Missing operand for adverb".to_string()))?;
                crate::adverbs::eval_adverb(name, op, args, env, ktree)
            }
            KFunc::Projection {
                func: proj_func,
                args: proj_args,
            } => {
                let mut new_args = Vec::new();
                let mut arg_iter = args.iter();
                for pa in proj_args {
                    if pa.is_none() {
                        if let Some(arg) = arg_iter.next() {
                            new_args.push(Some(arg.clone()));
                        } else {
                            new_args.push(None);
                        }
                    } else {
                        new_args.push(pa.clone());
                    }
                }
                for arg in arg_iter {
                    new_args.push(Some(arg.clone()));
                }

                let has_none = new_args.iter().any(|a| a.is_none());
                if has_none {
                    Ok(K::function(KFunc::projection(proj_func.clone(), new_args)))
                } else {
                    let final_args: Vec<K> = new_args.into_iter().map(|a| a.unwrap()).collect();
                    apply_func(proj_func, &final_args, env, ktree)
                }
            }
            KFunc::UserDefined {
                body,
                args: param_names,
                code: opt_code,
                env: captured_env,
            } => {
                let collapsed_args;
                let mut final_args = args;
                if !param_names.is_empty() && args.len() > param_names.len() && !args.is_empty() {
                    let all_char = args.iter().all(|a| matches!(**a, KData::CharAtom(_)));
                    if all_char {
                        let chars: Vec<u8> = args.iter().map(|a| {
                            if let KData::CharAtom(c) = &**a {
                                *c as u8
                            } else {
                                unreachable!()
                            }
                        }).collect();
                        collapsed_args = vec![K::char_list(chars)];
                        final_args = &collapsed_args;
                    }
                }

                if !param_names.is_empty() && final_args.len() > param_names.len() {
                    return Err(KonaError::Valence);
                }
                if !param_names.is_empty() && final_args.len() < param_names.len() {
                    let mut proj_args = Vec::new();
                    for i in 0..param_names.len() {
                        if i < final_args.len() {
                            proj_args.push(Some(final_args[i].clone()));
                        } else {
                            proj_args.push(None);
                        }
                    }
                    return Ok(K::function(KFunc::projection(func.clone(), proj_args)));
                }

                let mut local_env = if let Some(c_env) = captured_env {
                    c_env.clone()
                } else {
                    let mut empty = HashMap::new();
                    empty.insert("_local".to_string(), K::nil());
                    empty
                };
                local_env.insert("_local".to_string(), K::nil());
                local_env.insert("_f".to_string(), func.clone());
                let mut bound_params = std::collections::HashSet::new();
                for (name, val) in param_names.iter().zip(final_args.iter()) {
                    if bound_params.insert(name.clone()) {
                        local_env.insert(name.clone(), val.clone());
                    }
                }

                let local_words = if let Some(code_arc) = opt_code {
                    code_arc.clone()
                } else {
                    let clean_body = strip_param_list(body);
                    std::sync::Arc::new(crate::parser::parse(clean_body)?)
                };
                let locals = find_locals(&local_words);
                for name in &locals {
                    if !param_names.contains(name) {
                        local_env.insert(name.clone(), K::nil());
                    }
                }
                RECENT_LOCALS.with(|rl| {
                    let mut set = rl.borrow_mut();
                    set.clear();
                    for name in &locals {
                        set.insert(name.clone());
                    }
                });
                eval_words(&local_words, &mut local_env, ktree)
            }
            KFunc::Composition { f, g } => {
                let res_g = apply_func(g, args, env, ktree)?;
                apply_func(f, &[res_g], env, ktree)
            }
        },
        KData::IntList(_)
        | KData::FloatList(_)
        | KData::CharList(_)
        | KData::SymbolList(_)
        | KData::GeneralList(_)
        | KData::Dictionary { .. } => {
            if args.is_empty() {
                return Err(KonaError::Valence);
            }
            apply_index_chain(func, args, env, ktree)
        }
        KData::Nil => {
            if args.is_empty() {
                Ok(K::nil())
            } else if args.len() == 1 {
                Ok(args[0].clone())
            } else {
                Ok(crate::verbs::math::normalize_list(args.to_vec()))
            }
        }
        _ => Err(KonaError::Type),
    }
}

pub fn apply_func_brackets(
    func: &K,
    args: Vec<Option<K>>,
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    // same guard as apply_func: a:`a; a[1] used to recurse here until the native stack overflowed
    let _guard = DepthGuard::new()?;
    if let KData::SymbolAtom(sym) = &**func {
        let resolved = lookup_var(sym.as_ref(), env, ktree)?;
        return apply_func_brackets(&resolved, args, env, ktree);
    }
    if let KData::Function(kfunc) = &**func {
        let val = kfunc.valence();
        if args.len() > val {
            return Err(KonaError::Valence);
        }
    }
    match &**func {
        KData::IntList(_)
        | KData::FloatList(_)
        | KData::CharList(_)
        | KData::SymbolList(_)
        | KData::GeneralList(_)
        | KData::Dictionary { .. } => {
            if args.len() == 1 && args[0].is_none() {
                match &**func {
                    KData::Dictionary { values, .. } => return Ok(values.clone()),
                    _ => return Ok(func.clone()),
                }
            }
            let final_args: Vec<K> = args
                .iter()
                .map(|a| a.clone().unwrap_or_else(K::nil))
                .collect();
            return apply_index_chain(func, &final_args, env, ktree);
        }
        _ => {}
    }
    if let KData::Function(KFunc::UserDefined {
        args: param_names, ..
    }) = &**func
    {
        if param_names.is_empty() && args.len() == 1 && args[0].is_none() {
            return apply_func(func, &[], env, ktree);
        }
    }
    let has_none = args.iter().any(|a| a.is_none());
    if has_none {
        match &**func {
            KData::Function(KFunc::Projection {
                func: inner_func,
                args: inner_args,
            }) => {
                let mut merged = Vec::new();
                let mut new_arg_iter = args.into_iter();
                for ia in inner_args {
                    if ia.is_none() {
                        if let Some(na) = new_arg_iter.next() {
                            merged.push(na);
                        } else {
                            merged.push(None);
                        }
                    } else {
                        merged.push(ia.clone());
                    }
                }
                for na in new_arg_iter {
                    merged.push(na);
                }
                Ok(K::function(KFunc::projection(inner_func.clone(), merged)))
            }
            _ => Ok(K::function(KFunc::projection(func.clone(), args))),
        }
    } else {
        let final_args: Vec<K> = args.into_iter().map(|a| a.unwrap()).collect();
        apply_func(func, &final_args, env, ktree)
    }
}

fn apply_index_chain(
    target: &K,
    args: &[K],
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    use crate::verbs::math::{normalize_list, to_vec_of_atoms};

    if args.is_empty() {
        return Ok(target.clone());
    }

    if matches!(**target, KData::Function(_)) {
        return apply_func(target, args, env, ktree);
    }

    let first = &args[0];
    let rest = &args[1..];

    if let KData::Dictionary { keys, values } = &**target {
        if let KData::CharList(v) = &**first {
            let s = std::str::from_utf8(v).map_err(|_| KonaError::Type)?;
            let words = crate::parser::parse(s)?;
            let mut eval_env = env.clone();
            let keys_vec = to_vec_of_atoms(keys)?;
            let vals_vec = to_vec_of_atoms(values)?;
            for (k, val) in keys_vec.iter().zip(vals_vec.iter()) {
                if let KData::SymbolAtom(sym) = &**k {
                    eval_env.insert(sym.to_string(), val.clone());
                }
            }
            let res = eval_words(&words, &mut eval_env, ktree)?;
            return apply_index_chain(&res, rest, env, ktree);
        }
    }

    if first.is_nil() {
        match &**target {
            KData::Dictionary { values, .. } => {
                let vals = to_vec_of_atoms(values)?;
                let mut out = Vec::with_capacity(vals.len());
                for val in vals {
                    out.push(apply_index_chain(&val, rest, env, ktree)?);
                }
                Ok(normalize_list(out))
            }
            _ if target.is_list() => {
                let vals = to_vec_of_atoms(target)?;
                let mut out = Vec::with_capacity(vals.len());
                for val in vals {
                    out.push(apply_index_chain(&val, rest, env, ktree)?);
                }
                Ok(normalize_list(out))
            }
            _ => apply_index_chain(target, rest, env, ktree),
        }
    } else if first.is_list() {
        let indices = to_vec_of_atoms(first)?;
        let mut out = Vec::with_capacity(indices.len());
        for idx in indices {
            let val = crate::verbs::list::at(target, &idx)?;
            out.push(apply_index_chain(&val, rest, env, ktree)?);
        }
        Ok(normalize_list(out))
    } else {
        let val = crate::verbs::list::at(target, first)?;
        apply_index_chain(&val, rest, env, ktree)
    }
}

fn apply_amend_primitive(
    name: &str,
    args: &[K],
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    let mut is_var = false;
    let mut var_name = String::new();
    let target = if let KData::SymbolAtom(sym) = &*args[0] {
        let key = sym.to_string();
        if let Some(val) = resolve_dotted_name(&key, env, ktree) {
            is_var = true;
            var_name = key;
            val
        } else {
            args[0].clone()
        }
    } else {
        args[0].clone()
    };

    // args[3] is absent for the 3-argument form @[x;i;f] (monadic f on the chosen items)
    let result = if name == "." {
        let path = crate::verbs::math::to_vec_of_atoms(&args[1])?;
        apply_amend_deep(&target, &path, &args[2], args.get(3), env, ktree)?
    } else {
        apply_amend(&target, &args[1], &args[2], args.get(3), env, ktree)?
    };

    // a symbol names the K-tree variable, so the result goes back there from any scope
    if is_var {
        let mut dummy = HashMap::new();
        assign_var(var_name, result.clone(), &mut dummy, ktree, true)?;
    }

    Ok(result)
}

fn apply_amend_func(
    f: &K,
    args: &[K],
    old_val: &K,
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    let res = apply_func(f, args, env, ktree);
    match res {
        Ok(val) => Ok(val),
        Err(err) => {
            if IGNORE_PENDING_ERROR.with(|ipe| ipe.get()) {
                PENDING_ERROR.with(|pe| pe.set(Some(err)));
                Ok(old_val.clone())
            } else {
                Err(err)
            }
        }
    }
}

fn apply_amend(
    a: &K,
    i: &K,
    f: &K,
    v: Option<&K>,
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    use crate::verbs::math::to_vec_of_atoms;
    let mut i_resolved = i.clone();
    if i_resolved.is_nil() {
        match &**a {
            KData::Dictionary { keys, .. } => {
                i_resolved = keys.clone();
            }
            _ if a.is_list() => {
                let len = a.len()?;
                i_resolved = K::int_list((0..len as i64).collect());
            }
            _ => {}
        }
    }
    let i = &i_resolved;
    if matches!(**a, KData::Nil) && is_colon_function(f) {
        if let Some(ve) = v {
            let keys = to_vec_of_atoms(i)?;
            let values = if ve.is_list() {
                let ve_vec = to_vec_of_atoms(ve)?;
                if ve_vec.len() == keys.len() {
                    ve_vec
                } else {
                    vec![ve.clone(); keys.len()]
                }
            } else {
                vec![ve.clone(); keys.len()]
            };
            return Ok(K::dictionary(
                crate::verbs::math::normalize_list(keys),
                crate::verbs::math::normalize_list(values),
            ));
        }
    }

    match &**a {
        KData::Dictionary { keys, values } => {
            let mut key_vec = to_vec_of_atoms(keys)?;
            let mut val_vec = to_vec_of_atoms(values)?;

            if i.is_atom() {
                if let Some(pos) = key_vec.iter().position(|k| k == i) {
                    let old_val = &val_vec[pos];
                    let new_val = if let Some(ve) = v {
                        apply_amend_func(f, &[old_val.clone(), ve.clone()], old_val, env, ktree)?
                    } else {
                        apply_amend_func(f, &[old_val.clone()], old_val, env, ktree)?
                    };
                    val_vec[pos] = new_val;
                } else if is_colon_function(f) {
                    if let Some(ve) = v {
                        key_vec.push(i.clone());
                        val_vec.push(ve.clone());
                    } else {
                        return Err(KonaError::Index);
                    }
                } else {
                    return Err(KonaError::Index);
                }
            } else {
                let i_vec = to_vec_of_atoms(i)?;
                for (idx_pos, ie) in i_vec.iter().enumerate() {
                    if let Some(pos) = key_vec.iter().position(|k| k == ie) {
                        let old_val = &val_vec[pos];
                        let new_val = if let Some(ve) = v {
                            let r_arg = if ve.is_list() {
                                let ve_atoms = to_vec_of_atoms(ve)?;
                                if ve_atoms.len() == i_vec.len() {
                                    ve_atoms[idx_pos].clone()
                                } else {
                                    ve.clone()
                                }
                            } else {
                                ve.clone()
                            };
                            apply_amend_func(f, &[old_val.clone(), r_arg], old_val, env, ktree)?
                        } else {
                            apply_amend_func(f, &[old_val.clone()], old_val, env, ktree)?
                        };
                        val_vec[pos] = new_val;
                    } else if is_colon_function(f) {
                        if let Some(ve) = v {
                            let r_arg = if ve.is_list() {
                                let ve_atoms = to_vec_of_atoms(ve)?;
                                if ve_atoms.len() == i_vec.len() {
                                    ve_atoms[idx_pos].clone()
                                } else {
                                    ve.clone()
                                }
                            } else {
                                ve.clone()
                            };
                            key_vec.push(ie.clone());
                            val_vec.push(r_arg);
                        } else {
                            return Err(KonaError::Index);
                        }
                    } else {
                        return Err(KonaError::Index);
                    }
                }
            }
            Ok(K::dictionary(
                crate::verbs::math::normalize_list(key_vec),
                crate::verbs::math::normalize_list(val_vec),
            ))
        }
        _ => {
            // @[list; i; :; v] on a flat number list: one copy, values dropped in (lanes.rs)
            if is_colon_function(f) {
                if let Some(ve) = v {
                    if let Some(r) = crate::verbs::lanes::amend_assign(a, i, ve)? {
                        return Ok(r);
                    }
                }
            }
            let mut elements = to_vec_of_atoms(a)?;
            let len = elements.len() as i64;
            // unpack the new values once, not once per index
            let ve_atoms = match v {
                Some(ve) if ve.is_list() => Some(to_vec_of_atoms(ve)?),
                _ => None,
            };

            if i.is_atom() {
                let idx = i.as_int().ok_or(KonaError::Type)?;
                let mut wrapped_idx = idx;
                if wrapped_idx < 0 {
                    wrapped_idx += len;
                }
                if wrapped_idx < 0 || wrapped_idx >= len {
                    return Err(KonaError::Index);
                }
                let old_val = &elements[wrapped_idx as usize];
                let new_val = if let Some(ve) = v {
                    apply_amend_func(f, &[old_val.clone(), ve.clone()], old_val, env, ktree)?
                } else {
                    apply_amend_func(f, &[old_val.clone()], old_val, env, ktree)?
                };
                elements[wrapped_idx as usize] = new_val;
            } else {
                let i_vec = to_vec_of_atoms(i)?;
                for (idx_pos, ie) in i_vec.iter().enumerate() {
                    let idx = ie.as_int().ok_or(KonaError::Type)?;
                    let mut wrapped_idx = idx;
                    if wrapped_idx < 0 {
                        wrapped_idx += len;
                    }
                    if wrapped_idx < 0 || wrapped_idx >= len {
                        return Err(KonaError::Index);
                    }
                    let old_val = &elements[wrapped_idx as usize];
                    let new_val = if let Some(ve) = v {
                        let r_arg = match &ve_atoms {
                            Some(va) if va.len() == i_vec.len() => va[idx_pos].clone(),
                            _ => ve.clone(),
                        };
                        apply_amend_func(f, &[old_val.clone(), r_arg], old_val, env, ktree)?
                    } else {
                        apply_amend_func(f, &[old_val.clone()], old_val, env, ktree)?
                    };
                    elements[wrapped_idx as usize] = new_val;
                }
            }
            Ok(crate::verbs::math::normalize_list(elements))
        }
    }
}

fn apply_amend_deep(
    a: &K,
    path: &[K],
    f: &K,
    v: Option<&K>,
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    use crate::verbs::math::to_vec_of_atoms;
    if path.is_empty() {
        let args = if let Some(ve) = v {
            vec![a.clone(), ve.clone()]
        } else {
            vec![a.clone()]
        };
        return apply_amend_func(f, &args, a, env, ktree);
    }

    let first_idx = &path[0];
    let rest_path = &path[1..];

    if rest_path.is_empty() {
        return apply_amend(a, first_idx, f, v, env, ktree);
    }

    if first_idx.is_atom() {
        let sub_a = crate::verbs::list::at(a, first_idx)?;
        let new_sub_a = apply_amend_deep(&sub_a, rest_path, f, v, env, ktree)?;
        let assign_func = K::function(KFunc::Primitive(":".to_string()));
        apply_amend(a, first_idx, &assign_func, Some(&new_sub_a), env, ktree)
    } else {
        let idx_vec = to_vec_of_atoms(first_idx)?;
        let mut curr_a = a.clone();
        for (idx_pos, ie) in idx_vec.iter().enumerate() {
            let sub_a = crate::verbs::list::at(&curr_a, ie)?;

            let sub_v = if let Some(ve) = v {
                if ve.is_list() {
                    let ve_atoms = to_vec_of_atoms(ve)?;
                    if ve_atoms.len() == idx_vec.len() {
                        Some(ve_atoms[idx_pos].clone())
                    } else {
                        Some(ve.clone())
                    }
                } else {
                    Some(ve.clone())
                }
            } else {
                None
            };

            let new_sub_a = apply_amend_deep(&sub_a, rest_path, f, sub_v.as_ref(), env, ktree)?;
            let assign_func = K::function(KFunc::Primitive(":".to_string()));
            curr_a = apply_amend(&curr_a, ie, &assign_func, Some(&new_sub_a), env, ktree)?;
        }
        Ok(curr_a)
    }
}

fn is_colon_function(k: &K) -> bool {
    if let KData::Function(KFunc::Primitive(name)) = &**k {
        name == ":"
    } else {
        false
    }
}

/// e_val is the already-evaluated third argument: the caller needed it to tell a trap from a
/// 3-argument amend, and evaluating it again here ran its side effects twice
fn apply_trap(
    f: &K,
    x_expr: &[Word],
    e_val: K,
    is_deep: bool,
    env: &mut HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    let is_colon = is_colon_function(&e_val);

    let x_res = eval_words(x_expr, env, ktree);

    let res = match x_res {
        Ok(x_val) => {
            if is_deep {
                match crate::verbs::math::to_vec_of_atoms(&x_val) {
                    Ok(args) => apply_func(f, &args, env, ktree),
                    Err(e) => Err(e),
                }
            } else {
                apply_func(f, &[x_val], env, ktree)
            }
        }
        Err(err) => Err(err),
    };

    match res {
        Ok(val) => {
            if is_colon {
                Ok(crate::verbs::math::normalize_list(vec![K::int(0), val]))
            } else {
                Ok(val)
            }
        }
        Err(err) => {
            if is_colon {
                Ok(crate::verbs::math::normalize_list(vec![
                    K::int(1),
                    error_to_k(err),
                ]))
            } else if matches!(&*e_val, KData::Function(_)) {
                let err_k = error_to_k(err);
                apply_func(&e_val, &[err_k], env, ktree)
            } else {
                Ok(e_val)
            }
        }
    }
}

/// A brace function {..}: a noun to the verbs (@{x+y} applies @), unlike a primitive or an
/// adverb-derived verb, which compose
fn is_lambda(k: &K) -> bool {
    matches!(&**k, KData::Function(KFunc::UserDefined { .. }))
}

fn error_to_k(err: KonaError) -> K {
    let msg = match err {
        KonaError::Type => "type",
        KonaError::Value(_) => "value",
        KonaError::Index => "index",
        KonaError::Length => "length",
        KonaError::Valence => "valence",
        KonaError::Domain => "domain",
        KonaError::Parse(ref msg) if msg == "reserved" => "reserved",
        KonaError::Parse(_) => "parse",
        KonaError::File(_) => "file",
        KonaError::System(_) => "system",
        KonaError::FeatureDisabled(_) => "feature disabled",
        KonaError::Limit => "limit",
        KonaError::Rank => "rank",
        KonaError::Int => "int",
        KonaError::WsFull => "wsfull",
    };
    K::char_list(msg.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn test_eval_basic_math() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        let ast = parse("1 + 2 * 3").unwrap();
        let res = eval_words(&ast, &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(7));

        let ast2 = parse("1 + -2").unwrap();
        let res2 = eval_words(&ast2, &mut env, &ktree).unwrap();
        assert_eq!(res2, K::int(-1));
    }

    #[test]
    fn test_eval_assignments() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        let ast1 = parse("a: 10").unwrap();
        let res1 = eval_words(&ast1, &mut env, &ktree).unwrap();
        assert_eq!(res1, K::int(10));
        assert_eq!(ktree.lookup("a").unwrap(), K::int(10));

        let ast2 = parse("a + 5").unwrap();
        let res2 = eval_words(&ast2, &mut env, &ktree).unwrap();
        assert_eq!(res2, K::int(15));

        let ast3 = parse("b:: 20").unwrap();
        let res3 = eval_words(&ast3, &mut env, &ktree).unwrap();
        assert_eq!(res3, K::int(20));
        assert_eq!(ktree.lookup("b").unwrap(), K::int(20));
    }

    #[test]
    fn test_eval_conditional() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        let ast = parse(":[0; 10; 1; 20; 30]").unwrap();
        let res = eval_words(&ast, &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(20));

        let ast2 = parse(":[0; 10; 0; 20; 30]").unwrap();
        let res2 = eval_words(&ast2, &mut env, &ktree).unwrap();
        assert_eq!(res2, K::int(30));
    }

    #[test]
    fn test_eval_early_return_control_flow() {
        let cases = [
            (":1", K::int(1)),
            (":1;2;3", K::int(1)),
            ("1;:2;3", K::int(2)),
            ("(:1;:2;:3)", K::int(3)),
            (":[(:1;:2;:3);:4]", K::int(3)),
            (":4,5,6,7", K::int_list(vec![4, 5, 6, 7])),
            ("while[:5;:6]", K::int(5)),
            ("while[5;:6]", K::int(6)),
            ("do[5;:6]", K::int(6)),
            ("do[:5;:6]", K::int(5)),
            ("if[5;:6;:7;:8]", K::int(6)),
            ("{1+{1;:2;1+x}0}0", K::int(3)),
            ("{1+{1;:2;1+x}0;7;0}0", K::int(0)),
            ("{1+{1;:2;1+x}0;:7;0}0", K::int(7)),
        ];

        for (code, expected) in cases {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse(code).unwrap();
            let res = eval_words(&ast, &mut env, &ktree)
                .unwrap_or_else(|err| panic!("early return case errored: {} -> {:?}", code, err));
            assert_eq!(res, expected, "early return case failed: {}", code);
        }
    }

    #[test]
    fn test_eval_brackets_indexing() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        let ast = parse("(10 20 30)[1]").unwrap();
        let res = eval_words(&ast, &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(20));
    }

    #[test]
    fn test_eval_brace_lambda() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        let ast = parse("{x + y}[2; 3]").unwrap();
        let res = eval_words(&ast, &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(5));
    }

    #[test]
    fn test_eval_projection() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        let ast1 = parse("f: {x + y + z}[1;;3]").unwrap();
        let _ = eval_words(&ast1, &mut env, &ktree).unwrap();

        let ast2 = parse("f[2]").unwrap();
        let res = eval_words(&ast2, &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(6));
    }

    #[test]
    fn test_eval_adverb_over() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        let ast = parse("+/ 1 2 3 4").unwrap();
        let res = eval_words(&ast, &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(10));
    }

    #[test]
    fn test_eval_adverb_each_multithreaded() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        // 10 elements (> 8 elements triggers thread spawn)
        let ast = parse("{x * 2}' 1 2 3 4 5 6 7 8 9 10").unwrap();
        let res = eval_words(&ast, &mut env, &ktree).unwrap();
        assert_eq!(res, K::int_list(vec![2, 4, 6, 8, 10, 12, 14, 16, 18, 20]));
    }

    #[test]
    fn test_exact_user_probes() {
        // 1. ghost -> value error
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("ghost").unwrap();
            assert!(eval_words(&ast, &mut env, &ktree).is_err());
        }

        // 2. 1; ghost -> value error
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("1; ghost").unwrap();
            assert!(eval_words(&ast, &mut env, &ktree).is_err());
        }

        // 3. (ghost) -> returns nil (returns K::nil())
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("(ghost)").unwrap();
            let res = eval_words(&ast, &mut env, &ktree).unwrap();
            assert!(res.is_nil());
        }

        // 4. {ghost} -> returns function
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("{ghost}").unwrap();
            let res = eval_words(&ast, &mut env, &ktree).unwrap();
            assert!(res.is_function());
        }

        // 5. {ghost}0 -> returns nil
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("{ghost}0").unwrap();
            let res = eval_words(&ast, &mut env, &ktree).unwrap();
            assert!(res.is_nil());
        }

        // 6. f:{x}; f[ghost] -> returns nil
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast1 = parse("f:{x}; f[ghost]").unwrap();
            let res = eval_words(&ast1, &mut env, &ktree).unwrap();
            assert!(res.is_nil());
        }

        // 7. f:{x}; f ghost -> throws value error
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("f:{x}; f ghost").unwrap();
            assert!(eval_words(&ast, &mut env, &ktree).is_err());
        }

        // 8. a:10 20; a[ghost] -> throws value error
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("a:10 20; a[ghost]").unwrap();
            assert!(eval_words(&ast, &mut env, &ktree).is_err());
        }

        // 9. d:.((`a;1)); d[ghost] -> throws value error
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("d:.((`a;1)); d[ghost]").unwrap();
            assert!(eval_words(&ast, &mut env, &ktree).is_err());
        }

        // 10. {a:1;a+a}0;a -> returns nil
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("{a:1;a+a}0;a").unwrap();
            let res = eval_words(&ast, &mut env, &ktree).unwrap();
            assert!(res.is_nil());
        }

        // 11. {a:1;a+a}0;.k -> returns empty .k
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("{a:1;a+a}0;.k").unwrap();
            let res = eval_words(&ast, &mut env, &ktree).unwrap();
            assert_eq!(res.len().unwrap(), 0);
        }

        // 12. if[0;ghost:1]; .k must not create ghost
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("if[0;ghost:1]; .k").unwrap();
            let res = eval_words(&ast, &mut env, &ktree).unwrap();
            assert_eq!(res.len().unwrap(), 0);
        }

        // 13. do[0;temp:1]; .k must not create temp
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("do[0;temp:1]; .k").unwrap();
            let res = eval_words(&ast, &mut env, &ktree).unwrap();
            assert_eq!(res.len().unwrap(), 0);
        }

        // 14. f:{hidden:1}; .k must not create hidden
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("f:{hidden:1}; .k").unwrap();
            let res = eval_words(&ast, &mut env, &ktree).unwrap();
            assert_eq!(res.len().unwrap(), 1);
        }

        // 15. f:{hidden:1}; f[]; .k must not create hidden globally
        {
            let ktree = Arc::new(KTree::new());
            let mut env = HashMap::new();
            let ast = parse("f:{hidden:1}; f[]; .k").unwrap();
            let res = eval_words(&ast, &mut env, &ktree).unwrap();
            assert_eq!(res.len().unwrap(), 1);
        }
    }
}
