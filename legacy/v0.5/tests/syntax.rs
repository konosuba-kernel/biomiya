use biomiya::{source::FileId, syntax::parse};
#[test]
fn conformance_syntax() {
    for entry in std::fs::read_dir("tests/conformance").unwrap() {
        let p = entry.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) != Some("vt") {
            continue;
        }
        let n = p.file_name().unwrap().to_str().unwrap();
        if [
            "reject_one_tuple.vt",
            "reject_uninitialized_binding.vt",
            "reject_repeat_const_form.vt",
        ]
        .contains(&n)
        {
            continue;
        }
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(
            parse(FileId(0), &s).is_ok(),
            "{n}: {:?}",
            parse(FileId(0), &s).err()
        );
    }
}
#[test]
fn strict_grammar() {
    for s in [
        "fn f(){let x:i32;}",
        "fn f(){(1,)}",
        "type X=(i32,);",
        "fn f(){1 2}",
        "fn f(){let x=S{a};}",
        "fn f(){if Flag {value:true}.value {}}",
    ] {
        assert!(parse(FileId(0), s).is_err(), "{s}")
    }
}
#[test]
fn generics_and_comments() {
    assert!(
        parse(
            FileId(0),
            "/* a /* b */ c */ struct S<T>{x:T} type X=S<S<u32>>; fn f(){let x:i32=1; x>>1;}"
        )
        .is_ok()
    );
}
#[test]
fn literal_rules() {
    for s in [
        "fn f(){1_;}",
        "fn f(){0x_ff;}",
        "fn f(){0b2;}",
        "fn f(){\"\\u1234\";}",
        "\u{feff}fn f(){}",
    ] {
        assert!(parse(FileId(0), s).is_err(), "{s}")
    }
}
#[test]
fn expression_boundaries_and_fixed_unicode() {
    for s in [
        "fn f(){(x as i32).field;}",
        "fn f(){if (Flag{value:true}).value {}}",
        "fn f(){g::<S<S<u8>>>();}",
        "fn 𮯰(){}",
        "fn f(){arch::asm(\"\", in(\"a0\") 1, out(\"a1\") x, clobber(\"a2\"), effects {may_block},);}",
    ] {
        assert!(
            parse(FileId(0), s).is_ok(),
            "{s}: {:?}",
            parse(FileId(0), s).err()
        );
    }
    for s in [
        "fn f(){x as i32.field;}",
        "fn f(){0..1..2;}",
        "fn f(){g::<>()}",
        "fn f(){offsetof(S,01);}",
        "fn f(x:i32,self){}",
    ] {
        assert!(parse(FileId(0), s).is_err(), "{s}");
    }
}
#[test]
fn cast_precedence_and_min_adjacency() {
    use biomiya::syntax::{DeclKind, ExprKind, Statement};
    let ast = parse(FileId(0), "fn f(){a * b as i32; -128_i8; - 128_i8;}").unwrap();
    let DeclKind::Function(f) = &ast.declarations[0].kind else {
        panic!()
    };
    let stmts = &f.body.as_ref().unwrap().statements;
    let Statement::Expr(e) = &stmts[0] else {
        panic!()
    };
    let ExprKind::Binary { right, .. } = &e.kind else {
        panic!()
    };
    assert!(matches!(right.kind, ExprKind::Cast { .. }));
    let Statement::Expr(e) = &stmts[1] else {
        panic!()
    };
    assert!(matches!(e.kind, ExprKind::Unary { adjacent: true, .. }));
    let Statement::Expr(e) = &stmts[2] else {
        panic!()
    };
    assert!(matches!(
        e.kind,
        ExprKind::Unary {
            adjacent: false,
            ..
        }
    ));
}

#[test]
fn ungrouped_block_statement_boundary() {
    let f = biomiya::syntax::parse(
        biomiya::source::FileId(0),
        "fn f(p:&mut i32){if true{}*p=3;}",
    )
    .unwrap();
    let biomiya::syntax::DeclKind::Function(fun) = &f.declarations[0].kind else {
        panic!()
    };
    assert_eq!(fun.body.as_ref().unwrap().statements.len(), 2);
}
