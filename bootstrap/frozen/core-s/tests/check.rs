#![cfg(feature = "check")]
use std::collections::{BTreeMap, HashMap};

use biomiya_core_s::{ast as a, check::check, model::*, source::Span};

fn expr(kind: a::ExprKind) -> a::Expr {
    a::Expr {
        kind,
        span: Span::default(),
    }
}
fn stmt(kind: a::StatementKind) -> a::Statement {
    a::Statement {
        kind,
        span: Span::default(),
    }
}
fn block(statements: Vec<a::Statement>) -> a::Block {
    a::Block {
        statements,
        span: Span::default(),
    }
}
fn int(n: &str) -> a::Expr {
    expr(a::ExprKind::Integer(n.into()))
}
fn name(n: &str) -> a::Expr {
    expr(a::ExprKind::Name(n.into()))
}
fn ret(e: a::Expr) -> a::Statement {
    stmt(a::StatementKind::Return(e))
}

struct Input {
    p: ResolvedProgram,
    serial: usize,
}
impl Input {
    fn new() -> Self {
        let mut types = TypeTable::default();
        for (name, kind, size) in [
            ("()", TypeKind::Unit, 0),
            ("bool", TypeKind::Bool, 1),
            ("u8", TypeKind::Int(Integer::U8), 1),
            ("u32", TypeKind::Int(Integer::U32), 4),
            ("usize", TypeKind::Int(Integer::Usize), 8),
        ] {
            let ty = types
                .intern(
                    kind,
                    Layout {
                        size,
                        align: size.max(1),
                        field_offsets: vec![],
                        payload_offset: None,
                    },
                    Span::default(),
                )
                .unwrap();
            types.names.insert(name.into(), ty);
        }
        Self {
            p: ResolvedProgram {
                globals: vec![],
                externs: BTreeMap::new(),
                types,
                functions: vec![],
                constants: BTreeMap::new(),
                type_uses: HashMap::new(),
                repeat_lengths: HashMap::new(),
            },
            serial: 1,
        }
    }
    fn ty(&mut self, name: &str) -> a::Type {
        self.serial += 1;
        let span = Span::new(Default::default(), self.serial, self.serial + 1);
        self.p.type_uses.insert(span, self.p.types.names[name]);
        a::Type {
            kind: a::TypeKind::Named(name.into()),
            span,
        }
    }
    fn bind(&mut self, n: &str, value: Option<a::Expr>) -> a::Statement {
        stmt(a::StatementKind::Bind {
            name: n.into(),
            ty: self.ty("u32"),
            mutable: true,
            value,
        })
    }
    fn function(&mut self, n: &str, result: &str, body: Vec<a::Statement>) {
        self.p.functions.push(ResolvedFunction {
            name: n.into(),
            parameters: vec![],
            signature: Signature {
                parameters: vec![],
                result: self.p.types.names[result],
                unsafe_: false,
            },
            body: block(body),
            span: Span::default(),
        });
    }
}

#[test]
fn explicit_returns_and_dead_code_are_checked() {
    let mut input = Input::new();
    input.function("run", "u32", vec![ret(int("7"))]);
    let out = check(&input.p).unwrap();
    assert_eq!(out.functions.len(), 1);
    assert!(matches!(
        out.functions[0].blocks[0].terminator,
        Terminator::Return(Operand::Immediate { bits: 7, .. })
    ));
    input.p.functions[0]
        .body
        .statements
        .push(stmt(a::StatementKind::Expr(name("missing"))));
    assert_eq!(check(&input.p).unwrap_err().code, "E_NAME");
    let mut input = Input::new();
    input.function("run", "()", vec![]);
    assert_eq!(check(&input.p).unwrap_err().code, "E_TYPE");
}

#[test]
fn literals_do_not_unify_across_comparison_operands() {
    let mut input = Input::new();
    let binding = input.bind("n", Some(int("3")));
    let comparison = expr(a::ExprKind::Binary {
        op: a::BinaryOp::Eq,
        left: Box::new(name("n")),
        right: Box::new(int("0")),
    });
    input.function("run", "bool", vec![binding, ret(comparison)]);
    assert_eq!(check(&input.p).unwrap_err().code, "E_TYPE");
}

fn assign(n: &str, value: a::Expr) -> a::Statement {
    stmt(a::StatementKind::Assign {
        target: name(n),
        value,
    })
}

// This intentionally small probe executes only the straight-line scalar IR
// needed to observe return snapshots. Full target execution belongs to I.
fn straight_return(f: &Function) -> u64 {
    fn value(o: &Operand, slots: &HashMap<u32, u64>) -> u64 {
        match o {
            Operand::Immediate { bits, .. } => *bits,
            Operand::Slot(s) => slots[&s.0],
        }
    }
    fn slot(p: &Place) -> u32 {
        assert_eq!(p.byte_offset, 0);
        let PlaceBase::Slot(s) = p.base else {
            panic!("unexpected pointer")
        };
        s.0
    }
    let mut slots = HashMap::new();
    let mut at = f.entry;
    for _ in 0..100 {
        let b = &f.blocks[at.0 as usize];
        for i in &b.instructions {
            match &i.kind {
                InstructionKind::Set {
                    dst,
                    value: Rvalue::Load(p),
                } => {
                    let v = slots[&slot(p)];
                    slots.insert(dst.0, v);
                }
                InstructionKind::Set {
                    dst,
                    value: Rvalue::Use(v),
                } => {
                    let v = value(v, &slots);
                    slots.insert(dst.0, v);
                }
                InstructionKind::Store {
                    destination,
                    value: v,
                } => {
                    let v = value(v, &slots);
                    slots.insert(slot(destination), v);
                }
                InstructionKind::StorageLive(_) | InstructionKind::StorageDead(_) => {}
                _ => panic!("probe does not cover this instruction"),
            }
        }
        match &b.terminator {
            Terminator::Jump(to) => at = *to,
            Terminator::Return(v) => return value(v, &slots),
            _ => panic!("unexpected termination"),
        }
    }
    panic!("cycle in straight-line probe")
}

#[test]
fn return_snapshot_precedes_defer_and_captured_binding_is_stable() {
    let mut input = Input::new();
    let x = input.bind("x", Some(int("1")));
    let inner_x = input.bind("x", Some(int("2")));
    let defer = stmt(a::StatementKind::Defer(block(vec![assign("x", int("9"))])));
    input.function(
        "run",
        "u32",
        vec![
            x,
            defer,
            stmt(a::StatementKind::Block(block(vec![inner_x]))),
            ret(name("x")),
        ],
    );
    let out = check(&input.p).unwrap();
    assert_eq!(straight_return(&out.functions[0]), 1);
}

#[test]
fn cleanup_reads_exit_values_but_cannot_capture_later_bindings() {
    let mut input = Input::new();
    let x = input.bind("x", Some(int("1")));
    let y = input.bind("y", Some(int("2")));
    let defer = stmt(a::StatementKind::Defer(block(vec![assign("x", name("y"))])));
    input.function(
        "run",
        "u32",
        vec![
            x,
            y,
            stmt(a::StatementKind::Block(block(vec![
                defer,
                assign("y", int("5")),
            ]))),
            ret(name("x")),
        ],
    );
    assert_eq!(straight_return(&check(&input.p).unwrap().functions[0]), 5);
    let mut input = Input::new();
    let x = input.bind("x", Some(int("1")));
    let defer = stmt(a::StatementKind::Defer(block(vec![assign(
        "later",
        int("9"),
    )])));
    let later = input.bind("later", Some(int("2")));
    input.function("run", "u32", vec![x, defer, later, ret(name("x"))]);
    assert_eq!(check(&input.p).unwrap_err().code, "E_TYPE");
}

#[test]
fn defer_restrictions_and_raw_permissions_are_enforced() {
    let mut input = Input::new();
    let local = input.bind("x", None);
    input.function(
        "run",
        "()",
        vec![
            stmt(a::StatementKind::Defer(block(vec![local]))),
            ret(expr(a::ExprKind::Unit)),
        ],
    );
    assert_eq!(check(&input.p).unwrap_err().code, "E_TYPE");
    for forbidden in [
        stmt(a::StatementKind::Loop(block(vec![]))),
        ret(expr(a::ExprKind::Unit)),
        stmt(a::StatementKind::Defer(block(vec![]))),
    ] {
        let mut input = Input::new();
        input.function(
            "run",
            "()",
            vec![
                stmt(a::StatementKind::Defer(block(vec![forbidden]))),
                ret(expr(a::ExprKind::Unit)),
            ],
        );
        assert_eq!(check(&input.p).unwrap_err().code, "E_TYPE");
    }
    let mut input = Input::new();
    let x = input.bind("x", Some(int("1")));
    let address = expr(a::ExprKind::Call {
        name: "addr_of".into(),
        arguments: vec![name("x")],
    });
    input.function(
        "run",
        "u32",
        vec![
            x,
            ret(expr(a::ExprKind::Unary {
                op: a::UnaryOp::Deref,
                value: Box::new(address),
            })),
        ],
    );
    assert_eq!(check(&input.p).unwrap_err().code, "E_UNSAFE");
}

#[test]
fn normal_function_named_like_builtin_keeps_its_identity() {
    let mut input = Input::new();
    input.function("addr_of", "u32", vec![ret(int("42"))]);
    input.function(
        "run",
        "u32",
        vec![ret(expr(a::ExprKind::Call {
            name: "addr_of".into(),
            arguments: vec![],
        }))],
    );
    let out = check(&input.p).unwrap();
    assert!(
        out.functions[1]
            .blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .any(|i| matches!(
                i.kind,
                InstructionKind::Call {
                    function: FunctionId(0),
                    ..
                }
            ))
    );
}
