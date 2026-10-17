#![forbid(unsafe_code)]

pub mod dict;
pub mod lanes;
pub mod list;
pub mod math;

use crate::errors::KonaError;
use crate::types::{K, KData};

pub fn dispatch_monadic(
    verb: &str,
    x: &K,
    ktree: &std::sync::Arc<crate::verbs::dict::KTree>,
) -> Result<K, KonaError> {
    match verb {
        "+" => list::flip(x),
        "-" => math::negate(x),
        "%" => math::reciprocal(x),
        "~" => math::not(x),
        "*" => list::first(x),
        "|" => list::reverse(x),
        "," => list::enlist(x),
        "#" => list::count(x),
        "&" => list::where_verb(x),
        "!" => list::enumerate(x),
        "?" => list::distinct(x),
        "<" => list::grade_up(x),
        ">" => list::grade_down(x),
        "=" => list::group(x),
        "0:" => crate::io::read_text_file(x),
        "1:" => crate::io::read_binary_file(x),
        "2:" => crate::io::read_binary_file(x),
        "3:" => crate::io::open_or_close_socket(x),
        "4:" => math::type_integer(x),
        "5:" => math::string_verb(x),
        "6:" => crate::io::read_raw_file(x),
        "@" => math::atom_verb(x),
        "$" => math::format_verb(x),
        "." => eval_verb(x, ktree),
        "_" => math::floor_verb(x),
        "^" => math::shape_verb(x),
        "_abs" => math::abs_verb(x),
        "_floor" => math::floor_intolerant_verb(x),
        "_ceil" => math::ceil_verb(x),
        "_exp" => math::exp_verb(x),
        "_log" => math::log_verb(x),
        "_sqr" => math::sqr_verb(x),
        "_sin" => math::sin_verb(x),
        "_cos" => math::cos_verb(x),
        "_tan" => math::tan_verb(x),
        "_asin" => math::asin_verb(x),
        "_acos" => math::acos_verb(x),
        "_atan" => math::atan_verb(x),
        "_sinh" => math::sinh_verb(x),
        "_cosh" => math::cosh_verb(x),
        "_tanh" => math::tanh_verb(x),
        "_ci" => math::char_from_int(x),
        "_ic" => math::int_from_char(x),
        "_sqrt" => math::sqrt_verb(x),
        "_val" => math::val_verb(x, ktree),
        "_size" => math::size_verb(x),
        "_gup" => crate::gpu::up(x),
        "_gdown" => crate::gpu::down(x),
        "_gk" => crate::gpu::kernel(x),
        "_grun" => crate::gpu::run(x),
        "_gfree" => crate::gpu::free(x),
        "_sys" => crate::door::sys(x),
        "_getenv" => math::getenv_verb(x),
        "_exit" => math::exit_verb(x),
        "_inv" => math::inv_verb(x),
        "_jd" => math::jd_verb(x),
        "_dj" => math::dj_verb(x),
        "_gtime" => math::gtime_verb(x),
        "_ltime" => math::ltime_verb(x),
        "_lt" => math::lt_verb(x),
        "_hash" => list::hash_verb(x),
        "_bd" => math::bd_verb(x),
        "_db" => math::db_verb(x),
        "_ceiling" => math::ceil_verb(x),
        "_host" => math::host_verb(x),
        "\\" => {
            println!("{}", crate::io::format_k(x));
            Ok(x.clone())
        }
        "_conj" | "_real" | "_imag" | "_trans" | "_solve" | "_div" => Err(
            KonaError::FeatureDisabled(format!("{} is not implemented in K3.2", verb)),
        ),
        _ => Err(KonaError::Parse(format!("Unknown monadic verb: {}", verb))),
    }
}

pub fn eval_verb(x: &K, ktree: &std::sync::Arc<crate::verbs::dict::KTree>) -> Result<K, KonaError> {
    match &**x {
        KData::CharList(v) => {
            let code = std::str::from_utf8(v).map_err(|_| KonaError::Type)?;
            if code.starts_with('\\') {
                crate::handle_system_command(code, ktree, &mut std::collections::HashMap::new());
                return Ok(K::int(1));
            }
            let words = crate::parser::parse(code)?;
            let mut env = std::collections::HashMap::new();
            crate::eval::eval_words(&words, &mut env, ktree)
        }
        KData::CharAtom(c) => {
            let mut buf = [0; 4];
            let code = c.encode_utf8(&mut buf);
            let words = crate::parser::parse(code)?;
            let mut env = std::collections::HashMap::new();
            crate::eval::eval_words(&words, &mut env, ktree)
        }
        KData::SymbolAtom(s) => {
            if let Some(val) = ktree.lookup(s) {
                Ok(val)
            } else {
                Err(KonaError::Value(format!("Undefined variable: {}", s)))
            }
        }
        KData::GeneralList(v) => {
            let mut keys = Vec::with_capacity(v.len());
            let mut values = Vec::with_capacity(v.len());
            for pair in v {
                let pair_vec = math::to_vec_of_atoms(pair)?;
                if pair_vec.len() < 2 {
                    return Err(KonaError::Type);
                }
                keys.push(pair_vec[0].clone());
                values.push(pair_vec[1].clone());
            }
            dict::create_dict(&math::normalize_list(keys), &math::normalize_list(values))
        }
        KData::Dictionary { keys, values } => {
            let keys_vec = math::to_vec_of_atoms(keys)?;
            let values_vec = math::to_vec_of_atoms(values)?;
            let mut pairs = Vec::with_capacity(keys_vec.len());
            for (k, v) in keys_vec.iter().zip(values_vec.iter()) {
                pairs.push(K::general_list(vec![k.clone(), v.clone()]));
            }
            Ok(K::general_list(pairs))
        }
        _ => Err(KonaError::Type),
    }
}

/// n $ text as C Kona formats it: right-justified in n columns for n>0, left-justified for n<0,
/// all stars when the text does not fit (3 $ "hello" is "***")
fn pad_text(n: i64, text: &[u8]) -> K {
    let width = n.unsigned_abs() as usize;
    if text.len() > width {
        return K::char_list(vec![b'*'; width]);
    }
    let pad = vec![b' '; width - text.len()];
    let mut out = Vec::with_capacity(width);
    if n > 0 {
        out.extend_from_slice(&pad);
        out.extend_from_slice(text);
    } else {
        out.extend_from_slice(text);
        out.extend_from_slice(&pad);
    }
    K::char_list(out)
}

pub fn cast_verb(a: &K, b: &K) -> Result<K, KonaError> {
    match &**a {
        KData::SymbolAtom(sym) => match sym.as_ref() {
            "int" | "i" => match &**b {
                KData::CharList(v) => {
                    let s = std::str::from_utf8(v).map_err(|_| KonaError::Type)?;
                    let i = s.trim().parse::<i64>().map_err(|_| KonaError::Type)?;
                    Ok(K::int(i))
                }
                KData::CharAtom(c) => Ok(K::int(*c as i64)),
                KData::IntAtom(_) => Ok(b.clone()),
                KData::FloatAtom(f) => Ok(K::int(*f as i64)),
                KData::IntList(_) => Ok(b.clone()),
                KData::FloatList(v) => {
                    let ints = v.iter().map(|&f| f as i64).collect();
                    Ok(K::int_list(ints))
                }
                _ => Err(KonaError::Type),
            },
            "float" | "f" => match &**b {
                KData::CharList(v) => {
                    let s = std::str::from_utf8(v).map_err(|_| KonaError::Type)?;
                    let f = s.trim().parse::<f64>().map_err(|_| KonaError::Type)?;
                    Ok(K::float(f))
                }
                KData::CharAtom(_) => Ok(K::float(f64::NAN)),
                KData::IntAtom(i) => Ok(K::float(*i as f64)),
                KData::FloatAtom(_) => Ok(b.clone()),
                KData::IntList(v) => {
                    let floats = v.iter().map(|&i| i as f64).collect();
                    Ok(K::float_list(floats))
                }
                KData::FloatList(_) => Ok(b.clone()),
                _ => Err(KonaError::Type),
            },
            "char" | "c" => match &**b {
                KData::CharAtom(_) => Ok(b.clone()),
                KData::IntAtom(i) => Ok(K::char((*i as u8) as char)),
                KData::IntList(v) => {
                    let bytes = v.iter().map(|&i| i as u8).collect();
                    Ok(K::char_list(bytes))
                }
                _ => Err(KonaError::Type),
            },
            "symbol" | "s" | "" => match &**b {
                KData::CharList(v) => {
                    let s = std::str::from_utf8(v).map_err(|_| KonaError::Type)?;
                    Ok(K::symbol(s))
                }
                KData::CharAtom(c) => Ok(K::symbol(c.to_string())),
                KData::SymbolAtom(_) => Ok(b.clone()),
                _ => Err(KonaError::Type),
            },
            _ => Err(KonaError::Type),
        },
        KData::FloatAtom(_) => match &**b {
            KData::CharList(v) => {
                let s = std::str::from_utf8(v).map_err(|_| KonaError::Type)?;
                let f = s.trim().parse::<f64>().map_err(|_| KonaError::Type)?;
                Ok(K::float(f))
            }
            KData::CharAtom(_) => Ok(K::float(f64::NAN)),
            KData::IntAtom(i) => Ok(K::float(*i as f64)),
            KData::FloatAtom(_) => Ok(b.clone()),
            KData::IntList(v) => {
                let floats = v.iter().map(|&i| i as f64).collect();
                Ok(K::float_list(floats))
            }
            KData::FloatList(_) => Ok(b.clone()),
            _ => Err(KonaError::Type),
        },
        KData::IntAtom(n) => {
            if *n == i64::MIN {
                return Err(KonaError::Domain);
            }
            if *n == 0 {
                match &**b {
                    KData::CharList(v) => {
                        let s = std::str::from_utf8(v).map_err(|_| KonaError::Type)?;
                        let i = s.trim().parse::<i64>().unwrap_or(i64::MIN);
                        Ok(K::int(i))
                    }
                    KData::CharAtom(c) => {
                        let val = c.to_digit(10).map(|d| d as i64).unwrap_or(i64::MIN);
                        Ok(K::int(val))
                    }
                    KData::GeneralList(v) => {
                        let mut res = Vec::with_capacity(v.len());
                        for x in v {
                            res.push(cast_verb(a, x)?);
                        }
                        Ok(K::general_list(res))
                    }
                    _ => Err(KonaError::Type),
                }
            } else {
                match &**b {
                    // n $ x pads x's text to n columns (C Kona): right-justified for n>0, left for
                    // n<0, all stars when it does not fit. Done on bytes: slicing a String at a byte
                    // index panicked on "é", and K strings are bytes anyway.
                    KData::CharList(v) => Ok(pad_text(*n, v)),
                    KData::IntAtom(_) | KData::FloatAtom(_) | KData::SymbolAtom(_) => {
                        let text = math::format_verb(b)?;
                        let bytes = text.as_char_list().unwrap_or(&[]).to_vec();
                        Ok(pad_text(*n, &bytes))
                    }
                    KData::CharAtom(c) => {
                        let mut res_str = c.to_string();
                        let width = n.abs() as usize;
                        if width > 1 {
                            let padding = " ".repeat(width - 1);
                            if *n > 0 {
                                res_str.push_str(&padding);
                            } else {
                                res_str.insert_str(0, &padding);
                            }
                        } else if width == 0 {
                            res_str.clear();
                        }
                        Ok(K::char_list(res_str.into_bytes()))
                    }
                    _ => Err(KonaError::Type),
                }
            }
        }
        _ => Err(KonaError::Type),
    }
}

pub fn ssr_verb(a: &K, b: &K) -> Result<K, KonaError> {
    let s = match &**a {
        KData::CharList(v) => std::str::from_utf8(v).map_err(|_| KonaError::Type)?,
        _ => return Err(KonaError::Type),
    };
    let pair = match &**b {
        KData::GeneralList(v) => v,
        _ => return Err(KonaError::Type),
    };
    if pair.len() != 2 {
        return Err(KonaError::Length);
    }
    let pat = match &*pair[0] {
        KData::CharList(v) => std::str::from_utf8(v).map_err(|_| KonaError::Type)?,
        _ => return Err(KonaError::Type),
    };
    let rep = match &*pair[1] {
        KData::CharList(v) => std::str::from_utf8(v).map_err(|_| KonaError::Type)?,
        _ => return Err(KonaError::Type),
    };
    let res = s.replace(pat, rep);
    Ok(K::char_list(res.into_bytes()))
}

pub fn ssr_verb_triadic(a: &K, b: &K, c: &K) -> Result<K, KonaError> {
    let s = match &**a {
        KData::CharList(v) => std::str::from_utf8(v).map_err(|_| KonaError::Type)?,
        _ => return Err(KonaError::Type),
    };
    let pat = match &**b {
        KData::CharList(v) => std::str::from_utf8(v).map_err(|_| KonaError::Type)?,
        _ => return Err(KonaError::Type),
    };
    let rep = match &**c {
        KData::CharList(v) => std::str::from_utf8(v).map_err(|_| KonaError::Type)?,
        _ => return Err(KonaError::Type),
    };
    let res = s.replace(pat, rep);
    Ok(K::char_list(res.into_bytes()))
}

pub fn dispatch_dyadic(verb: &str, a: &K, b: &K) -> Result<K, KonaError> {
    match verb {
        "+" | "-" | "*" | "%" | "&" | "|" | "<" | ">" | "=" | "^" => math::eval_dyadic(a, b, verb),
        "," => list::join(a, b),
        "#" => list::take(a, b),
        "_" => {
            if a.is_list() && matches!(**a, KData::IntList(_)) {
                list::cut(a, b)
            } else {
                list::drop_verb(a, b)
            }
        }
        "@" => list::at(a, b),
        "!" => {
            fn is_symbol_like(k: &K) -> bool {
                match &**k {
                    KData::SymbolAtom(_) | KData::SymbolList(_) => true,
                    KData::GeneralList(v) => !v.is_empty() && v.iter().all(|x| is_symbol_like(x)),
                    _ => false,
                }
            }
            if is_symbol_like(a) {
                dict::create_dict(a, b)
            } else if b.is_list() {
                list::rotate(a, b)
            } else {
                math::eval_dyadic(a, b, "!")
            }
        }
        "~" => Ok(K::int(if crate::types::match_eq(a, b) { 1 } else { 0 })),
        "?" => {
            if matches!(**a, KData::IntAtom(_)) {
                math::draw_verb(a, b)
            } else {
                list::find(a, b)
            }
        }

        "." => list::deep_index(a, b),
        "$" => cast_verb(a, b),
        "0:" => crate::io::write_text_file(a, b),
        "1:" => crate::io::write_binary_file(a, b),
        "2:" => Err(KonaError::FeatureDisabled(
            "Dynamic C library loading is disabled for safety".to_string(),
        )),
        "3:" => crate::io::send_async(a, b),
        "4:" => crate::io::send_sync(a, b),
        "5:" => crate::io::append_binary_file(a, b),
        "6:" => crate::io::write_raw_file(a, b),
        "_draw" => math::draw_verb(a, b),
        "_ss" => list::substring_search(a, b),
        "_ssr" => ssr_verb(a, b),
        "_bin" => math::bin_search(a, b),
        "_in" => math::in_verb(a, b),
        "_vs" => math::vs_verb(a, b),
        "_vsx" => math::vsx_verb(a, b),
        "_sv" => math::sv_verb(a, b),
        "_sm" => list::sm(a, b),
        "_di" => list::di(a, b),
        "_dv" => list::dv(a, b),
        "_dvl" => list::dvl(a, b),
        "_hat" => list::hat(a, b),
        "_lin" => list::lin(a, b),
        "_binl" => math::bin_search(a, b),
        "_dot" => math::dot_verb(a, b),
        "_mul" => math::mul_verb(a, b),
        "_lsq" => math::lsq_verb(a, b),
        "_setenv" => math::setenv_verb(a, b),
        "_conj" | "_real" | "_imag" | "_trans" | "_solve" | "_div" => Err(
            KonaError::FeatureDisabled(format!("{} is not implemented in K3.2", verb)),
        ),
        _ => Err(KonaError::Parse(format!("Unknown dyadic verb: {}", verb))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch() {
        let ktree = std::sync::Arc::new(crate::verbs::dict::KTree::new());
        let x = K::int(5);
        let y = K::int(3);
        assert_eq!(dispatch_monadic("-", &x, &ktree).unwrap(), K::int(-5));
        assert_eq!(dispatch_dyadic("+", &x, &y).unwrap(), K::int(8));
        assert_eq!(dispatch_dyadic("*", &x, &y).unwrap(), K::int(15));

        // Test non-implemented verbs
        assert!(matches!(
            dispatch_monadic("_conj", &x, &ktree),
            Err(KonaError::FeatureDisabled(_))
        ));
        assert!(matches!(
            dispatch_dyadic("_div", &x, &y),
            Err(KonaError::FeatureDisabled(_))
        ));
    }
}
