use biomiya::{
    hir::{self, Expr, ExprKind as E},
    ir::ScopeId,
    layout::LayoutEngine,
    source::Span,
    types::*,
};
use core_b_prototype::constant::*;
fn e(ty: TyId, kind: E) -> Expr {
    Expr {
        ty,
        kind,
        span: Span::default(),
    }
}
fn n(ty: TyId, x: u64) -> Expr {
    e(ty, E::Integer(x))
}
fn bin(ty: TyId, op: &str, a: Expr, b: Expr) -> Expr {
    e(
        ty,
        E::Binary {
            op: op.into(),
            left: Box::new(a),
            right: Box::new(b),
        },
    )
}
fn eval(t: &mut TypeContext, e: &Expr) -> ConstValue {
    evaluate(t, e, &ConstInput::default()).unwrap()
}
fn spec() -> DataSpec {
    DataSpec {
        symbol: "image".into(),
        writable: false,
        exported: true,
        section: None,
        span: Span::default(),
    }
}
#[test]
fn scalar_boundaries_and_signed_operations() {
    let mut t = TypeContext::new();
    let i = t.intern(TypeKind::Int(Integer::I8));
    let u = t.intern(TypeKind::Int(Integer::U64));
    let b = t.intern(TypeKind::Bool);
    assert_eq!(
        eval(&mut t, &bin(i, "+", n(i, 255), n(i, 2))).kind,
        ConstKind::Scalar(1)
    );
    for x in [
        bin(i, "+", n(i, 127), n(i, 1)),
        bin(i, "/", n(i, 128), n(i, 255)),
        bin(i, "%", n(i, 128), n(i, 255)),
        bin(u, "*", n(u, u64::MAX), n(u, 2)),
    ] {
        assert_eq!(
            evaluate(&mut t, &x, &ConstInput::default())
                .unwrap_err()
                .code,
            "E_CONST"
        );
    }
    assert_eq!(
        eval(&mut t, &bin(b, "<", n(i, 255), n(i, 0))).kind,
        ConstKind::Scalar(1)
    );
    assert_eq!(
        eval(&mut t, &bin(i, "&", n(i, 255), n(i, 128))).kind,
        ConstKind::Scalar(128)
    );
    let v = eval(&mut t, &n(i, 255));
    assert_eq!(serialize(&mut t, &v, &spec()).unwrap().bytes, vec![255]);
}
#[test]
fn static_all_branches_but_execution_selected() {
    let mut t = TypeContext::new();
    let u = t.intern(TypeKind::Int(Integer::U32));
    let b = t.intern(TypeKind::Bool);
    let branch = |other| {
        e(
            u,
            E::If {
                condition: Box::new(n(b, 1)),
                then: Box::new(n(u, 7)),
                else_: Some(Box::new(other)),
            },
        )
    };
    assert_eq!(
        eval(&mut t, &branch(bin(u, "/", n(u, 1), n(u, 0)))).kind,
        ConstKind::Scalar(7)
    );
    let illegal = e(
        u,
        E::Intrinsic {
            name: "not.const".into(),
            arguments: vec![],
            type_arguments: vec![],
        },
    );
    assert_eq!(
        evaluate(&mut t, &branch(illegal), &ConstInput::default())
            .unwrap_err()
            .code,
        "E_CORE_B_UNSUPPORTED"
    );
    let short = bin(
        b,
        "&&",
        n(b, 0),
        bin(b, "==", bin(u, "/", n(u, 1), n(u, 0)), n(u, 1)),
    );
    assert_eq!(eval(&mut t, &short).kind, ConstKind::Scalar(0));
}
#[test]
fn block_binding_unused_initializer_and_projection() {
    let mut t = TypeContext::new();
    let u = t.intern(TypeKind::Int(Integer::U32));
    let pair = t.intern(TypeKind::Tuple(vec![u, u]));
    let value = e(
        pair,
        E::Aggregate {
            variant: None,
            fields: vec![(1, n(u, 9)), (0, n(u, 4))],
        },
    );
    let block = e(
        u,
        E::Block {
            scope: ScopeId(0),
            statements: vec![hir::Statement::Bind {
                pattern: hir::Pattern {
                    ty: pair,
                    kind: hir::PatternKind::Bind(hir::LocalId(0)),
                    span: Span::default(),
                },
                value,
            }],
            tail: Some(Box::new(e(
                u,
                E::Field {
                    base: Box::new(e(pair, E::Local(hir::LocalId(0)))),
                    field: 1,
                },
            ))),
        },
    );
    assert_eq!(eval(&mut t, &block).kind, ConstKind::Scalar(9));
    let mut unused = block.clone();
    if let E::Block { statements, .. } = &mut unused.kind {
        statements.push(hir::Statement::Expr(bin(u, "/", n(u, 1), n(u, 0))));
    }
    assert_eq!(
        evaluate(&mut t, &unused, &ConstInput::default())
            .unwrap_err()
            .code,
        "E_CONST"
    );
    let mut illegal = block.clone();
    if let E::Block { statements, .. } = &mut illegal.kind {
        statements.push(hir::Statement::Expr(e(
            u,
            E::Intrinsic {
                name: "no".into(),
                arguments: vec![],
                type_arguments: vec![],
            },
        )));
    }
    assert_eq!(
        evaluate(&mut t, &illegal, &ConstInput::default())
            .unwrap_err()
            .code,
        "E_CORE_B_UNSUPPORTED"
    );
}
#[test]
fn nominal_enum_tuple_and_layout_query() {
    let mut t = TypeContext::new();
    let u = t.intern(TypeKind::Int(Integer::U64));
    let byte = t.intern(TypeKind::Int(Integer::U8));
    let d = DefId(0);
    t.define(
        d,
        TypeDef {
            name: "Flag".into(),
            params: 0,
            kind: DefKind::Enum(vec![
                VariantDef {
                    name: "Empty".into(),
                    fields: vec![],
                },
                VariantDef {
                    name: "Full".into(),
                    fields: vec![FieldDef {
                        name: "value".into(),
                        ty: u,
                        span: Span::default(),
                    }],
                },
            ]),
            copy: true,
            resource: false,
            repr_c: false,
            packed: false,
            align: None,
            span: Span::default(),
            layout_dependencies: vec![],
        },
    );
    let en = t.intern(TypeKind::Nominal(d, vec![]));
    let tuple = t.intern(TypeKind::Tuple(vec![byte, en]));
    let source = e(
        tuple,
        E::Aggregate {
            variant: None,
            fields: vec![
                (
                    1,
                    e(
                        en,
                        E::Aggregate {
                            variant: Some(1),
                            fields: vec![(0, n(u, 0x1234))],
                        },
                    ),
                ),
                (0, n(byte, 3)),
            ],
        },
    );
    let v = eval(&mut t, &source);
    let data = serialize(&mut t, &v, &spec()).unwrap();
    let l = LayoutEngine::new(&mut t).layout(tuple).unwrap();
    let el = LayoutEngine::new(&mut t).layout(en).unwrap();
    let off = l.fields[1] as usize;
    assert_eq!(&data.bytes[off..off + 4], &1u32.to_le_bytes());
    assert_eq!(
        &data.bytes[off + el.payload_offset.unwrap() as usize
            ..off + el.payload_offset.unwrap() as usize + 8],
        &0x1234u64.to_le_bytes()
    );
    let us = t.intern(TypeKind::Int(Integer::Usize));
    for (name, want) in [("__const.sizeof", l.size), ("__const.alignof", l.align)] {
        assert_eq!(
            eval(
                &mut t,
                &e(
                    us,
                    E::Intrinsic {
                        name: name.into(),
                        arguments: vec![],
                        type_arguments: vec![tuple]
                    }
                )
            )
            .kind,
            ConstKind::Scalar(want)
        );
    }
}
#[test]
fn option_result_and_wrapper_initializers() {
    let mut t = TypeContext::new();
    let u = t.intern(TypeKind::Int(Integer::U32));
    for ty in [
        t.intern(TypeKind::Option(u)),
        t.intern(TypeKind::Result(u, u)),
    ] {
        let v = eval(
            &mut t,
            &e(
                ty,
                E::Aggregate {
                    variant: Some(0),
                    fields: vec![(0, n(u, 42))],
                },
            ),
        );
        let image = serialize(&mut t, &v, &spec()).unwrap();
        assert_eq!(&image.bytes[4..8], &42u32.to_le_bytes());
    }
    for (kind, name, wrapper) in [
        (TypeKind::Atomic(u), "atomic.new", Wrapper::Atomic),
        (
            TypeKind::UnsafeCell(u),
            "UnsafeCell.new",
            Wrapper::UnsafeCell,
        ),
    ] {
        let ty = t.intern(kind);
        let v = eval(
            &mut t,
            &e(
                ty,
                E::Intrinsic {
                    name: name.into(),
                    arguments: vec![n(u, 42)],
                    type_arguments: vec![],
                },
            ),
        );
        assert!(matches!(&v.kind,ConstKind::Wrapped{wrapper:w,..} if *w==wrapper));
        assert_eq!(
            serialize(&mut t, &v, &spec()).unwrap().bytes,
            42u32.to_le_bytes()
        );
    }
    let mu = t.intern(TypeKind::MaybeUninit(u));
    let initialized = ConstValue {
        ty: mu,
        kind: ConstKind::Wrapped {
            wrapper: Wrapper::MaybeUninit,
            value: Box::new(ConstValue {
                ty: u,
                kind: ConstKind::Scalar(5),
            }),
        },
    };
    assert_eq!(
        serialize(&mut t, &initialized, &spec()).unwrap().bytes,
        5u32.to_le_bytes()
    );
    let pair = t.intern(TypeKind::Tuple(vec![u, mu]));
    let v = eval(
        &mut t,
        &e(
            pair,
            E::Aggregate {
                variant: None,
                fields: vec![(0, n(u, 42)), (1, e(mu, E::Uninit))],
            },
        ),
    );
    assert_eq!(
        serialize(&mut t, &v, &spec()).unwrap().bytes,
        vec![42, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        evaluate(&mut t, &e(u, E::Uninit), &ConstInput::default())
            .unwrap_err()
            .code,
        "E_CONST"
    );
}
#[test]
fn addresses_aggregate_relocations_and_addends() {
    let mut t = TypeContext::new();
    let u = t.intern(TypeKind::Int(Integer::U64));
    let p = t.intern(TypeKind::Pointer {
        target: u,
        space: AddressSpace::Kernel,
        const_: true,
    });
    let pair = t.intern(TypeKind::Tuple(vec![u, p]));
    let mut input = ConstInput::default();
    input.symbols.insert("target".into(), u);
    let address = e(
        p,
        E::RawAddress {
            value: Box::new(e(
                u,
                E::Global {
                    symbol: "target".into(),
                    mutable: false,
                    external: false,
                },
            )),
            mutable: false,
        },
    );
    let v = e(
        pair,
        E::Aggregate {
            variant: None,
            fields: vec![(0, n(u, 9)), (1, address)],
        },
    );
    let v = evaluate(&mut t, &v, &input).unwrap();
    let image = serialize(&mut t, &v, &spec()).unwrap();
    assert_eq!(image.relocations[0].offset, 8);
    assert_eq!(image.relocations[0].symbol, "target");
    let v = ConstValue {
        ty: p,
        kind: ConstKind::Relocation {
            symbol: "target".into(),
            addend: -8,
        },
    };
    assert_eq!(
        serialize(&mut t, &v, &spec()).unwrap().relocations[0].addend,
        -8
    );
    let sig = FnType {
        params: vec![],
        result: u,
        abi: Abi::Biomiya,
        unsafe_: false,
        effects: Default::default(),
    };
    let f = t.intern(TypeKind::Function(sig));
    input.symbols.insert("fun".into(), f);
    let v = evaluate(
        &mut t,
        &e(
            f,
            E::Function {
                symbol: "fun".into(),
            },
        ),
        &input,
    )
    .unwrap();
    assert_eq!(
        serialize(&mut t, &v, &spec()).unwrap().relocations[0].symbol,
        "fun"
    );
}
#[test]
fn limits_and_simple_serializer_bounds() {
    let mut t = TypeContext::new();
    let u = t.intern(TypeKind::Int(Integer::U8));
    let huge = t.intern(TypeKind::Array(u, IMAGE_LIMIT + 1));
    assert_eq!(
        serialize(
            &mut t,
            &ConstValue {
                ty: huge,
                kind: ConstKind::Uninit
            },
            &spec()
        )
        .unwrap_err()
        .code,
        "I_LIMIT"
    );
    let pair = t.intern(TypeKind::Tuple(vec![u]));
    let bad = ConstValue {
        ty: pair,
        kind: ConstKind::Aggregate {
            variant: None,
            fields: vec![(
                2,
                ConstValue {
                    ty: u,
                    kind: ConstKind::Scalar(1),
                },
            )],
        },
    };
    assert_eq!(
        serialize(&mut t, &bad, &spec()).unwrap_err().code,
        "E_CONST"
    );
    let mut deep = n(u, 1);
    for _ in 0..140 {
        deep = e(
            u,
            E::Block {
                scope: ScopeId(0),
                statements: vec![],
                tail: Some(Box::new(deep)),
            },
        );
    }
    assert_eq!(
        evaluate(&mut t, &deep, &ConstInput::default())
            .unwrap_err()
            .code,
        "I_LIMIT"
    );
}
#[test]
fn rv64_consumes_image_and_links_real_relocation() {
    let mut t = TypeContext::new();
    let u = t.intern(TypeKind::Int(Integer::U64));
    let p = t.intern(TypeKind::Pointer {
        target: u,
        space: AddressSpace::Kernel,
        const_: true,
    });
    let pair = t.intern(TypeKind::Tuple(vec![u, p]));
    let mut input = ConstInput::default();
    input.symbols.insert("target".into(), u);
    let source = e(
        pair,
        E::Aggregate {
            variant: None,
            fields: vec![
                (0, n(u, 42)),
                (
                    1,
                    e(
                        p,
                        E::RawAddress {
                            value: Box::new(e(
                                u,
                                E::Global {
                                    symbol: "target".into(),
                                    mutable: false,
                                    external: false,
                                },
                            )),
                            mutable: false,
                        },
                    ),
                ),
            ],
        },
    );
    let v = evaluate(&mut t, &source, &input).unwrap();
    let data = serialize(&mut t, &v, &spec()).unwrap();
    let module = biomiya::ir::Module {
        functions: vec![],
        data: vec![data],
        externals: vec!["target".into()],
    };
    let mut emission = biomiya::target::rv64::emit(&mut t, &module).unwrap();
    emission.assembly.push_str("\n.section .data\n.balign 8\n.globl target\ntarget:\n.dword 77\n.section .text\n.globl _start\n_start:\nla t0, image\nld t1, 0(t0)\nli t2, 42\nbne t1, t2, fail\nld t1, 8(t0)\nla t2, target\nbne t1, t2, fail\nld t1, 0(t1)\nli t2, 77\nbne t1, t2, fail\nli a0, 0\nj finish\nfail:\nli a0, 1\nfinish:\nli a7, 93\necall\n");
    let root = std::path::Path::new(
        "/home/mii/code/konOSuba/biomiya/build/simplification/implementation/V",
    );
    let asm = root.join("relocation-image.s");
    let elf = root.join("relocation-image.elf");
    std::fs::write(&asm, emission.assembly).unwrap();
    let output = std::process::Command::new("clang")
        .args([
            "--target=riscv64-unknown-linux-gnu",
            "-march=rv64ima",
            "-mabi=lp64",
            "-nostdlib",
            "-static",
            "-fuse-ld=lld",
            "-Wl,--no-relax",
            "-Wl,-e,_start",
        ])
        .arg(&asm)
        .arg("-o")
        .arg(&elf)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = std::process::Command::new("qemu-riscv64")
        .arg(&elf)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}
#[test]
fn aggregate_and_address_operation_capability_is_static() {
    let mut t = TypeContext::new();
    let i = t.intern(TypeKind::Int(Integer::I8));
    let b = t.intern(TypeKind::Bool);
    let tuple = t.intern(TypeKind::Tuple(vec![i]));
    let aggregate = || {
        e(
            tuple,
            E::Aggregate {
                variant: None,
                fields: vec![(0, n(i, 1))],
            },
        )
    };
    let equality = bin(b, "==", aggregate(), aggregate());
    let dead = e(
        b,
        E::If {
            condition: Box::new(n(b, 1)),
            then: Box::new(n(b, 1)),
            else_: Some(Box::new(equality.clone())),
        },
    );
    for expr in [equality, dead] {
        assert_eq!(
            evaluate(&mut t, &expr, &ConstInput::default())
                .unwrap_err()
                .code,
            "E_CORE_B_UNSUPPORTED"
        );
    }
    let ptr = t.intern(TypeKind::Pointer {
        target: i,
        space: AddressSpace::Kernel,
        const_: true,
    });
    let address = || {
        e(
            ptr,
            E::RawAddress {
                value: Box::new(e(
                    i,
                    E::Global {
                        symbol: "target".into(),
                        mutable: false,
                        external: false,
                    },
                )),
                mutable: false,
            },
        )
    };
    let mut input = ConstInput::default();
    input.symbols.insert("target".into(), i);
    let comparison = bin(b, "==", address(), address());
    let short = bin(b, "||", n(b, 1), comparison);
    assert_eq!(
        evaluate(&mut t, &short, &input).unwrap_err().code,
        "E_CORE_B_UNSUPPORTED"
    );
}
#[test]
fn serializer_checks_wrapper_tag_and_payload_type() {
    let mut t = TypeContext::new();
    let i = t.intern(TypeKind::Int(Integer::I8));
    let u = t.intern(TypeKind::Int(Integer::U8));
    let cell = t.intern(TypeKind::UnsafeCell(i));
    for (wrapper, inner) in [(Wrapper::Atomic, i), (Wrapper::UnsafeCell, u)] {
        let v = ConstValue {
            ty: cell,
            kind: ConstKind::Wrapped {
                wrapper,
                value: Box::new(ConstValue {
                    ty: inner,
                    kind: ConstKind::Scalar(42),
                }),
            },
        };
        assert_eq!(serialize(&mut t, &v, &spec()).unwrap_err().code, "E_CONST");
    }
    for (ty, wrapper) in [
        (cell, Wrapper::UnsafeCell),
        (t.intern(TypeKind::MaybeUninit(i)), Wrapper::MaybeUninit),
        (t.intern(TypeKind::Atomic(u)), Wrapper::Atomic),
    ] {
        let inner = if wrapper == Wrapper::Atomic { u } else { i };
        let expected = if wrapper == Wrapper::Atomic {
            vec![42, 0, 0, 0]
        } else {
            vec![42]
        };
        let v = ConstValue {
            ty,
            kind: ConstKind::Wrapped {
                wrapper,
                value: Box::new(ConstValue {
                    ty: inner,
                    kind: ConstKind::Scalar(42),
                }),
            },
        };
        assert_eq!(serialize(&mut t, &v, &spec()).unwrap().bytes, expected);
    }
}
