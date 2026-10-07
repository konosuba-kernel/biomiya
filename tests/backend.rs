use biomiya::{
    ir::*,
    source::{FileId, Span},
    target::rv64::emit,
    types::*,
};
use std::{collections::BTreeSet, process::Command};
fn span() -> Span {
    Span::new(FileId(0), 0, 0)
}
fn function(types: &mut TypeContext, op: &str, a: u64, b: u64, integer: Integer) -> Function {
    let ty = types.intern(TypeKind::Int(integer));
    let ret = types.intern(TypeKind::Int(Integer::I32));
    let result = SlotId(0);
    Function {
        definition: DefId(0),
        symbol: "__biomiya_test_entry".into(),
        signature: FnType {
            params: vec![],
            result: ret,
            abi: Abi::C,
            unsafe_: false,
            effects: BTreeSet::new(),
        },
        parameters: vec![],
        slots: vec![
            Slot {
                ty,
                name: None,
                span: span(),
                scope: ScopeId(0),
            },
            Slot {
                ty: ret,
                name: None,
                span: span(),
                scope: ScopeId(0),
            },
        ],
        blocks: vec![Block {
            instructions: vec![
                Instruction {
                    kind: InstructionKind::Set {
                        dst: result,
                        value: Rvalue::Binary {
                            op: op.into(),
                            left: Operand::Integer { bits: a, ty },
                            right: Operand::Integer { bits: b, ty },
                        },
                    },
                    span: span(),
                },
                Instruction {
                    kind: InstructionKind::Set {
                        dst: SlotId(1),
                        value: Rvalue::Cast {
                            value: Operand::Slot(result),
                        },
                    },
                    span: span(),
                },
            ],
            terminator: Terminator::Return(Some(Operand::Slot(SlotId(1)))),
            span: span(),
        }],
        entry: BlockId(0),
        span: span(),
        special: None,
        section: None,
        exported: true,
    }
}
fn run(assembly: &str) -> String {
    run_with_c(assembly, None)
}
fn run_with_c(assembly: &str, c: Option<&str>) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!(
        "biomiya-backend-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let program = dir.join("program.S");
    std::fs::write(&program, assembly).unwrap();
    let mut inputs = vec![
        root.join("platforms/qemu-virt/start.S"),
        root.join("runtime/rv64/trap.S"),
        root.join("runtime/rv64/runtime.c"),
        program,
    ];
    if let Some(c) = c {
        let path = dir.join("interop.c");
        std::fs::write(&path, c).unwrap();
        inputs.push(path);
    }
    let mut objects = vec![];
    for (n, input) in inputs.iter().enumerate() {
        let object = dir.join(format!("{n}.o"));
        let out = Command::new("clang")
            .args([
                "--target=riscv64-unknown-elf",
                "-march=rv64ima_zicsr_zifencei",
                "-mabi=lp64",
                "-mno-relax",
                "-mcmodel=medany",
                "-ffreestanding",
                "-fno-builtin",
                "-fno-stack-protector",
                "-O1",
                "-I",
            ])
            .arg(root.join("runtime/rv64"))
            .arg("-c")
            .arg(input)
            .arg("-o")
            .arg(&object)
            .output()
            .expect("clang needed for backend execution tests");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        objects.push(object);
    }
    let elf = dir.join("program.elf");
    let out = Command::new("ld.lld")
        .args(["-m", "elf64lriscv", "--no-relax", "-T"])
        .arg(root.join("platforms/qemu-virt/link.ld"))
        .args(objects)
        .arg("-o")
        .arg(&elf)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = Command::new("timeout")
        .args([
            "10",
            "qemu-system-riscv64",
            "-machine",
            "virt",
            "-accel",
            "tcg",
            "-cpu",
            "rv64",
            "-m",
            "128M",
            "-smp",
            "1",
            "-bios",
            "none",
            "-display",
            "none",
            "-monitor",
            "none",
            "-serial",
            "stdio",
            "-kernel",
        ])
        .arg(&elf)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "QEMU failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    text
}
#[test]
fn arithmetic_checked_wrapping_and_panic_execute() {
    for (op, a, b, i, expected) in [
        ("+", 40, 2, Integer::I32, "BIOMIYA:PASS:42"),
        ("+%", 255, 1, Integer::U8, "BIOMIYA:PASS:0"),
        ("+", 127, 1, Integer::I8, "BIOMIYA:PANIC"),
        ("%", 43, 5, Integer::U32, "BIOMIYA:PASS:3"),
    ] {
        let mut types = TypeContext::new();
        let f = function(&mut types, op, a, b, i);
        let e = emit(
            &mut types,
            &Module {
                functions: vec![f],
                ..Module::default()
            },
        )
        .unwrap();
        assert!(run(&e.assembly).contains(expected));
    }
}
#[test]
fn c_register_stack_arguments_execute() {
    let mut types = TypeContext::new();
    let i = types.intern(TypeKind::Int(Integer::I32));
    let mut entry = function(&mut types, "+", 0, 0, Integer::I32);
    entry.blocks[0].instructions = vec![Instruction {
        kind: InstructionKind::Call {
            target: CallTarget::Direct("sum_ten".into()),
            signature: FnType {
                params: vec![i; 10],
                result: i,
                abi: Abi::C,
                unsafe_: false,
                effects: BTreeSet::new(),
            },
            arguments: (1..=10)
                .map(|bits| Operand::Integer { bits, ty: i })
                .collect(),
            result: Some(SlotId(1)),
        },
        span: span(),
    }];
    let callee = Function {
        definition: DefId(1),
        symbol: "sum_ten".into(),
        signature: FnType {
            params: vec![i; 10],
            result: i,
            abi: Abi::C,
            unsafe_: false,
            effects: BTreeSet::new(),
        },
        parameters: (0..10).map(SlotId).collect(),
        slots: (0..11)
            .map(|_| Slot {
                ty: i,
                name: None,
                span: span(),
                scope: ScopeId(0),
            })
            .collect(),
        blocks: vec![Block {
            instructions: vec![Instruction {
                kind: InstructionKind::Set {
                    dst: SlotId(10),
                    value: Rvalue::Binary {
                        op: "+".into(),
                        left: Operand::Slot(SlotId(8)),
                        right: Operand::Slot(SlotId(9)),
                    },
                },
                span: span(),
            }],
            terminator: Terminator::Return(Some(Operand::Slot(SlotId(10)))),
            span: span(),
        }],
        entry: BlockId(0),
        span: span(),
        special: None,
        section: None,
        exported: false,
    };
    let e = emit(
        &mut types,
        &Module {
            functions: vec![entry, callee],
            ..Module::default()
        },
    )
    .unwrap();
    assert!(run(&e.assembly).contains("BIOMIYA:PASS:19"));
    assert_eq!(e.frames[0].outgoing_bytes, 16);
}
fn slot(ty: TyId) -> Slot {
    Slot {
        ty,
        name: None,
        span: span(),
        scope: ScopeId(0),
    }
}
fn place(id: u32, ty: TyId, offset: u64, align: u64) -> Place {
    Place {
        base: PlaceBase::Slot(SlotId(id)),
        offset,
        ty,
        align,
    }
}
fn instruction(kind: InstructionKind) -> Instruction {
    Instruction { kind, span: span() }
}
fn c_struct(
    types: &mut TypeContext,
    id: u32,
    count: usize,
    ty: TyId,
    align: Option<u64>,
    repr: bool,
) -> TyId {
    types.define(
        DefId(id),
        TypeDef {
            name: format!("S{id}"),
            params: 0,
            kind: DefKind::Struct(
                (0..count)
                    .map(|n| FieldDef {
                        name: n.to_string(),
                        ty,
                        span: span(),
                    })
                    .collect(),
            ),
            copy: false,
            resource: false,
            repr_c: repr,
            packed: false,
            align,
            span: span(),
            layout_dependencies: vec![],
        },
    );
    types.intern(TypeKind::Nominal(DefId(id), vec![]))
}
#[test]
fn c_aggregate_register_split_and_indirect_return_interoperate() {
    let mut t = TypeContext::new();
    let u = t.intern(TypeKind::Int(Integer::U64));
    let i = t.intern(TypeKind::Int(Integer::I32));
    let pair = c_struct(&mut t, 10, 2, u, None, true);
    let triple = c_struct(&mut t, 11, 3, u, None, true);
    let mut f = function(&mut t, "+", 0, 0, Integer::I32);
    f.slots = vec![slot(pair), slot(triple), slot(u), slot(i)];
    let mut params = vec![u; 6];
    params.push(pair);
    f.blocks[0].instructions = vec![
        instruction(InstructionKind::Store {
            dst: place(0, u, 0, 8),
            value: Operand::Integer { bits: 20, ty: u },
        }),
        instruction(InstructionKind::Store {
            dst: place(0, u, 8, 8),
            value: Operand::Integer { bits: 22, ty: u },
        }),
        instruction(InstructionKind::Call {
            target: CallTarget::Direct("split_pair".into()),
            signature: FnType {
                params,
                result: triple,
                abi: Abi::C,
                unsafe_: false,
                effects: BTreeSet::new(),
            },
            arguments: (0..6)
                .map(|_| Operand::Integer { bits: 0, ty: u })
                .chain([Operand::Slot(SlotId(0))])
                .collect(),
            result: Some(SlotId(1)),
        }),
        instruction(InstructionKind::Set {
            dst: SlotId(2),
            value: Rvalue::Load(place(1, u, 16, 8)),
        }),
        instruction(InstructionKind::Set {
            dst: SlotId(3),
            value: Rvalue::Cast {
                value: Operand::Slot(SlotId(2)),
            },
        }),
    ];
    f.blocks[0].terminator = Terminator::Return(Some(Operand::Slot(SlotId(3))));
    let e = emit(
        &mut t,
        &Module {
            functions: vec![f],
            ..Module::default()
        },
    )
    .unwrap();
    let c = "typedef unsigned long U; struct P{U a,b;};struct T{U a,b,c;};struct T split_pair(U x0,U x1,U x2,U x3,U x4,U x5,struct P p){return(struct T){p.a,p.b,p.a+p.b};}";
    assert!(run_with_c(&e.assembly, Some(c)).contains("BIOMIYA:PASS:42"));
}
#[test]
fn ordinary_aggregate_snapshot_sret_and_4096_stack_alignment() {
    let mut t = TypeContext::new();
    let u = t.intern(TypeKind::Int(Integer::U64));
    let i = t.intern(TypeKind::Int(Integer::I32));
    let aligned = c_struct(&mut t, 12, 1, u, Some(4096), false);
    let mut entry = function(&mut t, "+", 0, 0, Integer::I32);
    entry.slots = vec![slot(aligned), slot(aligned), slot(u), slot(i)];
    let signature = FnType {
        params: vec![aligned],
        result: aligned,
        abi: Abi::Biomiya,
        unsafe_: false,
        effects: BTreeSet::new(),
    };
    entry.blocks[0].instructions = vec![
        instruction(InstructionKind::Store {
            dst: place(0, u, 0, 8),
            value: Operand::Integer { bits: 42, ty: u },
        }),
        instruction(InstructionKind::Call {
            target: CallTarget::Direct("copy_aligned".into()),
            signature: signature.clone(),
            arguments: vec![Operand::Slot(SlotId(0))],
            result: Some(SlotId(1)),
        }),
        instruction(InstructionKind::Set {
            dst: SlotId(2),
            value: Rvalue::Load(place(1, u, 0, 8)),
        }),
        instruction(InstructionKind::Set {
            dst: SlotId(3),
            value: Rvalue::Cast {
                value: Operand::Slot(SlotId(2)),
            },
        }),
    ];
    entry.blocks[0].terminator = Terminator::Return(Some(Operand::Slot(SlotId(3))));
    let callee = Function {
        definition: DefId(13),
        symbol: "copy_aligned".into(),
        signature,
        parameters: vec![SlotId(0)],
        slots: vec![slot(aligned)],
        blocks: vec![Block {
            instructions: vec![],
            terminator: Terminator::Return(Some(Operand::Slot(SlotId(0)))),
            span: span(),
        }],
        entry: BlockId(0),
        span: span(),
        special: None,
        section: None,
        exported: false,
    };
    let e = emit(
        &mut t,
        &Module {
            functions: vec![entry, callee],
            ..Module::default()
        },
    )
    .unwrap();
    assert_eq!(e.frames[0].alignment, 4096);
    assert!(run(&e.assembly).contains("BIOMIYA:PASS:42"));
}
#[test]
fn narrow_atomic_storage_and_cas_result_execute() {
    let mut t = TypeContext::new();
    let u = t.intern(TypeKind::Int(Integer::U8));
    let p = t.intern(TypeKind::Pointer {
        target: u,
        space: AddressSpace::Kernel,
        const_: false,
    });
    let order = t.intern(TypeKind::Int(Integer::Usize));
    let result = t.intern(TypeKind::Result(u, u));
    let i = t.intern(TypeKind::Int(Integer::I32));
    let mut f = function(&mut t, "+", 0, 0, Integer::I32);
    f.slots = vec![slot(p), slot(u), slot(result), slot(i)];
    let address = Operand::Symbol {
        name: "atomic_word".into(),
        addend: 0,
        ty: p,
    };
    let literal = |bits, ty| Operand::Integer { bits, ty };
    f.blocks[0].instructions = vec![
        instruction(InstructionKind::Intrinsic {
            name: "atomic.fetch_add".into(),
            arguments: vec![address.clone(), literal(1, u), literal(4, order)],
            result: Some(SlotId(1)),
            type_arguments: vec![u],
        }),
        instruction(InstructionKind::Intrinsic {
            name: "atomic.compare_exchange".into(),
            arguments: vec![
                address.clone(),
                literal(0, u),
                literal(42, u),
                literal(4, order),
                literal(4, order),
            ],
            result: Some(SlotId(2)),
            type_arguments: vec![u],
        }),
        instruction(InstructionKind::Intrinsic {
            name: "atomic.load".into(),
            arguments: vec![address, literal(4, order)],
            result: Some(SlotId(1)),
            type_arguments: vec![u],
        }),
        instruction(InstructionKind::Set {
            dst: SlotId(3),
            value: Rvalue::Cast {
                value: Operand::Slot(SlotId(1)),
            },
        }),
    ];
    f.blocks[0].terminator = Terminator::Return(Some(Operand::Slot(SlotId(3))));
    let data = Data {
        symbol: "atomic_word".into(),
        bytes: vec![255, 0, 0, 0],
        relocations: vec![],
        align: 4,
        writable: true,
        exported: false,
        section: None,
    };
    let e = emit(
        &mut t,
        &Module {
            functions: vec![f],
            data: vec![data],
            ..Module::default()
        },
    )
    .unwrap();
    assert!(run(&e.assembly).contains("BIOMIYA:PASS:42"));
}
#[test]
fn arbitrary_export_name_and_asm_output_registers_execute() {
    let mut t = TypeContext::new();
    let mut f = function(&mut t, "+", 0, 0, Integer::I32);
    let i = f.signature.result;
    f.blocks[0].instructions = vec![instruction(InstructionKind::Asm {
        template: "add t6, t0, t0".into(),
        inputs: vec![AsmInput {
            register: "t0".into(),
            value: Operand::Integer { bits: 21, ty: i },
        }],
        outputs: vec![AsmOutput {
            register: "t6".into(),
            place: place(1, i, 0, 4),
        }],
        clobbers: vec![],
        may_block: false,
    })];
    let mut caller = function(&mut t, "+", 0, 0, Integer::I32);
    f.symbol = "strange function name".into();
    caller.blocks[0].instructions = vec![instruction(InstructionKind::Call {
        target: CallTarget::Direct(f.symbol.clone()),
        signature: f.signature.clone(),
        arguments: vec![],
        result: Some(SlotId(1)),
    })];
    let e = emit(
        &mut t,
        &Module {
            functions: vec![caller, f],
            ..Module::default()
        },
    )
    .unwrap();
    assert!(run(&e.assembly).contains("BIOMIYA:PASS:42"));
}
#[test]
fn aggregate_equality_ignores_padding_and_inactive_enum_payload() {
    let mut t = TypeContext::new();
    let byte = t.intern(TypeKind::Int(Integer::U8));
    let word = t.intern(TypeKind::Int(Integer::U64));
    let boolean = t.intern(TypeKind::Bool);
    let i = t.intern(TypeKind::Int(Integer::I32));
    let tuple = t.intern(TypeKind::Tuple(vec![byte, word]));
    let option = t.intern(TypeKind::Option(word));
    let mut f = function(&mut t, "+", 0, 0, Integer::I32);
    f.slots = vec![
        slot(tuple),
        slot(tuple),
        slot(option),
        slot(option),
        slot(boolean),
        slot(boolean),
        slot(i),
        slot(i),
    ];
    let lit = |bits, ty| Operand::Integer { bits, ty };
    f.blocks[0].instructions = vec![
        instruction(InstructionKind::Zero {
            dst: place(0, tuple, 0, 8),
            size: 16,
        }),
        instruction(InstructionKind::Zero {
            dst: place(1, tuple, 0, 8),
            size: 16,
        }),
        instruction(InstructionKind::Store {
            dst: place(0, byte, 3, 1),
            value: lit(99, byte),
        }),
        instruction(InstructionKind::Set {
            dst: SlotId(4),
            value: Rvalue::Binary {
                op: "==".into(),
                left: Operand::Slot(SlotId(0)),
                right: Operand::Slot(SlotId(1)),
            },
        }),
        instruction(InstructionKind::Zero {
            dst: place(2, option, 0, 8),
            size: 16,
        }),
        instruction(InstructionKind::Zero {
            dst: place(3, option, 0, 8),
            size: 16,
        }),
        instruction(InstructionKind::Store {
            dst: place(2, i, 0, 4),
            value: lit(1, i),
        }),
        instruction(InstructionKind::Store {
            dst: place(3, i, 0, 4),
            value: lit(1, i),
        }),
        instruction(InstructionKind::Store {
            dst: place(2, word, 8, 8),
            value: lit(99, word),
        }),
        instruction(InstructionKind::Set {
            dst: SlotId(5),
            value: Rvalue::Binary {
                op: "==".into(),
                left: Operand::Slot(SlotId(2)),
                right: Operand::Slot(SlotId(3)),
            },
        }),
        instruction(InstructionKind::Set {
            dst: SlotId(6),
            value: Rvalue::Cast {
                value: Operand::Slot(SlotId(4)),
            },
        }),
        instruction(InstructionKind::Set {
            dst: SlotId(7),
            value: Rvalue::Cast {
                value: Operand::Slot(SlotId(5)),
            },
        }),
        instruction(InstructionKind::Set {
            dst: SlotId(6),
            value: Rvalue::Binary {
                op: "+".into(),
                left: Operand::Slot(SlotId(6)),
                right: Operand::Slot(SlotId(7)),
            },
        }),
    ];
    f.blocks[0].terminator = Terminator::Return(Some(Operand::Slot(SlotId(6))));
    let e = emit(
        &mut t,
        &Module {
            functions: vec![f],
            ..Module::default()
        },
    )
    .unwrap();
    assert!(run(&e.assembly).contains("BIOMIYA:PASS:2"));
}
#[test]
fn integration_fixtures_have_valid_core_syntax() {
    for file in std::fs::read_dir("tests/integration").unwrap() {
        let path = file.unwrap().path();
        if path.extension().and_then(|x| x.to_str()) == Some("vt") {
            let source = std::fs::read_to_string(&path).unwrap();
            assert!(
                biomiya::syntax::parse(FileId(0), &source).is_ok(),
                "{}: {:?}",
                path.display(),
                biomiya::syntax::parse(FileId(0), &source).err()
            );
        }
    }
}
#[test]
fn link_names_preserve_bytes_and_reject_malformed_objects() {
    use biomiya::linkage::{
        LinkageNames, NameMapping, encode_name, name_bytes, restore_object_names, symbol_alias,
    };
    let first = encode_name(b"byte_\xff");
    let second = encode_name(b"byte_\xfe");
    assert_ne!(first, second);
    assert_ne!(symbol_alias(&first), symbol_alias(&second));
    assert_eq!(name_bytes(&first).as_ref(), b"byte_\xff");
    for name in [
        b"quote\"slash\\".as_slice(),
        "汉字".as_bytes(),
        b"line\nbreak",
        b"byte_\xff",
    ] {
        assert_eq!(name_bytes(&encode_name(name)).as_ref(), name);
    }
    let names = LinkageNames {
        symbols: vec![NameMapping {
            alias: symbol_alias(&first),
            bytes: b"byte_\xff".to_vec(),
        }],
        sections: vec![],
    };
    assert_eq!(
        restore_object_names(b"not ELF", &names).unwrap_err().code,
        "E_LINK"
    );
    let mut corrupt = vec![0; 64];
    corrupt[..6].copy_from_slice(b"\x7fELF\x02\x01");
    corrupt[16..18].copy_from_slice(&1u16.to_le_bytes());
    corrupt[18..20].copy_from_slice(&243u16.to_le_bytes());
    corrupt[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(restore_object_names(&corrupt, &names).is_err());
}
#[test]
fn malformed_extended_section_counts_return_link_errors_before_allocation() {
    use biomiya::linkage::{LinkageNames, NameMapping, restore_object_names};
    let names = LinkageNames {
        symbols: vec![NameMapping {
            alias: "__biomiya_bytes_ff".into(),
            bytes: vec![255],
        }],
        sections: vec![],
    };
    for count in [u64::MAX, 2, 65_535] {
        let mut object = vec![0; 128];
        object[..6].copy_from_slice(b"\x7fELF\x02\x01");
        object[16..18].copy_from_slice(&1u16.to_le_bytes());
        object[18..20].copy_from_slice(&243u16.to_le_bytes());
        object[40..48].copy_from_slice(&64u64.to_le_bytes());
        object[58..60].copy_from_slice(&64u16.to_le_bytes());
        // e_shnum=0 selects the extended count in section zero's sh_size.
        object[96..104].copy_from_slice(&count.to_le_bytes());
        assert_eq!(
            restore_object_names(&object, &names).unwrap_err().code,
            "E_LINK"
        );
    }
}
