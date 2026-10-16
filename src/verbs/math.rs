#![forbid(unsafe_code)]

use crate::errors::KonaError;
use crate::types::{K, KData};

/// Convert a general K value into a homogeneous list if all elements are of the same type.
pub fn normalize_list(vec: Vec<K>) -> K {
    if vec.is_empty() {
        return K::general_list(vec);
    }
    let all_int = vec.iter().all(|k| matches!(**k, KData::IntAtom(_)));
    if all_int {
        let ints = vec.iter().map(|k| k.as_int().unwrap()).collect();
        return K::int_list(ints);
    }
    let all_float = vec.iter().all(|k| matches!(**k, KData::FloatAtom(_)));
    if all_float {
        let floats = vec.iter().map(|k| k.as_float().unwrap()).collect();
        return K::float_list_raw(floats);
    }
    let all_char = vec.iter().all(|k| matches!(**k, KData::CharAtom(_)));
    if all_char {
        let chars = vec.iter().map(|k| k.as_char().unwrap() as u8).collect();
        return K::char_list(chars);
    }
    let all_symbol = vec.iter().all(|k| matches!(**k, KData::SymbolAtom(_)));
    if all_symbol {
        let symbols = vec.iter().map(|k| k.as_symbol().unwrap().clone()).collect();
        return K::symbol_list(symbols);
    }
    K::general_list(vec)
}

/// Convert any K value to a vector of atoms/elements for broadcasting/recursion.
pub fn to_vec_of_atoms(k: &K) -> Result<Vec<K>, KonaError> {
    match &**k {
        KData::IntAtom(_) | KData::FloatAtom(_) | KData::CharAtom(_) | KData::SymbolAtom(_) => {
            Ok(vec![k.clone()])
        }
        KData::IntList(v) => Ok(v.iter().map(|&x| K::int(x)).collect()),
        KData::FloatList(v) => Ok(v.iter().map(|&x| K::float(x)).collect()),
        KData::CharList(v) => Ok(v.iter().map(|&x| K::char(x as char)).collect()),
        KData::SymbolList(v) => Ok(v.iter().map(|x| K::symbol(x.clone())).collect()),
        KData::GeneralList(v) => Ok(v.clone()),
        KData::HashList { indices, .. } => Ok(indices.iter().map(|&x| K::int(x)).collect()),
        KData::Dictionary { .. } => Err(KonaError::Type),
        KData::Nil => Ok(vec![k.clone()]),
        KData::Function(_) => Err(KonaError::Type),
    }
}

pub fn negate(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(i) => {
            if *i == i64::MIN {
                return Err(KonaError::Domain);
            }
            Ok(K::int(-i))
        }
        KData::FloatAtom(f) => Ok(K::float(-f)),
        KData::IntList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for &i in v {
                if i == i64::MIN {
                    return Err(KonaError::Domain);
                }
                res.push(-i);
            }
            Ok(K::int_list(res))
        }
        KData::FloatList(v) => Ok(K::float_list_raw(v.iter().map(|&f| -f).collect())),
        KData::GeneralList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(negate(item)?);
            }
            Ok(normalize_list(res))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn reciprocal(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(i) => Ok(K::float(1.0 / *i as f64)),
        KData::FloatAtom(f) => Ok(K::float(1.0 / f)),
        KData::IntList(v) => Ok(K::float_list_raw(
            v.iter().map(|&i| 1.0 / i as f64).collect(),
        )),
        KData::FloatList(v) => Ok(K::float_list_raw(v.iter().map(|&f| 1.0 / f).collect())),
        KData::GeneralList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(reciprocal(item)?);
            }
            Ok(normalize_list(res))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn kpow(mut a: f64, mut b: f64) -> f64 {
    if a.is_nan() {
        a = 0.0;
    }
    if b.is_nan() {
        if a != 0.0 {
            return f64::NAN;
        }
        b = 0.0;
    } else if b.is_infinite() && a == 1.0 {
        return f64::NAN;
    }
    if b == 0.0 {
        if a.is_infinite() { f64::NAN } else { 1.0 }
    } else if a == 0.0 {
        0.0
    } else {
        a.powf(b)
    }
}

pub fn atom_verb(x: &K) -> Result<K, KonaError> {
    Ok(K::int(if !x.is_list() { 1 } else { 0 }))
}

fn first_depth(x: &K) -> usize {
    match &**x {
        KData::GeneralList(v) if !v.is_empty() => 1 + first_depth(&v[0]),
        KData::IntList(_) | KData::FloatList(_) | KData::CharList(_) | KData::SymbolList(_) => 1,
        _ => 0,
    }
}

fn shape_check(a: &K, p: &mut [i64], d: usize) {
    if d >= p.len() {
        return;
    }
    let is_atom =
        a.is_atom() || a.is_nil() || matches!(**a, KData::Function(_) | KData::Dictionary { .. });
    let a_len = a.len().unwrap_or(0) as i64;
    if is_atom || a_len != p[d] {
        p[d] = -1;
    } else {
        let is_typed_list = matches!(
            **a,
            KData::IntList(_) | KData::FloatList(_) | KData::CharList(_) | KData::SymbolList(_)
        );
        if is_typed_list && d < p.len() - 1 {
            p[d + 1] = -1;
        } else if let KData::GeneralList(v) = &**a {
            if !v.is_empty() && p[d] != -1 && d < p.len() - 1 {
                for item in v {
                    shape_check(item, p, d + 1);
                }
            }
        }
    }
}

pub fn shape_verb(x: &K) -> Result<K, KonaError> {
    let depth = first_depth(x);
    let mut p = vec![0i64; depth];
    let mut b = x.clone();
    for i in 0..depth {
        p[i] = b.len().unwrap_or(0) as i64;
        if i < depth - 1 {
            if let KData::GeneralList(v) = &*b {
                if !v.is_empty() {
                    b = v[0].clone();
                }
            }
        }
    }
    shape_check(x, &mut p, 0);
    let mut n = 0;
    for &val in &p {
        if val == -1 {
            break;
        }
        n += 1;
    }
    let res = p[0..n].to_vec();
    Ok(K::int_list(res))
}

pub fn type_integer(x: &K) -> Result<K, KonaError> {
    let t = match &**x {
        KData::IntAtom(_) => 1,
        KData::FloatAtom(_) => 2,
        KData::CharAtom(_) => 3,
        KData::SymbolAtom(_) => 4,
        KData::IntList(_) => -1,
        KData::FloatList(_) => -2,
        KData::CharList(_) => -3,
        KData::SymbolList(_) => -4,
        KData::GeneralList(_) => 0,
        KData::HashList { .. } => -5,
        KData::Dictionary { .. } => 5,
        KData::Nil => 6,
        KData::Function(_) => 7,
    };
    Ok(K::int(t))
}

pub fn not(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(i) => Ok(K::int(if *i == 0 { 1 } else { 0 })),
        KData::FloatAtom(f) => Ok(K::int(if *f == 0.0 { 1 } else { 0 })),
        KData::CharAtom(c) => Ok(K::int(if *c == '\0' { 1 } else { 0 })),
        KData::SymbolAtom(s) => Ok(K::int(if s.is_empty() { 1 } else { 0 })),
        KData::IntList(v) => Ok(K::int_list(
            v.iter().map(|&i| if i == 0 { 1 } else { 0 }).collect(),
        )),
        KData::FloatList(v) => Ok(K::int_list(
            v.iter().map(|&f| if f == 0.0 { 1 } else { 0 }).collect(),
        )),
        KData::CharList(v) => Ok(K::int_list(
            v.iter().map(|&c| if c == 0 { 1 } else { 0 }).collect(),
        )),
        KData::SymbolList(v) => Ok(K::int_list(
            v.iter().map(|s| if s.is_empty() { 1 } else { 0 }).collect(),
        )),
        KData::GeneralList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(not(item)?);
            }
            Ok(normalize_list(res))
        }
        KData::Nil => Ok(K::int(1)),
        _ => Err(KonaError::Type),
    }
}

pub(crate) fn k_mod_int(x: i64, y: i64) -> Result<i64, KonaError> {
    // C Kona (va.c mod): a zero divisor returns the dividend unchanged, 5 ! 0 is 5
    if y == 0 {
        return Ok(x);
    }
    if x == i64::MIN && y == -1 {
        return Err(KonaError::Domain);
    }
    if x == i64::MIN || y == i64::MIN {
        return Ok(i64::MIN);
    }
    let x_f = x as f64;
    let y_f = y as f64;
    let res_f = x_f - y_f * (x_f / y_f).floor();
    if res_f.is_nan() {
        Ok(i64::MIN)
    } else if res_f == f64::INFINITY {
        Ok(i64::MAX)
    } else if res_f == f64::NEG_INFINITY {
        Ok(-i64::MAX)
    } else {
        Ok(res_f as i64)
    }
}

pub(crate) fn k_mod_float(x: f64, y: f64) -> Result<f64, KonaError> {
    if y == 0.0 {
        return Ok(x);
    }
    Ok(x - y * (x / y).floor())
}

pub(crate) fn checked_add_k(x: i64, y: i64) -> i64 {
    if x == i64::MIN || y == i64::MIN {
        return i64::MIN;
    }
    if x == i64::MAX || y == i64::MAX {
        if x == -i64::MAX || y == -i64::MAX {
            return i64::MIN;
        }
        return i64::MAX;
    }
    if x == -i64::MAX || y == -i64::MAX {
        return -i64::MAX;
    }
    match x.checked_add(y) {
        Some(res) => res,
        None => {
            if x > 0 {
                i64::MAX
            } else {
                -i64::MAX
            }
        }
    }
}

pub(crate) fn checked_sub_k(x: i64, y: i64) -> i64 {
    if x == i64::MIN || y == i64::MIN {
        return i64::MIN;
    }
    if x == i64::MAX {
        if y == i64::MAX {
            return i64::MIN;
        }
        return i64::MAX;
    }
    if x == -i64::MAX {
        if y == -i64::MAX {
            return i64::MIN;
        }
        return -i64::MAX;
    }
    if y == i64::MAX {
        return -i64::MAX;
    }
    if y == -i64::MAX {
        return i64::MAX;
    }
    match x.checked_sub(y) {
        Some(res) => res,
        None => {
            if x > 0 {
                i64::MAX
            } else {
                -i64::MAX
            }
        }
    }
}

pub(crate) fn checked_mul_k(x: i64, y: i64) -> i64 {
    if x == i64::MIN || y == i64::MIN {
        return i64::MIN;
    }
    let is_inf_x = x == i64::MAX || x == -i64::MAX;
    let is_inf_y = y == i64::MAX || y == -i64::MAX;
    if is_inf_x || is_inf_y {
        if x == 0 || y == 0 {
            return 0;
        }
        let positive = (x > 0) == (y > 0);
        return if positive { i64::MAX } else { -i64::MAX };
    }
    match x.checked_mul(y) {
        Some(res) => res,
        None => {
            let positive = (x > 0) == (y > 0);
            if positive { i64::MAX } else { -i64::MAX }
        }
    }
}

fn checked_div_k(x: i64, y: i64) -> Result<i64, KonaError> {
    if x == i64::MIN && y == -1 {
        return Err(KonaError::Domain);
    }
    if x == i64::MIN || y == i64::MIN {
        return Ok(i64::MIN);
    }
    if y == 0 {
        if x > 0 {
            return Ok(i64::MAX);
        } else if x < 0 {
            return Ok(-i64::MAX);
        } else {
            return Ok(i64::MIN);
        }
    }
    let is_inf_x = x == i64::MAX || x == -i64::MAX;
    let is_inf_y = y == i64::MAX || y == -i64::MAX;
    if is_inf_y {
        return Ok(0);
    }
    if is_inf_x {
        let positive = (x > 0) == (y > 0);
        return Ok(if positive { i64::MAX } else { -i64::MAX });
    }
    Ok(x / y)
}

fn apply_op_atoms(a: &K, b: &K, verb: &str) -> Result<KData, KonaError> {
    match (&**a, &**b) {
        (KData::IntAtom(x), KData::IntAtom(y)) => match verb {
            "+" => {
                let res = checked_add_k(*x, *y);
                Ok(KData::IntAtom(res))
            }
            "-" => {
                let res = checked_sub_k(*x, *y);
                Ok(KData::IntAtom(res))
            }
            "*" => {
                let res = checked_mul_k(*x, *y);
                Ok(KData::IntAtom(res))
            }
            "%" => {
                let res = checked_div_k(*x, *y)?;
                Ok(KData::IntAtom(res))
            }
            "&" => Ok(KData::IntAtom((*x).min(*y))),
            "|" => Ok(KData::IntAtom((*x).max(*y))),
            "<" | ">" | "=" => {
                let ord = crate::types::compare_numeric(&**a, &**b).ok_or(KonaError::Type)?;
                let res = match verb {
                    "<" => ord == std::cmp::Ordering::Less,
                    ">" => ord == std::cmp::Ordering::Greater,
                    "=" => ord == std::cmp::Ordering::Equal,
                    _ => unreachable!(),
                };
                Ok(KData::IntAtom(if res { 1 } else { 0 }))
            }
            "^" => Ok(KData::FloatAtom(kpow(*x as f64, *y as f64))),
            "!" => {
                let res = k_mod_int(*x, *y)?;
                Ok(KData::IntAtom(res))
            }
            _ => Err(KonaError::Type),
        },
        (KData::FloatAtom(x), KData::FloatAtom(y)) => match verb {
            "+" => Ok(KData::FloatAtom(x + y)),
            "-" => Ok(KData::FloatAtom(x - y)),
            "*" => Ok(KData::FloatAtom(x * y)),
            "%" => Ok(KData::FloatAtom(x / y)),
            "&" => Ok(KData::FloatAtom((*x).min(*y))),
            "|" => Ok(KData::FloatAtom((*x).max(*y))),
            "<" | ">" | "=" => {
                let ord = crate::types::compare_numeric(&**a, &**b).ok_or(KonaError::Type)?;
                let res = match verb {
                    "<" => ord == std::cmp::Ordering::Less,
                    ">" => ord == std::cmp::Ordering::Greater,
                    "=" => ord == std::cmp::Ordering::Equal,
                    _ => unreachable!(),
                };
                Ok(KData::IntAtom(if res { 1 } else { 0 }))
            }
            "^" => Ok(KData::FloatAtom(kpow(*x, *y))),
            "!" => {
                let res = k_mod_float(*x, *y)?;
                Ok(KData::FloatAtom(res))
            }
            _ => Err(KonaError::Type),
        },
        (KData::IntAtom(x), KData::FloatAtom(y)) => {
            let x_f = crate::types::i2f(*x);
            match verb {
                "+" => Ok(KData::FloatAtom(x_f + y)),
                "-" => Ok(KData::FloatAtom(x_f - y)),
                "*" => Ok(KData::FloatAtom(x_f * y)),
                "%" => Ok(KData::FloatAtom(x_f / y)),
                "&" => Ok(KData::FloatAtom(x_f.min(*y))),
                "|" => Ok(KData::FloatAtom(x_f.max(*y))),
                "<" | ">" | "=" => {
                    let ord = crate::types::compare_numeric(&**a, &**b).ok_or(KonaError::Type)?;
                    let res = match verb {
                        "<" => ord == std::cmp::Ordering::Less,
                        ">" => ord == std::cmp::Ordering::Greater,
                        "=" => ord == std::cmp::Ordering::Equal,
                        _ => unreachable!(),
                    };
                    Ok(KData::IntAtom(if res { 1 } else { 0 }))
                }
                "^" => Ok(KData::FloatAtom(kpow(x_f, *y))),
                "!" => {
                    let res = k_mod_float(x_f, *y)?;
                    Ok(KData::FloatAtom(res))
                }
                _ => Err(KonaError::Type),
            }
        }
        (KData::FloatAtom(x), KData::IntAtom(y)) => {
            let y_f = crate::types::i2f(*y);
            match verb {
                "+" => Ok(KData::FloatAtom(x + y_f)),
                "-" => Ok(KData::FloatAtom(x - y_f)),
                "*" => Ok(KData::FloatAtom(x * y_f)),
                "%" => Ok(KData::FloatAtom(x / y_f)),
                "&" => Ok(KData::FloatAtom((*x).min(y_f))),
                "|" => Ok(KData::FloatAtom((*x).max(y_f))),
                "<" | ">" | "=" => {
                    let ord = crate::types::compare_numeric(&**a, &**b).ok_or(KonaError::Type)?;
                    let res = match verb {
                        "<" => ord == std::cmp::Ordering::Less,
                        ">" => ord == std::cmp::Ordering::Greater,
                        "=" => ord == std::cmp::Ordering::Equal,
                        _ => unreachable!(),
                    };
                    Ok(KData::IntAtom(if res { 1 } else { 0 }))
                }
                "^" => Ok(KData::FloatAtom(kpow(*x, y_f))),
                "!" => {
                    let res = k_mod_float(*x, y_f)?;
                    Ok(KData::FloatAtom(res))
                }
                _ => Err(KonaError::Type),
            }
        }
        (KData::CharAtom(x), KData::CharAtom(y)) => match verb {
            "<" => Ok(KData::IntAtom(if x < y { 1 } else { 0 })),
            ">" => Ok(KData::IntAtom(if x > y { 1 } else { 0 })),
            "=" => Ok(KData::IntAtom(if x == y { 1 } else { 0 })),
            _ => Err(KonaError::Type),
        },
        (KData::SymbolAtom(x), KData::SymbolAtom(y)) => match verb {
            "<" => Ok(KData::IntAtom(if x < y { 1 } else { 0 })),
            ">" => Ok(KData::IntAtom(if x > y { 1 } else { 0 })),
            "=" => Ok(KData::IntAtom(if x == y { 1 } else { 0 })),
            _ => Err(KonaError::Type),
        },
        _ => Err(KonaError::Type),
    }
}

pub fn eval_dyadic(a: &K, b: &K, verb: &str) -> Result<K, KonaError> {
    // flat number lists go straight through lanes.rs (same answers, no per-element atoms, multi-core)
    if let Some(r) = crate::verbs::lanes::dyadic(a, b, verb)? {
        return Ok(r);
    }
    if a.is_atom() && b.is_atom() {
        let res = apply_op_atoms(a, b, verb)?;
        Ok(K::from(res))
    } else if a.is_atom() && b.is_list() {
        let b_vec = to_vec_of_atoms(b)?;
        let mut res = Vec::with_capacity(b_vec.len());
        for be in b_vec {
            res.push(eval_dyadic(a, &be, verb)?);
        }
        if res.is_empty() && matches!(verb, "<" | ">" | "=" | "<=" | ">=" | "&" | "|") {
            return Ok(K::int_list(vec![]));
        }
        Ok(normalize_list(res))
    } else if a.is_list() && b.is_atom() {
        let a_vec = to_vec_of_atoms(a)?;
        let mut res = Vec::with_capacity(a_vec.len());
        for ae in a_vec {
            res.push(eval_dyadic(&ae, b, verb)?);
        }
        if res.is_empty() && matches!(verb, "<" | ">" | "=" | "<=" | ">=" | "&" | "|") {
            return Ok(K::int_list(vec![]));
        }
        Ok(normalize_list(res))
    } else if a.is_list() && b.is_list() {
        let a_len = a.len()?;
        let b_len = b.len()?;
        let a_vec = to_vec_of_atoms(a)?;
        let b_vec = to_vec_of_atoms(b)?;
        if a_len != b_len {
            if a_len == 1 {
                let left = a_vec[0].clone();
                let mut res = Vec::with_capacity(b_len);
                for be in b_vec {
                    res.push(eval_dyadic(&left, &be, verb)?);
                }
                if res.is_empty() && matches!(verb, "<" | ">" | "=" | "<=" | ">=" | "&" | "|") {
                    return Ok(K::int_list(vec![]));
                }
                return Ok(normalize_list(res));
            }
            if b_len == 1 && matches!((&**a, &**b), (KData::CharList(_), KData::CharList(_))) {
                let right = b_vec[0].clone();
                let mut res = Vec::with_capacity(a_len);
                for ae in a_vec {
                    res.push(eval_dyadic(&ae, &right, verb)?);
                }
                if res.is_empty() && matches!(verb, "<" | ">" | "=" | "<=" | ">=" | "&" | "|") {
                    return Ok(K::int_list(vec![]));
                }
                return Ok(normalize_list(res));
            }
            return Err(KonaError::Length);
        }
        let mut res = Vec::with_capacity(a_len);
        for (ae, be) in a_vec.into_iter().zip(b_vec.into_iter()) {
            res.push(eval_dyadic(&ae, &be, verb)?);
        }
        if res.is_empty() && matches!(verb, "<" | ">" | "=" | "<=" | ">=" | "&" | "|") {
            return Ok(K::int_list(vec![]));
        }
        Ok(normalize_list(res))
    } else {
        Err(KonaError::Type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monadic_math() {
        let i = K::int(5);
        assert_eq!(negate(&i).unwrap(), K::int(-5));
        assert_eq!(reciprocal(&i).unwrap(), K::float(0.2));
        assert_eq!(not(&i).unwrap(), K::int(0));
        assert_eq!(not(&K::int(0)).unwrap(), K::int(1));

        let l = K::int_list(vec![0, 1, 2]);
        assert_eq!(negate(&l).unwrap(), K::int_list(vec![0, -1, -2]));
        assert_eq!(not(&l).unwrap(), K::int_list(vec![1, 0, 0]));
    }

    #[test]
    fn test_math_overflow_guards() {
        let min_val = K::int(i64::MIN);
        assert!(negate(&min_val).is_err());
        assert!(abs_verb(&min_val).is_err());
        assert!(eval_dyadic(&min_val, &K::int(-1), "%").is_err());
        assert!(eval_dyadic(&min_val, &K::int(-1), "!").is_err());

        let min_list = K::int_list(vec![0, i64::MIN]);
        assert!(negate(&min_list).is_err());
        assert!(abs_verb(&min_list).is_err());

        assert!(draw_verb(&min_val, &K::int(5)).is_err());
        assert!(draw_verb(&K::int(5), &min_val).is_err());
        assert!(vs_or_vsx(&K::int(2), &min_val, false).is_err());
        assert!(vs_or_vsx(&K::int_list(vec![2, 3]), &min_val, false).is_err());
        assert!(crate::verbs::cast_verb(&min_val, &K::char('a')).is_err());
    }

    #[test]
    fn test_dyadic_math_broadcasting() {
        // Atom and Atom
        let a1 = K::int(3);
        let a2 = K::int(4);
        assert_eq!(eval_dyadic(&a1, &a2, "+").unwrap(), K::int(7));
        assert_eq!(eval_dyadic(&a1, &a2, "*").unwrap(), K::int(12));

        // Atom and List
        let list = K::int_list(vec![10, 20, 30]);
        assert_eq!(
            eval_dyadic(&a1, &list, "+").unwrap(),
            K::int_list(vec![13, 23, 33])
        );
        assert_eq!(
            eval_dyadic(&list, &a1, "+").unwrap(),
            K::int_list(vec![13, 23, 33])
        );

        // List and List
        let list2 = K::int_list(vec![1, 2, 3]);
        assert_eq!(
            eval_dyadic(&list, &list2, "-").unwrap(),
            K::int_list(vec![9, 18, 27])
        );

        // Mismatched length
        let list_short = K::int_list(vec![1, 2]);
        assert!(eval_dyadic(&list, &list_short, "+").is_err());
    }

    #[test]
    fn test_kona_verbs_audit() {
        // Monadic @ (atom)
        assert_eq!(atom_verb(&K::int(42)).unwrap(), K::int(1));
        assert_eq!(atom_verb(&K::float(3.14)).unwrap(), K::int(1));
        assert_eq!(atom_verb(&K::int_list(vec![1, 2])).unwrap(), K::int(0));
        assert_eq!(
            atom_verb(&K::general_list(vec![K::int(1)])).unwrap(),
            K::int(0)
        );

        // Monadic ^ (shape)
        assert_eq!(shape_verb(&K::int(42)).unwrap(), K::int_list(vec![]));
        assert_eq!(
            shape_verb(&K::int_list(vec![1, 2, 3])).unwrap(),
            K::int_list(vec![3])
        );
        let matrix = K::general_list(vec![
            K::int_list(vec![1, 2]),
            K::int_list(vec![3, 4]),
            K::int_list(vec![5, 6]),
        ]);
        assert_eq!(shape_verb(&matrix).unwrap(), K::int_list(vec![3, 2]));

        let jagged = K::general_list(vec![K::int_list(vec![1, 2, 3]), K::int_list(vec![4, 5])]);
        assert_eq!(shape_verb(&jagged).unwrap(), K::int_list(vec![2]));

        // Dyadic ^ (power)
        assert_eq!(
            eval_dyadic(&K::int(2), &K::int(3), "^").unwrap(),
            K::float(8.0)
        );
        assert_eq!(
            eval_dyadic(&K::float(2.0), &K::float(3.0), "^").unwrap(),
            K::float(8.0)
        );
        let base = K::int_list(vec![2, 3]);
        let exp = K::int(3);
        assert_eq!(
            eval_dyadic(&base, &exp, "^").unwrap(),
            K::float_list(vec![8.0, 27.0])
        );

        // Monadic $ (format)
        assert_eq!(
            format_verb(&K::int(42)).unwrap(),
            K::char_list(b"42".to_vec())
        );
        assert_eq!(
            format_verb(&K::symbol("hello")).unwrap(),
            K::char_list(b"hello".to_vec())
        );
        let formatted_list = format_verb(&K::int_list(vec![10, 20])).unwrap();
        assert_eq!(
            formatted_list,
            K::general_list(vec![
                K::char_list(b"10".to_vec()),
                K::char_list(b"20".to_vec()),
            ])
        );
    }
}

pub fn char_from_int(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(i) => {
            let c = (*i as u8) as char;
            Ok(K::char(c))
        }
        KData::IntList(v) => {
            let bytes = v.iter().map(|&i| i as u8).collect();
            Ok(K::char_list(bytes))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn int_from_char(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::CharAtom(c) => Ok(K::int(*c as i64)),
        KData::CharList(v) => {
            let ints = v.iter().map(|&b| b as i64).collect();
            Ok(K::int_list(ints))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn sqrt_verb(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(i) => Ok(K::float((*i as f64).sqrt())),
        KData::FloatAtom(f) => Ok(K::float(f.sqrt())),
        KData::IntList(v) => Ok(K::float_list_raw(
            v.iter().map(|&i| (i as f64).sqrt()).collect(),
        )),
        KData::FloatList(v) => Ok(K::float_list_raw(v.iter().map(|&f| f.sqrt()).collect())),
        _ => Err(KonaError::Type),
    }
}

static PRNG_SEED: std::sync::Mutex<u64> = std::sync::Mutex::new(1337);

pub fn get_prng_seed() -> u64 {
    *PRNG_SEED.lock().unwrap()
}

pub fn set_prng_seed(seed: u64) {
    *PRNG_SEED.lock().unwrap() = seed;
}

/// One step of the generator. The state advances as a 64-bit LCG (so a \r seed still fixes the sequence)
/// but the number handed out is the splitmix64 mix of that state. The old code sampled `state % n`
/// straight off the LCG, and the low bits of a power-of-two LCG are periodic - bit 0 alternates, so
/// `10 _draw 2` was 1 0 1 0 1 0 1 0 1 0 every time and `10 _draw 4` cycled 2 1 0 3. (Made by Claude)
fn rand_next(rng: &mut u64) -> u64 {
    *rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut z = *rng;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// Uniform integer in 0..n (n > 0) from the top bits: no modulo bias, no low-bit patterns
fn rand_below(rng: &mut u64, n: usize) -> usize {
    ((rand_next(rng) as u128 * n as u128) >> 64) as usize
}

/// Uniform float in [0,1) from the top 53 bits
fn rand_unit(rng: &mut u64) -> f64 {
    (rand_next(rng) >> 11) as f64 / (1u64 << 53) as f64
}

/// -n _draw m deals n distinct indices below m: a sparse Fisher-Yates, so -5 _draw 1000000000 is O(5)
fn deal_indices(rng: &mut u64, count: usize, pool_size: usize) -> Vec<usize> {
    let mut chosen = Vec::with_capacity(count);
    let mut pool_map = std::collections::HashMap::new();
    for i in 0..count {
        let j = i + rand_below(rng, pool_size - i);
        let val_i = pool_map.get(&i).copied().unwrap_or(i);
        let val_j = pool_map.get(&j).copied().unwrap_or(j);
        chosen.push(val_j);
        pool_map.insert(j, val_i);
    }
    chosen
}

pub fn draw_verb(a: &K, b: &K) -> Result<K, KonaError> {
    // a shape on the left draws the product and reshapes it: 2 3 _draw 10 is a 2x3 matrix (C Kona r.c)
    if let KData::IntList(dims) = &**a {
        if dims.iter().any(|&d| d < 0) {
            return Err(KonaError::Int);
        }
        let total = dims
            .iter()
            .try_fold(1i64, |acc, &d| acc.checked_mul(d))
            .ok_or(KonaError::WsFull)?;
        let flat = draw_verb(&K::int(total), b)?;
        return crate::verbs::list::take(a, &flat);
    }
    let n = a.as_int().ok_or(KonaError::Type)?;
    if n == i64::MIN {
        return Err(KonaError::Domain);
    }
    let count = n.unsigned_abs() as usize;
    if count >= 2147483647 {
        // like enumerate: a wsfull error, not an aborted process from Vec::with_capacity
        return Err(KonaError::WsFull);
    }
    let deal = n < 0;

    let mut seed_guard = PRNG_SEED
        .lock()
        .map_err(|_| KonaError::System("PRNG lock failed".to_string()))?;
    let mut rng = *seed_guard;

    if rng == 1337 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1337);
        rng ^= now;
    }

    let result = if b.is_list() {
        let b_atoms = to_vec_of_atoms(b)?;
        let pool_size = b_atoms.len();
        if pool_size == 0 {
            return Err(KonaError::Domain);
        }
        if deal && count > pool_size {
            return Err(KonaError::Domain);
        }
        let chosen = if deal {
            deal_indices(&mut rng, count, pool_size)
        } else {
            (0..count).map(|_| rand_below(&mut rng, pool_size)).collect()
        };
        let res_atoms: Vec<K> = chosen.iter().map(|&idx| b_atoms[idx].clone()).collect();
        normalize_list(res_atoms)
    } else if let Some(f) = b.as_float() {
        let res: Vec<f64> = (0..count).map(|_| rand_unit(&mut rng) * f).collect();
        K::float_list_raw(res)
    } else {
        let m = b.as_int().ok_or(KonaError::Type)?;
        if m == i64::MIN {
            return Err(KonaError::Domain);
        }
        let pool_size = m.unsigned_abs() as usize;
        let deal = deal || m < 0;
        if pool_size == 0 && !deal {
            // n _draw 0 = n random floats in [0,1), as in C Kona
            let res: Vec<f64> = (0..count).map(|_| rand_unit(&mut rng)).collect();
            *seed_guard = rng;
            return Ok(K::float_list_raw(res));
        }
        if pool_size == 0 {
            return Err(KonaError::Domain);
        }
        if deal && count > pool_size {
            return Err(KonaError::Domain);
        }
        let chosen = if deal {
            deal_indices(&mut rng, count, pool_size)
        } else {
            (0..count).map(|_| rand_below(&mut rng, pool_size)).collect()
        };
        let res_ints: Vec<i64> = chosen.iter().map(|&idx| idx as i64).collect();
        K::int_list(res_ints)
    };

    *seed_guard = rng;
    Ok(result)
}

pub fn format_verb(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::CharAtom(c) => Ok(K::char_list(vec![*c as u8])),
        KData::CharList(_) => Ok(x.clone()),
        KData::Nil => Ok(K::char_list(vec![])),
        KData::SymbolAtom(s) => Ok(K::char_list(s.as_bytes().to_vec())),
        KData::IntAtom(i) => Ok(K::char_list(i.to_string().into_bytes())),
        KData::FloatAtom(f) => Ok(K::char_list(format!("{}", f).into_bytes())),
        KData::IntList(v) => {
            let res = v
                .iter()
                .map(|&i| format_verb(&K::int(i)))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(K::general_list(res))
        }
        KData::FloatList(v) => {
            let res = v
                .iter()
                .map(|&f| format_verb(&K::float(f)))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(K::general_list(res))
        }
        KData::SymbolList(v) => {
            let res = v
                .iter()
                .map(|s| format_verb(&K::symbol(s.clone())))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(K::general_list(res))
        }
        KData::GeneralList(v) => {
            let res = v
                .iter()
                .map(|item| format_verb(item))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(K::general_list(res))
        }
        KData::Dictionary { .. } => Ok(K::char_list(b".(..)".to_vec())),
        _ => {
            let formatted = crate::io::format_k(x);
            Ok(K::char_list(formatted.into_bytes()))
        }
    }
}

pub fn string_verb(x: &K) -> Result<K, KonaError> {
    Ok(K::char_list(crate::io::format_k(x).into_bytes()))
}

fn tolerant_floor_ceil(e: f64, is_ceil: bool) -> i64 {
    if e.is_nan() {
        return i64::MIN;
    }
    if e.is_infinite() {
        return if e.is_sign_negative() {
            -i64::MAX
        } else {
            i64::MAX
        };
    }
    if e <= -i64::MAX as f64 {
        return -i64::MAX;
    }
    if e >= i64::MAX as f64 {
        return i64::MAX;
    }
    let f = e.fract();
    let cond =
        (f > 0.0 && crate::types::float_eq(f, 1.0)) || (f < 0.0 && crate::types::float_eq(f, 0.0));
    let val = if is_ceil {
        if cond { e.floor() } else { e.ceil() }
    } else {
        if cond { e.ceil() } else { e.floor() }
    };
    val as i64
}

pub fn floor_verb(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(_) => Ok(x.clone()),
        KData::FloatAtom(f) => Ok(K::int(tolerant_floor_ceil(*f, false))),
        KData::IntList(_) => Ok(x.clone()),
        KData::FloatList(v) => {
            let ints = v.iter().map(|&f| tolerant_floor_ceil(f, false)).collect();
            Ok(K::int_list(ints))
        }
        KData::GeneralList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(floor_verb(item)?);
            }
            Ok(normalize_list(res))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn floor_intolerant_verb(x: &K) -> Result<K, KonaError> {
    apply_monadic_float_op(x, |f| f.floor())
}


pub fn is_null_data(kd: &KData) -> bool {
    match kd {
        KData::IntAtom(i) => *i == i64::MIN,
        KData::FloatAtom(f) => f.is_nan(),
        KData::CharAtom(c) => *c == ' ',
        KData::SymbolAtom(s) => s.is_empty(),
        _ => false,
    }
}

pub fn null_verb(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(_) | KData::FloatAtom(_) | KData::CharAtom(_) | KData::SymbolAtom(_) => {
            Ok(K::int(if is_null_data(x) { 1 } else { 0 }))
        }
        KData::IntList(v) => Ok(K::int_list(
            v.iter()
                .map(|&i| if i == i64::MIN { 1 } else { 0 })
                .collect(),
        )),
        KData::FloatList(v) => Ok(K::int_list(
            v.iter().map(|&f| if f.is_nan() { 1 } else { 0 }).collect(),
        )),
        KData::CharList(v) => Ok(K::int_list(
            v.iter().map(|&c| if c == b' ' { 1 } else { 0 }).collect(),
        )),
        KData::SymbolList(v) => Ok(K::int_list(
            v.iter().map(|s| if s.is_empty() { 1 } else { 0 }).collect(),
        )),
        KData::GeneralList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(null_verb(item)?);
            }
            Ok(normalize_list(res))
        }
        KData::Nil => Ok(K::int(1)),
        _ => Ok(K::int(0)),
    }
}

pub fn fill_verb(a: &K, b: &K) -> Result<K, KonaError> {
    if b.is_atom() {
        if is_null_data(b) {
            Ok(a.clone())
        } else {
            Ok(b.clone())
        }
    } else {
        match &**b {
            KData::IntList(v) => {
                let fill_val = a.as_int().ok_or(KonaError::Type)?;
                let res = v
                    .iter()
                    .map(|&i| if i == i64::MIN { fill_val } else { i })
                    .collect();
                Ok(K::int_list(res))
            }
            KData::FloatList(v) => {
                let fill_val = a
                    .as_float()
                    .or_else(|| a.as_int().map(|i| i as f64))
                    .ok_or(KonaError::Type)?;
                let res = v
                    .iter()
                    .map(|&f| if f.is_nan() { fill_val } else { f })
                    .collect();
                Ok(K::float_list_raw(res))
            }
            KData::CharList(v) => {
                let fill_val = a.as_char().ok_or(KonaError::Type)? as u8;
                let res = v
                    .iter()
                    .map(|&c| if c == b' ' { fill_val } else { c })
                    .collect();
                Ok(K::char_list(res))
            }
            KData::SymbolList(v) => {
                let fill_val = a.as_symbol().cloned().ok_or(KonaError::Type)?;
                let res = v
                    .iter()
                    .map(|s| {
                        if s.is_empty() {
                            fill_val.clone()
                        } else {
                            s.clone()
                        }
                    })
                    .collect();
                Ok(K::symbol_list(res))
            }
            KData::GeneralList(v) => {
                let mut res = Vec::with_capacity(v.len());
                for item in v {
                    res.push(fill_verb(a, item)?);
                }
                Ok(normalize_list(res))
            }
            _ => Ok(b.clone()),
        }
    }
}

pub fn apply_monadic_float_op<F>(x: &K, f: F) -> Result<K, KonaError>
where
    F: Fn(f64) -> f64 + Copy + Sync,
{
    // big lists run on every core into pooled memory (lanes::map1) - cos/sin/exp/log are heavy enough to scale
    match &**x {
        KData::IntAtom(i) => Ok(K::float(f(*i as f64))),
        KData::FloatAtom(val) => Ok(K::float(f(*val))),
        KData::IntList(v) => Ok(K::float_list_raw(crate::verbs::lanes::map1(v, |i: i64| f(i as f64)))),
        KData::FloatList(v) => Ok(K::float_list_raw(crate::verbs::lanes::map1(v, f))),
        KData::GeneralList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(apply_monadic_float_op(item, f)?);
            }
            Ok(normalize_list(res))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn abs_verb(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(i) => {
            if *i == i64::MIN {
                return Err(KonaError::Domain);
            }
            Ok(K::int(i.abs()))
        }
        KData::FloatAtom(f) => Ok(K::float(f.abs())),
        KData::IntList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for &i in v {
                if i == i64::MIN {
                    return Err(KonaError::Domain);
                }
                res.push(i.abs());
            }
            Ok(K::int_list(res))
        }
        KData::FloatList(v) => Ok(K::float_list_raw(v.iter().map(|&f| f.abs()).collect())),
        KData::GeneralList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(abs_verb(item)?);
            }
            Ok(normalize_list(res))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn ceil_verb(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(_) => Ok(x.clone()),
        KData::FloatAtom(f) => Ok(K::int(tolerant_floor_ceil(*f, true))),
        KData::IntList(_) => Ok(x.clone()),
        KData::FloatList(v) => {
            let ints = v.iter().map(|&f| tolerant_floor_ceil(f, true)).collect();
            Ok(K::int_list(ints))
        }
        KData::GeneralList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(ceil_verb(item)?);
            }
            Ok(normalize_list(res))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn exp_verb(x: &K) -> Result<K, KonaError> {
    apply_monadic_float_op(x, |f| f.exp())
}
pub fn log_verb(x: &K) -> Result<K, KonaError> {
    apply_monadic_float_op(x, |f| f.ln())
}
pub fn sqr_verb(x: &K) -> Result<K, KonaError> {
    apply_monadic_float_op(x, |f| f * f)
}
pub fn sin_verb(x: &K) -> Result<K, KonaError> {
    apply_monadic_float_op(x, |f| f.sin())
}
pub fn cos_verb(x: &K) -> Result<K, KonaError> {
    apply_monadic_float_op(x, |f| f.cos())
}
pub fn tan_verb(x: &K) -> Result<K, KonaError> {
    apply_monadic_float_op(x, |f| f.tan())
}
pub fn asin_verb(x: &K) -> Result<K, KonaError> {
    apply_monadic_float_op(x, |f| f.asin())
}
pub fn acos_verb(x: &K) -> Result<K, KonaError> {
    apply_monadic_float_op(x, |f| f.acos())
}
pub fn atan_verb(x: &K) -> Result<K, KonaError> {
    apply_monadic_float_op(x, |f| f.atan())
}
pub fn sinh_verb(x: &K) -> Result<K, KonaError> {
    apply_monadic_float_op(x, |f| f.sinh())
}
pub fn cosh_verb(x: &K) -> Result<K, KonaError> {
    apply_monadic_float_op(x, |f| f.cosh())
}
pub fn tanh_verb(x: &K) -> Result<K, KonaError> {
    apply_monadic_float_op(x, |f| f.tanh())
}

fn cmp_cix(k: &K, x: &K) -> std::cmp::Ordering {
    use crate::types::float_eq;
    match (&**k, &**x) {
        (KData::IntAtom(val_k), KData::FloatAtom(val_x)) => {
            if val_x.is_nan() {
                std::cmp::Ordering::Greater
            } else {
                let k_f = crate::types::i2f(*val_k);
                if float_eq(k_f, *val_x) {
                    std::cmp::Ordering::Equal
                } else {
                    k_f.partial_cmp(val_x).unwrap_or(std::cmp::Ordering::Equal)
                }
            }
        }
        (KData::FloatAtom(val_k), KData::IntAtom(val_x)) => {
            if val_k.is_nan() {
                std::cmp::Ordering::Less
            } else {
                let x_f = crate::types::i2f(*val_x);
                if float_eq(*val_k, x_f) {
                    std::cmp::Ordering::Equal
                } else {
                    val_k.partial_cmp(&x_f).unwrap_or(std::cmp::Ordering::Equal)
                }
            }
        }
        _ => crate::verbs::list::cmp_k(k, x),
    }
}

pub fn bin_search(x: &K, y: &K) -> Result<K, KonaError> {
    let x_atoms = to_vec_of_atoms(x)?;
    if x_atoms.is_empty() {
        return Err(KonaError::Domain);
    }
    if y.is_list() {
        let y_atoms = to_vec_of_atoms(y)?;
        let mut res = Vec::with_capacity(y_atoms.len());
        for ye in y_atoms {
            res.push(bin_search(x, &ye)?);
        }
        return Ok(normalize_list(res));
    }
    let mut low = 0 as i64;
    let mut high = x_atoms.len() as i64;

    while low < high {
        let mid = low + (high - low) / 2;
        if cmp_cix(&x_atoms[mid as usize], y) != std::cmp::Ordering::Less {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    Ok(K::int(low))
}

/// x _in y: is x, as a whole, one of the items of y. C Kona's tests.c has 4 3 _in 1 7 2 4 6 3 -> 0 and
/// "abc" _in ("abc";"def") is 1; the per-element form used to live here, but that is _lin's job.
pub fn in_verb(x: &K, y: &K) -> Result<K, KonaError> {
    let y_atoms = to_vec_of_atoms(y)?;
    let found = y_atoms.contains(x);
    Ok(K::int(if found { 1 } else { 0 }))
}

pub fn val_verb(x: &K, ktree: &std::sync::Arc<crate::verbs::dict::KTree>) -> Result<K, KonaError> {
    match &**x {
        KData::CharList(v) => {
            let s = std::str::from_utf8(v).map_err(|_| KonaError::Type)?;
            let trimmed = s.trim();
            if let Ok(i) = trimmed.parse::<i64>() {
                Ok(K::int(i))
            } else if let Ok(f) = trimmed.parse::<f64>() {
                Ok(K::float(f))
            } else {
                let words = crate::parser::parse(trimmed)?;
                let mut env = std::collections::HashMap::new();
                crate::eval::eval_words(&words, &mut env, ktree)
            }
        }
        KData::SymbolAtom(s) => {
            if let Some(val) = ktree.lookup(s) {
                Ok(val)
            } else {
                Err(KonaError::Value(format!("Undefined variable: {}", s)))
            }
        }
        _ => Err(KonaError::Type),
    }
}

pub fn size_verb(x: &K) -> Result<K, KonaError> {
    let path = match &**x {
        KData::SymbolAtom(s) => s.as_ref(),
        KData::CharList(v) => std::str::from_utf8(v).map_err(|_| KonaError::Type)?,
        _ => return Err(KonaError::Type),
    };
    let size = std::fs::metadata(path)
        .map(|m| m.len() as f64)
        .unwrap_or(0.0);
    Ok(K::float(size))
}

static CUSTOM_ENV: std::sync::Mutex<Option<std::collections::HashMap<String, String>>> =
    std::sync::Mutex::new(None);

fn set_custom_env(name: String, val: String) {
    if let Ok(mut guard) = CUSTOM_ENV.lock() {
        if guard.is_none() {
            *guard = Some(std::collections::HashMap::new());
        }
        if let Some(map) = guard.as_mut() {
            map.insert(name, val);
        }
    }
}

/// every variable set with _setenv, handed to the programs Kona starts (they used to read them from the
/// OS environment, which Rust 2024 only lets you change inside unsafe)
fn custom_env_pairs() -> Vec<(String, String)> {
    match CUSTOM_ENV.lock() {
        Ok(guard) => guard.as_ref().map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn get_custom_env(name: &str) -> Option<String> {
    if let Ok(guard) = CUSTOM_ENV.lock() {
        if let Some(map) = guard.as_ref() {
            return map.get(name).cloned();
        }
    }
    None
}

pub fn getenv_verb(x: &K) -> Result<K, KonaError> {
    let name = match &**x {
        KData::SymbolAtom(s) => s.as_ref(),
        KData::CharList(v) => std::str::from_utf8(v).map_err(|_| KonaError::Type)?,
        _ => return Err(KonaError::Type),
    };
    let val = get_custom_env(name).unwrap_or_else(|| std::env::var(name).unwrap_or_default());
    Ok(K::char_list(val.into_bytes()))
}

pub fn exit_verb(x: &K) -> Result<K, KonaError> {
    let code = x.as_int().ok_or(KonaError::Type)?;
    std::process::exit(code as i32);
}

fn jdn_from_date(year: i64, month: i64, day: i64) -> i64 {
    let a = (14 - month) / 12;
    let y = year + 4800 - a;
    let m = month + 12 * a - 3;
    day + (153 * m + 2) / 5 + y * 365 + y / 4 - y / 100 + y / 400 - 32045 - 2464329
}

fn date_from_jdn(j: i64) -> i64 {
    let a = j + 32044 + 2464329;
    let b = (4 * a + 3) / 146097;
    let c = a - (b * 146097) / 4;
    let d = (4 * c + 3) / 1461;
    let e = c - (1461 * d) / 4;
    let m = (5 * e + 2) / 153;
    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let year = b * 100 + d - 4800 + m / 10;
    year * 10000 + month * 100 + day
}

fn get_timezone_offset() -> i64 {
    static OFFSET: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *OFFSET.get_or_init(|| {
        if let Ok(output) = std::process::Command::new("date")
            .args(&["-d", "@0", "+%z"])
            .envs(custom_env_pairs())
            .output()
        {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if s.len() == 5 {
                let sign = if s.starts_with('+') { 1 } else { -1 };
                if let (Ok(hours), Ok(mins)) = (s[1..3].parse::<i64>(), s[3..5].parse::<i64>()) {
                    return sign * (hours * 3600 + mins * 60);
                }
            }
        }
        0
    })
}

pub fn jd_verb(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(val) => {
            let val = *val;
            Ok(K::int(jdn_from_date(
                val / 10000,
                (val / 100) % 100,
                val % 100,
            )))
        }
        KData::IntList(v) => {
            let res = v
                .iter()
                .map(|&val| jdn_from_date(val / 10000, (val / 100) % 100, val % 100))
                .collect();
            Ok(K::int_list(res))
        }
        KData::GeneralList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(jd_verb(item)?);
            }
            Ok(normalize_list(res))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn dj_verb(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(val) => Ok(K::int(date_from_jdn(*val))),
        KData::IntList(v) => {
            let res = v.iter().map(|&val| date_from_jdn(val)).collect();
            Ok(K::int_list(res))
        }
        KData::GeneralList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(dj_verb(item)?);
            }
            Ok(normalize_list(res))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn lt_verb(x: &K) -> Result<K, KonaError> {
    let offset = get_timezone_offset();
    match &**x {
        KData::IntAtom(val) => Ok(K::int(*val + offset)),
        KData::IntList(v) => {
            let res = v.iter().map(|&val| val + offset).collect();
            Ok(K::int_list(res))
        }
        KData::GeneralList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(lt_verb(item)?);
            }
            Ok(normalize_list(res))
        }
        _ => Err(KonaError::Type),
    }
}

fn gtime_single(x: i64) -> (i64, i64) {
    let days = x.div_euclid(86400);
    let secs = x.rem_euclid(86400);
    let date = date_from_jdn(days);
    let hour = secs / 3600;
    let min = (secs % 3600) / 60;
    let sec = secs % 60;
    let time = hour * 10000 + min * 100 + sec;
    (date, time)
}

pub fn gtime_verb(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(val) => {
            let (date, time) = gtime_single(*val);
            Ok(K::general_list(vec![K::int(date), K::int(time)]))
        }
        KData::IntList(v) => {
            let mut dates = Vec::with_capacity(v.len());
            let mut times = Vec::with_capacity(v.len());
            for &val in v {
                let (d, t) = gtime_single(val);
                dates.push(d);
                times.push(t);
            }
            Ok(K::general_list(vec![
                K::int_list(dates),
                K::int_list(times),
            ]))
        }
        KData::GeneralList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(gtime_verb(item)?);
            }
            Ok(normalize_list(res))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn ltime_verb(x: &K) -> Result<K, KonaError> {
    let lt = lt_verb(x)?;
    gtime_verb(&lt)
}

pub fn vs_verb(x: &K, y: &K) -> Result<K, KonaError> {
    vs_or_vsx(x, y, false)
}

pub fn vsx_verb(x: &K, y: &K) -> Result<K, KonaError> {
    vs_or_vsx(x, y, true)
}

fn split_by_slice(text: &[u8], delim: &[u8]) -> Vec<Vec<u8>> {
    if delim.is_empty() {
        return vec![text.to_vec()];
    }
    let mut result = Vec::new();
    let mut last_end = 0;
    let mut i = 0;
    while i + delim.len() <= text.len() {
        if &text[i..i + delim.len()] == delim {
            result.push(text[last_end..i].to_vec());
            last_end = i + delim.len();
            i = last_end;
        } else {
            i += 1;
        }
    }
    result.push(text[last_end..].to_vec());
    result
}

pub fn vs_or_vsx(x: &K, y: &K, is_extended: bool) -> Result<K, KonaError> {
    match (&**x, &**y) {
        (KData::CharAtom(c), KData::CharList(y_bytes)) => {
            let delim = [*c as u8];
            let parts = split_by_slice(y_bytes, &delim);
            let parts_k: Vec<K> = parts.into_iter().map(K::char_list).collect();
            Ok(K::general_list(parts_k))
        }
        (KData::CharList(delim), KData::CharList(y_bytes)) => {
            let parts = split_by_slice(y_bytes, delim);
            let parts_k: Vec<K> = parts.into_iter().map(K::char_list).collect();
            Ok(K::general_list(parts_k))
        }
        (_, KData::GeneralList(v)) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(vs_or_vsx(x, item, is_extended)?);
            }
            Ok(normalize_list(res))
        }
        (_, KData::IntList(v)) => {
            let mut res = Vec::with_capacity(v.len());
            for &val in v {
                res.push(vs_or_vsx(x, &K::int(val), is_extended)?);
            }
            Ok(normalize_list(res))
        }
        (KData::IntAtom(base), KData::IntAtom(val)) => {
            let a = *base;
            let mut b = *val;
            if a < 2 {
                return Err(KonaError::Domain);
            }
            if b == i64::MIN {
                return Err(KonaError::Domain);
            }
            if b == 0 {
                if is_extended {
                    Ok(K::int_list(vec![0]))
                } else {
                    Ok(K::int_list(vec![]))
                }
            } else {
                let mut z = Vec::new();
                let mut c = b / a;
                while z.is_empty() || b != c {
                    z.push(b);
                    b = c;
                    c = b / a;
                }
                for i in 0..z.len() {
                    z[i] = z[i] % a;
                }
                z.reverse();
                Ok(K::int_list(z))
            }
        }
        (KData::IntList(bases), KData::IntAtom(val)) => {
            let xn = bases.len();
            if xn == 0 {
                // (!0) _vs 5 is !0 in C Kona; the 0..xn-1 loops below would underflow
                return Ok(K::int_list(vec![]));
            }
            for i in 0..xn {
                if !is_extended && i == 0 && bases[0] == 0 {
                    continue;
                }
                if bases[i] < 1 {
                    return Err(KonaError::Domain);
                }
            }
            let mut a = *val;
            if a == i64::MIN {
                return Err(KonaError::Domain);
            }
            if a < 0 {
                let mut s = 1;
                for i in 0..xn {
                    let base_val = if !is_extended && i == 0 && bases[0] == 0 {
                        -1
                    } else {
                        bases[i]
                    };
                    s *= if base_val != 0 { base_val } else { -1 };
                }
                a = s - (-a % s);
            }
            let mut z = vec![0; xn];
            for i in 0..xn {
                z[i] = bases[xn - 1 - i];
            }
            if xn > 0 {
                z[0] = if z[0] != 0 { a / z[0] } else { 0 };
            }
            for i in 0..xn - 1 {
                let d = z[i + 1];
                z[i + 1] = if d != 0 { z[i] / d } else { 0 };
            }
            for i in 0..xn - 1 {
                let idx = xn - 1 - i;
                z[idx] = z[idx - 1] - z[idx] * bases[i];
            }
            if xn > 0 {
                z[0] = a - z[0] * bases[xn - 1];
            }
            z.reverse();
            Ok(K::int_list(z))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn sv_verb(x: &K, y: &K) -> Result<K, KonaError> {
    match (&**x, &**y) {
        (KData::CharAtom(_), _) | (KData::CharList(_), _) => {
            let delim_bytes = match &**x {
                KData::CharAtom(c) => vec![*c as u8],
                KData::CharList(v) => v.clone(),
                _ => unreachable!(),
            };
            let y_atoms = to_vec_of_atoms(y)?;
            let mut joined = Vec::new();
            for (idx, item) in y_atoms.iter().enumerate() {
                if idx > 0 {
                    joined.extend_from_slice(&delim_bytes);
                }
                match &**item {
                    KData::CharAtom(c) => joined.push(*c as u8),
                    KData::CharList(v) => joined.extend_from_slice(v),
                    KData::Nil => {}
                    _ => return Err(KonaError::Type),
                }
            }
            Ok(K::char_list(joined))
        }
        (_, KData::GeneralList(v)) => {
            let mut res = Vec::with_capacity(v.len());
            for item in v {
                res.push(sv_verb(x, item)?);
            }
            Ok(normalize_list(res))
        }
        (KData::IntAtom(_base), KData::IntAtom(digit)) => Ok(K::int(*digit)),
        (KData::IntAtom(base), KData::IntList(digits)) => {
            let mut acc = 0;
            for &digit in digits {
                acc = acc * (*base) + digit;
            }
            Ok(K::int(acc))
        }
        (KData::IntList(bases), KData::IntList(digits)) => {
            if bases.len() != digits.len() {
                return Err(KonaError::Length);
            }
            let mut acc = 0;
            for i in 0..bases.len() {
                acc = acc * bases[i] + digits[i];
            }
            Ok(K::int(acc))
        }
        _ => Err(KonaError::Type),
    }
}

fn sum_k(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(_) | KData::FloatAtom(_) => Ok(x.clone()),
        KData::IntList(v) => {
            let s: i64 = v.iter().sum();
            Ok(K::int(s))
        }
        KData::FloatList(v) => {
            let s: f64 = v.iter().sum();
            Ok(K::float(s))
        }
        KData::GeneralList(v) => {
            if v.is_empty() {
                return Ok(K::int(0));
            }
            let mut acc = v[0].clone();
            for item in &v[1..] {
                acc = eval_dyadic(&acc, item, "+")?;
            }
            Ok(acc)
        }
        _ => Err(KonaError::Type),
    }
}

pub fn dot_verb(a: &K, b: &K) -> Result<K, KonaError> {
    let prod = eval_dyadic(a, b, "*")?;
    sum_k(&prod)
}

pub fn mul_verb(x: &K, y: &K) -> Result<K, KonaError> {
    match &**x {
        KData::GeneralList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for xi in v {
                res.push(dot_verb(xi, y)?);
            }
            Ok(normalize_list(res))
        }
        KData::IntList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for &xi in v {
                res.push(dot_verb(&K::int(xi), y)?);
            }
            Ok(normalize_list(res))
        }
        KData::FloatList(v) => {
            let mut res = Vec::with_capacity(v.len());
            for &xi in v {
                res.push(dot_verb(&K::float(xi), y)?);
            }
            Ok(normalize_list(res))
        }
        _ => dot_verb(x, y),
    }
}

fn radius(u: f64, v: f64) -> f64 {
    let au = u.abs();
    let av = v.abs();
    if au > av {
        let aw = av / au;
        au * (1.0 + aw * aw).sqrt()
    } else if av != 0.0 {
        let aw = au / av;
        av * (1.0 + aw * aw).sqrt()
    } else {
        0.0
    }
}

fn sign(u: f64, v: f64) -> f64 {
    if v >= 0.0 { u.abs() } else { -u.abs() }
}

fn svdcmp(
    a: &mut [Vec<f64>],
    m: usize,
    n: usize,
    w: &mut [f64],
    v: &mut [Vec<f64>],
    rv1: &mut [f64],
) -> Result<(), KonaError> {
    let mut g: f64 = 0.0;
    let mut scale: f64 = 0.0;
    let mut anorm: f64 = 0.0;
    let nm1 = n - 1;
    let mm1 = m - 1;

    for i in 0..n {
        let l = i + 1;
        rv1[i] = scale * g;
        g = 0.0;
        let mut s = 0.0;
        scale = 0.0;

        if i < m {
            for k in i..m {
                scale += a[k][i].abs();
            }
            if scale != 0.0 {
                for k in i..m {
                    a[k][i] /= scale;
                    s += a[k][i] * a[k][i];
                }
                let f = a[i][i];
                g = -sign(s.sqrt(), f);
                let h = f * g - s;
                a[i][i] = f - g;
                if i != nm1 {
                    for j in l..n {
                        let mut s2 = 0.0;
                        for k in i..m {
                            s2 += a[k][i] * a[k][j];
                        }
                        let f2 = s2 / h;
                        for k in i..m {
                            a[k][j] += f2 * a[k][i];
                        }
                    }
                }
                for k in i..m {
                    a[k][i] *= scale;
                }
            }
        }

        w[i] = scale * g;
        g = 0.0;
        s = 0.0;
        scale = 0.0;

        if i < m && i != nm1 {
            for k in l..n {
                scale += a[i][k].abs();
            }
            if scale != 0.0 {
                for k in l..n {
                    a[i][k] /= scale;
                    s += a[i][k] * a[i][k];
                }
                let f = a[i][l];
                g = -sign(s.sqrt(), f);
                let h = f * g - s;
                a[i][l] = f - g;
                for k in l..n {
                    rv1[k] = a[i][k] / h;
                }
                if i != mm1 {
                    for j in l..m {
                        let mut s2 = 0.0;
                        for k in l..n {
                            s2 += a[j][k] * a[i][k];
                        }
                        for k in l..n {
                            a[j][k] += s2 * rv1[k];
                        }
                    }
                }
                for k in l..n {
                    a[i][k] *= scale;
                }
            }
        }
        anorm = anorm.max(w[i].abs() + rv1[i].abs());
    }

    for i in (0..n).rev() {
        let l = i + 1;
        if i < nm1 {
            if g != 0.0 {
                for j in l..n {
                    v[j][i] = (a[i][j] / a[i][l]) / g;
                }
                for j in l..n {
                    let mut s = 0.0;
                    for k in l..n {
                        s += a[i][k] * v[k][j];
                    }
                    for k in l..n {
                        v[k][j] += s * v[k][i];
                    }
                }
            }
            for j in l..n {
                v[i][j] = 0.0;
                v[j][i] = 0.0;
            }
        }
        v[i][i] = 1.0;
        g = rv1[i];
    }

    for i in (0..n).rev() {
        let l = i + 1;
        g = w[i];
        if i < nm1 {
            for j in l..n {
                a[i][j] = 0.0;
            }
        }
        if g != 0.0 {
            g = 1.0 / g;
            if i != nm1 {
                for j in l..n {
                    let mut s = 0.0;
                    for k in l..m {
                        s += a[k][i] * a[k][j];
                    }
                    let f = (s / a[i][i]) * g;
                    for k in i..m {
                        a[k][j] += f * a[k][i];
                    }
                }
            }
            for j in i..m {
                a[j][i] *= g;
            }
        } else {
            for j in i..m {
                a[j][i] = 0.0;
            }
        }
        a[i][i] += 1.0;
    }

    for k in (0..n).rev() {
        for its in 0..30 {
            let mut flag = true;
            let mut l = k;
            for l_idx in (0..=k).rev() {
                l = l_idx;
                if l == 0 {
                    break;
                }
                let nm = l - 1;
                if rv1[l].abs() + anorm == anorm {
                    flag = false;
                    break;
                }
                if w[nm].abs() + anorm == anorm {
                    break;
                }
            }

            if flag {
                let mut c = 0.0;
                let mut s = 1.0;
                for i in l..=k {
                    let f = s * rv1[i];
                    rv1[i] = c * rv1[i];
                    if f.abs() + anorm == anorm {
                        break;
                    }
                    let g = w[i];
                    let h = radius(f, g);
                    w[i] = h;
                    let h_inv = 1.0 / h;
                    c = g * h_inv;
                    s = -f * h_inv;
                    let nm_idx = l - 1;
                    for j in 0..m {
                        let y = a[j][nm_idx];
                        let z = a[j][i];
                        a[j][nm_idx] = y * c + z * s;
                        a[j][i] = z * c - y * s;
                    }
                }
            }

            let z = w[k];
            if l == k {
                if z < 0.0 {
                    w[k] = -z;
                    for j in 0..n {
                        v[j][k] = -v[j][k];
                    }
                }
                break;
            }
            if its == 29 {
                return Err(KonaError::Limit);
            }

            let mut x = w[l];
            let nm_k = k - 1;
            let y = w[nm_k];
            g = rv1[nm_k];
            let h = rv1[k];
            let f = ((y - z) * (y + z) + (g - h) * (g + h)) / (2.0 * h * y);
            g = radius(f, 1.0);
            let mut f = ((x - z) * (x + z) + h * ((y / (f + sign(g, f))) - h)) / x;
            let mut c = 1.0;
            let mut s = 1.0;
            for j in l..=nm_k {
                let i = j + 1;
                g = rv1[i];
                let y = w[i];
                let h = s * g;
                g = c * g;
                let z = radius(f, h);
                rv1[j] = z;
                c = f / z;
                s = h / z;
                let f_new = x * c + g * s;
                g = g * c - x * s;
                let h_new = y * s;
                let y_new = y * c;
                for jj in 0..n {
                    x = v[jj][j];
                    let z_val = v[jj][i];
                    v[jj][j] = x * c + z_val * s;
                    v[jj][i] = z_val * c - x * s;
                }
                let z = radius(f_new, h_new);
                w[j] = z;
                let (c_rot, s_rot) = if z != 0.0 {
                    let z_inv = 1.0 / z;
                    (f_new * z_inv, h_new * z_inv)
                } else {
                    (c, s)
                };
                let f_next = c_rot * g + s_rot * y_new;
                let x_next = c_rot * y_new - s_rot * g;
                for jj in 0..m {
                    let y_val = a[jj][j];
                    let z_val = a[jj][i];
                    a[jj][j] = y_val * c_rot + z_val * s_rot;
                    a[jj][i] = z_val * c_rot - y_val * s_rot;
                }
                f = f_next;
                x = x_next;
            }
            rv1[l] = 0.0;
            rv1[k] = f;
            w[k] = x;
        }
    }

    Ok(())
}

pub fn lsq_verb(a: &K, b: &K) -> Result<K, KonaError> {
    let at = match &**a {
        KData::IntList(_) | KData::FloatList(_) => -1,
        KData::GeneralList(_) => 0,
        _ => return Err(KonaError::Type),
    };

    let b_cols = match &**b {
        KData::GeneralList(cols) => cols,
        _ => return Err(KonaError::Type),
    };

    if b_cols.is_empty() {
        return Err(KonaError::Length);
    }

    let bn = b_cols.len();
    let r = b_cols[0].len()?;
    if r == 0 {
        return Err(KonaError::Length);
    }

    for col in b_cols {
        let len = col.len()?;
        if len != r {
            return Err(KonaError::Length);
        }
        match &**col {
            KData::IntList(_) | KData::FloatList(_) => {}
            _ => return Err(KonaError::Type),
        }
    }

    if at == 0 {
        let a_cols = a.as_general_list().unwrap();
        if a_cols.is_empty() {
            return Err(KonaError::Length);
        }
        for col in a_cols {
            let len = col.len()?;
            if len != r {
                return Err(KonaError::Length);
            }
            match &**col {
                KData::IntList(_) | KData::FloatList(_) => {}
                _ => return Err(KonaError::Type),
            }
        }
    } else {
        let len = a.len()?;
        if len != r {
            return Err(KonaError::Length);
        }
    }

    let n = bn;
    let m = r.max(n);

    let mut u = vec![vec![0.0; n]; m];
    let mut w = vec![0.0; n];
    let mut v = vec![vec![0.0; n]; n];
    let mut t = vec![0.0; n];
    let mut rv1 = vec![0.0; n];

    for j in 0..n {
        let col = &b_cols[j];
        match &**col {
            KData::IntList(vec) => {
                for i in 0..r {
                    u[i][j] = vec[i] as f64;
                }
            }
            KData::FloatList(vec) => {
                for i in 0..r {
                    u[i][j] = vec[i];
                }
            }
            _ => unreachable!(),
        }
    }

    svdcmp(&mut u, m, n, &mut w, &mut v, &mut rv1)?;

    let mut wmax = 0.0;
    for i in 0..n {
        if w[i] > wmax {
            wmax = w[i];
        }
    }
    let thresh = 1.0e-6 * wmax;
    for i in 0..n {
        if w[i] < thresh {
            w[i] = 0.0;
        }
    }

    let get_a_val = |col: &K, idx: usize| -> f64 {
        match &**col {
            KData::IntList(vec) => vec[idx] as f64,
            KData::FloatList(vec) => vec[idx],
            _ => 0.0,
        }
    };

    if at == 0 {
        let a_cols = a.as_general_list().unwrap();
        let an = a_cols.len();
        let mut z_cols = Vec::with_capacity(an);
        for k in 0..an {
            let y_col = &a_cols[k];
            let mut x_col = vec![0.0; n];

            for i in 0..n {
                let mut s = 0.0;
                if w[i] != 0.0 {
                    for j in 0..m {
                        let y_val = if j < r { get_a_val(y_col, j) } else { 0.0 };
                        s += u[j][i] * y_val;
                    }
                    s /= w[i];
                }
                t[i] = s;
            }

            for i in 0..n {
                let mut s = 0.0;
                for j in 0..n {
                    s += v[i][j] * t[j];
                }
                x_col[i] = s;
            }
            z_cols.push(K::float_list_raw(x_col));
        }
        Ok(K::general_list(z_cols))
    } else {
        let mut x_col = vec![0.0; n];

        for i in 0..n {
            let mut s = 0.0;
            if w[i] != 0.0 {
                for j in 0..m {
                    let y_val = if j < r { get_a_val(a, j) } else { 0.0 };
                    s += u[j][i] * y_val;
                }
                s /= w[i];
            }
            t[i] = s;
        }

        for i in 0..n {
            let mut s = 0.0;
            for j in 0..n {
                s += v[i][j] * t[j];
            }
            x_col[i] = s;
        }
        Ok(K::float_list(x_col))
    }
}

pub fn inv_verb(x: &K) -> Result<K, KonaError> {
    let cols = match &**x {
        KData::GeneralList(v) => v,
        _ => return Err(KonaError::Type),
    };
    if cols.is_empty() {
        return Err(KonaError::Length);
    }
    let c = cols.len();
    let mut identity_cols = Vec::with_capacity(c);
    for i in 0..c {
        let mut col = vec![0.0; c];
        col[i] = 1.0;
        identity_cols.push(K::float_list_raw(col));
    }
    let identity = K::general_list(identity_cols);
    lsq_verb(&identity, x)
}

pub fn bd_verb(x: &K) -> Result<K, KonaError> {
    let bytes = crate::io::serialize_k(x);
    Ok(K::char_list(bytes))
}

pub fn db_verb(x: &K) -> Result<K, KonaError> {
    let bytes = match &**x {
        KData::CharList(v) => v,
        _ => return Err(KonaError::Type),
    };
    let mut deserializer = crate::io::KDeserializer::new(bytes);
    let res = deserializer.read_k()?;
    if deserializer.remaining() > 0 {
        return Err(KonaError::Parse("Corrupt binary data".to_string()));
    }
    Ok(res)
}

use std::net::ToSocketAddrs;

pub fn host_verb(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::SymbolAtom(s) => {
            let host_str = s.as_ref();
            if host_str == "localhost" || host_str == "127.0.0.1" {
                return Ok(K::int(2130706433));
            }
            let query = if host_str.contains(':') {
                host_str.to_string()
            } else {
                format!("{}:80", host_str)
            };
            if let Ok(mut addrs) = query.to_socket_addrs() {
                if let Some(addr) = addrs.next() {
                    if let std::net::IpAddr::V4(ipv4) = addr.ip() {
                        let octets = ipv4.octets();
                        let val = ((octets[0] as i64) << 24)
                            | ((octets[1] as i64) << 16)
                            | ((octets[2] as i64) << 8)
                            | (octets[3] as i64);
                        return Ok(K::int(val));
                    }
                }
            }
            Err(KonaError::System("DNS lookup failed".to_string()))
        }
        KData::IntAtom(val) => {
            let ip_val = *val;
            let ip_str = format!(
                "{}.{}.{}.{}",
                (ip_val >> 24) & 0xFF,
                (ip_val >> 16) & 0xFF,
                (ip_val >> 8) & 0xFF,
                ip_val & 0xFF
            );
            if let Ok(output) = std::process::Command::new("getent")
                .args(&["hosts", &ip_str])
                .envs(custom_env_pairs())
                .output()
            {
                let s = String::from_utf8_lossy(&output.stdout);
                let parts: Vec<&str> = s.split_whitespace().collect();
                if parts.len() >= 2 {
                    return Ok(K::symbol(parts[1]));
                }
            }
            if ip_val == 2130706433 {
                return Ok(K::symbol("localhost"));
            }
            Err(KonaError::System("Reverse DNS lookup failed".to_string()))
        }
        _ => Err(KonaError::Type),
    }
}

/// Sets an environment variable in Kona's own environment cache. _getenv reads the cache first, and the
/// programs Kona starts get the cache passed to them (custom_env_pairs), so the OS environment is never
/// written - that write is unsafe in Rust 2024 because other threads may be reading it.
pub fn setenv_verb(a: &K, b: &K) -> Result<K, KonaError> {
    let name = match &**a {
        KData::SymbolAtom(s) => s.as_ref(),
        _ => return Err(KonaError::Type),
    };
    let val = match &**b {
        KData::CharList(v) => std::str::from_utf8(v).map_err(|_| KonaError::Type)?,
        _ => return Err(KonaError::Type),
    };
    set_custom_env(name.to_string(), val.to_string());
    Ok(K::nil())
}
