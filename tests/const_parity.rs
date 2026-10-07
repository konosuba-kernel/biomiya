//! Ordinary and instantiated generic constant blocks obey the same Core rules.
use biomiya::{check, source::FileId, syntax};
#[test]
fn ordinary_and_dependent_constants_have_normative_parity() {
    let cases: &[(&str, &str, Option<&str>)] = &[
        ("bool_bitnot", "~(K==4_usize)", Some("E_TYPE")),
        (
            "static_assert_int",
            "{static_assert(K);1_usize}",
            Some("E_TYPE"),
        ),
        ("int_bitnot", "~(K as u8)", None),
        ("int_neg", "-(K as i8)", None),
        ("bool_cast", "(K==4_usize) as u8", Some("E_TYPE")),
        ("int_add_overflow", "(K as u8)+252_u8", Some("E_CONST")),
        (
            "signed_neg_overflow",
            "-((K as i8)-%132_i8)",
            Some("E_TYPE"),
        ),
        ("wrapped_add", "(K as u8)+%252_u8", None),
        (
            "static_assert_valid",
            "{static_assert(K==4_usize);1_usize}",
            None,
        ),
        ("checked_add", "(K as u8)+?252_u8", None),
        ("checked_sub", "(K as u8)-?5_u8", None),
        ("checked_mul", "(K as u8)*?64_u8", None),
        ("cast_unsigned", "(K-5_usize) as u8", Some("E_CONST")),
        (
            "checked_cast",
            "checked_cast::<i8>((K as u8)+%124_u8)",
            None,
        ),
        (
            "static_assert_false",
            "{static_assert(K!=4_usize);1_usize}",
            Some("E_CONST"),
        ),
        ("shift_negative", "(K as i8)<<-1_i8", Some("E_CONST")),
        ("shift_width", "(K as u8)<<8_u8", Some("E_CONST")),
        ("div_zero", "K/0_usize", Some("E_CONST")),
        ("div_min", "((K as i8)-%132_i8)/-1_i8", Some("E_TYPE")),
        ("remainder_min", "((K as i8)-%132_i8)%-1_i8", Some("E_TYPE")),
        ("int_logic", "K&&1_usize", Some("E_TYPE")),
        ("bool_arithmetic", "(K==4_usize)+true", Some("E_TYPE")),
        ("typed_if", "if K==4_usize{7_u8}else{8_u8}", None),
        (
            "dead_division",
            "if K==4_usize{1_usize}else{1_usize/0_usize}",
            None,
        ),
        (
            "dead_panic",
            "if K==4_usize{1_usize}else{panic(\"x\")}",
            None,
        ),
        (
            "dead_call",
            "if K==4_usize{1_usize}else{runtime()}",
            Some("E_CONST"),
        ),
        (
            "dead_index",
            "if K==4_usize{1_usize}else{[1_usize][2_usize]}",
            None,
        ),
        ("tuple_copy_field", "((K as u8),2_u8).0", None),
        ("noncopy_struct_field", "S{x:K}.x", Some("E_CONST")),
        ("noncopy_tuple_field", "(S{x:K},3_usize).1", Some("E_CONST")),
        ("array_index", "[K,2_usize][0_usize]", None),
        (
            "option_match",
            "match Some(K){Some(v)=>v,None=>0_usize}",
            None,
        ),
        (
            "checked_match",
            "match (K as u8)+?252_u8{Some(v)=>v as usize,None=>9_usize}",
            None,
        ),
        (
            "nested_bool_condition",
            "if !(K!=4_usize){true}else{false}",
            None,
        ),
        (
            "refutable_let",
            "{let Some(n)=Some(K);n}",
            Some("E_PATTERN"),
        ),
        (
            "refutable_let_tuple",
            "{let (Some(n),m)=(Some(K),1_usize);n+m}",
            Some("E_PATTERN"),
        ),
        ("duplicate_let", "{let n=K;let n=K;n}", Some("E_NAME")),
        ("duplicate_pattern", "{let (n,n)=(K,K);n}", Some("E_NAME")),
        (
            "signed_min_valid",
            "-((K as i8)-%(-124_i8))",
            Some("E_CONST"),
        ),
        (
            "div_min_valid",
            "((K as i8)-%(-124_i8))/-1_i8",
            Some("E_CONST"),
        ),
    ];
    assert_eq!(cases.len(), 40);
    let mut failures = vec![];
    for &(name, expression, expected) in cases {
        let common = "struct S{x:usize} fn runtime()->usize{9}\n";
        let ordinary = format!(
            "{common}const K:usize=4; const N:usize={{let v={expression};4_usize}}; fn test()->i32{{N as i32}}"
        );
        let dependent = expression.replace("K", "sizeof(T)");
        let generic = format!(
            "{common}struct B<T>{{bytes:[{{let v={dependent};sizeof(T)}}]u8}} fn test()->i32{{sizeof(B<u32>) as i32}}"
        );
        for (context, source) in [("ordinary", ordinary), ("generic", generic)] {
            let result = syntax::parse(FileId(0), &source).and_then(|file| check::check(&[file]));
            let actual = result.err();
            if actual.as_ref().map(|d| d.code) != expected {
                failures.push(format!(
                    "{name} ({context}): expected {expected:?}, got {actual:?}\n{source}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
