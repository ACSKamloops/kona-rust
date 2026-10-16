#![forbid(unsafe_code)]

use crate::errors::KonaError;
use crate::types::K;
use crate::verbs::dict::KTree;
use crate::verbs::math::{normalize_list, to_vec_of_atoms};
use std::collections::HashMap;
use std::sync::Arc;


pub fn eval_adverb(
    name: &str,
    operand: &K,
    args: &[K],
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    if operand.is_atom() && !matches!(**operand, crate::types::KData::CharAtom(_)) {
        return Err(KonaError::Rank);
    }
    match name {
        "/" => eval_over(operand, args, env, ktree),
        "\\" => eval_scan(operand, args, env, ktree),
        "'" => eval_each(operand, args, env, ktree),
        "/:" => {
            if args.len() == 1 {
                if let Some((func, left)) = left_bound_projection(operand) {
                    return eval_each_right(&func, &left, Some(&args[0]), env, ktree);
                }
                return Err(KonaError::Valence);
            }
            if args.len() != 2 {
                return Err(KonaError::Valence);
            }
            eval_each_right(operand, &args[0], Some(&args[1]), env, ktree)
        }
        "\\:" => {
            if args.len() == 1 {
                if let Some((func, left)) = left_bound_projection(operand) {
                    return eval_each_left(&func, &left, Some(&args[0]), env, ktree);
                }
                return Err(KonaError::Valence);
            }
            if args.len() != 2 {
                return Err(KonaError::Valence);
            }
            eval_each_left(operand, &args[0], Some(&args[1]), env, ktree)
        }
        "':" => {
            if args.len() == 1 {
                if let Some((func, left)) = left_bound_projection(operand) {
                    return eval_each_pair(&func, &left, Some(&args[0]), env, ktree);
                }
                eval_each_pair(operand, &args[0], None, env, ktree)
            } else if args.len() == 2 {
                eval_each_pair(operand, &args[0], Some(&args[1]), env, ktree)
            } else {
                Err(KonaError::Valence)
            }
        }
        _ => Err(KonaError::Parse(format!("Unknown adverb: {}", name))),
    }
}

fn is_truthy_internal(k: &K) -> bool {
    match &**k {
        crate::types::KData::IntAtom(i) => *i != 0,
        crate::types::KData::FloatAtom(f) => *f != 0.0,
        crate::types::KData::CharAtom(c) => *c != '\0',
        crate::types::KData::SymbolAtom(s) => !s.is_empty(),
        crate::types::KData::IntList(v) => !v.is_empty() && v.iter().any(|&x| x != 0),
        crate::types::KData::FloatList(v) => !v.is_empty() && v.iter().any(|&x| x != 0.0),
        crate::types::KData::CharList(v) => !v.is_empty() && v.iter().any(|&x| x != 0),
        crate::types::KData::SymbolList(v) => !v.is_empty() && v.iter().any(|s| !s.is_empty()),
        crate::types::KData::GeneralList(v) => {
            !v.is_empty() && v.iter().any(|item| is_truthy_internal(item))
        }
        crate::types::KData::HashList { indices, .. } => {
            !indices.is_empty() && indices.iter().any(|&x| x != 0)
        }
        crate::types::KData::Dictionary { keys, .. } => !keys.is_empty(),
        crate::types::KData::Nil => false,
        crate::types::KData::Function(_) => true,
    }
}

fn valence_of(k: &K) -> usize {
    match &**k {
        crate::types::KData::Function(kfunc) => kfunc.valence(),
        _ => 0,
    }
}

fn is_empty_list(k: &K) -> bool {
    match &**k {
        crate::types::KData::IntList(v) => v.is_empty(),
        crate::types::KData::FloatList(v) => v.is_empty(),
        crate::types::KData::CharList(v) => v.is_empty(),
        crate::types::KData::SymbolList(v) => v.is_empty(),
        crate::types::KData::GeneralList(v) => v.is_empty(),
        _ => false,
    }
}

fn can_be_monadic(k: &K) -> bool {
    match &**k {
        crate::types::KData::Function(f) => match f {
            crate::types::KFunc::Primitive(_) => valence_of(k) <= 1,
            crate::types::KFunc::Adverb { .. } => true,
            crate::types::KFunc::Projection { func, args } => {
                let base = valence_of(func);
                let bound = args.iter().filter(|a| a.is_some()).count();
                base - bound <= 1
            }
            crate::types::KFunc::UserDefined { args, .. } => args.len() <= 1,
            crate::types::KFunc::Composition { g, .. } => can_be_monadic(g),
        },
        _ => false,
    }
}

fn left_bound_projection(operand: &K) -> Option<(K, K)> {
    match &**operand {
        crate::types::KData::Function(crate::types::KFunc::Projection { func, args })
            if args.len() == 2 && args[0].is_some() && args[1].is_none() =>
        {
            Some((func.clone(), args[0].clone().unwrap()))
        }
        _ => None,
    }
}

/// Items per each-right/each-left before the work is split across threads. Only pure primitives get
/// there (see parallel_safe), and a thread start costs tens of microseconds, so small lists stay serial.
const PAR_EACH_MIN: usize = 2048;

/// Only pure arithmetic primitives may run on worker threads. A user function (or anything that can
/// reach one: projections, compositions, @ and . applying a function value) updates globals through two
/// separate KTree locks, so running it on several threads loses updates: `cnt:0; 0 {cnt+:y}/: !20000`
/// came back ~108M instead of 199990000 on every multi-core run. _draw, 0: 1: and the doors stay off
/// the threads too, so a fixed seed and printed output keep program order. (Made by Claude)
fn parallel_safe(operand: &K, fixed: &K) -> bool {
    if fixed.is_function() {
        return false;
    }
    match &**operand {
        crate::types::KData::Function(crate::types::KFunc::Primitive(name)) => matches!(
            name.as_str(),
            "+" | "-" | "*" | "%" | "&" | "|" | "<" | ">" | "=" | "~" | "," | "#" | "_" | "!" | "^"
                | "$" | "_sin" | "_cos" | "_tan" | "_asin" | "_acos" | "_atan" | "_sinh" | "_cosh"
                | "_tanh" | "_exp" | "_log" | "_sqr" | "_sqrt" | "_abs" | "_floor" | "_dot" | "_mul"
                | "_in" | "_lin" | "_bin" | "_binl" | "_vs" | "_sv" | "_vsx" | "_sm" | "_ss" | "_di"
                | "_dv" | "_dvl" | "_ci" | "_ic"
        ),
        _ => false,
    }
}

fn eval_over(
    operand: &K,
    args: &[K],
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    if args.is_empty() {
        return Err(KonaError::Valence);
    }

    let is_amend_prim =
        if let crate::types::KData::Function(crate::types::KFunc::Primitive(name)) = &**operand {
            name == "@" || name == "."
        } else {
            false
        };

    if is_amend_prim && (args.len() == 3 || args.len() == 4) {
        let init = &args[0];
        let i_arg = &args[1];
        let f_arg = &args[2];
        let v_arg = if args.len() == 4 {
            Some(&args[3])
        } else {
            None
        };

        let i_vec = to_vec_of_atoms(i_arg)?;
        let mut acc = init.clone();

        if let Some(ve) = v_arg {
            if ve.is_list() {
                let ve_atoms = to_vec_of_atoms(ve)?;
                if ve_atoms.len() != i_vec.len() {
                    return Err(KonaError::Length);
                }
                for (idx_pos, ie) in i_vec.iter().enumerate() {
                    let step_args = vec![
                        acc.clone(),
                        ie.clone(),
                        f_arg.clone(),
                        ve_atoms[idx_pos].clone(),
                    ];
                    acc = crate::eval::apply_func(operand, &step_args, env, ktree)?;
                }
            } else {
                for ie in i_vec {
                    let step_args = vec![acc.clone(), ie.clone(), f_arg.clone(), ve.clone()];
                    acc = crate::eval::apply_func(operand, &step_args, env, ktree)?;
                }
            }
        } else {
            for ie in i_vec {
                let step_args = vec![acc.clone(), ie.clone(), f_arg.clone()];
                acc = crate::eval::apply_func(operand, &step_args, env, ktree)?;
            }
        }
        return Ok(acc);
    }

    if args.len() == 1 {
        // Monadic over: f/ x
        let x = &args[0];

        if let crate::types::KData::GeneralList(funcs) = &**operand {
            if funcs.len() == 2 && funcs.iter().all(|f| f.is_composable_func()) {
                let cond = &funcs[0];
                let body = &funcs[1];
                let mut curr = x.clone();
                loop {
                    let cond_val = crate::eval::apply_func(cond, &[curr.clone()], env, ktree)?;
                    if !is_truthy_internal(&cond_val) {
                        return Ok(curr);
                    }
                    curr = crate::eval::apply_func(body, &[curr], env, ktree)?;
                }
            }
        }

        if !matches!(**operand, crate::types::KData::Function(_)) {
            let x_vec = to_vec_of_atoms(x)?;
            if x_vec.is_empty() {
                return Ok(K::char_list(vec![]));
            }
            let mut res = x_vec[0].clone();
            for ye in &x_vec[1..] {
                let step1 = crate::verbs::list::join(&res, operand)?;
                res = crate::verbs::list::join(&step1, ye)?;
            }
            return Ok(res);
        }

        if can_be_monadic(operand) {
            let mut curr = x.clone();
            loop {
                let next = crate::eval::apply_func(operand, &[curr.clone()], env, ktree)?;
                if next == *x || next == curr {
                    return Ok(curr);
                }
                curr = next;
            }
        } else {
            if matches!(**x, crate::types::KData::Nil) {
                return Ok(K::nil());
            }
            // +/ */ &/ |/ over a flat number list: one pass in lanes.rs, no per-element atoms
            if let crate::types::KData::Function(crate::types::KFunc::Primitive(name)) = &**operand {
                if let Some(r) = crate::verbs::lanes::fold(name, x) {
                    return Ok(r);
                }
            }
            let x_vec = to_vec_of_atoms(x)?;
            if x_vec.is_empty() {
                if let crate::types::KData::Function(crate::types::KFunc::Primitive(name)) =
                    &**operand
                {
                    match name.as_str() {
                        "+" | "|" => Ok(K::int(0)),
                        "*" | "&" | "=" => Ok(K::int(1)),
                        "," => Ok(K::general_list(vec![])),
                        _ => Err(KonaError::Length),
                    }
                } else {
                    Ok(K::nil())
                }
            } else if x_vec.len() == 1 {
                Ok(x_vec[0].clone())
            } else {
                let mut res = x_vec[0].clone();
                for i in 1..x_vec.len() {
                    res = crate::eval::apply_func(operand, &[res, x_vec[i].clone()], env, ktree)?;
                }
                Ok(res)
            }
        }
    } else if args.len() == 2 {
        // Dyadic over: x f/ y
        let x_val = &args[0];
        let y_val = &args[1];
        if is_empty_list(y_val)
            && matches!(&**operand, crate::types::KData::Function(crate::types::KFunc::Primitive(name)) if name == ",")
        {
            if x_val.is_list() {
                return Ok(x_val.clone());
            } else {
                return crate::verbs::list::enlist(x_val);
            }
        }
        if matches!(**x_val, crate::types::KData::Function(_)) {
            let mut curr = y_val.clone();
            loop {
                let cond_val = crate::eval::apply_func(x_val, &[curr.clone()], env, ktree)?;
                if !is_truthy_internal(&cond_val) {
                    return Ok(curr);
                }
                curr = crate::eval::apply_func(operand, &[curr], env, ktree)?;
            }
        } else if let Some(n) = x_val.as_int() {
            if can_be_monadic(operand) {
                let mut res = y_val.clone();
                for _ in 0..n {
                    res = crate::eval::apply_func(operand, &[res], env, ktree)?;
                }
                Ok(res)
            } else {
                let mut res = x_val.clone();
                let y_vec = to_vec_of_atoms(y_val)?;
                for ye in y_vec {
                    res = crate::eval::apply_func(operand, &[res, ye], env, ktree)?;
                }
                Ok(res)
            }
        } else {
            let mut res = x_val.clone();
            let y_vec = to_vec_of_atoms(y_val)?;
            for ye in y_vec {
                res = crate::eval::apply_func(operand, &[res, ye], env, ktree)?;
            }
            Ok(res)
        }
    } else {
        // N-ary over: f/[init; y1; y2; ...; yk]
        let init = &args[0];
        let y_args = &args[1..];
        let mut len: Option<usize> = None;
        let mut has_list = false;
        for a in y_args {
            if a.is_list() {
                has_list = true;
                let a_len = a.len()?;
                match len {
                    None => len = Some(a_len),
                    Some(l) if l != a_len => return Err(KonaError::Length),
                    _ => {}
                }
            }
        }
        let len = len.unwrap_or(0);
        let count = if has_list { len } else { 1 };
        let mut y_vecs = Vec::with_capacity(y_args.len());
        for a in y_args {
            if a.is_list() {
                y_vecs.push(to_vec_of_atoms(a)?);
            } else {
                y_vecs.push(vec![a.clone(); count]);
            }
        }
        let mut acc = init.clone();
        for i in 0..count {
            let mut step_args = Vec::with_capacity(y_args.len() + 1);
            step_args.push(acc);
            for j in 0..y_args.len() {
                step_args.push(y_vecs[j][i].clone());
            }
            acc = crate::eval::apply_func(operand, &step_args, env, ktree)?;
        }
        Ok(acc)
    }
}

fn normalize_scan_result(operand: &K, mut res: Vec<K>) -> K {
    let is_plus =
        if let crate::types::KData::Function(crate::types::KFunc::Primitive(name)) = &**operand {
            name == "+"
        } else {
            false
        };
    if is_plus {
        let has_float = res
            .iter()
            .any(|k| matches!(**k, crate::types::KData::FloatAtom(_)));
        let all_numeric = res.iter().all(|k| {
            matches!(
                **k,
                crate::types::KData::IntAtom(_) | crate::types::KData::FloatAtom(_)
            )
        });
        if has_float && all_numeric {
            res = res
                .into_iter()
                .map(|k| match &*k.0 {
                    crate::types::KData::IntAtom(i) => K::float(*i as f64),
                    _ => k,
                })
                .collect();
        }
    }
    normalize_list(res)
}

fn eval_scan(
    operand: &K,
    args: &[K],
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    if args.is_empty() {
        return Err(KonaError::Valence);
    }

    let is_amend_prim =
        if let crate::types::KData::Function(crate::types::KFunc::Primitive(name)) = &**operand {
            name == "@" || name == "."
        } else {
            false
        };

    if is_amend_prim && (args.len() == 3 || args.len() == 4) {
        let init = &args[0];
        let i_arg = &args[1];
        let f_arg = &args[2];
        let v_arg = if args.len() == 4 {
            Some(&args[3])
        } else {
            None
        };

        let i_vec = to_vec_of_atoms(i_arg)?;
        let mut acc = init.clone();
        let mut res = vec![acc.clone()];

        if let Some(ve) = v_arg {
            if ve.is_list() {
                let ve_atoms = to_vec_of_atoms(ve)?;
                if ve_atoms.len() != i_vec.len() {
                    return Err(KonaError::Length);
                }
                for (idx_pos, ie) in i_vec.iter().enumerate() {
                    let step_args = vec![
                        acc.clone(),
                        ie.clone(),
                        f_arg.clone(),
                        ve_atoms[idx_pos].clone(),
                    ];
                    acc = crate::eval::apply_func(operand, &step_args, env, ktree)?;
                    res.push(acc.clone());
                }
            } else {
                for ie in i_vec {
                    let step_args = vec![acc.clone(), ie.clone(), f_arg.clone(), ve.clone()];
                    acc = crate::eval::apply_func(operand, &step_args, env, ktree)?;
                    res.push(acc.clone());
                }
            }
        } else {
            for ie in i_vec {
                let step_args = vec![acc.clone(), ie.clone(), f_arg.clone()];
                acc = crate::eval::apply_func(operand, &step_args, env, ktree)?;
                res.push(acc.clone());
            }
        }
        return Ok(normalize_scan_result(operand, res));
    }

    if args.len() == 1 {
        // Monadic scan: f\ x
        let x = &args[0];

        if let crate::types::KData::GeneralList(funcs) = &**operand {
            if funcs.len() == 2 && funcs.iter().all(|f| f.is_composable_func()) {
                let cond = &funcs[0];
                let body = &funcs[1];
                let mut curr = x.clone();
                let mut res = vec![curr.clone()];
                loop {
                    let cond_val = crate::eval::apply_func(cond, &[curr.clone()], env, ktree)?;
                    if !is_truthy_internal(&cond_val) {
                        return Ok(normalize_scan_result(operand, res));
                    }
                    curr = crate::eval::apply_func(body, &[curr], env, ktree)?;
                    res.push(curr.clone());
                }
            }
        }

        if matches!(**operand, crate::types::KData::Function(_)) {
            if can_be_monadic(operand) {
                let mut curr = x.clone();
                let mut res = vec![curr.clone()];
                loop {
                    let next = crate::eval::apply_func(operand, &[curr.clone()], env, ktree)?;
                    if next == *x || next == curr {
                        return Ok(normalize_scan_result(operand, res));
                    }
                    curr = next;
                    res.push(curr.clone());
                }
            } else {
                let x_vec = to_vec_of_atoms(x)?;
                if x_vec.is_empty() {
                    Ok(normalize_scan_result(operand, vec![]))
                } else if x_vec.len() == 1 {
                    Ok(normalize_scan_result(operand, vec![x_vec[0].clone()]))
                } else {
                    let mut res = vec![x_vec[0].clone()];
                    let mut curr = x_vec[0].clone();
                    for i in 1..x_vec.len() {
                        curr = crate::eval::apply_func(
                            operand,
                            &[curr, x_vec[i].clone()],
                            env,
                            ktree,
                        )?;
                        res.push(curr.clone());
                    }
                    Ok(normalize_scan_result(operand, res))
                }
            }
        } else if x.is_atom() {
            if matches!(**operand, crate::types::KData::GeneralList(_)) {
                return Ok(x.clone());
            }
            let mut curr = x.clone();
            let mut res = vec![curr.clone()];
            loop {
                let next = crate::eval::apply_func(operand, &[curr.clone()], env, ktree)?;
                if res.iter().any(|seen| seen == &next) {
                    return Ok(normalize_scan_result(operand, res));
                }
                curr = next;
                res.push(curr.clone());
            }
        } else {
            let x_vec = to_vec_of_atoms(x)?;
            if x_vec.is_empty() {
                Ok(normalize_scan_result(operand, vec![]))
            } else if x_vec.len() == 1 {
                Ok(normalize_scan_result(operand, vec![x_vec[0].clone()]))
            } else {
                let mut res = vec![x_vec[0].clone()];
                let mut curr = x_vec[0].clone();
                for i in 1..x_vec.len() {
                    curr = crate::eval::apply_func(operand, &[curr, x_vec[i].clone()], env, ktree)?;
                    res.push(curr.clone());
                }
                Ok(normalize_scan_result(operand, res))
            }
        }
    } else if args.len() == 2 {
        // Dyadic scan: x f\ y
        let x_val = &args[0];
        let y_val = &args[1];
        if matches!(**x_val, crate::types::KData::Function(_)) {
            let mut curr = y_val.clone();
            let mut res = vec![curr.clone()];
            loop {
                let cond_val = crate::eval::apply_func(x_val, &[curr.clone()], env, ktree)?;
                if !is_truthy_internal(&cond_val) {
                    return Ok(normalize_scan_result(operand, res));
                }
                curr = crate::eval::apply_func(operand, &[curr], env, ktree)?;
                res.push(curr.clone());
            }
        } else if let Some(n) = x_val.as_int() {
            if can_be_monadic(operand) {
                let mut res = vec![y_val.clone()];
                let mut curr = y_val.clone();
                for _ in 0..n {
                    curr = crate::eval::apply_func(operand, &[curr], env, ktree)?;
                    res.push(curr.clone());
                }
                Ok(normalize_scan_result(operand, res))
            } else {
                if y_val.is_atom() {
                    crate::eval::apply_func(operand, &[x_val.clone(), y_val.clone()], env, ktree)
                } else {
                    let mut res = vec![x_val.clone()];
                    let mut curr = x_val.clone();
                    let y_vec = to_vec_of_atoms(y_val)?;
                    for ye in y_vec {
                        curr = crate::eval::apply_func(operand, &[curr, ye], env, ktree)?;
                        res.push(curr.clone());
                    }
                    Ok(normalize_scan_result(operand, res))
                }
            }
        } else {
            if y_val.is_atom() {
                crate::eval::apply_func(operand, &[x_val.clone(), y_val.clone()], env, ktree)
            } else {
                let mut res = vec![x_val.clone()];
                let mut curr = x_val.clone();
                let y_vec = to_vec_of_atoms(y_val)?;
                for ye in y_vec {
                    curr = crate::eval::apply_func(operand, &[curr, ye], env, ktree)?;
                    res.push(curr.clone());
                }
                Ok(normalize_scan_result(operand, res))
            }
        }
    } else {
        // N-ary scan: f\[init; y1; y2; ...; yk]
        let init = &args[0];
        let y_args = &args[1..];
        let mut len: Option<usize> = None;
        let mut has_list = false;
        for a in y_args {
            if a.is_list() {
                has_list = true;
                let a_len = a.len()?;
                match len {
                    None => len = Some(a_len),
                    Some(l) if l != a_len => return Err(KonaError::Length),
                    _ => {}
                }
            }
        }
        let len = len.unwrap_or(0);
        let count = if has_list { len } else { 1 };
        let mut y_vecs = Vec::with_capacity(y_args.len());
        for a in y_args {
            if a.is_list() {
                y_vecs.push(to_vec_of_atoms(a)?);
            } else {
                y_vecs.push(vec![a.clone(); count]);
            }
        }
        let mut acc = init.clone();
        if !has_list {
            let mut step_args = Vec::with_capacity(y_args.len() + 1);
            step_args.push(acc);
            for j in 0..y_args.len() {
                step_args.push(y_vecs[j][0].clone());
            }
            let res_val = crate::eval::apply_func(operand, &step_args, env, ktree)?;
            Ok(res_val)
        } else {
            let mut res = vec![acc.clone()];
            for i in 0..count {
                let mut step_args = Vec::with_capacity(y_args.len() + 1);
                step_args.push(acc);
                for j in 0..y_args.len() {
                    step_args.push(y_vecs[j][i].clone());
                }
                acc = crate::eval::apply_func(operand, &step_args, env, ktree)?;
                res.push(acc.clone());
            }
            Ok(normalize_scan_result(operand, res))
        }
    }
}

fn eval_each(
    operand: &K,
    args: &[K],
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    if args.is_empty() {
        return Ok(normalize_list(vec![]));
    }
    if args.len() == 1 {
        let x = &args[0];
        if left_bound_projection(operand).is_some() && x.is_atom() {
            return crate::eval::apply_func(operand, &[x.clone()], env, ktree);
        }
        if matches!(**x, crate::types::KData::Nil) {
            return Ok(K::nil());
        }
        let x_vec = to_vec_of_atoms(x)?;
        let mut res = Vec::with_capacity(x_vec.len());
        for xe in x_vec {
            res.push(crate::eval::apply_func(operand, &[xe], env, ktree)?);
        }
        Ok(normalize_list(res))
    } else {
        // Dyadic or N-ary each: f'[x; y; ...]
        // 1. Check if all arguments are atoms.
        let all_atoms = args.iter().all(|a| a.is_atom());
        if all_atoms {
            return crate::eval::apply_func(operand, args, env, ktree);
        }
        // 2. Find the length of the lists.
        // None until the first list is seen: an empty list in first position used to leave the
        // length at 0, so a later 1 2 3 passed the check and the loop below indexed into the empty vec
        let mut len: Option<usize> = None;
        for a in args {
            if a.is_list() {
                let a_len = to_vec_of_atoms(a)?.len();
                match len {
                    None => len = Some(a_len),
                    Some(l) if l != a_len => return Err(KonaError::Length),
                    _ => {}
                }
            }
        }
        let len = len.unwrap_or(0);
        // 3. Collect elements and apply.
        let mut args_vecs = Vec::with_capacity(args.len());
        for a in args {
            args_vecs.push(to_vec_of_atoms(a)?);
        }
        let mut res = Vec::with_capacity(len);
        for i in 0..len {
            let mut slice = Vec::with_capacity(args.len());
            for j in 0..args.len() {
                if args[j].is_list() {
                    slice.push(args_vecs[j][i].clone());
                } else {
                    slice.push(args[j].clone());
                }
            }
            res.push(crate::eval::apply_func(operand, &slice, env, ktree)?);
        }
        Ok(normalize_list(res))
    }
}

/// Run job on every item. Long lists are split into one chunk per CPU core and each chunk runs on its own
/// scoped thread (std::thread::scope: the threads borrow env/ktree instead of cloning them, and all finish
/// before this returns). Results come back in the original order; the first error in item order wins,
/// like the old one-thread-per-item version. (Made by Claude)
fn par_apply<F>(items: Vec<K>, job: F) -> Result<Vec<K>, KonaError>
where
    F: Fn(K) -> Result<K, KonaError> + Sync,
{
    let workers = crate::verbs::lanes::cores().min(items.len()).max(1);
    if workers == 1 {
        return items.into_iter().map(&job).collect();
    }
    let chunk = items.len().div_ceil(workers);
    let parts: Vec<Result<Vec<K>, KonaError>> = std::thread::scope(|s| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|c| {
                let job = &job;
                s.spawn(move || c.iter().cloned().map(job).collect::<Result<Vec<K>, KonaError>>())
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err(KonaError::System("Thread panicked".to_string()))))
            .collect()
    });
    let mut out = Vec::with_capacity(items.len());
    for p in parts {
        out.extend(p?);
    }
    Ok(out)
}

fn eval_each_right(
    operand: &K,
    x: &K,
    y: Option<&K>,
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    let y_val = y.ok_or(KonaError::Valence)?;
    if y_val.is_atom() {
        return crate::eval::apply_func(operand, &[x.clone(), y_val.clone()], env, ktree);
    }
    let y_vec = to_vec_of_atoms(y_val)?;
    let len = y_vec.len();

    if len >= PAR_EACH_MIN && parallel_safe(operand, x) {
        let res = par_apply(y_vec, |ye| {
            crate::eval::apply_func(operand, &[x.clone(), ye], env, ktree)
        })?;
        Ok(normalize_list(res))
    } else {
        let mut res = Vec::new();
        for ye in y_vec {
            res.push(crate::eval::apply_func(
                operand,
                &[x.clone(), ye],
                env,
                ktree,
            )?);
        }
        Ok(normalize_list(res))
    }
}

fn eval_each_left(
    operand: &K,
    x: &K,
    y: Option<&K>,
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    let y_val = y.ok_or(KonaError::Valence)?;
    if x.is_atom() {
        return crate::eval::apply_func(operand, &[x.clone(), y_val.clone()], env, ktree);
    }
    let x_vec = to_vec_of_atoms(x)?;
    let len = x_vec.len();

    if len >= PAR_EACH_MIN && parallel_safe(operand, y_val) {
        let res = par_apply(x_vec, |xe| {
            crate::eval::apply_func(operand, &[xe, y_val.clone()], env, ktree)
        })?;
        Ok(normalize_list(res))
    } else {
        let mut res = Vec::new();
        for xe in x_vec {
            res.push(crate::eval::apply_func(
                operand,
                &[xe, y_val.clone()],
                env,
                ktree,
            )?);
        }
        Ok(normalize_list(res))
    }
}

fn eval_each_pair(
    operand: &K,
    x: &K,
    y: Option<&K>,
    env: &HashMap<String, K>,
    ktree: &Arc<KTree>,
) -> Result<K, KonaError> {
    let (lhs, rhs) = if let Some(y_val) = y {
        (Some(x), y_val)
    } else {
        (None, x)
    };

    // C Kona's each-pair (checked against its tests.c and the binary): f': y gives f[y[i];y[i-1]] for
    // i>0, so one item fewer than y; an atom gives f[y;y]; a one-item list gives (). With a left
    // argument, x f': y puts x itself in front of those pairs (10 -': 2 3 7 is 10 1 4) and x f': atom
    // is just x. The old code returned y[0] / f[y[0];x] first, which is k4's prior. (Made by Claude)
    if rhs.is_atom() {
        return match lhs {
            Some(left) => Ok(left.clone()),
            None => crate::eval::apply_func(operand, &[rhs.clone(), rhs.clone()], env, ktree),
        };
    }
    let rhs_vec = to_vec_of_atoms(rhs)?;
    if rhs_vec.is_empty() && lhs.is_none() {
        return Err(KonaError::Length);
    }
    if rhs_vec.len() < 2 {
        return Ok(K::general_list(vec![]));
    }
    let mut res = Vec::with_capacity(rhs_vec.len());
    if let Some(left) = lhs {
        res.push(left.clone());
    }
    {
        for i in 1..rhs_vec.len() {
            res.push(crate::eval::apply_func(
                operand,
                &[rhs_vec[i].clone(), rhs_vec[i - 1].clone()],
                env,
                ktree,
            )?);
        }
        Ok(normalize_list(res))
    }
}
