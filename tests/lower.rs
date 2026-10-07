//! HIR/MIR probes are interface/lowering evidence, not source conformance.
use biomiya::{hir, ir, lower, source, types};
use hir::{Expr, ExprKind as E, LocalId, Pattern, PatternKind, Statement};
use ir::{InstructionKind, ScopeId, SemanticEvent};
use source::{FileId, Span};
use std::collections::BTreeSet;
use types::*;
fn span() -> Span {
    Span::new(FileId(0), 0, 1)
}
fn expr(ty: TyId, kind: E) -> Expr {
    Expr {
        ty,
        kind,
        span: span(),
    }
}
fn local(ty: TyId, id: u32) -> Expr {
    expr(ty, E::Local(LocalId(id)))
}
fn int(ty: TyId, value: u64) -> Expr {
    expr(ty, E::Integer(value))
}
fn program(types: TypeContext, body: Expr, locals: Vec<hir::Local>) -> hir::Program {
    hir::Program {
        types,
        functions: vec![hir::Function {
            definition: DefId(0),
            name: "test".into(),
            symbol: "__biomiya_test_entry".into(),
            signature: FnType {
                params: vec![],
                result: body.ty,
                abi: Abi::Biomiya,
                unsafe_: false,
                effects: BTreeSet::new(),
            },
            type_arguments: vec![],
            parameters: vec![],
            locals,
            body,
            span: span(),
            special: None,
            exported: true,
            section: None,
        }],
        data: vec![],
        externals: vec![],
        declarations: None,
    }
}
#[test]
fn cleanup_is_late_and_return_is_saved() {
    let mut t = TypeContext::new();
    let i = t.intern(TypeKind::Int(Integer::I32));
    let u = t.intern(TypeKind::Unit);
    let body = expr(
        i,
        E::Block {
            scope: ScopeId(0),
            statements: vec![
                Statement::Bind {
                    pattern: Pattern {
                        ty: i,
                        kind: PatternKind::Bind(LocalId(0)),
                        span: span(),
                    },
                    value: int(i, 7),
                },
                Statement::Defer {
                    id: 0,
                    expression: expr(
                        u,
                        E::Assign {
                            op: "=".into(),
                            place: Box::new(local(i, 0)),
                            value: Box::new(int(i, 99)),
                        },
                    ),
                    span: span(),
                },
            ],
            tail: Some(Box::new(local(i, 0))),
        },
    );
    let p = program(
        t,
        body,
        vec![hir::Local {
            name: "x".into(),
            ty: i,
            mutable: true,
            scope: ScopeId(0),
            span: span(),
        }],
    );
    let c = lower::lower(&p).unwrap();
    let instructions = &c.module.functions[0].blocks[0].instructions;
    let registration = instructions
        .iter()
        .position(|x| {
            matches!(
                &x.kind,
                InstructionKind::Event(SemanticEvent::Defer {
                    registration: true,
                    ..
                })
            )
        })
        .unwrap();
    let cleanup = instructions
        .iter()
        .position(|x| {
            matches!(
                &x.kind,
                InstructionKind::Event(SemanticEvent::Defer {
                    registration: false,
                    ..
                })
            )
        })
        .unwrap();
    assert!(registration < cleanup);
    assert!(
        instructions[registration + 1..cleanup]
            .iter()
            .any(|x| matches!(x.kind, InstructionKind::Set { .. }))
    );
    let mut types = c.types;
    biomiya::target::rv64::emit(&mut types, &c.module).unwrap();
}
fn source_compilation(text: &str) -> ir::Compilation {
    let ast = biomiya::syntax::parse(FileId(0), text).unwrap();
    let hir = biomiya::check::check(&[ast]).unwrap();
    biomiya::lower::lower(&hir).unwrap()
}
fn execute_source(text: &str, expected: i32) {
    let ast = biomiya::syntax::parse(FileId(0), text).unwrap();
    let hir = biomiya::check::check(&[ast]).unwrap();
    let symbol = hir
        .functions
        .iter()
        .find(|f| f.name == "test")
        .unwrap()
        .symbol
        .clone();
    let mut compilation = biomiya::lower::lower(&hir).unwrap();
    let source = compilation
        .module
        .functions
        .iter()
        .find(|f| f.symbol == symbol)
        .unwrap()
        .clone();
    let ret = source.signature.result;
    compilation.module.functions.push(ir::Function {
        definition: DefId(u32::MAX),
        symbol: "__biomiya_test_entry".into(),
        signature: source.signature.clone(),
        parameters: vec![],
        slots: vec![ir::Slot {
            ty: ret,
            name: None,
            span: span(),
            scope: ScopeId(0),
        }],
        blocks: vec![ir::Block {
            instructions: vec![ir::Instruction {
                kind: ir::InstructionKind::Call {
                    target: ir::CallTarget::Direct(source.symbol),
                    signature: source.signature,
                    arguments: vec![],
                    result: Some(ir::SlotId(0)),
                },
                span: span(),
            }],
            terminator: ir::Terminator::Return(Some(ir::Operand::Slot(ir::SlotId(0)))),
            span: span(),
        }],
        entry: ir::BlockId(0),
        span: span(),
        special: None,
        exported: true,
        section: None,
    });
    let mut emission =
        biomiya::target::rv64::emit(&mut compilation.types, &compilation.module).unwrap();
    if let Some(entry) = compilation
        .module
        .functions
        .iter()
        .find(|f| f.special.as_deref() == Some("trap_entry"))
    {
        emission.assembly.push_str(&format!(
            "\n.text\n.globl __biomiya_trap_entry\n__biomiya_trap_entry:\nla t0, {}\njr t0\n",
            biomiya::linkage::asm_symbol(&entry.symbol)
        ));
    }
    let directory = std::env::temp_dir().join(format!(
        "biomiya-lower-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let elf = directory.join("program.elf");
    biomiya::toolchain::build(
        &emission.assembly,
        &emission.linkage,
        &biomiya::toolchain::Options {
            clang: "clang".into(),
            linker: "ld.lld".into(),
            sysroot: biomiya::toolchain::default_sysroot(),
            platform: Some("qemu-virt-core-v1".into()),
            script: None,
            objects: vec![],
            output: elf.clone(),
            emit: biomiya::toolchain::Emit::Elf,
            test: true,
            keep_temporaries: false,
        },
    )
    .unwrap();
    let output=std::process::Command::new("python3").arg("-c").arg("import sys,pathlib,json;sys.path.insert(0,'tools');import test_runtime as t;print(json.dumps(t.run(pathlib.Path(sys.argv[1]))))").arg(&elf).current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "pass", "{report}");
    assert_eq!(report["value"], expected, "{report}");
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn full_expression_temporaries_end_before_next_statement() {
    let c = source_compilation("pub fn test()->i32 {let x=1+2;let y=x+4;y}");
    let f = &c.module.functions[0];
    let instructions: Vec<_> = f
        .blocks
        .iter()
        .flat_map(|b| b.instructions.iter())
        .collect();
    let first_end = instructions
        .iter()
        .position(|i| {
            matches!(
                i.kind,
                InstructionKind::Event(SemanticEvent::FullExpression {
                    entering: false,
                    ..
                })
            )
        })
        .unwrap();
    assert!(instructions[..first_end].iter().any(|i| matches!(
        i.kind,
        InstructionKind::Event(SemanticEvent::StorageDead(_))
    )));
    execute_source("pub fn test()->i32 {let x=1+2;let y=x+4;y}", 7);
}
#[test]
fn replacement_snapshot_and_checked_cast_execute_on_target() {
    execute_source(
        "pub fn test()->i32 {var x=3;var y=4;let old=replace(&mut x,9);swap(&mut x,&mut y);old+x}",
        7,
    );
}
#[test]
fn checked_cast_boundaries_do_not_panic() {
    execute_source(
        "pub fn test()->i32 {let a=match checked_cast::<u8>(-1_i32){Some(x)=>x as i32,None=>7};let b=match checked_cast::<i8>(128_u32){Some(x)=>x as i32,None=>9};a+b}",
        16,
    );
}
#[test]
fn borrowed_match_mutates_original_payload() {
    execute_source(
        "pub fn test()->i32 {var value:Option<i32>=Some(3);match &mut value {Some(item)=>{*item=9;},None=>{}};match value {Some(item)=>item,None=>0}}",
        9,
    );
}
#[test]
fn comparisons_do_not_move_mutable_reference() {
    let c = source_compilation(
        "pub fn test()->i32 {var value=3;let a=&mut value;let same=a==a;if same{1}else{0}}",
    );
    let f = &c.module.functions[0];
    let moves = f
        .blocks
        .iter()
        .flat_map(|b| b.instructions.iter())
        .filter(|i| matches!(&i.kind,InstructionKind::Event(SemanticEvent::Move(ir::Place{base:ir::PlaceBase::Slot(id),..})) if f.slots[id.0 as usize].name.as_deref()==Some("a")))
        .count();
    assert_eq!(moves, 0);
    execute_source(
        "pub fn test()->i32 {var value=3;let a=&mut value;let same=a==a;if same{1}else{0}}",
        1,
    );
}
#[test]
fn dereference_and_shared_reborrow_inspect_mutable_reference() {
    let text = "pub fn test()->i32 {var value=3;let a=&mut value;let b:&i32=a;*b+*a}";
    let c = source_compilation(text);
    let f = &c.module.functions[0];
    let instructions: Vec<_> = f
        .blocks
        .iter()
        .flat_map(|b| b.instructions.iter())
        .collect();
    assert!(!instructions.iter().any(|i|matches!(&i.kind,InstructionKind::Event(SemanticEvent::Move(ir::Place{base:ir::PlaceBase::Slot(id),..})) if f.slots[id.0 as usize].name.as_deref()==Some("a"))));
    assert!(instructions.iter().any(|i|matches!(&i.kind,InstructionKind::Event(SemanticEvent::Inspect(ir::Place{base:ir::PlaceBase::Slot(id),..})) if f.slots[id.0 as usize].name.as_deref()==Some("a"))));
    assert!(instructions.iter().any(|i| matches!(
        &i.kind,
        InstructionKind::Event(SemanticEvent::Borrow {
            place: ir::Place {
                base: ir::PlaceBase::Pointer(_),
                ..
            },
            mutable: false,
            reborrow: true,
            ..
        })
    )));
    execute_source(text, 6);
}
#[test]
fn ordinary_by_value_mutable_reference_still_moves() {
    let c = source_compilation(
        "fn consume(x:&mut i32)->i32{*x} pub fn test()->i32{var value=3;let a=&mut value;consume(a)}",
    );
    let f = c
        .module
        .functions
        .iter()
        .find(|f| f.slots.iter().any(|s| s.name.as_deref() == Some("a")))
        .unwrap();
    assert!(f.blocks.iter().flat_map(|b|b.instructions.iter()).any(|i|matches!(&i.kind,InstructionKind::Event(SemanticEvent::Move(ir::Place{base:ir::PlaceBase::Slot(id),..})) if f.slots[id.0 as usize].name.as_deref()==Some("a"))));
}
#[test]
fn readonly_slice_weakening_inspects_descriptor() {
    let text =
        "pub fn test()->i32{var data=[3,4];let a=data[0..data.len];let b:[]const i32=a;b[0]+a[1]}";
    let c = source_compilation(text);
    let f = &c.module.functions[0];
    assert!(!f.blocks.iter().flat_map(|b|b.instructions.iter()).any(|i|matches!(&i.kind,InstructionKind::Event(SemanticEvent::Move(ir::Place{base:ir::PlaceBase::Slot(id),..})) if f.slots[id.0 as usize].name.as_deref()==Some("a"))));
    execute_source(text, 7);
}
#[test]
fn assume_init_consumes_wrapper_once() {
    let text = "pub fn test()->i32{var storage:MaybeUninit<i32>=MaybeUninit::uninit();storage.write(7);unsafe{storage.assume_init()}}";
    let c = source_compilation(text);
    let f = &c.module.functions[0];
    let count=f.blocks.iter().flat_map(|b|b.instructions.iter()).filter(|i|matches!(&i.kind,InstructionKind::Event(SemanticEvent::Move(ir::Place{base:ir::PlaceBase::Slot(id),..})) if f.slots[id.0 as usize].name.as_deref()==Some("storage"))).count();
    assert_eq!(count, 1);
    execute_source(text, 7);
}
#[test]
fn huge_zero_sized_repeat_and_equality_need_no_element_loop() {
    execute_source(
        "pub fn test()->i32{let a=[();18446744073709551615_usize];let b=[();18446744073709551615_usize];if a==b{1}else{0}}",
        1,
    );
}
#[test]
fn custom_executable_section_can_trap_and_return() {
    execute_source(
        "@trap fn handler(frame:&mut arch::TrapFrame)->(){frame.sepc+=4;} @trap_entry fn entry(frame:&mut arch::TrapFrame)->!{arch::enter_trap_handler(frame,handler)} @section(\".custom.exec\") pub fn test()->i32{unsafe{arch::asm(\"ecall\");}7}",
        7,
    );
}

#[test]
fn checked_generic_lengths_construct_and_project_each_instance() {
    execute_source(include_str!("audit/generic_dependent_instantiation.vt"), 27);
}
