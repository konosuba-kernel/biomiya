use biomiya::{
    abi::*,
    layout::LayoutEngine,
    source::{FileId, Span},
    types::*,
};
use std::collections::BTreeSet;
fn span() -> Span {
    Span::new(FileId(0), 0, 0)
}
fn def(name: &str, params: u32, fields: Vec<TyId>) -> TypeDef {
    TypeDef {
        name: name.into(),
        params,
        kind: DefKind::Struct(
            fields
                .into_iter()
                .enumerate()
                .map(|(n, ty)| FieldDef {
                    name: n.to_string(),
                    ty,
                    span: span(),
                })
                .collect(),
        ),
        copy: false,
        resource: false,
        repr_c: false,
        packed: false,
        align: None,
        span: span(),
        layout_dependencies: vec![],
    }
}
fn sig(params: Vec<TyId>, result: TyId, abi: Abi) -> FnType {
    FnType {
        params,
        result,
        abi,
        unsafe_: false,
        effects: BTreeSet::new(),
    }
}
#[test]
fn finite_nested_generic_and_growing_recursion() {
    let mut c = TypeContext::new();
    let p = c.intern(TypeKind::Param(0));
    let u = c.intern(TypeKind::Int(Integer::U32));
    c.define(DefId(0), def("Wrap", 1, vec![p]));
    let w = c.intern(TypeKind::Nominal(DefId(0), vec![u]));
    let ww = c.intern(TypeKind::Nominal(DefId(0), vec![w]));
    assert_eq!(LayoutEngine::new(&mut c).layout(ww).unwrap().size, 4);
    let tuple = c.intern(TypeKind::Tuple(vec![p, u]));
    let bad = c.intern(TypeKind::Nominal(DefId(1), vec![tuple]));
    c.define(DefId(1), def("Bad", 1, vec![bad]));
    let instance = c.intern(TypeKind::Nominal(DefId(1), vec![u]));
    assert!(LayoutEngine::new(&mut c).layout(instance).is_err());
}
#[test]
fn indirect_generic_parameter_does_not_require_layout() {
    let mut c = TypeContext::new();
    let p = c.intern(TypeKind::Param(0));
    let ptr = c.intern(TypeKind::Pointer {
        target: p,
        space: AddressSpace::Kernel,
        const_: false,
    });
    c.define(DefId(0), def("Link", 1, vec![ptr]));
    let tree = c.intern(TypeKind::Nominal(DefId(1), vec![]));
    let link = c.intern(TypeKind::Nominal(DefId(0), vec![tree]));
    c.define(DefId(1), def("Tree", 0, vec![link]));
    assert_eq!(LayoutEngine::new(&mut c).layout(tree).unwrap().size, 8);
}
#[test]
fn enum_padding_zero_array_alignment_and_overflow() {
    let mut c = TypeContext::new();
    let u = c.intern(TypeKind::Int(Integer::U64));
    let o = c.intern(TypeKind::Option(u));
    let l = LayoutEngine::new(&mut c).layout(o).unwrap();
    assert_eq!((l.size, l.align, l.payload_offset), (16, 8, Some(8)));
    let z = c.intern(TypeKind::Array(u, 0));
    assert_eq!(LayoutEngine::new(&mut c).layout(z).unwrap().align, 8);
    let huge = c.intern(TypeKind::Array(u, u64::MAX));
    assert!(LayoutEngine::new(&mut c).layout(huge).is_err());
}
#[test]
fn lp64_scalar_extensions_and_stack_arguments() {
    let mut c = TypeContext::new();
    let u = c.intern(TypeKind::Int(Integer::U32));
    let void = c.intern(TypeKind::Unit);
    let plan = classify(&mut c, &sig(vec![u; 10], void, Abi::C)).unwrap();
    assert_eq!(
        plan.args[0].extension,
        Extension::ZeroTo32ThenSign64 { bits: 32 }
    );
    assert_eq!(
        plan.args[8].pieces[0].location,
        Location::Stack { offset: 0 }
    );
    assert_eq!(
        plan.args[9].pieces[0].location,
        Location::Stack { offset: 8 }
    );
    assert_eq!(plan.stack_size, 16);
}
#[test]
fn c_aggregate_split_and_sret() {
    let mut c = TypeContext::new();
    let u = c.intern(TypeKind::Int(Integer::U64));
    let mut d = def("Pair", 0, vec![u, u]);
    d.repr_c = true;
    c.define(DefId(0), d);
    let pair = c.intern(TypeKind::Nominal(DefId(0), vec![]));
    let void = c.intern(TypeKind::Unit);
    let mut ps = vec![u; 7];
    ps.push(pair);
    let p = classify(&mut c, &sig(ps, void, Abi::C)).unwrap();
    assert_eq!(p.args[7].pieces[0].location, Location::Register(7));
    assert_eq!(p.args[7].pieces[1].location, Location::Stack { offset: 0 });
    let mut d = def("Triple", 0, vec![u, u, u]);
    d.repr_c = true;
    c.define(DefId(1), d);
    let triple = c.intern(TypeKind::Nominal(DefId(1), vec![]));
    let p = classify(&mut c, &sig(vec![triple, u], triple, Abi::C)).unwrap();
    assert_eq!(p.hidden_result, Some(Location::Register(0)));
    assert_eq!(p.args[0].passing, Passing::IndirectSnapshot);
    assert_eq!(p.args[0].pieces[0].location, Location::Register(1));
}
#[test]
fn biomiya_snapshots_and_zero_sized_omission() {
    let mut c = TypeContext::new();
    let u = c.intern(TypeKind::Int(Integer::U8));
    let a = c.intern(TypeKind::Array(u, 1));
    let z = c.intern(TypeKind::Unit);
    let p = classify(&mut c, &sig(vec![z, a], a, Abi::Biomiya)).unwrap();
    assert_eq!(p.args[0].passing, Passing::Ignore);
    assert_eq!(p.args[1].passing, Passing::IndirectSnapshot);
    assert_eq!(p.args[1].pieces[0].location, Location::Register(1));
}
#[test]
fn effects_are_in_identity_but_not_core_compatibility() {
    let mut c = TypeContext::new();
    let u = c.intern(TypeKind::Unit);
    let f = sig(vec![], u, Abi::Biomiya);
    let a = c.intern(TypeKind::Function(f.clone()));
    let mut g = f;
    g.effects.insert("may_block".into());
    let b = c.intern(TypeKind::Function(g));
    let a = c.intern(TypeKind::Tuple(vec![a, u]));
    let b = c.intern(TypeKind::Tuple(vec![b, u]));
    assert_ne!(a, b);
    assert!(c.core_compatible(a, b));
}
#[test]
fn symbolic_layout_constants_substitute_and_dead_branch_dependencies_remain() {
    let mut c = TypeContext::new();
    let p = c.intern(TypeKind::Param(0));
    let byte = c.intern(TypeKind::Int(Integer::U8));
    let u64_ = c.intern(TypeKind::Int(Integer::U64));
    let len = ConstLen {
        kind: ConstLenKind::SizeOf(p),
        integer: Integer::Usize,
    };
    let array = c.intern(TypeKind::ArrayExpr(byte, len));
    c.define(DefId(20), def("BytesOf", 1, vec![array]));
    let instance = c.intern(TypeKind::Nominal(DefId(20), vec![u64_]));
    assert_eq!(LayoutEngine::new(&mut c).layout(instance).unwrap().size, 8);
    let recursive = c.intern(TypeKind::Nominal(DefId(21), vec![]));
    let conditional = ConstLen {
        integer: Integer::Usize,
        kind: ConstLenKind::If(
            Box::new(ConstLen {
                integer: Integer::U8,
                kind: ConstLenKind::Literal(1),
            }),
            Box::new(ConstLen {
                integer: Integer::Usize,
                kind: ConstLenKind::Literal(1),
            }),
            Box::new(ConstLen {
                integer: Integer::Usize,
                kind: ConstLenKind::SizeOf(recursive),
            }),
        ),
    };
    let array = c.intern(TypeKind::ArrayExpr(byte, conditional));
    c.define(DefId(21), def("RecursiveConstant", 0, vec![array]));
    assert!(LayoutEngine::new(&mut c).layout(recursive).is_err());
}
#[test]
fn packed_alignment_and_atomic_legality() {
    let mut c = TypeContext::new();
    let u8_ = c.intern(TypeKind::Int(Integer::U8));
    let u64_ = c.intern(TypeKind::Int(Integer::U64));
    let mut d = def("Packed", 0, vec![u8_, u64_]);
    d.packed = true;
    c.define(DefId(30), d);
    let p = c.intern(TypeKind::Nominal(DefId(30), vec![]));
    let l = LayoutEngine::new(&mut c).layout(p).unwrap();
    assert_eq!((l.size, l.align, l.fields), (9, 1, vec![0, 1]));
    let a = c.intern(TypeKind::Atomic(u8_));
    assert_eq!(LayoutEngine::new(&mut c).layout(a).unwrap().size, 4);
    let signed = c.intern(TypeKind::Int(Integer::I32));
    let a = c.intern(TypeKind::Atomic(signed));
    assert_eq!(
        LayoutEngine::new(&mut c).layout(a).unwrap_err().code,
        "E_TYPE"
    );
}
#[test]
fn concrete_symbolic_array_lengths_compare_by_value() {
    let mut c = TypeContext::new();
    let byte = c.intern(TypeKind::Int(Integer::U8));
    let two = c.intern(TypeKind::Array(byte, 2));
    let literal = |n| ConstLen {
        integer: Integer::Usize,
        kind: ConstLenKind::Literal(n),
    };
    let expression = c.intern(TypeKind::ArrayExpr(
        byte,
        ConstLen {
            integer: Integer::Usize,
            kind: ConstLenKind::Binary("+".into(), Box::new(literal(1)), Box::new(literal(1))),
        },
    ));
    assert!(c.core_compatible(two, expression));
}
#[test]
fn symbolic_assertions_and_dead_panic_follow_constant_semantics() {
    let mut c = TypeContext::new();
    let value = |n| ConstLen {
        integer: Integer::Usize,
        kind: ConstLenKind::Literal(n),
    };
    let yes = ConstLen {
        integer: Integer::Usize,
        kind: ConstLenKind::Assert(Box::new(value(1)), Box::new(value(4))),
    };
    assert_eq!(LayoutEngine::new(&mut c).evaluate_len(&yes).unwrap(), 4);
    let no = ConstLen {
        integer: Integer::Usize,
        kind: ConstLenKind::Assert(Box::new(value(0)), Box::new(value(4))),
    };
    assert_eq!(
        LayoutEngine::new(&mut c)
            .evaluate_len(&no)
            .unwrap_err()
            .code,
        "E_CONST"
    );
    let dead = ConstLen {
        integer: Integer::Usize,
        kind: ConstLenKind::If(
            Box::new(value(1)),
            Box::new(value(4)),
            Box::new(ConstLen {
                integer: Integer::Usize,
                kind: ConstLenKind::Panic,
            }),
        ),
    };
    assert_eq!(LayoutEngine::new(&mut c).evaluate_len(&dead).unwrap(), 4);
}

#[test]
fn dependent_checked_operations_preserve_widths_and_operand_errors() {
    let mut c = TypeContext::new();
    let literal = |integer, n| ConstLen {
        integer,
        kind: ConstLenKind::Literal(n),
    };
    for (op, integer, x, y, expected) in [
        ("+", Integer::U8, 255, 1, 0),
        ("+", Integer::U8, 254, 1, 1),
        ("-", Integer::U64, 0, 1, 0),
        ("*", Integer::U64, u64::MAX, u64::MAX, 0),
        ("*", Integer::I8, 64, 2, 0),
        ("/", Integer::I8, 128, 255, 0),
        ("/", Integer::U32, 7, 0, 0),
        ("<<", Integer::U8, 1, 8, 0),
    ] {
        let term = ConstLen {
            integer: Integer::U8,
            kind: ConstLenKind::CheckedFits {
                op: op.into(),
                left: Box::new(literal(integer, x)),
                right: Box::new(literal(integer, y)),
            },
        };
        assert_eq!(
            LayoutEngine::new(&mut c).evaluate_len(&term).unwrap(),
            expected,
            "{op} {integer:?}"
        );
    }
    for (source, target, n, expected) in [
        (Integer::I32, Integer::U8, u32::MAX as u64, 0),
        (Integer::U32, Integer::I8, 128, 0),
        (Integer::U64, Integer::I64, u64::MAX, 0),
        (Integer::U64, Integer::U64, u64::MAX, 1),
        (Integer::I8, Integer::I64, 255, 1),
    ] {
        let term = ConstLen {
            integer: Integer::U8,
            kind: ConstLenKind::CastFits {
                target,
                value: Box::new(literal(source, n)),
            },
        };
        assert_eq!(
            LayoutEngine::new(&mut c).evaluate_len(&term).unwrap(),
            expected
        );
    }
    let panic = ConstLen {
        integer: Integer::U32,
        kind: ConstLenKind::Panic,
    };
    let term = ConstLen {
        integer: Integer::U8,
        kind: ConstLenKind::CheckedFits {
            op: "+".into(),
            left: Box::new(panic),
            right: Box::new(literal(Integer::U32, 1)),
        },
    };
    assert_eq!(
        LayoutEngine::new(&mut c)
            .evaluate_len(&term)
            .unwrap_err()
            .code,
        "E_CONST"
    );
    let term = ConstLen {
        integer: Integer::U8,
        kind: ConstLenKind::CastFits {
            target: Integer::U8,
            value: Box::new(ConstLen {
                integer: Integer::U32,
                kind: ConstLenKind::Binary(
                    "/".into(),
                    Box::new(literal(Integer::U32, 1)),
                    Box::new(literal(Integer::U32, 0)),
                ),
            }),
        },
    };
    assert_eq!(
        LayoutEngine::new(&mut c)
            .evaluate_len(&term)
            .unwrap_err()
            .code,
        "E_CONST"
    );
}
#[test]
fn checked_length_conditions_substitute_query_operands() {
    let mut c = TypeContext::new();
    let p = c.intern(TypeKind::Param(0));
    let u32_ = c.intern(TypeKind::Int(Integer::U32));
    let byte = c.intern(TypeKind::Int(Integer::U8));
    let query = ConstLen {
        integer: Integer::Usize,
        kind: ConstLenKind::SizeOf(p),
    };
    let literal = |n| ConstLen {
        integer: Integer::Usize,
        kind: ConstLenKind::Literal(n),
    };
    let fit = ConstLen {
        integer: Integer::U8,
        kind: ConstLenKind::CastFits {
            target: Integer::U8,
            value: Box::new(query.clone()),
        },
    };
    let check = ConstLen {
        integer: Integer::U8,
        kind: ConstLenKind::CheckedFits {
            op: "+".into(),
            left: Box::new(query),
            right: Box::new(literal(1)),
        },
    };
    for condition in [fit, check] {
        let array = c.intern(TypeKind::ArrayExpr(
            byte,
            ConstLen {
                integer: Integer::Usize,
                kind: ConstLenKind::If(
                    Box::new(condition),
                    Box::new(literal(7)),
                    Box::new(literal(9)),
                ),
            },
        ));
        let concrete = c.substitute(array, &[u32_]);
        assert_eq!(LayoutEngine::new(&mut c).layout(concrete).unwrap().size, 7);
        let seven = c.intern(TypeKind::Array(byte, 7));
        assert!(c.core_compatible(concrete, seven));
    }
}

fn deferred(id: u32, args: Vec<TyId>, queries: Vec<TyId>) -> ConstLen {
    ConstLen {
        integer: Integer::Usize,
        kind: ConstLenKind::Deferred {
            id: ConstExprId(id),
            args,
            queries,
        },
    }
}
#[test]
fn checked_deferred_instances_substitute_independently_and_survive_cloning() {
    let mut c = TypeContext::new();
    let p = c.intern(TypeKind::Param(0));
    let byte = c.intern(TypeKind::Int(Integer::U8));
    let word = c.intern(TypeKind::Int(Integer::U16));
    let field = c.intern(TypeKind::ArrayExpr(byte, deferred(0, vec![p], vec![])));
    c.define(DefId(0), def("ByCast", 1, vec![field]));
    c.cache_const(ConstExprId(0), &[byte], 2);
    c.cache_const(ConstExprId(0), &[word], 258);
    let small = c.intern(TypeKind::Nominal(DefId(0), vec![byte]));
    let large = c.intern(TypeKind::Nominal(DefId(0), vec![word]));
    let small_field = c.substitute(field, &[byte]);
    assert_eq!(c.kind(small_field), &TypeKind::Array(byte, 2));
    let large_field = c.substitute(field, &[word]);
    assert_eq!(c.kind(large_field), &TypeKind::Array(byte, 258));
    let mut cloned = c.clone();
    assert_eq!(cloned.cached_const(ConstExprId(0), &[word]), Some(258));
    assert_eq!(
        LayoutEngine::new(&mut cloned).layout(small).unwrap().size,
        2
    );
    assert_eq!(
        LayoutEngine::new(&mut cloned).layout(large).unwrap().size,
        258
    );
    assert_eq!(cloned.kind(large), &TypeKind::Nominal(DefId(0), vec![word]));
    assert!(!cloned.core_compatible(small, large));
    let zero = c.intern(TypeKind::ArrayExpr(byte, deferred(1, vec![word], vec![])));
    assert_eq!(
        LayoutEngine::new(&mut c).layout(zero).unwrap_err().code,
        "I_BUG"
    );
    c.cache_const(ConstExprId(1), &[word], 0);
    assert_eq!(LayoutEngine::new(&mut c).layout(zero).unwrap().size, 0);
}
#[test]
fn deferred_queries_preserve_dead_dependencies_and_fixed_indirection_stop() {
    let mut c = TypeContext::new();
    let p = c.intern(TypeKind::Param(0));
    let byte = c.intern(TypeKind::Int(Integer::U8));
    let tree = c.intern(TypeKind::Nominal(DefId(1), vec![]));
    let pointer = c.intern(TypeKind::Pointer {
        target: p,
        space: AddressSpace::Kernel,
        const_: true,
    });
    let field = c.intern(TypeKind::ArrayExpr(
        byte,
        deferred(0, vec![p], vec![pointer]),
    ));
    c.define(DefId(0), def("IndirectQuery", 1, vec![field]));
    let link = c.intern(TypeKind::Nominal(DefId(0), vec![tree]));
    c.define(DefId(1), def("Tree", 0, vec![link]));
    c.cache_const(ConstExprId(0), &[tree], 8);
    assert_eq!(LayoutEngine::new(&mut c).layout(tree).unwrap().size, 8);
    let bad = c.intern(TypeKind::Nominal(DefId(2), vec![]));
    let badfield = c.intern(TypeKind::ArrayExpr(byte, deferred(1, vec![], vec![bad])));
    c.define(DefId(2), def("DeadQueryCycle", 0, vec![badfield]));
    c.cache_const(ConstExprId(1), &[], 0);
    assert_eq!(
        LayoutEngine::new(&mut c).layout(bad).unwrap_err().code,
        "E_LAYOUT"
    );
}
