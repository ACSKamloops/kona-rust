#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use crate::errors::KonaError;
    use crate::eval::eval_words;
    use crate::io::*;
    use crate::parser::parse;
    use crate::types::{K, KData};
    use crate::verbs::dict::{KTree, create_dict};
    use std::collections::HashMap;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;

    #[test]
    fn test_text_file_io() {
        let temp_path = "temp_test_text.txt";
        let lines = K::general_list(vec![
            K::char_list(b"hello".to_vec()),
            K::char_list(b"world".to_vec()),
        ]);

        let path_k = K::symbol(temp_path);
        let write_res = write_text_file(&path_k, &lines).unwrap();
        assert_eq!(write_res, lines);

        let read_res = read_text_file(&path_k).unwrap();
        assert_eq!(read_res, lines);

        let _ = fs::remove_file(temp_path);
    }

    #[test]
    fn test_stdout_printing() {
        let empty_sym = K::symbol("");
        let msg = K::char_list(b"Testing stdout printing".to_vec());
        let res = write_text_file(&empty_sym, &msg).unwrap();
        assert_eq!(res, msg);
    }

    #[test]
    fn test_composition_scan() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();
        let code = "{x(|+\\)\\1 1} 5";
        let words = parse(code).unwrap();
        println!("WORDS: {:?}", words);
        let res = eval_words(&words, &mut env, &ktree).unwrap();
        println!("RESULT: {:?}", res);
    }

    #[test]
    fn test_binary_file_io_and_safety() {
        let temp_path = "temp_test_bin.bin";
        let path_k = K::symbol(temp_path);

        let k_obj = K::general_list(vec![
            K::int(42),
            K::float(123.45),
            K::char('A'),
            K::symbol("test_sym"),
            K::int_list(vec![1, 2, 3]),
            K::float_list(vec![1.1, 2.2]),
            K::char_list(b"string_val".to_vec()),
            K::symbol_list(vec![Arc::from("a"), Arc::from("b")]),
            create_dict(
                &K::symbol_list(vec![Arc::from("k")]),
                &K::int_list(vec![100]),
            )
            .unwrap(),
        ]);

        let write_res = write_binary_file(&path_k, &k_obj).unwrap();
        assert_eq!(write_res, k_obj);

        let read_res = read_binary_file(&path_k).unwrap();
        assert_eq!(read_res, k_obj);

        let mut data = fs::read(temp_path).unwrap();
        if data.len() > 10 {
            data.truncate(10);
            fs::write(temp_path, &data).unwrap();
            let read_corrupt = read_binary_file(&path_k);
            assert!(read_corrupt.is_err());
            assert_eq!(
                read_corrupt.unwrap_err(),
                KonaError::Parse("Corrupt binary file".to_string())
            );
        }

        // Test defensive bounds check against malformed/huge length prefix
        // Tag 5 is IntList, followed by a huge length (e.g. 100,000,000)
        let corrupt_data = {
            let mut bytes = vec![5]; // tag 5
            bytes.extend_from_slice(&(100_000_000u32).to_le_bytes()); // len
            bytes
        };
        let mut corrupt_deserializer = KDeserializer::new(&corrupt_data);
        let corrupt_res = corrupt_deserializer.read_k();
        assert!(corrupt_res.is_err());
        assert_eq!(
            corrupt_res.unwrap_err(),
            KonaError::Parse("Corrupt binary file".to_string())
        );

        // Tag 5 followed by a length that exceeds MAX_ALLOC_LIMIT (e.g. 200,000,000)
        let limit_exceeded_data = {
            let mut bytes = vec![5]; // tag 5
            bytes.extend_from_slice(&(200_000_000u32).to_le_bytes()); // len
            bytes
        };
        let mut limit_deserializer = KDeserializer::new(&limit_exceeded_data);
        let limit_res = limit_deserializer.read_k();
        assert!(limit_res.is_err());
        assert_eq!(
            limit_res.unwrap_err(),
            KonaError::Parse("Header size exceeds limit".to_string())
        );

        let _ = fs::remove_file(temp_path);
    }

    #[test]
    fn test_dynamic_library_loading_disabled() {
        let path_k = K::symbol("mylib.so");
        let res = load_c_library(&path_k);
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err(),
            KonaError::FeatureDisabled(
                "Dynamic C library loading is disabled for safety".to_string()
            )
        );
    }

    #[test]
    fn test_counter_app_state_tracking() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        let init_expr = parse("cnt::0").unwrap();
        eval_words(&init_expr, &mut env, &ktree).unwrap();
        assert_eq!(ktree.lookup("cnt").unwrap(), K::int(0));

        let inc_expr = parse("inc:{cnt::cnt+1}").unwrap();
        eval_words(&inc_expr, &mut env, &ktree).unwrap();

        let run_inc = parse("inc[]").unwrap();
        eval_words(&run_inc, &mut env, &ktree).unwrap();
        assert_eq!(ktree.lookup("cnt").unwrap(), K::int(1));

        eval_words(&run_inc, &mut env, &ktree).unwrap();
        assert_eq!(ktree.lookup("cnt").unwrap(), K::int(2));
    }

    #[test]
    fn test_dict_indexing() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        let dict_expr = parse("d:`a`b!10 20").unwrap();
        eval_words(&dict_expr, &mut env, &ktree).unwrap();

        let index_a = parse("d[`a]").unwrap();
        let res_a = eval_words(&index_a, &mut env, &ktree).unwrap();
        assert_eq!(res_a, K::int(10));

        let index_b = parse("d[`b]").unwrap();
        let res_b = eval_words(&index_b, &mut env, &ktree).unwrap();
        assert_eq!(res_b, K::int(20));
    }

    #[test]
    fn test_proof_web_server() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        // Note: the original parser handles ~ as match/not. Let's register a handler that uses ~ (match) if supported.
        // Or we can just define: .m.h: {[path] "HTTP/1.1 200 OK\r\n\r\nONLINE"}
        let register_handler =
            parse(".m.h:: {[path] \"HTTP/1.1 200 OK\\r\\n\\r\\nONLINE\"}").unwrap();
        eval_words(&register_handler, &mut env, &ktree).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let ktree_clone = ktree.clone();
        std::thread::spawn(move || {
            for stream_res in listener.incoming() {
                if let Ok(stream) = stream_res {
                    let _ = handle_http_connection(stream, ktree_clone.clone());
                }
            }
        });

        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(b"GET /status HTTP/1.1\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("200 OK"));
        assert!(response.contains("ONLINE"));
    }

    #[test]
    fn test_ipc_server() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        // In Q/K, the sync handler is evaluated with the message. Let's register:
        // .m.g: {[msg] msg}
        let register_sync = parse(".m.g:: {[msg] msg}").unwrap();
        eval_words(&register_sync, &mut env, &ktree).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let ktree_clone = ktree.clone();
        std::thread::spawn(move || {
            for stream_res in listener.incoming() {
                if let Ok(stream) = stream_res {
                    let _ = handle_ipc_connection(stream, ktree_clone.clone());
                }
            }
        });

        let mut stream = TcpStream::connect(addr).unwrap();
        let payload = K::int(42);
        write_ipc_message(&mut stream, 1, &payload).unwrap();

        let (resp_type, resp_payload) = read_ipc_message(&mut stream).unwrap();
        assert_eq!(resp_type, 2);
        assert_eq!(resp_payload, K::int(42));
    }

    #[test]
    fn test_paren_lists_and_monadic_dot() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        // 1. Paren list evaluation (1;2;3)
        let expr1 = parse("(1;2;3)").unwrap();
        let res1 = eval_words(&expr1, &mut env, &ktree).unwrap();
        assert_eq!(res1, K::int_list(vec![1, 2, 3]));

        // 2. Monadic dot on a list of pairs to create a dictionary
        let expr2 = parse(".((`a;10;);(`b;20;))").unwrap();
        let res2 = eval_words(&expr2, &mut env, &ktree).unwrap();

        // 3. Dictionary empty bracket indexing d[]
        env.insert("d".to_string(), res2.clone());
        let expr3 = parse("d[]").unwrap();
        let res3 = eval_words(&expr3, &mut env, &ktree).unwrap();
        assert_eq!(res3, K::int_list(vec![10, 20]));

        // List empty bracket indexing l[]
        let expr4 = parse("(10 20)[]").unwrap();
        let res4 = eval_words(&expr4, &mut env, &ktree).unwrap();
        assert_eq!(res4, K::int_list(vec![10, 20]));

        // 4. Sequential statement evaluation with semicolons
        let expr5 = parse("a:10; b:a+5; b*2").unwrap();
        let res5 = eval_words(&expr5, &mut env, &ktree).unwrap();
        assert_eq!(res5, K::int(30));

        // 5. Adverb bracket application: {x+y}'[1 2; 3 4]
        let expr6 = parse("{x+y}'[1 2; 3 4]").unwrap();
        let res6 = eval_words(&expr6, &mut env, &ktree).unwrap();
        assert_eq!(res6, K::int_list(vec![4, 6]));

        // 6. Monadic type verb 4:x
        let expr7 = parse("4: 123").unwrap();
        let res7 = eval_words(&expr7, &mut env, &ktree).unwrap();
        assert_eq!(res7, K::int(1));

        let expr8 = parse("4: `x`y").unwrap();
        let res8 = eval_words(&expr8, &mut env, &ktree).unwrap();
        assert_eq!(res8, K::int(-4));

        // 7. Join over with separator: ","/("a";"b";"c")
        let expr9 = parse("\",\"/(\"a\";\"b\";\"c\")").unwrap();
        let res9 = eval_words(&expr9, &mut env, &ktree).unwrap();
        assert_eq!(res9, K::char_list(b"a,b,c".to_vec()));
    }

    #[test]
    fn test_jobj_debug() {
        let ktree = Arc::new(KTree::new());

        // Load json.k
        crate::load_and_run_script("json.k", &ktree).unwrap();

        // Define state
        let mut env = HashMap::new();
        let def_state = parse("state: {.((`count;0;);(`kind;\"proof\";))}").unwrap();
        eval_words(&def_state, &mut env, &ktree).unwrap();

        // Run jobj state[]
        let run_jobj = parse("jobj state[]").unwrap();
        let res = eval_words(&run_jobj, &mut env, &ktree).unwrap();
        assert!(res.is_list());
    }

    #[test]
    fn test_json_round_trip_conditional_index_assignment() {
        let ktree = Arc::new(KTree::new());
        crate::load_and_run_script("json.k", &ktree).unwrap();
        let mut env = HashMap::new();

        let conditional_assign = parse("d:3#0;i:1;inq:1;dep:5;d[i]: :[inq;-1;dep];d").unwrap();
        let res = eval_words(&conditional_assign, &mut env, &ktree).unwrap();
        assert_eq!(res, K::int_list(vec![0, -1, 0]));

        let round_trip =
            parse("d:.((`name;\"james\";);(`age;30;);(`scores;1 2 3;));j:jobj d;pjson j").unwrap();
        let res = eval_words(&round_trip, &mut env, &ktree).unwrap();
        let expected = create_dict(
            &K::symbol_list(vec![
                Arc::from("name"),
                Arc::from("age"),
                Arc::from("scores"),
            ]),
            &K::general_list(vec![
                K::char_list(b"james".to_vec()),
                K::int(30),
                K::int_list(vec![1, 2, 3]),
            ]),
        )
        .unwrap();
        assert_eq!(res, expected);
    }

    #[test]
    fn test_primitive_verbs_new() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        // 1. flip (monadic +)
        let expr1 = parse("+ (1 2; 3 4)").unwrap();
        let res1 = eval_words(&expr1, &mut env, &ktree).unwrap();
        assert_eq!(
            res1,
            K::general_list(vec![K::int_list(vec![1, 3]), K::int_list(vec![2, 4]),])
        );

        // 2. distinct (monadic ?)
        let expr2 = parse("? 1 2 1 3 2 4").unwrap();
        let res2 = eval_words(&expr2, &mut env, &ktree).unwrap();
        assert_eq!(res2, K::int_list(vec![1, 2, 3, 4]));

        // 3. find (dyadic ?)
        let expr3 = parse("10 20 30 ? 20").unwrap();
        let res3 = eval_words(&expr3, &mut env, &ktree).unwrap();
        assert_eq!(res3, K::int(1));

        let expr3b = parse("10 20 30 ? 40").unwrap();
        let res3b = eval_words(&expr3b, &mut env, &ktree).unwrap();
        assert_eq!(res3b, K::int(3));

        // 4. deep_index (dyadic .)
        let expr4 = parse("(10 20; 30 40) . 1 0").unwrap();
        let res4 = eval_words(&expr4, &mut env, &ktree).unwrap();
        assert_eq!(res4, K::int(30));

        // 5. rotate (dyadic !)
        let expr5 = parse("1 ! 10 20 30").unwrap();
        let res5 = eval_words(&expr5, &mut env, &ktree).unwrap();
        assert_eq!(res5, K::int_list(vec![20, 30, 10]));

        let expr5b = parse("-1 ! 10 20 30").unwrap();
        let res5b = eval_words(&expr5b, &mut env, &ktree).unwrap();
        assert_eq!(res5b, K::int_list(vec![30, 10, 20]));

        // 6. cut (dyadic _)
        let expr6 = parse("1 3 _ 10 20 30 40 50").unwrap();
        let res6 = eval_words(&expr6, &mut env, &ktree).unwrap();
        assert_eq!(
            res6,
            K::general_list(vec![K::int_list(vec![20, 30]), K::int_list(vec![40, 50]),])
        );

        // 7. floor (monadic _) and ceil
        let expr7 = parse("_ 3.14").unwrap();
        let res7 = eval_words(&expr7, &mut env, &ktree).unwrap();
        assert_eq!(res7, K::int(3));

        // 8. shape (monadic ^) and power (dyadic ^)
        let expr8_shape = parse("^ (1 2; 3 4)").unwrap();
        let res8_shape = eval_words(&expr8_shape, &mut env, &ktree).unwrap();
        assert_eq!(res8_shape, K::int_list(vec![2, 2]));

        let expr8_power = parse("2 ^ 3").unwrap();
        let res8_power = eval_words(&expr8_power, &mut env, &ktree).unwrap();
        assert_eq!(res8_power, K::float(8.0));

        // 8b. atom (monadic @) and format (monadic $)
        let expr_atom = parse("@ 42").unwrap();
        let res_atom = eval_words(&expr_atom, &mut env, &ktree).unwrap();
        assert_eq!(res_atom, K::int(1));

        let expr_format = parse("$ 1 2 3").unwrap();
        let res_format = eval_words(&expr_format, &mut env, &ktree).unwrap();
        assert_eq!(
            res_format,
            K::general_list(vec![
                K::char_list(b"1".to_vec()),
                K::char_list(b"2".to_vec()),
                K::char_list(b"3".to_vec()),
            ])
        );

        // 9. match (dyadic ~)
        let expr9 = parse("1 2 3 ~ 1 2 3").unwrap();
        let res9 = eval_words(&expr9, &mut env, &ktree).unwrap();
        assert_eq!(res9, K::int(1));

        let expr9b = parse("1 2 ~ 1 2 3").unwrap();
        let res9b = eval_words(&expr9b, &mut env, &ktree).unwrap();
        assert_eq!(res9b, K::int(0));

        // 10. negative indexing wrapping
        let expr10 = parse("(10 20 30)[-1]").unwrap();
        let res10 = eval_words(&expr10, &mut env, &ktree).unwrap();
        assert_eq!(res10, K::int(30));

        // 11. cast alignment
        let expr11 = parse("`$ \"a\"").unwrap();
        let res11 = eval_words(&expr11, &mut env, &ktree).unwrap();
        assert_eq!(res11, K::symbol("a"));

        let expr11b = parse("0 $ \"123\"").unwrap();
        let res11b = eval_words(&expr11b, &mut env, &ktree).unwrap();
        assert_eq!(res11b, K::int(123));

        let expr11c = parse("0.0 $ \"12.3\"").unwrap();
        let res11c = eval_words(&expr11c, &mut env, &ktree).unwrap();
        assert_eq!(res11c, K::float(12.3));
    }

    #[test]
    fn test_system_math_and_utilities() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        // 1. _abs, _ceil
        let expr1 = parse("_abs -42").unwrap();
        let res1 = eval_words(&expr1, &mut env, &ktree).unwrap();
        assert_eq!(res1, K::int(42));

        let expr2 = parse("_ceil 3.14").unwrap();
        let res2 = eval_words(&expr2, &mut env, &ktree).unwrap();
        assert_eq!(res2, K::int(4));

        // 2. _bin
        let expr3 = parse("10 20 30 _bin 25").unwrap();
        let res3 = eval_words(&expr3, &mut env, &ktree).unwrap();
        assert_eq!(res3, K::int(2));

        // 3. _in
        let expr4 = parse("20 _in 10 20 30").unwrap();
        let res4 = eval_words(&expr4, &mut env, &ktree).unwrap();
        assert_eq!(res4, K::int(1));

        let expr4b = parse("40 _in 10 20 30").unwrap();
        let res4b = eval_words(&expr4b, &mut env, &ktree).unwrap();
        assert_eq!(res4b, K::int(0));

        // 4. _val
        let expr5 = parse("_val \"123\"").unwrap();
        let res5 = eval_words(&expr5, &mut env, &ktree).unwrap();
        assert_eq!(res5, K::int(123));

        // Test _val and monadic . on symbol lookup
        ktree.assign("myvar".to_string(), K::int(999));
        let expr_val_sym = parse("_val `myvar").unwrap();
        let res_val_sym = eval_words(&expr_val_sym, &mut env, &ktree).unwrap();
        assert_eq!(res_val_sym, K::int(999));

        let expr_dot_sym = parse(". `myvar").unwrap();
        let res_dot_sym = eval_words(&expr_dot_sym, &mut env, &ktree).unwrap();
        assert_eq!(res_dot_sym, K::int(999));

        // Test system constants
        let expr_d = parse("_d").unwrap();
        let res_d = eval_words(&expr_d, &mut env, &ktree).unwrap();
        assert_eq!(res_d, K::symbol(".k"));

        let expr_t = parse("_t").unwrap();
        let res_t = eval_words(&expr_t, &mut env, &ktree).unwrap();
        assert!(res_t.as_int().is_some());

        let expr_big_t = parse("_T").unwrap();
        let res_big_t = eval_words(&expr_big_t, &mut env, &ktree).unwrap();
        assert!(res_big_t.as_float().is_some());

        let expr_n = parse("_n").unwrap();
        let res_n = eval_words(&expr_n, &mut env, &ktree).unwrap();
        assert!(res_n.is_nil());

        let expr_f = parse("f:{:[x<2;1;x*_f[x-1]]}; f 5").unwrap();
        let res_f = eval_words(&expr_f, &mut env, &ktree).unwrap();
        assert_eq!(res_f, K::int(120));

        let expr_h = parse("_h").unwrap();
        let res_h = eval_words(&expr_h, &mut env, &ktree).unwrap();
        assert!(res_h.as_symbol().is_some());

        let expr_u = parse("_u").unwrap();
        let res_u = eval_words(&expr_u, &mut env, &ktree).unwrap();
        assert!(res_u.as_symbol().is_some());

        // 5. _ssr (string search replace)
        let expr6 = parse("_ssr[\"hello world\";\"world\";\"kona\"]").unwrap();
        let res6 = eval_words(&expr6, &mut env, &ktree).unwrap();
        assert_eq!(res6, K::char_list(b"hello kona".to_vec()));
    }

    #[test]
    fn test_adverbs_new() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        // 1. each-right (/:)
        let expr1 = parse("10 +/: 1 2 3").unwrap();
        let res1 = eval_words(&expr1, &mut env, &ktree).unwrap();
        assert_eq!(res1, K::int_list(vec![11, 12, 13]));

        // 1b. each-right with atom RHS
        let expr1b = parse("10 20 +/: 3").unwrap();
        let res1b = eval_words(&expr1b, &mut env, &ktree).unwrap();
        assert_eq!(res1b, K::int_list(vec![13, 23]));

        // 2. each-left (\:)
        let expr2 = parse("1 2 3 +\\: 10").unwrap();
        let res2 = eval_words(&expr2, &mut env, &ktree).unwrap();
        assert_eq!(res2, K::int_list(vec![11, 12, 13]));

        // 2b. each-left with atom LHS
        let expr2b = parse("3 +\\: 10 20").unwrap();
        let res2b = eval_words(&expr2b, &mut env, &ktree).unwrap();
        assert_eq!(res2b, K::int_list(vec![13, 23]));

        // 3. each-pair ('): C Kona gives f[y[i];y[i-1]] for i>0, one item fewer than y
        let expr3 = parse("-': 10 20 35").unwrap();
        let res3 = eval_words(&expr3, &mut env, &ktree).unwrap();
        assert_eq!(res3, K::int_list(vec![10, 15]));

        // 3b. each-pair with LHS (dyadic): x itself goes in front of the pairs (tests.c: 10 -': 2 3 is 10 1)
        let expr3b = parse("10 -': 10 20 35").unwrap();
        let res3b = eval_words(&expr3b, &mut env, &ktree).unwrap();
        assert_eq!(res3b, K::int_list(vec![10, 10, 15]));

        // 3c. each-pair with atom RHS is f[y;y] (tests.c: +':2 is 4)
        let expr3c = parse("-': 10").unwrap();
        let res3c = eval_words(&expr3c, &mut env, &ktree).unwrap();
        assert_eq!(res3c, K::int(0));

        // 4. convergence `/`
        // halving until 0: {x / 2}/ 100 -> 0
        let expr4 = parse("{x % 2}/ 10").unwrap();
        let res4 = eval_words(&expr4, &mut env, &ktree).unwrap();
        assert_eq!(res4, K::int(0));

        // 4b. monadic over convergence on a list
        let expr4b = parse("{x * 0.5}/ 10 20").unwrap();
        let res4b = eval_words(&expr4b, &mut env, &ktree).unwrap();
        if let KData::FloatList(v) = &*res4b {
            assert!(v[0].abs() < 1e-13);
            assert!(v[1].abs() < 1e-13);
        } else {
            panic!("Expected FloatList");
        }

        // 5. while loop over `{cond} {body}/ x`
        // while x < 10, double: ({x < 10}; {x * 2})/ 1
        let expr5 = parse("({x < 10}; {x * 2})/ 1").unwrap();
        let res5 = eval_words(&expr5, &mut env, &ktree).unwrap();
        assert_eq!(res5, K::int(16));

        // 5b. dyadic while loop over: cond f/ y
        // while x > 10000, half: {x > 10000} {x * 0.5}/ 640640
        let expr5b = parse("{x > 10000} {x * 0.5}/ 640640").unwrap();
        let res5b = eval_words(&expr5b, &mut env, &ktree).unwrap();
        assert_eq!(res5b, K::float(5005.0));

        // 5c. dyadic while loop scan: cond f\ y
        let expr5c = parse("{x > 10000} {x * 0.5}\\ 640640.0").unwrap();
        let res5c = eval_words(&expr5c, &mut env, &ktree).unwrap();
        assert_eq!(
            res5c,
            K::float_list(vec![
                640640.0, 320320.0, 160160.0, 80080.0, 40040.0, 20020.0, 10010.0, 5005.0
            ])
        );

        // 6. dyadic each x f' y with two atoms
        let expr6 = parse("1 +' 2").unwrap();
        let res6 = eval_words(&expr6, &mut env, &ktree).unwrap();
        assert_eq!(res6, K::int(3));

        // 6b. dyadic each with list LHS and atom RHS
        let expr6b = parse("1 2 +' 3").unwrap();
        let res6b = eval_words(&expr6b, &mut env, &ktree).unwrap();
        assert_eq!(res6b, K::int_list(vec![4, 5]));

        let over_proj = parse("+/[1;]2 3 4").unwrap();
        let over_proj_res = eval_words(&over_proj, &mut env, &ktree).unwrap();
        assert_eq!(over_proj_res, K::int(10));

        let scan_proj = parse("+\\[1;]2 3 4").unwrap();
        let scan_proj_res = eval_words(&scan_proj, &mut env, &ktree).unwrap();
        assert_eq!(scan_proj_res, K::int_list(vec![1, 3, 6, 10]));

        let matrix_scan_atom = parse("m:3 4#!12;m\\2").unwrap();
        let matrix_scan_atom_res = eval_words(&matrix_scan_atom, &mut env, &ktree).unwrap();
        assert_eq!(matrix_scan_atom_res, K::int(2));

        let two_row_scan_atom = parse("a:(0 1;2 3);a\\1").unwrap();
        let two_row_scan_atom_res = eval_words(&two_row_scan_atom, &mut env, &ktree).unwrap();
        assert_eq!(two_row_scan_atom_res, K::int(1));

        let each_proj = parse("+'[1;]2 3 4").unwrap();
        let each_proj_res = eval_words(&each_proj, &mut env, &ktree).unwrap();
        assert_eq!(each_proj_res, K::int_list(vec![3, 4, 5]));
    }

    #[test]
    fn test_colon_runs_and_wsfull() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        let colon_expr = parse("5:(::::)").unwrap();
        let colon_res = eval_words(&colon_expr, &mut env, &ktree).unwrap();
        assert_eq!(colon_res, K::char_list(b"::::".to_vec()));

        let identity_expr = parse("(::)[1]").unwrap();
        let identity_res = eval_words(&identity_expr, &mut env, &ktree).unwrap();
        assert_eq!(identity_res, K::int(1));

        let ws_expr = parse("@[.:;\"!0I\";:]").unwrap();
        let ws_res = eval_words(&ws_expr, &mut env, &ktree).unwrap();
        assert_eq!(
            ws_res,
            K::general_list(vec![K::int(1), K::char_list(b"wsfull".to_vec())])
        );
    }

    #[test]
    fn test_control_flow_and_amend() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        // 1. while[...]
        let expr1 = parse("i:0; while[i<5; i:i+1]; i").unwrap();
        let res1 = eval_words(&expr1, &mut env, &ktree).unwrap();
        assert_eq!(res1, K::int(5));

        // 2. do[...]
        let expr2 = parse("val:0; do[3; val:val+10]; val").unwrap();
        let res2 = eval_words(&expr2, &mut env, &ktree).unwrap();
        assert_eq!(res2, K::int(30));

        // 3. if[...]
        let expr3 = parse("x:10; if[x>5; y:100; y:200]; y").unwrap();
        let res3 = eval_words(&expr3, &mut env, &ktree).unwrap();
        assert_eq!(res3, K::int(200));

        // 4. Amend triadic @[a;i;f] - a bare name returns the changed copy and leaves a alone (as in C Kona)
        let expr4 = parse("a:10 20 30; @[a;1;-]").unwrap();
        let res4 = eval_words(&expr4, &mut env, &ktree).unwrap();
        assert_eq!(res4, K::int_list(vec![10, -20, 30]));
        let a4 = eval_words(&parse("a").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(a4, K::int_list(vec![10, 20, 30]));

        // 5. Amend tetradic @[a;i;f;v] - same, copy back, a untouched
        let expr5 = parse("a:10 20 30; @[a;1;+;5]").unwrap();
        let res5 = eval_words(&expr5, &mut env, &ktree).unwrap();
        assert_eq!(res5, K::int_list(vec![10, 25, 30]));
        let a5 = eval_words(&parse("a").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(a5, K::int_list(vec![10, 20, 30]));

        // 5b. With a symbol, @[`a;i;f;v] changes a itself
        let _ = eval_words(&parse("@[`a;1;+;5]").unwrap(), &mut env, &ktree).unwrap();
        let a5b = eval_words(&parse("a").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(a5b, K::int_list(vec![10, 25, 30]));
    }

    #[test]
    fn test_new_system_verbs() {
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        // 1. Radix/Clock Base Conversion: _vs, _vsx, _sv
        // Unbounded radix
        let res = eval_words(&parse("2 _vs 0").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int_list(vec![]));
        let res = eval_words(&parse("2 _vsx 0").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int_list(vec![0]));
        let res = eval_words(&parse("2 _vs 11").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int_list(vec![1, 0, 1, 1]));
        let res = eval_words(&parse("2 _sv 1 0 1 1").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(11));

        // Bounded clock base conversion
        let res = eval_words(&parse("24 60 60 _vs 4000").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int_list(vec![1, 6, 40]));
        let res = eval_words(&parse("24 60 60 _sv 1 6 40").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(4000));
        let res = eval_words(&parse("0 100 _vs 20151212").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int_list(vec![201512, 12]));

        // 2. Deletion Verbs: _di, _dv, _dvl, _hat
        let res = eval_words(&parse("10 20 30 40 _di 1 3").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int_list(vec![10, 30]));
        let res = eval_words(&parse("10 20 30 20 40 _dv 20").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int_list(vec![10, 30, 40]));
        let res = eval_words(
            &parse("10 20 30 20 40 _dvl 20 40").unwrap(),
            &mut env,
            &ktree,
        )
        .unwrap();
        assert_eq!(res, K::int_list(vec![10, 30]));
        let res = eval_words(&parse("10 20 30 40 _hat 20 40").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int_list(vec![10, 30]));

        // Dictionary deletion and hat
        let res = eval_words(&parse("(`a`b!10 20) _di `a").unwrap(), &mut env, &ktree).unwrap();
        if let KData::Dictionary { keys, values } = &*res {
            assert_eq!(keys, &K::symbol_list(vec![Arc::from("b")]));
            assert_eq!(values, &K::int_list(vec![20]));
        } else {
            panic!("Expected dictionary");
        }

        // 3. Search & Membership: _lin, _binl
        let res = eval_words(&parse("1 2 5 _lin 2 3 4").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int_list(vec![0, 1, 0]));
        let res = eval_words(&parse("0 2 4 _binl 3").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(2));

        // 4. Matrix & Vector Algebra: _dot, _mul, _lsq, _inv
        let res = eval_words(&parse("1 2 3 _dot 4 5 6").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(32));

        let res = eval_words(
            &parse("(1 2; 3 4) _mul (2 0; 0 2)").unwrap(),
            &mut env,
            &ktree,
        )
        .unwrap();
        if let KData::GeneralList(rows) = &*res {
            assert_eq!(rows[0], K::int_list(vec![2, 4]));
            assert_eq!(rows[1], K::int_list(vec![6, 8]));
        } else {
            panic!("Expected general list of rows");
        }

        let res = eval_words(&parse("_inv (2.0 0.0; 0.0 2.0)").unwrap(), &mut env, &ktree).unwrap();
        if let KData::GeneralList(rows) = &*res {
            let col0 = rows[0].as_float_list().unwrap();
            let col1 = rows[1].as_float_list().unwrap();
            assert!((col0[0] - 0.5).abs() < 1e-6);
            assert!((col0[1] - 0.0).abs() < 1e-6);
            assert!((col1[0] - 0.0).abs() < 1e-6);
            assert!((col1[1] - 0.5).abs() < 1e-6);
        } else {
            panic!("Expected general list of rows");
        }

        // 5. Calendar & Time: _jd, _dj, _gtime, _ltime, _lt
        let res = eval_words(&parse("_jd 20100101").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(-9131));
        let res = eval_words(&parse("_dj -9131").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(20100101));

        let res = eval_words(&parse("_gtime 0").unwrap(), &mut env, &ktree).unwrap();
        if let KData::GeneralList(parts) = &*res {
            assert_eq!(parts[0], K::int(20350101));
            assert_eq!(parts[1], K::int(0));
        } else {
            panic!("Expected general list");
        }

        // 6. Glob matching: _sm
        let res = eval_words(
            &parse("(\"a.c\";\"bc\") _sm \"*.[ch]\"").unwrap(),
            &mut env,
            &ktree,
        )
        .unwrap();
        assert_eq!(res, K::int_list(vec![1, 0]));

        // 7. Hash table optimization: (x; _hash x) ? y
        let res = eval_words(
            &parse("x:10 20 30; (x; _hash x) ? 20").unwrap(),
            &mut env,
            &ktree,
        )
        .unwrap();
        assert_eq!(res, K::int(1));

        // 8. Serializing & Deserializing: _bd and _db
        let res = eval_words(&parse("_db _bd 42").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(42));
        let res = eval_words(&parse("_db _bd 1 2 3").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int_list(vec![1, 2, 3]));

        // 9. Ceiling alias: _ceiling
        let res = eval_words(&parse("_ceiling 2.3").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(3));

        // 10. DNS lookup: _host
        let res = eval_words(&parse("_host `localhost").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(2130706433));

        let res = eval_words(&parse("_host 2130706433").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::symbol("localhost"));

        // 11. Environment variables set/get: _setenv and _getenv
        let _ = eval_words(
            &parse("_setenv[`MY_VAR; \"MY_VAL\"]").unwrap(),
            &mut env,
            &ktree,
        )
        .unwrap();
        let res = eval_words(&parse("_getenv `MY_VAR").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::char_list(b"MY_VAL".to_vec()));
    }

    #[test]
    fn test_deep_amend_and_trap() {
        use crate::eval::eval_words;
        use crate::parser::parse;
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        // 1. Monadic Trap Success
        let res = eval_words(&parse("@[{x+1}; 2; 42]").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(3));

        // 2. Monadic Trap Failure (caught, returns constant)
        let res = eval_words(&parse("@[{x+1}; \"a\"; 42]").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(42));

        // 3. Monadic Trap Failure (caught, evaluates error function)
        let res = eval_words(&parse("@[{x+1}; \"a\"; {x}]").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::char_list(b"type".to_vec()));

        // 4. Dyadic/N-adic Trap Success
        let res = eval_words(&parse(".[+; 2 3; 42]").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(5));

        // 5. Dyadic/N-adic Trap Failure
        let res = eval_words(&parse(".[+; (2; \"a\"); {x}]").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::char_list(b"type".to_vec()));

        // 6. Deep Indexing / Amend. Index 0 1 is 2; 2 + 10 = 12. A bare name returns the changed copy
        // and leaves x alone (C Kona's \. help: ".[a;();+;1] returns a+1 but does not affect a").
        let original = K::general_list(vec![K::int_list(vec![1, 2]), K::int_list(vec![3, 4])]);
        let _ = eval_words(&parse("x: (1 2; 3 4)").unwrap(), &mut env, &ktree).unwrap();
        let res = eval_words(&parse(".[x; 0 1; +; 10]").unwrap(), &mut env, &ktree).unwrap();
        let expected = K::general_list(vec![K::int_list(vec![1, 12]), K::int_list(vec![3, 4])]);
        assert_eq!(res, expected);
        assert_eq!(ktree.lookup("x").unwrap(), original);

        // 6b. With a symbol, .[`x; ...] changes x itself
        let _ = eval_words(&parse(".[`x; 0 1; +; 10]").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(ktree.lookup("x").unwrap(), expected);

        // 7. Deep Indexing / Amend with replacement (:), in place via the symbol
        let _ = eval_words(&parse(".[`x; 1 0; :; 99]").unwrap(), &mut env, &ktree).unwrap();
        let expected2 = K::general_list(vec![K::int_list(vec![1, 12]), K::int_list(vec![99, 4])]);
        assert_eq!(ktree.lookup("x").unwrap(), expected2);
    }

    #[test]
    fn test_socket_io() {
        use crate::eval::eval_words;
        use crate::parser::parse;
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut header = [0; 5];
                if stream.read_exact(&mut header).is_ok() {
                    let payload_len = u32::from_le_bytes(header[1..5].try_into().unwrap()) as usize;
                    let mut payload = vec![0; payload_len];
                    if stream.read_exact(&mut payload).is_ok() {
                        let resp = K::int(100);
                        let serialized = crate::io::serialize_k(&resp);
                        let mut resp_header = [0; 5];
                        resp_header[0] = 2;
                        resp_header[1..5].copy_from_slice(&(serialized.len() as u32).to_le_bytes());
                        let _ = stream.write_all(&resp_header);
                        let _ = stream.write_all(&serialized);
                        let _ = stream.flush();
                    }
                }
            }
        });

        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        let connect_cmd = format!("h: 3: (`; {})", port);
        let h_val = eval_words(&parse(&connect_cmd).unwrap(), &mut env, &ktree).unwrap();
        let h = h_val.as_int().unwrap();
        assert!(h >= 3);

        let send_cmd = "h 4: 42";
        let res = eval_words(&parse(send_cmd).unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res, K::int(100));

        let close_cmd = "3: h";
        let close_res = eval_words(&parse(close_cmd).unwrap(), &mut env, &ktree).unwrap();
        assert!(close_res.is_nil());
    }

    #[test]
    fn test_missing_io_verbs() {
        use crate::eval::eval_words;
        use crate::parser::parse;

        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        // 1. Test 6: monadic and dyadic
        // Write raw bytes using dyadic 6:
        let _ = eval_words(
            &parse("\"test_temp_raw.txt\" 6: \"hello raw bytes\"").unwrap(),
            &mut env,
            &ktree,
        )
        .unwrap();
        // Read raw bytes using monadic 6:
        let res = eval_words(
            &parse("6: \"test_temp_raw.txt\"").unwrap(),
            &mut env,
            &ktree,
        )
        .unwrap();
        assert_eq!(res, K::char_list(b"hello raw bytes".to_vec()));

        // 2. Test append to raw file using enlisted file path
        let _ = eval_words(
            &parse("(,\"test_temp_raw.txt\") 6: \" appended\"").unwrap(),
            &mut env,
            &ktree,
        )
        .unwrap();
        let res2 = eval_words(
            &parse("6: \"test_temp_raw.txt\"").unwrap(),
            &mut env,
            &ktree,
        )
        .unwrap();
        assert_eq!(res2, K::char_list(b"hello raw bytes appended".to_vec()));

        // Clean up temp file
        let _ = std::fs::remove_file("test_temp_raw.txt");

        // 3. Test 5: dyadic append binary file
        // First append creates the file because it doesn't exist
        let _ = eval_words(
            &parse("\"test_temp_bin.k\" 5: 10 20 30").unwrap(),
            &mut env,
            &ktree,
        )
        .unwrap();
        // Read it back
        let res_bin =
            eval_words(&parse("1: \"test_temp_bin.k\"").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res_bin, K::int_list(vec![10, 20, 30]));

        // Append single atom
        let new_len1 = eval_words(
            &parse("\"test_temp_bin.k\" 5: 40").unwrap(),
            &mut env,
            &ktree,
        )
        .unwrap();
        assert_eq!(new_len1, K::int(4));
        let res_bin2 =
            eval_words(&parse("1: \"test_temp_bin.k\"").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res_bin2, K::int_list(vec![10, 20, 30, 40]));

        // Append a list
        let new_len2 = eval_words(
            &parse("\"test_temp_bin.k\" 5: 50 60").unwrap(),
            &mut env,
            &ktree,
        )
        .unwrap();
        assert_eq!(new_len2, K::int(6));
        let res_bin3 =
            eval_words(&parse("1: \"test_temp_bin.k\"").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res_bin3, K::int_list(vec![10, 20, 30, 40, 50, 60]));

        // Clean up temp file
        let _ = std::fs::remove_file("test_temp_bin.k");
    }

    #[test]
    fn test_list_grading_comparison() {
        use crate::parser::parse;
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        // Test grading list of lists
        let expr = parse("< (1 3; 1 2)").unwrap();
        let res = eval_words(&expr, &mut env, &ktree).unwrap();
        // Since (1 2) is less than (1 3) lexicographically, grade up should put indices as 1 0
        assert_eq!(res, K::int_list(vec![1, 0]));

        // Test grading mixed lists element-by-element
        let expr2 = parse("< (1 2.0; 1.0 1.0)").unwrap();
        let res2 = eval_words(&expr2, &mut env, &ktree).unwrap();
        assert_eq!(res2, K::int_list(vec![1, 0]));
    }

    #[test]
    fn test_amend_over_debugging() {
        use crate::parser::parse;
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();
        let expr = parse("@/[,1;0 0 0;+;1 2 3]").unwrap();
        let res = eval_words(&expr, &mut env, &ktree);
        println!("DEBUG AMEND OVER: {:?}", res);
        assert!(res.is_ok());
    }

    #[test]
    fn test_lookup_a_and_p() {
        use crate::parser::parse;
        let ktree = Arc::new(KTree::new());
        let mut env = HashMap::new();

        // Check defaults first
        let res_a = eval_words(&parse("_a").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res_a, K::symbol_list(vec![]));

        let res_p = eval_words(&parse("_p").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res_p, K::int(7)); // default PRINT_PRECISION/port value from KTree::new()

        // Modify in ktree and lookup again
        ktree.assign(
            "_a".to_string(),
            K::symbol_list(vec![Arc::from("arg1"), Arc::from("arg2")]),
        );
        ktree.assign("_p".to_string(), K::int(1234));

        let res_a2 = eval_words(&parse("_a").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(
            res_a2,
            K::symbol_list(vec![Arc::from("arg1"), Arc::from("arg2")])
        );

        let res_p2 = eval_words(&parse("_p").unwrap(), &mut env, &ktree).unwrap();
        assert_eq!(res_p2, K::int(1234));

        // _p sets the process-wide print precision; put it back or the C-suite test running in
        // parallel formats $ 1.2e-34 with 1234 digits
        ktree.assign("_p".to_string(), K::int(7));
    }

    #[test]
    fn test_count_braces_comments() {
        assert_eq!(crate::count_braces("x: 1 / comment with { brace"), 0);
        assert_eq!(crate::count_braces(" { x: 1 } / comment with } brace"), 0);
        assert_eq!(crate::count_braces(" { x: 1 }"), 0);
        assert_eq!(crate::count_braces(" { { x: 1 } "), 1);
        assert_eq!(
            crate::count_braces("x: 1/2 / division then comment { brace"),
            0
        );
        assert_eq!(crate::count_braces("/ comment with { brace"), 0);
    }
}
