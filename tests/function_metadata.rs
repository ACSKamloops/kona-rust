#![forbid(unsafe_code)]

use kona::{eval::{apply_func, eval_words}, io::{serialize_k, KDeserializer}, parser::parse,
    types::{K, KData, KFunc}, verbs::dict::KTree};
use std::{collections::HashMap, sync::Arc};

fn run(code: &str) -> K {
    eval_words(&parse(code).unwrap(), &mut HashMap::new(), &Arc::new(KTree::new())).unwrap()
}

#[test]
fn repeated_parameters_projections_and_shadowing() {
    for (source, expected) in [
        ("{[a;a]a}[10;20]", 10),
        ("{[a;a]a:a+1;a}[10;20]", 11),
        ("p:{[a;a]a}[;20];p[10]", 10),
        ("a:99;{[x]a:x+1;a}4", 5),
        ("a:99;{[x]a:x+1;a}4;a", 99),
        ("f:{:[x<2;1;x*_f[x-1]]};f[6]", 720),
    ] {
        assert_eq!(run(source), K::int(expected), "{source}");
    }
}

#[test]
fn captured_environments_remain_private_per_call() {
    // Captures retain their construction-time values, and a call's local
    // assignment must not alter the environment used by subsequent calls.
    assert_eq!(run("g:{a:3;g:{a+:1;a};a:8;g}0;(g[];g[])"), K::int_list(vec![4, 4]));
}

#[test]
fn locals_inside_parentheses_and_brackets_are_discovered() {
    assert_eq!(run("a:99;{[x]:[x;1;(a:7)];a}[1]"), K::nil());
    assert_eq!(run("a:99;{[x]:[x;a:7;1];a}[0]"), K::nil());
    // Assignments in an inner function must not hide an outer global.
    assert_eq!(run("a:99;{g:{a:7};a}0"), K::int(99));
}

#[test]
fn deserialized_and_api_functions_cache_without_changing_wire_format() {
    let original = run("{[a;a]local:a+1;local}");
    let bytes = serialize_k(&original);
    let restored = KDeserializer::new(&bytes).read_k().unwrap();
    let ktree = Arc::new(KTree::new());
    let env = HashMap::new();
    let clone = if let KData::Function(f) = &*restored { K::function(f.clone()) } else { panic!() };
    for func in [&restored, &clone] {
        for _ in 0..3 {
            assert_eq!(apply_func(func, &[K::int(5), K::int(9)], &env, &ktree).unwrap(), K::int(6));
        }
        assert_eq!(serialize_k(func), bytes);
        assert_eq!(func, &original);
    }
    if let (KData::Function(KFunc::UserDefined { metadata: a, .. }),
            KData::Function(KFunc::UserDefined { metadata: b, .. })) = (&*restored, &*clone) {
        assert!(Arc::ptr_eq(a, b));
        assert!(a.get().is_some());
    } else { panic!() }

    let invalid = K::function(KFunc::user_defined("[x](", vec!["x".into()]));
    // Creating a projection must still succeed before parsing an invalid body.
    assert!(apply_func(&invalid, &[], &env, &ktree).unwrap().is_function());
    assert!(matches!(apply_func(&invalid, &[K::int(1)], &env, &ktree), Err(kona::errors::KonaError::Parse(_))));
}

#[test]
fn lazy_initialization_is_safe_for_concurrent_embedding_calls() {
    let func = K::function(KFunc::user_defined("[a;a]local:a+1;local", vec!["a".into(), "a".into()]));
    let ktree = Arc::new(KTree::new());
    std::thread::scope(|scope| {
        for i in 0..8 {
            let func = &func;
            let ktree = &ktree;
            scope.spawn(move || {
                for _ in 0..20 {
                    assert_eq!(apply_func(func, &[K::int(i), K::int(99)], &HashMap::new(), ktree).unwrap(), K::int(i + 1));
                }
            });
        }
    });
}
