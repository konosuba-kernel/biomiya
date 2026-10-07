use biomiya::{
    diagnostic::Diagnostic,
    hir, lower,
    source::FileId,
    syntax, target,
    types::{TyId, TypeContext, TypeKind},
};

fn canonical_type(types: &TypeContext, ty: TyId) -> String {
    match types.kind(ty) {
        TypeKind::Unit => "()".into(),
        TypeKind::Never => "!".into(),
        TypeKind::Bool => "bool".into(),
        TypeKind::Int(integer) => format!("{integer:?}").to_lowercase(),
        TypeKind::Param(index) => format!("Param({index})"),
        TypeKind::Tuple(fields) => format!(
            "({})",
            fields
                .iter()
                .map(|ty| canonical_type(types, *ty))
                .collect::<Vec<_>>()
                .join(",")
        ),
        TypeKind::Option(inner) => format!("Option<{}>", canonical_type(types, *inner)),
        TypeKind::Function(signature) => format!(
            "fn({})->{}",
            signature
                .params
                .iter()
                .map(|ty| canonical_type(types, *ty))
                .collect::<Vec<_>>()
                .join(","),
            canonical_type(types, signature.result)
        ),
        other => panic!("unsupported snapshot type {other:?}"),
    }
}

fn checked(source: &str) -> Result<hir::Program, Diagnostic> {
    let file = syntax::parse(FileId(0), source)?;
    core_b_prototype::check::check(&[file])
}

#[test]
fn shift_count_only_direct_literal_gets_left_type() {
    for op in ["<<", ">>"] {
        for count in ["(1+1)", "{1+1}", "count"] {
            rejects(
                &format!("fn test(){{let count=2;let x:u8=8_u8 {op} {count};}}"),
                "E_TYPE",
            );
        }
        for count in ["2", "(1_u8+1_u8)", "{1_u8+1_u8}"] {
            accepts(&format!("fn test(){{let x:u8=8_u8 {op} {count};}}"));
        }
    }
    accepts("fn test(){let x:u8=1+2;}");
}

#[test]
fn bindings_reject_same_scope_duplicates_but_allow_nested_shadowing() {
    rejects("fn test(){let x=1;let x=2;}", "E_NAME");
    rejects("fn unused<T>(t:T){let x=1;let x=2;}", "E_NAME");
    accepts("fn test()->i32{let x=1;let y={let x=2;x};x+y}");
}
fn accepts(source: &str) -> hir::Program {
    checked(source).unwrap_or_else(|error| panic!("{source}\n{error:?}"))
}
fn rejects(source: &str, code: &str) -> Diagnostic {
    let error = checked(source).expect_err(source);
    assert_eq!(error.code, code, "{source}: {error:?}");
    assert!(error.span.end >= error.span.start);
    error
}

#[test]
fn immediate_defaults_and_explicit_parameters() {
    rejects(
        "fn consume(x:u64){} fn test(){let x=1;consume(x);}",
        "E_TYPE",
    );
    let p = accepts("fn consume(x:u64){} fn test(){let x:u64=1;consume(x);}");
    let test = p.functions.iter().find(|f| f.name == "test").unwrap();
    assert_eq!(
        p.types.kind(test.locals[0].ty),
        &TypeKind::Int(biomiya::types::Integer::U64)
    );
    rejects(
        "fn identity<T>(x:T)->T{x} fn test(){let x=1+identity::<u64>(2_u64);}",
        "E_TYPE",
    );
    accepts("fn identity<T>(x:T)->T{x} fn test(){let x=1_u64+identity::<u64>(2_u64);}");
    accepts("fn test()->u64{1+2}");
    rejects("fn test()->bool{1==2_u8}", "E_TYPE");
    accepts("fn test()->bool{1_u8==2_u8}");
}

#[test]
fn generic_paths_require_complete_explicit_arguments_even_with_expected_result() {
    for source in [
        "fn id<T>(x:T)->T{x} fn test()->u64{id(1)}",
        "fn choose<A,B>(a:A,b:B)->A{a} fn test()->i32{choose::<i32>(1,2)}",
        "fn id<T>(x:T)->T{x} fn test(){let f:fn(u64)->u64=id;}",
        "fn id<T>(x:T)->T{x} fn test()->u64{id::<u32>(1)}",
        "fn f()->i32{1} fn test()->i32{f::<u32>()}",
    ] {
        rejects(source, "E_TYPE");
    }
    accepts("fn id<T>(x:T)->T{x} fn test()->u64{id::<u64>(1)}");
    accepts("fn id<T>(x:T)->T{x} fn test()->u64{let f:fn(u64)->u64=id::<u64>;f(1)}");
}

#[test]
fn tuple_projections_and_branches_have_no_peer_feedback() {
    for branches in ["if true{1}else{2_u8}", "if true{2_u8}else{1}"] {
        rejects(&format!("fn test(){{let value={branches};}}"), "E_TYPE");
        accepts(&format!("fn test(){{let value:u8={branches};}}"));
    }
    rejects(
        "fn test(){let pair=if true{(1_u8,7)}else{(2,3_u8)};}",
        "E_TYPE",
    );
    accepts("fn test(){let pair:(u8,u8)=if true{(1,7)}else{(2,3)};}");
    accepts("fn consume(x:u8)->u8{x} fn test()->u8{let pair:(u8,u8)=(1,2);consume(pair.0)}");
    rejects("fn test()->u64{let pair=(1,2);pair.0}", "E_TYPE");
    accepts("fn test()->u8{if true{return 7;}else{3}}");
    accepts("fn test()->i32{return 7;}");
    rejects("fn test()->i32{return;}", "E_TYPE");
}

#[test]
fn constructors_use_only_known_concrete_nominal_expectations() {
    accepts("fn test(){let p:Option<u32>=Some(3);let n:Option<u32>=None;}");
    accepts("fn test(){let p=Option::<u32>::Some(3);let n=Option::<u32>::None;}");
    accepts("fn test()->Option<u32>{if true{Some(3)}else{None}}");
    accepts("fn f<T>(x:T)->Option<T>{Option::<T>::Some(x)} fn test()->Option<i32>{f::<i32>(1)}");
    accepts("fn Some(x:i32)->i32{x} fn test()->i32{Some(3)}");
    for source in [
        "fn test(){let p=Some(3_u32);}",
        "fn test(){let p=None;}",
        "fn test(){let p:u32=Some(3);}",
        "fn test(){let p:Option<u32>=Option::<u8>::Some(3);}",
        "fn f<T>(x:T)->Option<T>{Some(x)}",
        "fn test(){let p:Option<u32>=Some();}",
    ] {
        rejects(source, "E_TYPE");
    }
}

#[test]
fn templates_are_checked_even_without_calls() {
    rejects("fn unused<T>(){let x:u8=true;} fn test(){}", "E_TYPE");
    rejects("fn unused<T>(){missing;} fn test(){}", "E_NAME");
    rejects(
        "fn id<T>(x:T)->T{x} fn unused<U>(x:U)->U{id(x)} fn test(){}",
        "E_TYPE",
    );
    rejects(
        "fn unused<T>(x:T)->T{x+x} fn test(){}",
        "E_CORE_B_UNSUPPORTED",
    );
    rejects(
        "fn unused<T>(x:T){let y=x.0;} fn test(){}",
        "E_CORE_B_UNSUPPORTED",
    );
    accepts("fn forward<T>(x:T)->(T,T){(x,x)} fn test()->i32{forward::<i32>(3).0}");
    let p = accepts("fn forward<T>(x:T)->T{x} fn unused<U>(x:U)->U{forward::<U>(x)} fn test(){}");
    assert!(p.functions.iter().all(|f| f.type_arguments.is_empty()));
}

#[test]
fn instance_recursion_and_limits_are_bounded() {
    accepts("fn recur<T>(x:T)->T{recur::<T>(x)} fn test()->i32{recur::<i32>(1)}");
    let error = rejects(
        "fn grow<T>(){grow::<(T,T)>();} fn test(){grow::<i32>();}",
        "E_INSTANTIATION",
    );
    assert!(!error.notes.is_empty());
    let mut source = String::new();
    for i in 0..35 {
        source.push_str(&format!("fn f{i}<T>(x:T)->T{{f{}::<T>(x)}}", i + 1));
    }
    source.push_str("fn f35<T>(x:T)->T{x} fn test()->i32{f0::<i32>(1)}");
    rejects(&source, "E_CORE_B_UNSUPPORTED");
}

#[test]
fn copy_constraints_and_symbolic_forwarding() {
    accepts(
        "fn id<T:Copy>(x:T)->T{x} fn forward<U:Copy>(x:U)->U{id::<U>(x)} fn test()->i32{forward::<i32>(1)}",
    );
    rejects(
        "fn id<T:Copy>(x:T)->T{x} fn forward<U>(x:U)->U{id::<U>(x)}",
        "E_CORE_B_UNSUPPORTED",
    );
    // Function/tuple/Option types supported here are Copy when all payloads are Copy.
    accepts("fn id<T:Copy>(x:T)->T{x} fn test()->Option<u8>{id::<Option<u8>>(Some(3))}");
}

#[test]
fn unsupported_boundaries_are_distinct_from_core_rejections() {
    for source in [
        "fn test(){let x=[1,2];}",
        "fn test(){let x=[1;4];}",
        "fn unused<T>(){static_assert(sizeof(T)>0_usize);}",
        "fn test(){static_assert(true);}",
        "fn test(){var x=1;}",
        "fn test(){loop{break;}}",
        "fn test(){let x=null;}",
        "fn test(){unsafe{1;}}",
        "fn test(){defer 1;}",
        "fn test(x:&u8){}",
        "struct S{value:i32} fn test(){}",
        "const C:i32=1; fn test(){}",
        "fn test(){let x=match true{true=>1,false=>2};}",
    ] {
        rejects(source, "E_CORE_B_UNSUPPORTED");
    }
}

#[test]
fn integer_widths_shifts_and_checked_operations() {
    accepts("fn test()->i8{-128_i8}");
    rejects("fn test()->i8{128_i8}", "E_TYPE");
    rejects("fn test(){let x=2147483648;}", "E_TYPE");
    accepts("fn test()->u64{18446744073709551615_u64}");
    rejects("fn test()->u8{-1_u8}", "E_TYPE");
    accepts("fn test(){let x=1_u8<<2;}");
    rejects("fn test(){let x=1_u8<<{2};}", "E_TYPE");
    accepts("fn test()->Option<u8>{1+?2}");
    rejects("fn test(){let x=1+?2_u8;}", "E_TYPE");
}

#[test]
fn real_hir_local_scope_signature_and_lower_emitter_contracts() {
    let p = accepts(include_str!("check/fixtures/shared.vt"));
    assert!(p.declarations.is_some());
    assert!(
        p.functions
            .iter()
            .any(|f| f.name == "identity" && f.type_arguments.len() == 1)
    );
    for function in &p.functions {
        for parameter in &function.parameters {
            assert!((parameter.0 as usize) < function.locals.len());
            assert!(!matches!(
                p.types.kind(function.locals[parameter.0 as usize].ty),
                TypeKind::Param(_)
            ));
        }
        assert_eq!(function.signature.params.len(), function.parameters.len());
        assert!(!matches!(
            p.types.kind(function.body.ty),
            TypeKind::Param(_)
        ));
        assert!(function.span.end > function.span.start);
    }
    let mut compilation = lower::lower(&p).unwrap();
    if let Ok(directory) = std::env::var("CORE_B_EVIDENCE") {
        let directory = std::path::Path::new(&directory);
        let mut types = String::new();
        for function in &p.functions {
            types.push_str(&format!(
                "{} definition={:?} args={:?} params={:?} result={} span={:?}\n",
                function.name,
                function.definition,
                function
                    .type_arguments
                    .iter()
                    .map(|t| canonical_type(&p.types, *t))
                    .collect::<Vec<_>>(),
                function
                    .signature
                    .params
                    .iter()
                    .map(|t| canonical_type(&p.types, *t))
                    .collect::<Vec<_>>(),
                canonical_type(&p.types, function.signature.result),
                function.span
            ));
            for (index, local) in function.locals.iter().enumerate() {
                types.push_str(&format!(
                    "  local {index} {} {} scope={:?} span={:?}\n",
                    local.name,
                    canonical_type(&p.types, local.ty),
                    local.scope,
                    local.span
                ));
            }
        }
        std::fs::write(directory.join("candidate-types.txt"), types).unwrap();
        std::fs::write(directory.join("candidate-hir.txt"), format!("{p:#?}")).unwrap();
        std::fs::write(
            directory.join("candidate-mir.txt"),
            format!("{compilation:#?}"),
        )
        .unwrap();
    }
    assert_eq!(compilation.module.functions.len(), p.functions.len());
    let emitted = target::rv64::emit(&mut compilation.types, &compilation.module).unwrap();
    assert!(emitted.assembly.contains("__core_b_test_"));
    assert!(emitted.assembly.contains("__core_b_identity_"));
    assert!(emitted.assembly.contains("call"));
}

#[test]
fn unsupported_builtin_names_and_unused_ordinary_errors_do_not_escape() {
    rejects("fn test(){panic(\"stop\");}", "E_CORE_B_UNSUPPORTED");
    rejects("fn test(){replace::<u8>(1,2);}", "E_CORE_B_UNSUPPORTED");
    rejects("fn unused()->u8{true} fn test(){}", "E_TYPE");
    rejects("fn unused<T>(x:Missing)->T{x} fn test(){}", "E_NAME");
    rejects("fn test(){return;let x:u8=true;}", "E_TYPE");
    rejects("fn unused<T>(){return;missing;}", "E_NAME");
    accepts("fn unused<T>()->Option<u32>{Some(3)} fn test(){}");
    rejects("fn Some<T>(x:T)->T{x} fn test()->u32{Some(3)}", "E_TYPE");
    accepts("fn Some<T>(x:T)->T{x} fn test()->u32{Some::<u32>(3)}");
}

#[test]
fn cached_instance_dependencies_cannot_hide_growing_recursion() {
    for main in [
        "fn test(){a::<i32>();a::<u8>();}",
        "fn test(){a::<u8>();a::<i32>();}",
    ] {
        let source = format!("fn a<T>(){{b::<T>();}}fn b<U>(){{a::<u8>();}}{main}");
        rejects(&source, "E_INSTANTIATION");
    }
    accepts("fn a<T>(){b::<T>();}fn b<U>(){a::<U>();}fn test(){a::<i32>();}");
}

#[test]
fn expression_nesting_is_bounded_before_host_stack_exhaustion() {
    let mut expression = "1".to_string();
    for _ in 0..40 {
        expression = format!("{{{expression}}}");
    }
    rejects(
        &format!("fn test()->i32{{{expression}}}"),
        "E_CORE_B_UNSUPPORTED",
    );
}

#[test]
fn option_and_integer_cases_construct_real_lowerable_hir() {
    for source in [
        "fn test()->i32{let some:Option<u32>=Some(3);let none:Option<u32>=None;42}",
        "fn test()->i8{-128_i8}",
        "fn test()->u8{1_u8<<2}",
        "fn test()->Option<u8>{1+?2}",
        "fn id<T>(x:T)->T{x}fn test()->u64{let f=id::<u64>;f(3)}",
    ] {
        let program = accepts(source);
        let mut compilation = lower::lower(&program).unwrap();
        target::rv64::emit(&mut compilation.types, &compilation.module).unwrap();
    }
}

#[test]
fn many_distinct_instances_hit_the_explicit_count_bound() {
    let mut source = "fn id<T>(x:T)->T{x} fn test(){".to_string();
    for count in 2..131 {
        let fields = vec!["i32"; count].join(",");
        source.push_str(&format!("id::<({fields},)>;"));
    }
    source.push('}');
    rejects(&source, "E_CORE_B_UNSUPPORTED");
}

#[test]
fn fixtures_preserve_change_and_unsupported_categories() {
    rejects(include_str!("check/fixtures/changed-original.vt"), "E_TYPE");
    accepts(include_str!("check/fixtures/changed-migrated.vt"));
    rejects(
        include_str!("check/fixtures/unsupported-t2.vt"),
        "E_CORE_B_UNSUPPORTED",
    );
    if let Ok(directory) = std::env::var("CORE_B_EVIDENCE") {
        let mut costs = String::new();
        for (name, source) in [
            (
                "changed-original",
                include_str!("check/fixtures/changed-original.vt"),
            ),
            (
                "changed-migrated",
                include_str!("check/fixtures/changed-migrated.vt"),
            ),
            ("shared", include_str!("check/fixtures/shared.vt")),
        ] {
            let tokens = syntax::lex(FileId(0), source).unwrap();
            costs.push_str(&format!("{name} physical_lines={} lexer_tokens_including_eof={} explicit_generic_argument_sites={} source_bytes={}\n",source.lines().count(),tokens.len(),source.matches("::<").count(),source.len()));
        }
        std::fs::write(
            std::path::Path::new(&directory).join("source-costs.txt"),
            costs,
        )
        .unwrap();
    }
}
