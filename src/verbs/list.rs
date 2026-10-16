#![forbid(unsafe_code)]

use crate::errors::KonaError;
use crate::types::{K, KData};
use crate::verbs::math::{normalize_list, to_vec_of_atoms};

// --- Monadic Verbs ---

pub fn first(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(_) | KData::FloatAtom(_) | KData::CharAtom(_) | KData::SymbolAtom(_) => {
            Ok(x.clone())
        }
        KData::IntList(v) => {
            if v.is_empty() {
                Ok(K::nil())
            } else {
                Ok(K::int(v[0]))
            }
        }
        KData::FloatList(v) => {
            if v.is_empty() {
                Ok(K::nil())
            } else {
                Ok(K::float(v[0]))
            }
        }
        KData::CharList(v) => {
            if v.is_empty() {
                Ok(K::nil())
            } else {
                Ok(K::char(v[0] as char))
            }
        }
        KData::SymbolList(v) => {
            if v.is_empty() {
                Ok(K::nil())
            } else {
                Ok(K::symbol(v[0].clone()))
            }
        }
        KData::GeneralList(v) => {
            if v.is_empty() {
                Ok(K::nil())
            } else {
                Ok(v[0].clone())
            }
        }
        KData::HashList { indices, .. } => {
            if indices.is_empty() {
                Ok(K::nil())
            } else {
                Ok(K::int(indices[0]))
            }
        }
        KData::Dictionary { keys, .. } => first(keys),
        KData::Nil => Ok(K::nil()),
        KData::Function(_) => Err(KonaError::Type),
    }
}

pub fn reverse(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(_) | KData::FloatAtom(_) | KData::CharAtom(_) | KData::SymbolAtom(_) => {
            Ok(x.clone())
        }
        KData::IntList(v) => {
            let mut rev = v.clone();
            rev.reverse();
            Ok(K::int_list(rev))
        }
        KData::FloatList(v) => {
            let mut rev = v.clone();
            rev.reverse();
            Ok(K::float_list(rev))
        }
        KData::CharList(v) => {
            let mut rev = v.clone();
            rev.reverse();
            Ok(K::char_list(rev))
        }
        KData::SymbolList(v) => {
            let mut rev = v.clone();
            rev.reverse();
            Ok(K::symbol_list(rev))
        }
        KData::GeneralList(v) => {
            let mut rev = v.clone();
            rev.reverse();
            Ok(K::general_list(rev))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn enlist(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(i) => Ok(K::int_list(vec![*i])),
        KData::FloatAtom(f) => Ok(K::float_list(vec![*f])),
        KData::CharAtom(c) => Ok(K::char_list(vec![*c as u8])),
        KData::SymbolAtom(s) => Ok(K::symbol_list(vec![s.clone()])),
        _ => Ok(K::general_list(vec![x.clone()])),
    }
}

pub fn count(x: &K) -> Result<K, KonaError> {
    let len = x.len()?;
    Ok(K::int(len as i64))
}

pub fn where_verb(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(n) => {
            if *n < 0 {
                return Err(KonaError::Domain);
            }
            if *n >= 2147483647 {
                return Err(KonaError::Int);
            }
            Ok(K::int_list(vec![0; *n as usize]))
        }
        KData::IntList(v) => {
            let mut total_size: i64 = 0;
            for &count in v {
                if count < 0 {
                    return Err(KonaError::Domain);
                }
                total_size = total_size.checked_add(count).ok_or(KonaError::Int)?;
            }
            if total_size >= 2147483647 {
                return Err(KonaError::Int);
            }
            let mut res = Vec::with_capacity(total_size as usize);
            for (i, &count) in v.iter().enumerate() {
                for _ in 0..count {
                    res.push(i as i64);
                }
            }
            Ok(K::int_list(res))
        }
        _ => Err(KonaError::Type),
    }
}

pub fn enumerate(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::IntAtom(n) => {
            if *n < 0 {
                return Err(KonaError::Domain);
            }
            if *n >= 2147483647 {
                return Err(KonaError::WsFull);
            }
            Ok(K::int_list((0..*n).collect()))
        }
        KData::Dictionary { keys, .. } => Ok(keys.clone()),
        _ => Err(KonaError::Type),
    }
}

pub fn c_type_code(kd: &KData) -> i8 {
    match kd {
        KData::HashList { .. } => -5,
        KData::SymbolList(_) => -4,
        KData::CharList(_) => -3,
        KData::FloatList(_) => -2,
        KData::IntList(_) => -1,
        KData::GeneralList(_) => 0,
        KData::IntAtom(_) => 1,
        KData::FloatAtom(_) => 2,
        KData::CharAtom(_) => 3,
        KData::SymbolAtom(_) => 4,
        KData::Dictionary { .. } => 5,
        KData::Nil => 6,
        KData::Function(_) => 7,
    }
}

pub fn cmp_k(a: &K, b: &K) -> std::cmp::Ordering {
    use crate::types::float_eq;
    
    let ta = c_type_code(&**a);
    let tb = c_type_code(&**b);
    
    if ta != tb {
        return ta.cmp(&tb);
    }
    
    if ta <= 0 && ta.abs() != 3 {
        if let (Some(la), Some(lb)) = (a.len().ok(), b.len().ok()) {
            if la != lb {
                return la.cmp(&lb);
            }
        }
    }
    
    match (&**a, &**b) {
        (KData::IntAtom(x), KData::IntAtom(y)) => x.cmp(y),
        (KData::FloatAtom(x), KData::FloatAtom(y)) => {
            if x.is_nan() && y.is_nan() {
                std::cmp::Ordering::Equal
            } else if x.is_nan() {
                std::cmp::Ordering::Less
            } else if y.is_nan() {
                std::cmp::Ordering::Greater
            } else if float_eq(*x, *y) {
                std::cmp::Ordering::Equal
            } else {
                x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
            }
        }
        (KData::IntAtom(x), KData::FloatAtom(y)) => {
            if y.is_nan() {
                std::cmp::Ordering::Greater
            } else {
                let x_f = crate::types::i2f(*x);
                if float_eq(x_f, *y) {
                    std::cmp::Ordering::Equal
                } else {
                    x_f.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
                }
            }
        }
        (KData::FloatAtom(x), KData::IntAtom(y)) => {
            if x.is_nan() {
                std::cmp::Ordering::Less
            } else {
                let y_f = crate::types::i2f(*y);
                if float_eq(*x, y_f) {
                    std::cmp::Ordering::Equal
                } else {
                    x.partial_cmp(&y_f).unwrap_or(std::cmp::Ordering::Equal)
                }
            }
        }
        (KData::CharAtom(x), KData::CharAtom(y)) => x.cmp(y),
        (KData::SymbolAtom(x), KData::SymbolAtom(y)) => x.cmp(y),
        (_, _) if a.is_list() && b.is_list() => {
            if let (Ok(va), Ok(vb)) = (to_vec_of_atoms(a), to_vec_of_atoms(b)) {
                let len = va.len().min(vb.len());
                for i in 0..len {
                    let cmp = cmp_k(&va[i], &vb[i]);
                    if cmp != std::cmp::Ordering::Equal {
                        return cmp;
                    }
                }
                va.len().cmp(&vb.len())
            } else {
                std::cmp::Ordering::Equal
            }
        }
        _ => std::cmp::Ordering::Equal,
    }
}

pub fn grade_up(x: &K) -> Result<K, KonaError> {
    if let KData::FloatList(v) = &**x {
        let mut indices: Vec<i64> = (0..v.len() as i64).collect();
        indices.sort_by(|&i, &j| {
            let fi = v[i as usize];
            let fj = v[j as usize];
            if fi.is_nan() && fj.is_nan() {
                std::cmp::Ordering::Equal
            } else if fi.is_nan() {
                std::cmp::Ordering::Less
            } else if fj.is_nan() {
                std::cmp::Ordering::Greater
            } else {
                fi.total_cmp(&fj)
            }
        });
        return Ok(K::int_list(indices));
    }
    let elements = to_vec_of_atoms(x)?;
    let mut indices: Vec<i64> = (0..elements.len() as i64).collect();
    indices.sort_by(|&i, &j| cmp_k(&elements[i as usize], &elements[j as usize]));
    Ok(K::int_list(indices))
}

pub fn grade_down(x: &K) -> Result<K, KonaError> {
    if let KData::FloatList(v) = &**x {
        let mut indices: Vec<i64> = (0..v.len() as i64).collect();
        indices.sort_by(|&i, &j| {
            let fi = v[i as usize];
            let fj = v[j as usize];
            if fi.is_nan() && fj.is_nan() {
                std::cmp::Ordering::Equal
            } else if fi.is_nan() {
                std::cmp::Ordering::Greater
            } else if fj.is_nan() {
                std::cmp::Ordering::Less
            } else {
                fj.total_cmp(&fi)
            }
        });
        return Ok(K::int_list(indices));
    }
    let elements = to_vec_of_atoms(x)?;
    let mut indices: Vec<i64> = (0..elements.len() as i64).collect();
    indices.sort_by(|&i, &j| cmp_k(&elements[j as usize], &elements[i as usize]));
    Ok(K::int_list(indices))
}

pub fn group(x: &K) -> Result<K, KonaError> {
    let elements = to_vec_of_atoms(x)?;
    let mut indices: Vec<Vec<i64>> = Vec::new();
    let mut key_positions: std::collections::HashMap<crate::types::ExactK, usize> = std::collections::HashMap::new();

    for (i, val) in elements.iter().enumerate() {
        match key_positions.entry(crate::types::ExactK(val.clone())) {
            std::collections::hash_map::Entry::Occupied(oe) => {
                let pos: usize = *oe.get();
                let vec: &mut Vec<i64> = &mut indices[pos];
                vec.push(i as i64);
            }
            std::collections::hash_map::Entry::Vacant(ve) => {
                let pos = indices.len();
                indices.push(vec![i as i64]);
                ve.insert(pos);
            }
        }
    }
    let val_list = K::general_list(indices.into_iter().map(K::int_list).collect());
    Ok(val_list)
}

// --- Dyadic Verbs ---

pub fn join(a: &K, b: &K) -> Result<K, KonaError> {
    let mut a_vec = to_vec_of_atoms(a)?;
    let mut b_vec = to_vec_of_atoms(b)?;
    a_vec.append(&mut b_vec);
    Ok(normalize_list(a_vec))
}

pub fn take(a: &K, b: &K) -> Result<K, KonaError> {
    let mut dims = match &**a {
        KData::IntAtom(n) => vec![*n],
        KData::IntList(v) => v.clone(),
        KData::GeneralList(v) => {
            let mut dims = Vec::new();
            for item in v {
                if let Some(n) = item.as_int() {
                    dims.push(n);
                } else {
                    return Err(KonaError::Type);
                }
            }
            dims
        }
        _ => return Err(KonaError::Type),
    };

    let neg_ones = dims.iter().filter(|&&d| d == -1).count();
    if neg_ones == 1 {
        let b_len = b.len().unwrap_or(0) as i64;
        let other_prod: i64 = dims
            .iter()
            .filter(|&&d| d != -1)
            .map(|&d| d.abs())
            .product();
        if other_prod > 0 {
            let auto_dim = b_len / other_prod;
            for d in &mut dims {
                if *d == -1 {
                    *d = auto_dim;
                }
            }
        } else {
            return Err(KonaError::Length);
        }
    }

    if dims.is_empty() {
        return Ok(normalize_list(vec![]));
    }

    let mut total_elems: i64 = 1;
    for &d in &dims {
        if d == 0 {
            total_elems = 0;
            break;
        }
        total_elems = total_elems.checked_mul(d.abs()).unwrap_or(i64::MAX);
    }
    if total_elems >= 2147483647 {
        return Err(KonaError::WsFull);
    }

    fn group_elems(elems: &[K], dims: &[i64]) -> K {
        if dims.len() == 1 {
            normalize_list(elems.to_vec())
        } else {
            let chunk_size: i64 = dims[1..].iter().product();
            let mut sub_lists = Vec::new();
            for chunk in elems.chunks(chunk_size as usize) {
                sub_lists.push(group_elems(chunk, &dims[1..]));
            }
            normalize_list(sub_lists)
        }
    }

    if dims.len() == 1 {
        let n = dims[0];
        if n == 0 {
            let empty_res = match &**b {
                KData::IntAtom(_) | KData::IntList(_) => K::int_list(vec![]),
                KData::FloatAtom(_) | KData::FloatList(_) => K::float_list(vec![]),
                KData::CharAtom(_) | KData::CharList(_) => K::char_list(vec![]),
                KData::SymbolAtom(_) | KData::SymbolList(_) => K::symbol_list(vec![]),
                _ => K::general_list(vec![]),
            };
            return Ok(empty_res);
        }
        let elements = to_vec_of_atoms(b)?;
        if elements.is_empty() {
            let count = n.abs() as usize;
            let proto = match &**b {
                KData::IntList(_) => K::int(0),
                KData::FloatList(_) => K::float(0.0),
                KData::CharList(_) => K::char(' '),
                KData::SymbolList(_) => K::symbol(""),
                _ => K::nil(),
            };
            return Ok(normalize_list(vec![proto; count]));
        }

        if n >= 0 {
            let mut res = Vec::with_capacity(n as usize);
            for i in 0..n {
                res.push(elements[(i as usize) % elements.len()].clone());
            }
            Ok(normalize_list(res))
        } else {
            let take_len = -n as usize;
            let len = elements.len();
            let mut res = Vec::with_capacity(take_len);
            for i in 0..take_len {
                let idx = (i + len - (take_len % len)) % len;
                res.push(elements[idx].clone());
            }
            Ok(normalize_list(res))
        }
    } else {
        if dims.iter().any(|&d| d <= 0) {
            return Ok(normalize_list(vec![]));
        }
        let prod: i64 = dims.iter().product();
        let orig_elems = to_vec_of_atoms(b)?;
        if orig_elems.is_empty() {
            let proto = match &**b {
                KData::IntList(_) => K::int(0),
                KData::FloatList(_) => K::float(0.0),
                KData::CharList(_) => K::char(' '),
                KData::SymbolList(_) => K::symbol(""),
                _ => K::nil(),
            };
            let flat_elems = vec![proto; prod as usize];
            return Ok(group_elems(&flat_elems, &dims));
        }
        let mut flat_elems = Vec::with_capacity(prod as usize);
        for i in 0..prod {
            flat_elems.push(orig_elems[(i as usize) % orig_elems.len()].clone());
        }

        Ok(group_elems(&flat_elems, &dims))
    }
}

pub fn drop_verb(a: &K, b: &K) -> Result<K, KonaError> {
    let n = a.as_int().ok_or(KonaError::Type)?;
    if !b.is_list() {
        return Ok(b.clone());
    }
    let elements = to_vec_of_atoms(b)?;
    let count = elements.len();
    if n >= 0 {
        if n as usize >= count {
            let empty_res = match &**b {
                KData::IntList(_) => K::int_list(vec![]),
                KData::FloatList(_) => K::float_list_raw(vec![]),
                KData::CharList(_) => K::char_list(vec![]),
                KData::SymbolList(_) => K::symbol_list(vec![]),
                _ => K::general_list(vec![]),
            };
            Ok(empty_res)
        } else {
            Ok(normalize_list(elements[n as usize..].to_vec()))
        }
    } else {
        let drop_len = -n as usize;
        if drop_len >= count {
            let empty_res = match &**b {
                KData::IntList(_) => K::int_list(vec![]),
                KData::FloatList(_) => K::float_list_raw(vec![]),
                KData::CharList(_) => K::char_list(vec![]),
                KData::SymbolList(_) => K::symbol_list(vec![]),
                _ => K::general_list(vec![]),
            };
            Ok(empty_res)
        } else {
            Ok(normalize_list(elements[..count - drop_len].to_vec()))
        }
    }
}

pub fn at(x: &K, y: &K) -> Result<K, KonaError> {
    match &**x {
        KData::Dictionary { keys, values } => {
            let key_vec = to_vec_of_atoms(keys)?;
            let val_vec = to_vec_of_atoms(values)?;
            if y.is_atom() {
                if let Some(pos) = key_vec.iter().position(|k| k == y) {
                    Ok(val_vec[pos].clone())
                } else {
                    Err(KonaError::Index)
                }
            } else {
                let y_vec = to_vec_of_atoms(y)?;
                let mut res = Vec::with_capacity(y_vec.len());
                for ye in y_vec {
                    if let Some(pos) = key_vec.iter().position(|k| k == &ye) {
                        res.push(val_vec[pos].clone());
                    } else {
                        return Err(KonaError::Index);
                    }
                }
                Ok(normalize_list(res))
            }
        }
        _ => {
            // flat number list indexed by ints: pick straight out of it (lanes.rs)
            if let Some(r) = crate::verbs::lanes::index(x, y)? {
                return Ok(r);
            }
            let elements = to_vec_of_atoms(x)?;
            if y.is_atom() {
                if let Some(idx) = y.as_int() {
                    let mut idx = idx;
                    if idx < 0 {
                        idx += elements.len() as i64;
                    }
                    if idx < 0 || idx >= elements.len() as i64 {
                        Err(KonaError::Index)
                    } else {
                        Ok(elements[idx as usize].clone())
                    }
                } else {
                    Err(KonaError::Type)
                }
            } else {
                let y_vec = to_vec_of_atoms(y)?;
                let mut res = Vec::with_capacity(y_vec.len());
                for ye in y_vec {
                    if let Some(idx) = ye.as_int() {
                        let mut idx = idx;
                        if idx < 0 {
                            idx += elements.len() as i64;
                        }
                        if idx < 0 || idx >= elements.len() as i64 {
                            return Err(KonaError::Index);
                        } else {
                            res.push(elements[idx as usize].clone());
                        }
                    } else {
                        return Err(KonaError::Type);
                    }
                }
                Ok(normalize_list(res))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monadic_list_ops() {
        let list = K::int_list(vec![10, 20, 30]);

        assert_eq!(first(&list).unwrap(), K::int(10));
        assert_eq!(reverse(&list).unwrap(), K::int_list(vec![30, 20, 10]));
        assert_eq!(count(&list).unwrap(), K::int(3));

        let one = K::int(1);
        assert_eq!(enlist(&one).unwrap(), K::int_list(vec![1]));

        let count_vec = K::int_list(vec![2, 0, 1]);
        assert_eq!(where_verb(&count_vec).unwrap(), K::int_list(vec![0, 0, 2]));

        let four = K::int(4);
        assert_eq!(enumerate(&four).unwrap(), K::int_list(vec![0, 1, 2, 3]));
    }

    #[test]
    fn test_grade_and_group() {
        let list = K::int_list(vec![30, 10, 20]);
        assert_eq!(grade_up(&list).unwrap(), K::int_list(vec![1, 2, 0]));
        assert_eq!(grade_down(&list).unwrap(), K::int_list(vec![0, 2, 1]));

        let list_group = K::int_list(vec![2, 3, 2]);
        let grp = group(&list_group).unwrap();
        // Result should be a list of lists: (0 2; 1)
        let val_slice = grp.as_general_list().unwrap();
        assert_eq!(val_slice[0], K::int_list(vec![0, 2]));
        assert_eq!(val_slice[1], K::int_list(vec![1]));
    }

    #[test]
    fn test_dyadic_list_ops() {
        let l1 = K::int_list(vec![1, 2]);
        let l2 = K::int_list(vec![3, 4]);

        assert_eq!(join(&l1, &l2).unwrap(), K::int_list(vec![1, 2, 3, 4]));

        let l3 = K::int_list(vec![10, 20]);
        assert_eq!(
            take(&K::int(3), &l3).unwrap(),
            K::int_list(vec![10, 20, 10])
        );
        assert_eq!(
            take(&K::int(-3), &l3).unwrap(),
            K::int_list(vec![20, 10, 20])
        );

        assert_eq!(drop_verb(&K::int(1), &l3).unwrap(), K::int_list(vec![20]));
        assert_eq!(drop_verb(&K::int(-1), &l3).unwrap(), K::int_list(vec![10]));

        assert_eq!(at(&l3, &K::int(1)).unwrap(), K::int(20));
        assert_eq!(
            at(&l3, &K::int_list(vec![1, 0])).unwrap(),
            K::int_list(vec![20, 10])
        );
    }

    #[test]
    fn test_float_grading_exactness() {
        let flist = K::float_list(vec![1.0, 1.0 + 1e-15, 0.999999999999999, f64::NAN, -0.0, 0.0]);
        let graded_up = grade_up(&flist).unwrap();
        assert_eq!(graded_up, K::int_list(vec![3, 4, 5, 2, 0, 1]));

        let graded_down = grade_down(&flist).unwrap();
        assert_eq!(graded_down, K::int_list(vec![1, 0, 2, 5, 4, 3]));
    }
}

pub fn substring_search(a: &K, b: &K) -> Result<K, KonaError> {
    let a_bytes = match &**a {
        KData::CharList(v) => v,
        _ => return Err(KonaError::Type),
    };
    let b_bytes = match &**b {
        KData::CharList(v) => v,
        _ => return Err(KonaError::Type),
    };
    if b_bytes.is_empty() {
        return Ok(K::int_list(vec![]));
    }

    let mut indices = Vec::new();
    let mut i = 0;
    while i + b_bytes.len() <= a_bytes.len() {
        if &a_bytes[i..i + b_bytes.len()] == b_bytes {
            indices.push(i as i64);
        }
        i += 1;
    }
    Ok(K::int_list(indices))
}

pub fn flip(x: &K) -> Result<K, KonaError> {
    match &**x {
        KData::GeneralList(v) => {
            if v.is_empty() {
                return Ok(x.clone());
            }
            // Check if all elements are lists and have the same length
            let first_len = match v[0].len() {
                Ok(l) => l,
                Err(_) => return Ok(x.clone()),
            };
            for item in v {
                if item.len().unwrap_or(0) != first_len {
                    return Err(KonaError::Length);
                }
            }
            // Transpose
            let mut sublists = Vec::new();
            for item in v {
                sublists.push(to_vec_of_atoms(item)?);
            }
            let mut res_list = Vec::with_capacity(first_len);
            for col in 0..first_len {
                let mut col_vec = Vec::with_capacity(v.len());
                for row in 0..v.len() {
                    col_vec.push(sublists[row][col].clone());
                }
                res_list.push(normalize_list(col_vec));
            }
            Ok(normalize_list(res_list))
        }
        _ => Ok(x.clone()),
    }
}

pub fn distinct(x: &K) -> Result<K, KonaError> {
    if x.is_atom() {
        return Ok(x.clone());
    }
    let elements = to_vec_of_atoms(x)?;
    let mut seen = Vec::new();
    let mut seen_exact = std::collections::HashSet::new();
    for e in elements {
        if seen_exact.insert(crate::types::ExactK(e.clone())) {
            seen.push(e);
        }
    }
    Ok(normalize_list(seen))
}

/// Tolerant numeric equality for K find (?), used to match C Kona behavior on
/// int/float and close-float cases (e.g. 1.9999999999999 matching 2 or 2.0 in find,
/// while group (=) and global K == remain exact-bit for floats to preserve
/// Hash/Eq invariants in HashMap<K> used by group/_hash).
/// Cross int/float and float/float use the existing float_eq (relative ~1e-13);
/// everything else falls to structural == .
fn numeric_tolerant_eq(a: &K, b: &K) -> bool {
    match (&**a, &**b) {
        (KData::IntAtom(i), KData::FloatAtom(f)) | (KData::FloatAtom(f), KData::IntAtom(i)) => {
            crate::types::float_eq(*i as f64, *f)
        }
        (KData::FloatAtom(x), KData::FloatAtom(y)) => crate::types::float_eq(*x, *y),
        _ => a == b,
    }
}

pub fn find(a: &K, b: &K) -> Result<K, KonaError> {
    if let KData::GeneralList(v) = &**a {
        if v.len() == 2 {
            if let KData::HashList { map, .. } = &*v[1] {
                let x = &v[0];
                let x_len = x.len()?;

                let lookup = |key: &K| -> i64 {
                    if let Some(&pos) = map.0.get(&crate::types::ExactK(key.clone())) {
                        pos
                    } else {
                        x_len as i64
                    }
                };

                if b.is_atom() {
                    return Ok(K::int(lookup(b)));
                } else {
                    let b_vec = to_vec_of_atoms(b)?;
                    let mut res = Vec::with_capacity(b_vec.len());
                    for be in b_vec {
                        res.push(K::int(lookup(&be)));
                    }
                    return Ok(normalize_list(res));
                }
            }
        }
    }

    let a_vec = to_vec_of_atoms(a)?;
    if b.is_atom() {
        if let Some(pos) = a_vec.iter().position(|x| numeric_tolerant_eq(x, b)) {
            Ok(K::int(pos as i64))
        } else {
            Ok(K::int(a_vec.len() as i64))
        }
    } else {
        let b_vec = to_vec_of_atoms(b)?;
        let mut res = Vec::with_capacity(b_vec.len());
        for be in b_vec {
            if let Some(pos) = a_vec.iter().position(|x| numeric_tolerant_eq(x, &be)) {
                res.push(K::int(pos as i64));
            } else {
                res.push(K::int(a_vec.len() as i64));
            }
        }
        Ok(normalize_list(res))
    }
}

pub fn at_deep(x: &K, args: &[K]) -> Result<K, KonaError> {
    if args.is_empty() {
        return Ok(x.clone());
    }
    let first = &args[0];
    let rest = &args[1..];

    if first.is_nil() {
        match &**x {
            KData::Dictionary { values, .. } => {
                let val_vec = to_vec_of_atoms(values)?;
                let mut res = Vec::with_capacity(val_vec.len());
                for val in val_vec {
                    res.push(at_deep(&val, rest)?);
                }
                Ok(normalize_list(res))
            }
            _ => {
                if x.is_list() {
                    let elements = to_vec_of_atoms(x)?;
                    let mut res = Vec::with_capacity(elements.len());
                    for el in elements {
                        res.push(at_deep(&el, rest)?);
                    }
                    Ok(normalize_list(res))
                } else {
                    at_deep(x, rest)
                }
            }
        }
    } else if first.is_list() {
        let indices = to_vec_of_atoms(first)?;
        let mut res = Vec::with_capacity(indices.len());
        for idx in indices {
            let el = at(x, &idx)?;
            res.push(at_deep(&el, rest)?);
        }
        Ok(normalize_list(res))
    } else {
        let el = at(x, first)?;
        at_deep(&el, rest)
    }
}

pub fn deep_index(a: &K, b: &K) -> Result<K, KonaError> {
    if b.is_atom() {
        at_deep(a, &[b.clone()])
    } else {
        let indices = to_vec_of_atoms(b)?;
        at_deep(a, &indices)
    }
}

pub fn rotate(a: &K, b: &K) -> Result<K, KonaError> {
    let n = a.as_int().ok_or(KonaError::Type)?;
    let elements = to_vec_of_atoms(b)?;
    if elements.is_empty() {
        return Ok(b.clone());
    }
    let len = elements.len() as i64;
    let shift = ((n % len) + len) % len;
    if shift == 0 {
        return Ok(b.clone());
    }
    let mut res = Vec::with_capacity(elements.len());
    res.extend_from_slice(&elements[shift as usize..]);
    res.extend_from_slice(&elements[..shift as usize]);
    Ok(normalize_list(res))
}

pub fn cut(a: &K, b: &K) -> Result<K, KonaError> {
    let indices = match &**a {
        KData::IntAtom(i) => vec![*i],
        KData::IntList(v) => v.clone(),
        _ => return Err(KonaError::Type),
    };
    let elements = to_vec_of_atoms(b)?;
    let mut res = Vec::new();
    let len = elements.len() as i64;

    for i in 0..indices.len() {
        let start = indices[i];
        if start < 0 {
            return Err(KonaError::Domain);
        }
        if start > len {
            return Err(KonaError::Length);
        }
        let end = if i + 1 < indices.len() {
            let next = indices[i + 1];
            if next < start {
                return Err(KonaError::Domain);
            }
            if next > len {
                return Err(KonaError::Length);
            }
            next
        } else {
            len
        };
        res.push(normalize_list(
            elements[start as usize..end as usize].to_vec(),
        ));
    }
    Ok(K::general_list(res))
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let p_chars: Vec<char> = pattern.chars().collect();
    let t_chars: Vec<char> = text.chars().collect();

    fn match_helper(p: &[char], t: &[char]) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }

        if p[0] == '*' {
            if match_helper(&p[1..], t) {
                return true;
            }
            if !t.is_empty() && match_helper(p, &t[1..]) {
                return true;
            }
            return false;
        }

        if t.is_empty() {
            return false;
        }

        if p[0] == '?' {
            return match_helper(&p[1..], &t[1..]);
        }

        if p[0] == '[' {
            if let Some(close_idx) = p.iter().position(|&c| c == ']') {
                let inner = &p[1..close_idx];
                let rest_p = &p[close_idx + 1..];

                if inner.is_empty() {
                    return false;
                }

                let negate = inner[0] == '^' || inner[0] == '!';
                let char_class = if negate { &inner[1..] } else { inner };

                let mut matched = false;
                let char_to_match = t[0];

                let mut idx = 0;
                while idx < char_class.len() {
                    if idx + 2 < char_class.len() && char_class[idx + 1] == '-' {
                        let start = char_class[idx];
                        let end = char_class[idx + 2];
                        if char_to_match >= start && char_to_match <= end {
                            matched = true;
                            break;
                        }
                        idx += 3;
                    } else {
                        if char_class[idx] == char_to_match {
                            matched = true;
                            break;
                        }
                        idx += 1;
                    }
                }

                let actual_match = if negate { !matched } else { matched };
                if actual_match {
                    return match_helper(rest_p, &t[1..]);
                } else {
                    return false;
                }
            }
        }

        p[0] == t[0] && match_helper(&p[1..], &t[1..])
    }

    match_helper(&p_chars, &t_chars)
}

fn k_to_str(k: &K) -> Result<String, KonaError> {
    match &**k {
        KData::CharAtom(c) => Ok(c.to_string()),
        KData::CharList(v) => std::str::from_utf8(v)
            .map(|s| s.to_string())
            .map_err(|_| KonaError::Type),
        KData::SymbolAtom(s) => Ok(s.to_string()),
        _ => Err(KonaError::Type),
    }
}

pub fn sm(a: &K, b: &K) -> Result<K, KonaError> {
    let a_is_list = a.is_list() && !matches!(**a, KData::CharList(_));
    let b_is_list = b.is_list() && !matches!(**b, KData::CharList(_));
    if a_is_list || b_is_list {
        if a_is_list && b_is_list {
            let a_len = a.len()?;
            let b_len = b.len()?;
            if a_len != b_len {
                return Err(KonaError::Length);
            }
            let a_vec = to_vec_of_atoms(a)?;
            let b_vec = to_vec_of_atoms(b)?;
            let mut res = Vec::with_capacity(a_len);
            for i in 0..a_len {
                res.push(sm(&a_vec[i], &b_vec[i])?);
            }
            Ok(normalize_list(res))
        } else if a_is_list {
            let a_len = a.len()?;
            let a_vec = to_vec_of_atoms(a)?;
            let mut res = Vec::with_capacity(a_len);
            for item in a_vec {
                res.push(sm(&item, b)?);
            }
            Ok(normalize_list(res))
        } else {
            let b_len = b.len()?;
            let b_vec = to_vec_of_atoms(b)?;
            let mut res = Vec::with_capacity(b_len);
            for item in b_vec {
                res.push(sm(a, &item)?);
            }
            Ok(normalize_list(res))
        }
    } else {
        let text = k_to_str(a)?;
        let pattern = k_to_str(b)?;
        let matched = glob_match(&pattern, &text);
        Ok(K::int(if matched { 1 } else { 0 }))
    }
}

pub fn di(x: &K, y: &K) -> Result<K, KonaError> {
    match &**x {
        KData::Dictionary { keys, values } => {
            let to_delete = to_vec_of_atoms(y)?;
            let keys_vec = to_vec_of_atoms(keys)?;
            let values_vec = to_vec_of_atoms(values)?;

            let mut keep = vec![true; keys_vec.len()];
            for key in &to_delete {
                for (i, k) in keys_vec.iter().enumerate() {
                    if k == key {
                        keep[i] = false;
                    }
                }
            }

            let mut new_keys = Vec::new();
            let mut new_values = Vec::new();
            for i in 0..keys_vec.len() {
                if keep[i] {
                    new_keys.push(keys_vec[i].clone());
                    new_values.push(values_vec[i].clone());
                }
            }
            Ok(K::dictionary(
                normalize_list(new_keys),
                normalize_list(new_values),
            ))
        }
        _ => {
            let idxs_vec = to_vec_of_atoms(y)?;
            let mut to_delete = Vec::new();
            for idx in idxs_vec {
                if let Some(i) = idx.as_int() {
                    to_delete.push(i);
                } else {
                    return Err(KonaError::Type);
                }
            }

            let x_len = x.len()?;
            let mut keep = vec![true; x_len];
            for idx in to_delete {
                if idx >= 0 && idx < x_len as i64 {
                    keep[idx as usize] = false;
                }
            }

            let x_vec = to_vec_of_atoms(x)?;
            let mut res = Vec::new();
            for i in 0..x_len {
                if keep[i] {
                    res.push(x_vec[i].clone());
                }
            }
            Ok(normalize_list(res))
        }
    }
}

pub fn dvl(x: &K, y: &K) -> Result<K, KonaError> {
    let x_atoms = to_vec_of_atoms(x)?;
    let y_atoms = to_vec_of_atoms(y)?;
    let y_set: std::collections::HashSet<K> = y_atoms.into_iter().collect();

    let mut res = Vec::new();
    for xi in x_atoms {
        if !y_set.contains(&xi) {
            res.push(xi);
        }
    }
    Ok(normalize_list(res))
}

pub fn dv(x: &K, y: &K) -> Result<K, KonaError> {
    let enlisted_y = K::general_list(vec![y.clone()]);
    dvl(x, &enlisted_y)
}

pub fn hat(x: &K, y: &K) -> Result<K, KonaError> {
    match &**x {
        KData::Dictionary { .. } => di(x, y),
        _ => {
            if y.is_atom() {
                let enlisted_y = K::general_list(vec![y.clone()]);
                dvl(x, &enlisted_y)
            } else {
                dvl(x, y)
            }
        }
    }
}

pub fn lin(x: &K, y: &K) -> Result<K, KonaError> {
    let x_atoms = to_vec_of_atoms(x)?;
    let mut res = Vec::with_capacity(x_atoms.len());

    if y.is_atom() {
        for xe in x_atoms {
            let found = &xe == y;
            res.push(K::int(if found { 1 } else { 0 }));
        }
    } else {
        let y_elements = match &**y {
            KData::IntList(v) => v.iter().map(|&val| K::int(val)).collect::<Vec<_>>(),
            KData::FloatList(v) => v.iter().map(|&val| K::float(val)).collect::<Vec<_>>(),
            KData::CharList(v) => v
                .iter()
                .map(|&val| K::char(val as char))
                .collect::<Vec<_>>(),
            KData::SymbolList(v) => v
                .iter()
                .map(|val| K::symbol(val.clone()))
                .collect::<Vec<_>>(),
            KData::GeneralList(v) => v.clone(),
            _ => to_vec_of_atoms(y)?,
        };
        let y_set: std::collections::HashSet<K> = y_elements.into_iter().collect();
        for xe in x_atoms {
            let found = y_set.contains(&xe);
            res.push(K::int(if found { 1 } else { 0 }));
        }
    }
    Ok(normalize_list(res))
}

pub fn hash_verb(x: &K) -> Result<K, KonaError> {
    if !x.is_list() {
        return Err(KonaError::Type);
    }
    let atoms = to_vec_of_atoms(x)?;
    let mut map = std::collections::HashMap::new();
    let mut indices = Vec::with_capacity(atoms.len());
    for (i, item) in atoms.iter().enumerate() {
        let exact_item = crate::types::ExactK(item.clone());
        if !map.contains_key(&exact_item) {
            map.insert(exact_item, i as i64);
        }
        indices.push(i as i64);
    }
    Ok(K::hash_list(indices, map))
}
