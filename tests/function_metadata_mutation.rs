#![forbid(unsafe_code)]

use kona::{eval::{apply_func, eval_words}, io::{serialize_k, KDeserializer}, parser::parse,
    types::{K, KData, KFunc}, verbs::dict::KTree};
use std::{collections::HashMap, sync::Arc};

fn definition(source: &str) -> KFunc {
    let value = eval_words(&parse(source).unwrap(), &mut HashMap::new(), &Arc::new(KTree::new())).unwrap();
    let KData::Function(function) = &*value else { panic!("not a function") };
    function.clone()
}

fn call(function: &KFunc, arguments: &[K]) -> Result<K, kona::errors::KonaError> {
    apply_func(&K::function(function.clone()), arguments, &HashMap::new(), &Arc::new(KTree::new()))
}

#[test]
fn changed_duplicate_parameters_cannot_reuse_stale_binding_rules() {
    let original = definition("{[x;y]x}");
    let mut edited = original.clone();
    if let KFunc::UserDefined { args, .. } = &mut edited { args[1] = "x".into(); }
    assert_eq!(call(&edited, &[K::int(2), K::int(5)]).unwrap(), K::int(2));
    assert_eq!(call(&original, &[K::int(2), K::int(5)]).unwrap(), K::int(2));
    assert_eq!(call(&edited, &[K::int(7), K::int(9)]).unwrap(), K::int(7));
}

#[test]
fn changed_parsed_code_and_locals_are_private_to_the_edited_clone() {
    let original = definition("{[x]old:x+1;old}");
    let mut edited = original.clone();
    if let KFunc::UserDefined { code: Some(code), .. } = &mut edited {
        *Arc::make_mut(code) = parse("new:x*3;new").unwrap();
    }
    assert_eq!(call(&edited, &[K::int(4)]).unwrap(), K::int(12));
    assert_eq!(call(&original, &[K::int(4)]).unwrap(), K::int(5));
    // Identical allocation length is insufficient as a validity check.
    if let KFunc::UserDefined { code: Some(code), .. } = &mut edited {
        *Arc::make_mut(code) = parse("new:x*8;new").unwrap();
    }
    assert_eq!(call(&edited, &[K::int(4)]).unwrap(), K::int(32));
}

#[test]
fn lazy_body_mutation_and_code_presence_transitions_obey_original_precedence() {
    let mut function = KFunc::user_defined("[x]x+1", vec!["x".into()]);
    assert_eq!(call(&function, &[K::int(3)]).unwrap(), K::int(4));
    if let KFunc::UserDefined { body, .. } = &mut function { *body = "[x]x*2".into(); }
    assert_eq!(call(&function, &[K::int(3)]).unwrap(), K::int(6));
    if let KFunc::UserDefined { code, .. } = &mut function {
        *code = Some(Arc::new(parse("x-1").unwrap()));
    }
    assert_eq!(call(&function, &[K::int(3)]).unwrap(), K::int(2));
    if let KFunc::UserDefined { body, .. } = &mut function { *body = "[x](".into(); }
    // A supplied parsed code is authoritative; body text is display/wire data.
    assert_eq!(call(&function, &[K::int(3)]).unwrap(), K::int(2));
    if let KFunc::UserDefined { code, .. } = &mut function { *code = None; }
    // Projection and valence must still precede lazy parsing, even after edits.
    assert!(call(&function, &[]).unwrap().is_function());
    assert_eq!(call(&function, &[K::int(1), K::int(2)]), Err(kona::errors::KonaError::Valence));
    assert!(matches!(call(&function, &[K::int(3)]), Err(kona::errors::KonaError::Parse(_))));
}

#[test]
fn divergent_lazy_clones_race_without_publishing_each_others_definition() {
    let original = KFunc::user_defined("[x]x+1", vec!["x".into()]);
    let functions: Vec<_> = (0..12).map(|i| {
        let mut function = original.clone();
        if let KFunc::UserDefined { body, .. } = &mut function { *body = format!("[x]x+{i}"); }
        (function, i)
    }).collect();
    let barrier = Arc::new(std::sync::Barrier::new(functions.len()));
    std::thread::scope(|scope| {
        for (function, i) in functions {
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                for _ in 0..12 { assert_eq!(call(&function, &[K::int(10)]).unwrap(), K::int(10 + i)); }
            });
        }
    });
    assert_eq!(call(&original, &[K::int(3)]).unwrap(), K::int(4));
}

#[test]
fn edited_wire_definition_is_restored_independently_of_any_old_cache() {
    let mut function = definition("{[x]x+1}");
    if let KFunc::UserDefined { body, code, .. } = &mut function {
        *body = "[x]x*7".into();
        *code = None;
    }
    let function = K::function(function);
    let bytes = serialize_k(&function);
    let restored = KDeserializer::new(&bytes).read_k().unwrap();
    for value in [&function, &restored] {
        assert_eq!(apply_func(value, &[K::int(3)], &HashMap::new(), &Arc::new(KTree::new())).unwrap(), K::int(21));
        assert_eq!(serialize_k(value), bytes);
    }
}
