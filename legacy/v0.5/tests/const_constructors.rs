//! Constructor checks apply equally in ordinary and dependent constant blocks.
use biomiya::{check, diagnostic::Result, hir::Program, source::FileId, syntax};
fn source_check(sources: &[String]) -> Result<Program> {
    let files = sources
        .iter()
        .enumerate()
        .map(|(i, s)| syntax::parse(FileId(i as u32), s))
        .collect::<Result<Vec<_>>>()?;
    check::check(&files)
}
#[test]
fn constructors_preserve_field_and_generic_contracts() {
    let declarations = "struct S{x:usize,y:usize}struct Small{x:u8}struct G<U>{x:U}struct N(usize);enum E{Unit,Payload(usize),Record{x:usize,y:usize}}enum GE<U>{Unit,Payload(U),Record{x:U}}";
    let cases: &[(&str, &str, Option<&str>)] = &[
        ("complete", "let s=S{x:K,y:1_usize};", None),
        ("reordered", "let s=S{y:1_usize,x:K};", None),
        ("missing", "let s=S{x:K};", Some("E_TYPE")),
        ("duplicate", "let s=S{x:K,x:K};", Some("E_TYPE")),
        ("unknown", "let s=S{x:K,z:K};", Some("E_TYPE")),
        ("extra", "let s=S{x:K,y:K,z:K};", Some("E_TYPE")),
        ("wrong_width", "let s=S{x:4_u8,y:K};", Some("E_TYPE")),
        ("wrong_shape", "let s=S{x:(K,K),y:K};", Some("E_TYPE")),
        ("expected_literal", "let s=Small{x:1};", None),
        (
            "expected_literal_range",
            "let s=Small{x:256};",
            Some("E_TYPE"),
        ),
        ("explicit_generic", "let s=G::<usize>{x:K};", None),
        ("explicit_generic_literal", "let s=G::<u8>{x:1};", None),
        (
            "explicit_generic_mismatch",
            "let s=G::<u8>{x:K};",
            Some("E_TYPE"),
        ),
        (
            "explicit_generic_arity",
            "let s=G::<u8,u32>{x:1};",
            Some("E_TYPE"),
        ),
        (
            "explicit_generic_context",
            "let s:G<u8>=G::<usize>{x:K};",
            Some("E_TYPE"),
        ),
        ("newtype", "let s=N(K);", None),
        ("newtype_missing", "let s=N();", Some("E_TYPE")),
        ("newtype_extra", "let s=N(K,K);", Some("E_TYPE")),
        ("enum_unit", "let s=E::Unit;", None),
        ("enum_unit_payload", "let s=E::Unit(K);", Some("E_TYPE")),
        ("enum_payload", "let s=E::Payload(K);", None),
        (
            "enum_payload_missing",
            "let s=E::Payload();",
            Some("E_TYPE"),
        ),
        (
            "enum_payload_extra",
            "let s=E::Payload(K,K);",
            Some("E_TYPE"),
        ),
        ("enum_record", "let s=E::Record{x:K,y:1};", None),
        (
            "enum_record_missing",
            "let s=E::Record{x:K};",
            Some("E_TYPE"),
        ),
        (
            "enum_record_duplicate",
            "let s=E::Record{x:K,x:K};",
            Some("E_TYPE"),
        ),
        (
            "enum_record_wrong_width",
            "let s=E::Record{x:4_u8,y:K};",
            Some("E_TYPE"),
        ),
        (
            "generic_enum_payload",
            "let s=GE::<usize>::Payload(K);",
            None,
        ),
        (
            "generic_enum_payload_mismatch",
            "let s=GE::<u8>::Payload(K);",
            Some("E_TYPE"),
        ),
        ("generic_enum_record", "let s=GE::<u8>::Record{x:1};", None),
    ];
    let mut failures = vec![];
    for &(name, statements, expected) in cases {
        let ordinary = format!(
            "{declarations}const K:usize=4;const C:usize={{{statements}4_usize}};fn test()->i32{{C as i32}}"
        );
        let statements = statements.replace("K", "sizeof(T)");
        let generic = format!(
            "{declarations}struct B<T>{{bytes:[{{{statements}sizeof(T)}}]u8}}fn test()->i32{{sizeof(B<u32>) as i32}}"
        );
        for (context, source) in [("ordinary", ordinary), ("generic", generic)] {
            let actual = source_check(std::slice::from_ref(&source)).err();
            if actual.as_ref().map(|d| d.code) != expected {
                failures.push(format!(
                    "{name} ({context}): expected {expected:?}, got {actual:?}\n{source}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
#[test]
fn private_fields_remain_private_in_constant_construction_projection_and_layout() {
    let owner =
        "module a; @copy pub struct S{x:usize,pub y:usize} pub const V:S=S{x:4_usize,y:3_usize};"
            .to_owned();
    let cases = [
        ("construct", "let s=a::S{x:K,y:1_usize};", Some("E_NAME")),
        ("project", "let n=a::V.x;", Some("E_NAME")),
        ("offsetof", "let n=offsetof(a::S,x);", Some("E_NAME")),
        ("public_project", "let n=a::V.y;", None),
        ("public_offsetof", "let n=offsetof(a::S,y);", None),
    ];
    let mut failures = vec![];
    for (name, statements, expected) in cases {
        let ordinary = format!(
            "module b;import a;const K:usize=4;const C:usize={{{statements}4_usize}};fn test()->i32{{C as i32}}"
        );
        let statements = statements.replace("K", "sizeof(T)");
        let generic = format!(
            "module b;import a;struct B<T>{{bytes:[{{{statements}sizeof(T)}}]u8}}fn test()->i32{{sizeof(B<u32>) as i32}}"
        );
        for (context, source) in [("ordinary", ordinary), ("generic", generic)] {
            let actual = source_check(&[owner.clone(), source.clone()]).err();
            if actual.as_ref().map(|d| d.code) != expected {
                failures.push(format!(
                    "{name} ({context}): expected {expected:?}, got {actual:?}\n{source}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn deferred_offsetof_retains_definition_module_visibility() {
    let hidden = "module a;@copy pub struct S{x:usize,pub y:usize}";
    let consumer = "module b;import a;struct B<T>{bytes:[offsetof(T,x)]u8}fn test()->i32{sizeof(B<a::S>) as i32}";
    assert_eq!(
        source_check(&[hidden.into(), consumer.into()])
            .unwrap_err()
            .code,
        "E_NAME"
    );
    let owner =
        "module a;@copy pub struct S{x:usize,pub y:usize}pub struct B<T>{bytes:[offsetof(T,x)]u8}";
    let consumer = "module b;import a;fn test()->i32{sizeof(a::B<a::S>) as i32}";
    assert!(source_check(&[owner.into(), consumer.into()]).is_ok());
}

#[test]
fn deferred_dependencies_include_queries_nested_in_type_arguments_and_annotations() {
    for statement in [
        "let a=MaybeUninit::<[sizeof(B<T>)]u8>::uninit();",
        "let a:MaybeUninit<[sizeof(B<T>)]u8>=MaybeUninit::uninit();",
    ] {
        let source = format!(
            "struct B<T>{{bytes:[if true{{1_usize}}else{{{statement}1_usize}}]u8}}fn test()->i32{{sizeof(B<u32>) as i32}}"
        );
        assert_eq!(source_check(&[source]).unwrap_err().code, "E_LAYOUT");
    }
    let source = "fn f<T>(){}struct B<T>{bytes:[{let pointer=f::<*B<T>>;1_usize}]u8}fn test()->i32{sizeof(B<u32>) as i32}";
    assert!(source_check(&[source.into()]).is_ok());
}

#[test]
fn by_value_constant_constructors_participate_in_declaration_dependencies() {
    for source in [
        "struct B<T>{bytes:[{let value=B::<(T,T)>{bytes:[]};1_usize}]u8}fn test()->i32{sizeof(B<u32>) as i32}",
        "struct B<T>{bytes:[if true{1_usize}else{let value=B::<(T,T)>{bytes:[]};1_usize}]u8}fn test()->i32{sizeof(B<u32>) as i32}",
    ] {
        assert_eq!(source_check(&[source.into()]).unwrap_err().code, "E_LAYOUT");
    }
    let source = "@copy struct Wrap<T>{value:T}struct B<T>{bytes:[{let value=Wrap::<Wrap<usize>>{value:Wrap::<usize>{value:sizeof(T)}};1_usize}]u8}fn test()->i32{sizeof(B<u32>) as i32}";
    assert!(source_check(&[source.into()]).is_ok());
}

#[test]
fn nested_length_queries_replay_concrete_lexical_prefixes() {
    for source in [
        "struct B<T>{x:[{let n=sizeof(T);sizeof([n]u8)}]u8}static_assert(sizeof(B<u32>)==4_usize);",
        "struct B<T>{x:[{let n:T=258 as T;sizeof([n as usize]u8)}]u8}static_assert(sizeof(B<u8>)==2_usize);static_assert(sizeof(B<u16>)==258_usize);",
    ] {
        assert!(source_check(&[source.into()]).is_ok(), "{source}");
    }
    let source = "struct B<T>{x:[{let n:T=258 as T;sizeof([n as usize]u8)}]u8}fn test()->i32{sizeof(B<bool>) as i32}";
    let diagnostic = source_check(&[source.into()]).unwrap_err();
    assert_eq!(diagnostic.code, "E_TYPE");
    assert!(!diagnostic.notes.is_empty());
}
