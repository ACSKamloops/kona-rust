#![forbid(unsafe_code)]

use crate::errors::KonaError;
use std::cell::Cell;
use std::sync::{Arc, OnceLock};

/// Immutable call setup shared by every invocation (and clone) of a user function.
/// Captured values still live in the function's environment and are cloned per call.
pub struct FunctionMetadata {
    definition: FunctionDefinitionKey,
    pub(crate) code: Arc<Vec<crate::parser::Word>>,
    pub(crate) locals: Arc<std::collections::HashSet<String>>,
    pub(crate) initialized_locals: Vec<String>,
    pub(crate) parameter_bindings: Vec<usize>,
}

/// A cache key for the still-public, editable embedding definition fields.
/// Parsed code is authoritative, as it was before metadata caching. Lazy bodies
/// instead compare their text. An Arc clone holds the code allocation alive so
/// that an unrelated allocation cannot reuse its identity.
pub(crate) struct FunctionDefinitionKey {
    params: Vec<String>,
    code: Option<Arc<Vec<crate::parser::Word>>>,
    lazy_body: Option<String>,
}

impl FunctionDefinitionKey {
    pub(crate) fn new(body: &str, params: &[String], code: &Option<Arc<Vec<crate::parser::Word>>>) -> Self {
        Self { params: params.to_vec(), code: code.clone(),
            lazy_body: code.is_none().then(|| body.to_owned()) }
    }

    pub(crate) fn matches(&self, body: &str, params: &[String], code: &Option<Arc<Vec<crate::parser::Word>>>) -> bool {
        self.params == params && match (&self.code, code) {
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            (None, None) => self.lazy_body.as_deref() == Some(body),
            _ => false,
        }
    }
}

impl FunctionMetadata {
    pub(crate) fn new(code: Arc<Vec<crate::parser::Word>>, params: &[String], body: &str,
        definition_code: &Option<Arc<Vec<crate::parser::Word>>>) -> Self {
        use crate::parser::Word;
        use std::collections::HashSet;
        fn traverse(words: &[Word], locals: &mut HashSet<String>) {
            for word in words {
                match word {
                    Word::LocalAssign(name) => { locals.insert(name.clone()); }
                    Word::Paren(exprs) | Word::Bracket(exprs) => {
                        for expr in exprs { traverse(expr, locals); }
                    }
                    // A nested brace function has its own local scope.
                    _ => {}
                }
            }
        }
        let mut locals = HashSet::new();
        traverse(&code, &mut locals);
        let mut bound = HashSet::new();
        // K binds a repeated parameter name to its FIRST argument.
        let parameter_bindings = params.iter().enumerate()
            .filter_map(|(i, name)| bound.insert(name).then_some(i)).collect();
        let initialized_locals = locals.iter()
            .filter(|name| !bound.contains(name)).cloned().collect();
        Self { definition: FunctionDefinitionKey::new(body, params, definition_code),
            code, locals: Arc::new(locals), initialized_locals, parameter_bindings }
    }

    pub(crate) fn matches_definition(&self, body: &str, params: &[String], code: &Option<Arc<Vec<crate::parser::Word>>>) -> bool {
        self.definition.matches(body, params, code)
    }
}

thread_local! {
    static RECURSION_DEPTH: Cell<usize> = Cell::new(0);
}

const MAX_DEPTH: usize = 200;

struct DepthGuard;

impl DepthGuard {
    fn new() -> Option<Self> {
        RECURSION_DEPTH.with(|d| {
            let current = d.get();
            if current >= MAX_DEPTH {
                None
            } else {
                d.set(current + 1);
                Some(DepthGuard)
            }
        })
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        RECURSION_DEPTH.with(|d| d.set(d.get() - 1));
    }
}

/// KFunc represents the different kinds of K functions:
/// - Primitive verbs (e.g., +, -)
/// - Adverbs (e.g., /, \) applied to operands
/// - Projections (partially applied functions)
/// - User-defined brace functions (e.g., {x+y})
pub enum KFunc {
    Primitive(String),
    Adverb {
        name: String,
        operand: Option<K>,
    },
    Projection {
        func: K,
        args: Vec<Option<K>>,
    },
    UserDefined {
        body: String,
        args: Vec<String>,
        code: Option<std::sync::Arc<Vec<crate::parser::Word>>>,
        env: Option<std::collections::HashMap<String, K>>,
        /// Parsed functions fill this at construction. Deserialized/API functions
        /// fill it on the first fully applied call, preserving lazy parse errors.
        metadata: Arc<OnceLock<FunctionMetadata>>,
    },
    Composition {
        f: K,
        g: K,
    },
}

impl PartialEq for KFunc {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (KFunc::Primitive(s1), KFunc::Primitive(s2)) => s1 == s2,
            (
                KFunc::Adverb {
                    name: n1,
                    operand: o1,
                },
                KFunc::Adverb {
                    name: n2,
                    operand: o2,
                },
            ) => n1 == n2 && o1 == o2,
            (
                KFunc::Projection { func: f1, args: a1 },
                KFunc::Projection { func: f2, args: a2 },
            ) => {
                if let Some(_g) = DepthGuard::new() {
                    f1 == f2 && a1 == a2
                } else {
                    false
                }
            }
            (
                KFunc::UserDefined {
                    body: b1,
                    args: ar1,
                    ..
                },
                KFunc::UserDefined {
                    body: b2,
                    args: ar2,
                    ..
                },
            ) => b1 == b2 && ar1 == ar2,
            (KFunc::Composition { f: f1, g: g1 }, KFunc::Composition { f: f2, g: g2 }) => {
                if let Some(_g) = DepthGuard::new() {
                    f1 == f2 && g1 == g2
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

impl KFunc {
    pub fn primitive<S: Into<String>>(s: S) -> Self {
        KFunc::Primitive(s.into())
    }

    pub fn adverb<S: Into<String>>(name: S, operand: Option<K>) -> Self {
        KFunc::Adverb {
            name: name.into(),
            operand,
        }
    }

    pub fn projection(func: K, args: Vec<Option<K>>) -> Self {
        KFunc::Projection { func, args }
    }

    pub fn user_defined<S: Into<String>>(body: S, args: Vec<String>) -> Self {
        KFunc::UserDefined {
            body: body.into(),
            args,
            code: None,
            env: None,
            metadata: Arc::new(OnceLock::new()),
        }
    }

    pub(crate) fn parsed_user_defined(
        body: String,
        args: Vec<String>,
        code: Vec<crate::parser::Word>,
        env: Option<std::collections::HashMap<String, K>>,
    ) -> Self {
        let code = Arc::new(code);
        let metadata = Arc::new(OnceLock::from(FunctionMetadata::new(code.clone(), &args, &body, &Some(code.clone()))));
        Self::UserDefined { body, args, code: Some(code), env, metadata }
    }

    pub fn composition(f: K, g: K) -> Self {
        KFunc::Composition { f, g }
    }
}

pub fn is_inplace_assign_name(name: &str) -> bool {
    name.ends_with(':')
        && name.len() > 1
        && name != "::"
        && !name.chars().all(|c| c == ':')
        && !matches!(name, "0:" | "1:" | "2:" | "3:" | "4:" | "5:" | "6:")
}

impl KFunc {
    pub fn is_composable(&self) -> bool {
        match self {
            KFunc::Primitive(name) => {
                if name == ":" {
                    false
                } else if name.ends_with(':') {
                    name == "::" || !name.chars().next().map_or(false, |c| c.is_ascii_digit())
                } else {
                    true
                }
            }
            KFunc::Projection { func, .. } => func.is_composable_func(),
            KFunc::Composition { f, g } => f.is_composable_func() && g.is_composable_func(),
            KFunc::Adverb { operand, .. } => {
                operand.as_ref().map_or(true, |op| op.is_composable_func())
            }
            KFunc::UserDefined { .. } => true,
        }
    }

    pub fn valence(&self) -> usize {
        match self {
            KFunc::Primitive(name) => {
                if name == "." || name == "@" {
                    4
                } else if name == "_ssr" {
                    3
                } else if is_inplace_assign_name(name) {
                    1
                } else if name.starts_with('_') {
                    match name.as_str() {
                        "_ssr" => 3,
                        "_bin" | "_draw" | "_in" | "_sv" | "_vs" | "_vsx" | "_di" | "_lin"
                        | "_lsq" | "_mul" | "_ss" | "_sm" | "_dv" | "_dvl" | "_hat" | "_binl"
                        | "_dot" | "_setenv" => 2,
                        _ => 1,
                    }
                } else {
                    2
                }
            }
            KFunc::Adverb { operand, name } => {
                if let Some(op) = operand {
                    if name == "'" {
                        if let KData::Function(kfunc) = &**op {
                            if let KFunc::Adverb { name: op_name, .. } = kfunc {
                                if op_name == "/:" || op_name == "\\:" {
                                    return 1;
                                }
                            }
                        }
                    }
                    if name == "/" || name == "\\" || name == "':" || name == "/:" || name == "\\:"
                    {
                        std::cmp::max(2, op.valence())
                    } else {
                        op.valence()
                    }
                } else {
                    2
                }
            }
            KFunc::Projection { func, args } => {
                let base = func.valence();
                let bound = args.iter().filter(|a| a.is_some()).count();
                if base > bound { base - bound } else { 0 }
            }
            KFunc::UserDefined { args, .. } => std::cmp::max(1, args.len()),
            KFunc::Composition { g, .. } => g.valence(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExactK(pub K);

impl PartialEq for ExactK {
    fn eq(&self, other: &Self) -> bool {
        match (&*self.0, &*other.0) {
            (KData::FloatAtom(x), KData::FloatAtom(y)) => {
                if x.is_nan() && y.is_nan() {
                    true
                } else {
                    x.to_bits() == y.to_bits()
                }
            }
            (KData::FloatList(x), KData::FloatList(y)) => {
                if x.len() != y.len() {
                    return false;
                }
                x.iter().zip(y.iter()).all(|(&a, &b)| {
                    if a.is_nan() && b.is_nan() {
                        true
                    } else {
                        a.to_bits() == b.to_bits()
                    }
                })
            }
            _ => self.0 == other.0,
        }
    }
}

impl Eq for ExactK {}

impl std::hash::Hash for ExactK {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match &*self.0 {
            KData::FloatAtom(f) => {
                1.hash(state);
                let bits = if f.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    f.to_bits()
                };
                bits.hash(state);
            }
            KData::FloatList(v) => {
                5.hash(state);
                for &f in v {
                    let bits = if f.is_nan() {
                        f64::NAN.to_bits()
                    } else {
                        f.to_bits()
                    };
                    bits.hash(state);
                }
            }
            _ => self.0.hash(state),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HashListMap(pub std::collections::HashMap<ExactK, i64>);

impl PartialEq for HashListMap {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl std::hash::Hash for HashListMap {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let mut total_hash: u64 = 0;
        for (k, v) in &self.0 {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            k.hash(&mut hasher);
            v.hash(&mut hasher);
            use std::hash::Hasher;
            total_hash = total_hash.wrapping_add(hasher.finish());
        }
        total_hash.hash(state);
    }
}

#[inline]
pub fn i2f(i: i64) -> f64 {
    if i == i64::MIN {
        f64::NAN
    } else if i == i64::MAX {
        f64::INFINITY
    } else if i == -i64::MAX {
        f64::NEG_INFINITY
    } else {
        i as f64
    }
}

pub fn fc(a: f64, b: f64) -> i32 {
    let mut x_bits = a.to_bits() as i64;
    let mut y_bits = b.to_bits() as i64;
    let xu = x_bits | y_bits;

    let is_nan_x = ((x_bits as u64) & 0x7fffffffffffffff) > 0x7ff0000000000000;
    let is_nan_y = ((y_bits as u64) & 0x7fffffffffffffff) > 0x7ff0000000000000;

    if is_nan_x {
        if is_nan_y {
            return 0;
        } else {
            return -1;
        }
    }

    if x_bits < 0 {
        x_bits = i64::MIN.wrapping_sub(x_bits);
    }

    if is_nan_y {
        return 1;
    }

    if y_bits < 0 {
        y_bits = i64::MIN.wrapping_sub(y_bits);
    }

    let diff = x_bits.wrapping_sub(y_bits);
    let ad = if diff == i64::MIN { i64::MAX } else { diff.abs() };

    if ad < 1023 {
        let is_subn_xu = ((xu as u64) & 0x7fffffffffffffff) < 0x0010000000000000;
        if is_subn_xu {
            if x_bits < y_bits {
                return -1;
            } else if x_bits != y_bits {
                return 1;
            } else {
                return 0;
            }
        }

        if ad < 513 {
            return 0;
        }

        let xu_val = 513 + ((255 & (xu >> 44)) << 1);
        if ad < xu_val {
            return 0;
        }
    }

    if x_bits < y_bits { -1 } else { 1 }
}

#[inline]
pub fn float_eq(x: f64, y: f64) -> bool {
    fc(x, y) == 0
}

pub fn compare_numeric(a: &KData, b: &KData) -> Option<std::cmp::Ordering> {
    // two ints compare exactly, as in C Kona; only a float side goes through the tolerant fc
    if let (KData::IntAtom(x), KData::IntAtom(y)) = (a, b) {
        return Some(x.cmp(y));
    }
    let val_a = match a {
        KData::IntAtom(i) => i2f(*i),
        KData::FloatAtom(f) => *f,
        _ => return None,
    };
    let val_b = match b {
        KData::IntAtom(i) => i2f(*i),
        KData::FloatAtom(f) => *f,
        _ => return None,
    };
    let res = fc(val_a, val_b);
    if res == 0 {
        Some(std::cmp::Ordering::Equal)
    } else if res < 0 {
        Some(std::cmp::Ordering::Less)
    } else {
        Some(std::cmp::Ordering::Greater)
    }
}

pub fn match_eq(a: &K, b: &K) -> bool {
    if Arc::ptr_eq(&a.0, &b.0) {
        return true;
    }
    match_eq_data(&a.0, &b.0)
}

fn match_int_float(i: i64, f: f64) -> bool {
    f.is_finite() && i != i64::MIN && i != i64::MAX && i != -i64::MAX && float_eq(i as f64, f)
}

fn match_eq_data(a: &KData, b: &KData) -> bool {
    match (a, b) {
        (KData::IntAtom(x), KData::IntAtom(y)) => x == y,
        (KData::IntAtom(x), KData::FloatAtom(y)) | (KData::FloatAtom(y), KData::IntAtom(x)) => {
            match_int_float(*x, *y)
        }
        (KData::FloatAtom(x), KData::FloatAtom(y)) => float_eq(*x, *y),
        (KData::CharAtom(x), KData::CharAtom(y)) => x == y,
        (KData::SymbolAtom(x), KData::SymbolAtom(y)) => x == y,
        (KData::IntList(x), KData::IntList(y)) => x == y,
        (KData::IntList(x), KData::FloatList(y)) | (KData::FloatList(y), KData::IntList(x)) => {
            if x.len() != y.len() {
                return false;
            }
            x.iter()
                .zip(y.iter())
                .all(|(&xv, &yv)| match_int_float(xv, yv))
        }
        (KData::FloatList(x), KData::FloatList(y)) => {
            if x.len() != y.len() {
                return false;
            }
            x.iter().zip(y.iter()).all(|(&xv, &yv)| float_eq(xv, yv))
        }
        (KData::CharList(x), KData::CharList(y)) => x == y,
        (KData::SymbolList(x), KData::SymbolList(y)) => x == y,
        (KData::GeneralList(x), KData::GeneralList(y)) => {
            if x.len() != y.len() {
                return false;
            }
            x.iter().zip(y.iter()).all(|(xv, yv)| match_eq(xv, yv))
        }
        (
            KData::Dictionary {
                keys: k1,
                values: v1,
            },
            KData::Dictionary {
                keys: k2,
                values: v2,
            },
        ) => match_eq(k1, k2) && match_eq(v1, v2),
        (KData::Nil, KData::Nil) => true,
        (KData::Function(x), KData::Function(y)) => x == y,
        (
            KData::HashList {
                indices: i1,
                map: m1,
            },
            KData::HashList {
                indices: i2,
                map: m2,
            },
        ) => i1 == i2 && m1 == m2,
        _ => false,
    }
}

/// KData represents the various data variants in K:
/// atoms, lists of basic types, general lists, dictionaries, nil, and functions.
pub enum KData {
    IntAtom(i64),
    FloatAtom(f64),
    CharAtom(char),
    SymbolAtom(Arc<str>),
    IntList(Vec<i64>),
    FloatList(Vec<f64>),
    CharList(Vec<u8>),
    SymbolList(Vec<Arc<str>>),
    GeneralList(Vec<K>),
    Dictionary { keys: K, values: K },
    Nil,
    Function(KFunc),
    HashList { indices: Vec<i64>, map: HashListMap },
}

impl PartialEq for KData {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (KData::IntAtom(x), KData::IntAtom(y)) => x == y,
            (KData::FloatAtom(x), KData::FloatAtom(y)) => {
                // Exact bit equality for floats (with NaN-all-equal and signed-zero handled by IEEE ==).
                // This preserves the Eq/Hash invariant required for HashMap<K> (used by group/_hash).
                // Tolerant numeric matching for find/group/grade is handled explicitly in those verbs
                // using float_eq (see list.rs find, cmp_k).
                if x.is_nan() && y.is_nan() {
                    true
                } else {
                    *x == *y
                }
            }
            (KData::CharAtom(x), KData::CharAtom(y)) => x == y,
            (KData::SymbolAtom(x), KData::SymbolAtom(y)) => x == y,
            (KData::IntList(x), KData::IntList(y)) => x == y,
            (KData::FloatList(x), KData::FloatList(y)) => {
                if x.len() != y.len() {
                    return false;
                }
                x.iter().zip(y.iter()).all(|(&a, &b)| {
                    if a.is_nan() && b.is_nan() {
                        true
                    } else {
                        a == b
                    }
                })
            }
            (KData::CharList(x), KData::CharList(y)) => x == y,
            (KData::SymbolList(x), KData::SymbolList(y)) => x == y,
            (KData::GeneralList(x), KData::GeneralList(y)) => {
                if let Some(_g) = DepthGuard::new() {
                    x == y
                } else {
                    false
                }
            }
            (
                KData::Dictionary {
                    keys: k1,
                    values: v1,
                },
                KData::Dictionary {
                    keys: k2,
                    values: v2,
                },
            ) => {
                if let Some(_g) = DepthGuard::new() {
                    k1 == k2 && v1 == v2
                } else {
                    false
                }
            }
            (KData::Nil, KData::Nil) => true,
            (KData::Function(x), KData::Function(y)) => x == y,
            (
                KData::HashList {
                    indices: i1,
                    map: m1,
                },
                KData::HashList {
                    indices: i2,
                    map: m2,
                },
            ) => i1 == i2 && m1 == m2,
            _ => false,
        }
    }
}

impl Eq for KData {}
impl Eq for K {}

impl std::hash::Hash for K {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl std::hash::Hash for KData {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            KData::IntAtom(i) => {
                0.hash(state);
                i.hash(state);
            }
            KData::FloatAtom(f) => {
                1.hash(state);
                let bits = if f.is_nan() {
                    f64::NAN.to_bits()
                } else if *f == 0.0 {
                    0.0f64.to_bits()
                } else {
                    f.to_bits()
                };
                bits.hash(state);
            }
            KData::CharAtom(c) => {
                2.hash(state);
                c.hash(state);
            }
            KData::SymbolAtom(s) => {
                3.hash(state);
                s.hash(state);
            }
            KData::IntList(v) => {
                4.hash(state);
                v.hash(state);
            }
            KData::FloatList(v) => {
                5.hash(state);
                for &f in v {
                    let bits = if f.is_nan() {
                        f64::NAN.to_bits()
                    } else if f == 0.0 {
                        0.0f64.to_bits()
                    } else {
                        f.to_bits()
                    };
                    bits.hash(state);
                }
            }
            KData::CharList(v) => {
                6.hash(state);
                v.hash(state);
            }
            KData::SymbolList(v) => {
                7.hash(state);
                v.hash(state);
            }
            KData::GeneralList(v) => {
                8.hash(state);
                if let Some(_g) = DepthGuard::new() {
                    v.hash(state);
                }
            }
            KData::Dictionary { keys, values } => {
                9.hash(state);
                if let Some(_g) = DepthGuard::new() {
                    keys.hash(state);
                    values.hash(state);
                }
            }
            KData::Nil => {
                10.hash(state);
            }
            KData::Function(f) => {
                11.hash(state);
                match f {
                    KFunc::Primitive(s) => {
                        0.hash(state);
                        s.hash(state);
                    }
                    KFunc::Adverb { name, operand } => {
                        1.hash(state);
                        name.hash(state);
                        operand.hash(state);
                    }
                    KFunc::Projection { func, args } => {
                        2.hash(state);
                        if let Some(_g) = DepthGuard::new() {
                            func.hash(state);
                            args.hash(state);
                        }
                    }
                    KFunc::UserDefined { body, args, .. } => {
                        3.hash(state);
                        body.hash(state);
                        args.hash(state);
                    }
                    KFunc::Composition { f, g } => {
                        4.hash(state);
                        if let Some(_g) = DepthGuard::new() {
                            f.hash(state);
                            g.hash(state);
                        }
                    }
                }
            }
            KData::HashList { indices, map } => {
                12.hash(state);
                indices.hash(state);
                map.hash(state);
            }
        }
    }
}

/// K is the primary type wrapper for values in the Kona interpreter.
/// It wraps an Arc<KData> for cheap cloning, sharing, and thread safety.
#[derive(Debug, Clone)]
pub struct K(pub Arc<KData>);

impl std::ops::Deref for K {
    type Target = KData;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// When the last holder of a big number list lets go, keep its memory for the next operation (pool.rs)
/// instead of handing it back to the OS. Arc::get_mut only gives access when nobody else holds the value,
/// so nothing that is still in use can be touched. (Made by Claude)
impl Drop for K {
    fn drop(&mut self) {
        if let Some(d) = Arc::get_mut(&mut self.0) {
            match d {
                KData::FloatList(v) if v.len() >= crate::pool::MIN_LEN => crate::pool::give(std::mem::take(v)),
                KData::IntList(v) if v.len() >= crate::pool::MIN_LEN => crate::pool::give(std::mem::take(v)),
                _ => {}
            }
        }
    }
}

impl From<KData> for K {
    fn from(mut data: KData) -> Self {
        match &mut data {
            KData::FloatList(v) => {
                for x in v {
                    if *x == -9223372036854775808.0 {
                        *x = f64::NAN;
                    }
                }
            }
            _ => {}
        }
        K(Arc::new(data))
    }
}

impl PartialEq for K {
    fn eq(&self, other: &Self) -> bool {
        if Arc::ptr_eq(&self.0, &other.0) {
            return true;
        }
        *self.0 == *other.0
    }
}

impl K {
    // --- Atom Constructors ---

    pub fn int(i: i64) -> Self {
        K::from(KData::IntAtom(i))
    }

    pub fn float(f: f64) -> Self {
        K::from(KData::FloatAtom(f))
    }

    pub fn char(c: char) -> Self {
        K::from(KData::CharAtom(c))
    }

    pub fn symbol<S: Into<Arc<str>>>(s: S) -> Self {
        K::from(KData::SymbolAtom(s.into()))
    }

    // --- List Constructors ---

    pub fn int_list(v: Vec<i64>) -> Self {
        K::from(KData::IntList(v))
    }

    pub fn float_list(v: Vec<f64>) -> Self {
        K::from(KData::FloatList(v))
    }

    pub fn float_list_raw(v: Vec<f64>) -> Self {
        K(Arc::new(KData::FloatList(v)))
    }

    pub fn char_list(v: Vec<u8>) -> Self {
        K::from(KData::CharList(v))
    }

    pub fn symbol_list(v: Vec<Arc<str>>) -> Self {
        K::from(KData::SymbolList(v))
    }

    pub fn general_list(v: Vec<K>) -> Self {
        K::from(KData::GeneralList(v))
    }

    // --- Dictionary, Nil, Function Constructors ---

    pub fn dictionary(keys: K, values: K) -> Self {
        K::from(KData::Dictionary { keys, values })
    }

    pub fn nil() -> Self {
        K::from(KData::Nil)
    }

    pub fn hash_list(indices: Vec<i64>, map: std::collections::HashMap<ExactK, i64>) -> Self {
        K::from(KData::HashList {
            indices,
            map: HashListMap(map),
        })
    }

    pub fn function(f: KFunc) -> Self {
        K::from(KData::Function(f))
    }

    // --- Type Inspection Helpers ---

    pub fn is_nil(&self) -> bool {
        matches!(**self, KData::Nil)
    }

    pub fn is_function(&self) -> bool {
        matches!(**self, KData::Function(_))
    }

    pub fn valence(&self) -> usize {
        match &**self {
            KData::Function(kfunc) => kfunc.valence(),
            _ => 0,
        }
    }

    pub fn is_composable_func(&self) -> bool {
        match &**self {
            KData::Function(f) => f.is_composable(),
            KData::GeneralList(funcs) => {
                funcs.len() == 2 && funcs.iter().all(|f| f.is_composable_func())
            }
            _ => false,
        }
    }

    pub fn is_atom(&self) -> bool {
        matches!(
            **self,
            KData::IntAtom(_) | KData::FloatAtom(_) | KData::CharAtom(_) | KData::SymbolAtom(_)
        )
    }

    pub fn is_list(&self) -> bool {
        matches!(
            **self,
            KData::IntList(_)
                | KData::FloatList(_)
                | KData::CharList(_)
                | KData::SymbolList(_)
                | KData::GeneralList(_)
                | KData::HashList { .. }
        )
    }

    pub fn len(&self) -> Result<usize, KonaError> {
        match &**self {
            KData::IntAtom(_) | KData::FloatAtom(_) | KData::CharAtom(_) | KData::SymbolAtom(_) => {
                Ok(1)
            }
            KData::IntList(v) => Ok(v.len()),
            KData::FloatList(v) => Ok(v.len()),
            KData::CharList(v) => Ok(v.len()),
            KData::SymbolList(v) => Ok(v.len()),
            KData::GeneralList(v) => Ok(v.len()),
            KData::HashList { indices, .. } => Ok(indices.len()),
            KData::Dictionary { keys, .. } => keys.len(),
            KData::Nil => Ok(0),
            KData::Function(_) => Err(KonaError::Type),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len().map(|l| l == 0).unwrap_or(false)
    }

    // --- Data Extraction Helpers ---

    pub fn as_int(&self) -> Option<i64> {
        match &**self {
            KData::IntAtom(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match &**self {
            KData::FloatAtom(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_char(&self) -> Option<char> {
        match &**self {
            KData::CharAtom(c) => Some(*c),
            _ => None,
        }
    }

    pub fn as_symbol(&self) -> Option<&Arc<str>> {
        match &**self {
            KData::SymbolAtom(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int_list(&self) -> Option<&[i64]> {
        match &**self {
            KData::IntList(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    pub fn as_float_list(&self) -> Option<&[f64]> {
        match &**self {
            KData::FloatList(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    pub fn as_char_list(&self) -> Option<&[u8]> {
        match &**self {
            KData::CharList(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    pub fn as_symbol_list(&self) -> Option<&[Arc<str>]> {
        match &**self {
            KData::SymbolList(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    pub fn as_general_list(&self) -> Option<&[K]> {
        match &**self {
            KData::GeneralList(v) => Some(v.as_slice()),
            _ => None,
        }
    }
}

impl Clone for KFunc {
    fn clone(&self) -> Self {
        match self {
            KFunc::Primitive(s) => KFunc::Primitive(s.clone()),
            KFunc::Adverb { name, operand } => KFunc::Adverb {
                name: name.clone(),
                operand: operand.clone(),
            },
            KFunc::Projection { func, args } => {
                if let Some(_g) = DepthGuard::new() {
                    KFunc::Projection {
                        func: func.clone(),
                        args: args.clone(),
                    }
                } else {
                    panic!("Recursion limit exceeded")
                }
            }
            KFunc::UserDefined {
                body,
                args,
                code,
                env,
                metadata,
            } => KFunc::UserDefined {
                body: body.clone(),
                args: args.clone(),
                code: code.clone(),
                env: env.clone(),
                metadata: metadata.clone(),
            },
            KFunc::Composition { f, g } => {
                if let Some(_g) = DepthGuard::new() {
                    KFunc::Composition {
                        f: f.clone(),
                        g: g.clone(),
                    }
                } else {
                    panic!("Recursion limit exceeded")
                }
            }
        }
    }
}

impl std::fmt::Debug for KFunc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KFunc::Primitive(s) => f.debug_tuple("Primitive").field(s).finish(),
            KFunc::Adverb { name, operand } => f
                .debug_struct("Adverb")
                .field("name", name)
                .field("operand", operand)
                .finish(),
            KFunc::Projection { func, args } => {
                if let Some(_g) = DepthGuard::new() {
                    f.debug_struct("Projection")
                        .field("func", func)
                        .field("args", args)
                        .finish()
                } else {
                    write!(f, "Projection {{ <recursion limit> }}")
                }
            }
            KFunc::UserDefined {
                body,
                args,
                code,
                env,
                ..
            } => f
                .debug_struct("UserDefined")
                .field("body", body)
                .field("args", args)
                .field("code", code)
                .field("env", env)
                .finish(),
            KFunc::Composition { f: f_comp, g } => {
                if let Some(_g) = DepthGuard::new() {
                    f.debug_struct("Composition")
                        .field("f", f_comp)
                        .field("g", g)
                        .finish()
                } else {
                    write!(f, "Composition {{ <recursion limit> }}")
                }
            }
        }
    }
}

impl Clone for KData {
    fn clone(&self) -> Self {
        match self {
            KData::IntAtom(i) => KData::IntAtom(*i),
            KData::FloatAtom(fl) => KData::FloatAtom(*fl),
            KData::CharAtom(c) => KData::CharAtom(*c),
            KData::SymbolAtom(s) => KData::SymbolAtom(s.clone()),
            KData::IntList(v) => KData::IntList(v.clone()),
            KData::FloatList(v) => KData::FloatList(v.clone()),
            KData::CharList(v) => KData::CharList(v.clone()),
            KData::SymbolList(v) => KData::SymbolList(v.clone()),
            KData::GeneralList(v) => {
                if let Some(_g) = DepthGuard::new() {
                    KData::GeneralList(v.clone())
                } else {
                    panic!("Recursion limit exceeded")
                }
            }
            KData::Dictionary { keys, values } => {
                if let Some(_g) = DepthGuard::new() {
                    KData::Dictionary {
                        keys: keys.clone(),
                        values: values.clone(),
                    }
                } else {
                    panic!("Recursion limit exceeded")
                }
            }
            KData::Nil => KData::Nil,
            KData::Function(func) => KData::Function(func.clone()),
            KData::HashList { indices, map } => KData::HashList {
                indices: indices.clone(),
                map: map.clone(),
            },
        }
    }
}

impl std::fmt::Debug for KData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KData::IntAtom(i) => f.debug_tuple("IntAtom").field(i).finish(),
            KData::FloatAtom(fl) => f.debug_tuple("FloatAtom").field(fl).finish(),
            KData::CharAtom(c) => f.debug_tuple("CharAtom").field(c).finish(),
            KData::SymbolAtom(s) => f.debug_tuple("SymbolAtom").field(s).finish(),
            KData::IntList(v) => f.debug_tuple("IntList").field(v).finish(),
            KData::FloatList(v) => f.debug_tuple("FloatList").field(v).finish(),
            KData::CharList(v) => f.debug_tuple("CharList").field(v).finish(),
            KData::SymbolList(v) => f.debug_tuple("SymbolList").field(v).finish(),
            KData::GeneralList(v) => {
                if let Some(_g) = DepthGuard::new() {
                    f.debug_tuple("GeneralList").field(v).finish()
                } else {
                    write!(f, "GeneralList([<recursion limit>])")
                }
            }
            KData::Dictionary { keys, values } => {
                if let Some(_g) = DepthGuard::new() {
                    f.debug_struct("Dictionary")
                        .field("keys", keys)
                        .field("values", values)
                        .finish()
                } else {
                    write!(f, "Dictionary {{ <recursion limit> }}")
                }
            }
            KData::Nil => write!(f, "Nil"),
            KData::Function(func) => f.debug_tuple("Function").field(func).finish(),
            KData::HashList { indices, map } => f
                .debug_struct("HashList")
                .field("indices", indices)
                .field("map", map)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atoms() {
        let i = K::int(42);
        assert_eq!(i.as_int(), Some(42));
        assert!(i.is_atom());
        assert!(!i.is_list());
        assert_eq!(i.len().unwrap(), 1);

        let f = K::float(123.45);
        assert_eq!(f.as_float(), Some(123.45));

        let c = K::char('k');
        assert_eq!(c.as_char(), Some('k'));

        let s = K::symbol("sym");
        assert_eq!(s.as_symbol().unwrap().as_ref(), "sym");
    }

    #[test]
    fn test_lists() {
        let il = K::int_list(vec![1, 2, 3]);
        assert_eq!(il.as_int_list(), Some([1, 2, 3].as_slice()));
        assert!(il.is_list());
        assert_eq!(il.len().unwrap(), 3);

        let fl = K::float_list(vec![1.1, 2.2]);
        assert_eq!(fl.as_float_list(), Some([1.1, 2.2].as_slice()));

        let cl = K::char_list(vec![b'a', b'b']);
        assert_eq!(cl.as_char_list(), Some([b'a', b'b'].as_slice()));

        let sl = K::symbol_list(vec![Arc::from("a"), Arc::from("b")]);
        assert_eq!(sl.as_symbol_list().unwrap().len(), 2);

        let gl = K::general_list(vec![K::int(1), K::char('a')]);
        assert_eq!(gl.as_general_list().unwrap().len(), 2);
    }

    #[test]
    fn test_eq_and_clone() {
        let k1 = K::int(42);
        let k2 = k1.clone();
        assert_eq!(k1, k2);

        let k3 = K::int(42);
        assert_eq!(k1, k3);

        let k4 = K::int(43);
        assert_ne!(k1, k4);
    }

    #[test]
    fn test_hash_list_eq() {
        let mut map1 = std::collections::HashMap::new();
        map1.insert(ExactK(K::int(10)), 0);
        map1.insert(ExactK(K::int(20)), 1);
        let hl1 = K::hash_list(vec![0, 1], map1);

        let mut map2 = std::collections::HashMap::new();
        map2.insert(ExactK(K::int(10)), 0);
        map2.insert(ExactK(K::int(20)), 1);
        let hl2 = K::hash_list(vec![0, 1], map2);

        let mut map3 = std::collections::HashMap::new();
        map3.insert(ExactK(K::int(10)), 0);
        map3.insert(ExactK(K::int(30)), 1);
        let hl3 = K::hash_list(vec![0, 1], map3);

        assert_eq!(hl1, hl2);
        assert_ne!(hl1, hl3);

        let mut hasher1 = std::collections::hash_map::DefaultHasher::new();
        use std::hash::Hash;
        hl1.hash(&mut hasher1);
        use std::hash::Hasher;
        let h1 = hasher1.finish();

        let mut hasher2 = std::collections::hash_map::DefaultHasher::new();
        hl2.hash(&mut hasher2);
        let h2 = hasher2.finish();

        let mut hasher3 = std::collections::hash_map::DefaultHasher::new();
        hl3.hash(&mut hasher3);
        let h3 = hasher3.finish();

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_nan_and_signed_zero_hash_eq() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        fn calculate_hash<T: Hash>(t: &T) -> u64 {
            let mut s = DefaultHasher::new();
            t.hash(&mut s);
            s.finish()
        }

        // Test NaNs
        let nan1 = K::float(f64::NAN);
        let nan2 = K::float(f64::NAN);
        let nan_other = K::float(f64::from_bits(0x7ff8000000000001)); // different NaN bit pattern

        assert_eq!(nan1, nan2);
        assert_eq!(nan1, nan_other);
        assert_eq!(calculate_hash(&nan1), calculate_hash(&nan2));
        assert_eq!(calculate_hash(&nan1), calculate_hash(&nan_other));

        // Ensure NaNs do not hash to the same value as 0.0
        let zero = K::float(0.0);
        assert_ne!(nan1, zero);
        assert_ne!(calculate_hash(&nan1), calculate_hash(&zero));

        // Test signed zeros
        let pzero = K::float(0.0);
        let nzero = K::float(-0.0);
        assert_eq!(pzero, nzero);
        assert_eq!(calculate_hash(&pzero), calculate_hash(&nzero));

        // Test FloatList NaNs and signed zero
        let list_nan1 = K::float_list(vec![f64::NAN, 1.2, -0.0]);
        let list_nan2 = K::float_list(vec![f64::from_bits(0x7ff8000000000001), 1.2, 0.0]);
        assert_eq!(list_nan1, list_nan2);
        assert_eq!(calculate_hash(&list_nan1), calculate_hash(&list_nan2));
    }

    #[test]
    fn test_c_kona_float_compatibility_and_sentinels() {
        // 1. Float compatibility
        assert!(float_eq(1.0, 1.0 + 1.13e-13));
        assert!(!float_eq(1.0, 1.0 + 1.14e-13));

        // 2. i2f sentinel conversions
        assert!(i2f(i64::MIN).is_nan());
        assert_eq!(i2f(i64::MAX), f64::INFINITY);
        assert_eq!(i2f(-i64::MAX), f64::NEG_INFINITY);
        assert_eq!(i2f(0), 0.0);
        assert_eq!(i2f(100), 100.0);

        // 3. compare_numeric
        let nan_k = KData::FloatAtom(f64::NAN);
        let int_null = KData::IntAtom(i64::MIN);
        let normal_k = KData::FloatAtom(1.0);
        let normal_int = KData::IntAtom(1);

        assert_eq!(
            compare_numeric(&nan_k, &int_null),
            Some(std::cmp::Ordering::Equal)
        );
        assert_eq!(
            compare_numeric(&int_null, &normal_k),
            Some(std::cmp::Ordering::Less)
        );
        assert_eq!(
            compare_numeric(&normal_k, &int_null),
            Some(std::cmp::Ordering::Greater)
        );
        assert_eq!(
            compare_numeric(&normal_int, &normal_k),
            Some(std::cmp::Ordering::Equal)
        );
    }
}
