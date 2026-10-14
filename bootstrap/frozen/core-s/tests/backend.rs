//! Independent typed-IR tests; no parser/checker or archived compiler dependency.
//! Machine tests assemble/link fresh emitted RV64 and execute a small M-mode
//! QEMU fixture. The pipeline's real-source/S-mode acceptance belongs to I.

use biomiya_core_s::{
    ast::{BinaryOp as Op, UnaryOp},
    backend::emit,
    model::*,
    source::Span,
};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const U8: TypeId = TypeId(0);
const U32: TypeId = TypeId(1);
const USIZE: TypeId = TypeId(2);
const BOOL: TypeId = TypeId(3);
const UNIT: TypeId = TypeId(4);

fn table() -> TypeTable {
    let mut table = TypeTable::default();
    for (kind, size, align) in [
        (TypeKind::Int(Integer::U8), 1, 1),
        (TypeKind::Int(Integer::U32), 4, 4),
        (TypeKind::Int(Integer::Usize), 8, 8),
        (TypeKind::Bool, 1, 1),
        (TypeKind::Unit, 0, 1),
    ] {
        table.entries.push(TypeInfo {
            kind,
            layout: Layout {
                size,
                align,
                field_offsets: vec![],
                payload_offset: None,
            },
        });
    }
    table
}

fn ty(
    table: &mut TypeTable,
    kind: TypeKind,
    size: u64,
    align: u64,
    fields: Vec<u64>,
    payload: Option<u64>,
) -> TypeId {
    table
        .intern(
            kind,
            Layout {
                size,
                align,
                field_offsets: fields,
                payload_offset: payload,
            },
            Span::default(),
        )
        .unwrap()
}

fn imm(ty: TypeId, bits: u64) -> Operand {
    Operand::Immediate { bits, ty }
}
fn slot(id: u32) -> Operand {
    Operand::Slot(SlotId(id))
}
fn place(id: u32, offset: u64, ty: TypeId) -> Place {
    Place {
        base: PlaceBase::Slot(SlotId(id)),
        byte_offset: offset,
        ty,
    }
}
fn set(id: u32, value: Rvalue) -> InstructionKind {
    InstructionKind::Set {
        dst: SlotId(id),
        value,
    }
}
fn store(destination: Place, value: Operand) -> InstructionKind {
    InstructionKind::Store { destination, value }
}
fn binary(id: u32, op: Op, left: Operand, right: Operand) -> InstructionKind {
    set(id, Rvalue::Binary { op, left, right })
}
fn call(id: u32, arguments: Vec<Operand>, result: Option<u32>) -> InstructionKind {
    InstructionKind::Call {
        function: FunctionId(id),
        arguments,
        result: result.map(SlotId),
    }
}
fn block(instructions: Vec<InstructionKind>, terminator: Terminator) -> Block {
    Block {
        instructions: instructions
            .into_iter()
            .map(|kind| Instruction {
                kind,
                span: Span::default(),
            })
            .collect(),
        terminator,
        span: Span::default(),
    }
}

fn function(
    id: u32,
    parameters: Vec<TypeId>,
    result: TypeId,
    slots: Vec<TypeId>,
    blocks: Vec<Block>,
) -> Function {
    Function {
        name: format!("f{id}"),
        symbol: format!("__core_s_f{id}"),
        parameter_slots: (0..parameters.len() as u32).map(SlotId).collect(),
        signature: Signature {
            parameters,
            result,
            unsafe_: false,
        },
        slots: slots
            .into_iter()
            .map(|ty| Slot {
                ty,
                name: None,
                span: Span::default(),
            })
            .collect(),
        blocks,
        entry: BlockId(0),
        span: Span::default(),
    }
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "core-s-backend-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn command(command: &mut Command) {
    let output = command
        .output()
        .expect("required RV64 tool must be installed");
    assert!(
        output.status.success(),
        "{command:?}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// Some(expected a0) requires a normal return; None requires the panic hook.
fn machine(assembly: &str, expected: Option<u64>) {
    let scratch = Scratch::new();
    let normal = if let Some(value) = expected {
        format!(
            "li t0, {}\nbne a0, t0, __test_fail\nj __test_pass",
            value as i64
        )
    } else {
        "j __test_fail".into()
    };
    let panic = if expected.is_none() {
        "j __test_pass"
    } else {
        "j __test_fail"
    };
    let fixture = format!(
        ".section .text.start,\"ax\"\n.globl _start\n_start:\nla sp, __test_stack_end\ncall __core_s_f0\n{normal}\n\
         .globl __core_s_panic\n__core_s_panic:\n{panic}\n\
         __test_pass:\nli t1, 0x5555\nj __test_stop\n\
         __test_fail:\nli t1, 0x13333\n\
         __test_stop:\nli t0, 0x100000\nsw t1, 0(t0)\n\
         __test_halt:\nwfi\nj __test_halt\n\
         .section .bss,\"aw\",@nobits\n.balign 16\n.space 65536\n__test_stack_end:\n"
    );
    fs::write(
        scratch.0.join("program.S"),
        format!("{assembly}\n{fixture}"),
    )
    .unwrap();
    fs::write(scratch.0.join("link.ld"), "ENTRY(_start)\nSECTIONS { . = 0x80000000; .text : { *(.text.start) *(.text*) } .rodata : { *(.rodata*) } .data : { *(.data*) } .bss : { *(.bss*) } }\n").unwrap();
    command(Command::new("clang").current_dir(&scratch.0).args([
        "--target=riscv64-unknown-elf",
        "-march=rv64ima_zicsr_zifencei",
        "-mabi=lp64",
        "-mno-relax",
        "-c",
        "program.S",
        "-o",
        "program.o",
    ]));
    command(Command::new("ld.lld").current_dir(&scratch.0).args([
        "-m",
        "elf64lriscv",
        "--no-relax",
        "--no-undefined",
        "-T",
        "link.ld",
        "program.o",
        "-o",
        "program.elf",
    ]));
    let mut child = Command::new("qemu-system-riscv64")
        .current_dir(&scratch.0)
        .args([
            "-machine",
            "virt",
            "-bios",
            "none",
            "-m",
            "128M",
            "-display",
            "none",
            "-monitor",
            "none",
            "-serial",
            "none",
            "-kernel",
            "program.elf",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("QEMU is required");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "RV64 execution timed out: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "RV64 assertion failed, expected {expected:?}: {}\n{assembly}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn arithmetic(ty: TypeId, op: Op, left: u64, right: u64, expected: Option<u64>) {
    let result = if matches!(op, Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge) {
        BOOL
    } else {
        ty
    };
    let compilation = Compilation {
        data: vec![],
        externs: Default::default(),
        types: table(),
        functions: vec![function(
            0,
            vec![],
            result,
            vec![result],
            vec![block(
                vec![binary(0, op, imm(ty, left), imm(ty, right))],
                Terminator::Return(slot(0)),
            )],
        )],
    };
    let expected = expected.map(|n| {
        if result == U32 {
            n as u32 as i32 as i64 as u64
        } else {
            n
        }
    });
    machine(&emit(&compilation).unwrap(), expected);
}

#[test]
fn integer_widths_wrapping_comparisons_and_lp64_results_execute() {
    for (ty, op, a, b, result) in [
        (U8, Op::Add, 250, 5, 255),
        (U32, Op::Add, 0xffff_fffe, 1, 0xffff_ffff),
        (USIZE, Op::Add, u64::MAX - 1, 1, u64::MAX),
        (U8, Op::WrappingAdd, 255, 1, 0),
        (U32, Op::WrappingSub, 0, 1, 0xffff_ffff),
        (USIZE, Op::WrappingMul, u64::MAX, 2, u64::MAX - 1),
        (U32, Op::Mul, 65536, 65535, 4294901760),
        (U32, Op::Div, 100, 7, 14),
        (U8, Op::Rem, 100, 7, 2),
        (U32, Op::Shl, 0x8000_0000, 1, 0),
        (U32, Op::Shr, 0x8000_0000, 31, 1),
        (USIZE, Op::Shl, 1, 63, 1_u64 << 63),
        (U8, Op::BitAnd, 0xa5, 0x0f, 5),
        (U32, Op::BitOr, 0x8000_0000, 1, 0x8000_0001),
        (U8, Op::BitXor, 0x55, 0xaa, 0xff),
        (U32, Op::Eq, u32::MAX as u64, u32::MAX as u64, 1),
        (U32, Op::Ne, 1, 2, 1),
        (U32, Op::Lt, 0xffff_ffff, 1, 0),
        (USIZE, Op::Gt, u64::MAX, 1, 1),
        (U32, Op::Le, 8, 8, 1),
        (U8, Op::Ge, 2, 3, 0),
    ] {
        arithmetic(ty, op, a, b, Some(result));
    }
}

#[test]
fn checked_arithmetic_and_shifts_really_stop() {
    for (ty, op, a, b) in [
        (U8, Op::Add, 255, 1),
        (U32, Op::Add, u32::MAX as u64, 1),
        (USIZE, Op::Add, u64::MAX, 1),
        (U8, Op::Sub, 0, 1),
        (USIZE, Op::Sub, 1, 2),
        (U32, Op::Mul, 65536, 65536),
        (USIZE, Op::Mul, u64::MAX, 2),
        (U8, Op::Div, 1, 0),
        (U32, Op::Rem, 1, 0),
        (U8, Op::Shl, 1, 8),
        (U32, Op::Shr, 1, 32),
        (USIZE, Op::Shl, 1, 64),
    ] {
        arithmetic(ty, op, a, b, None);
    }
}

#[test]
fn unary_casts_unit_comparison_and_unreachable_execute() {
    let cases = [
        (
            U8,
            set(
                0,
                Rvalue::Cast {
                    value: imm(U32, 511),
                },
            ),
            Some(255),
        ),
        (
            U32,
            set(
                0,
                Rvalue::Unary {
                    op: UnaryOp::BitNot,
                    value: imm(U32, 0),
                },
            ),
            Some(u64::MAX),
        ),
        (
            BOOL,
            set(
                0,
                Rvalue::Unary {
                    op: UnaryOp::Not,
                    value: imm(BOOL, 0),
                },
            ),
            Some(1),
        ),
        (BOOL, binary(0, Op::Eq, imm(UNIT, 0), imm(UNIT, 0)), Some(1)),
    ];
    for (result, instruction, expected) in cases {
        let compilation = Compilation {
            data: vec![],
            externs: Default::default(),
            types: table(),
            functions: vec![function(
                0,
                vec![],
                result,
                vec![result],
                vec![block(vec![instruction], Terminator::Return(slot(0)))],
            )],
        };
        machine(&emit(&compilation).unwrap(), expected);
    }
    let compilation = Compilation {
        data: vec![],
        externs: Default::default(),
        types: table(),
        functions: vec![function(
            0,
            vec![],
            UNIT,
            vec![],
            vec![block(vec![], Terminator::Unreachable)],
        )],
    };
    machine(&emit(&compilation).unwrap(), None);
}

#[test]
fn branches_loops_and_recursive_calls_preserve_frames() {
    let loop_function = function(
        0,
        vec![],
        USIZE,
        vec![USIZE, USIZE, BOOL],
        vec![
            block(
                vec![
                    set(0, Rvalue::Use(imm(USIZE, 0))),
                    set(1, Rvalue::Use(imm(USIZE, 0))),
                ],
                Terminator::Jump(BlockId(1)),
            ),
            block(
                vec![binary(2, Op::Lt, slot(0), imm(USIZE, 5))],
                Terminator::Branch {
                    condition: slot(2),
                    yes: BlockId(2),
                    no: BlockId(3),
                },
            ),
            block(
                vec![
                    binary(1, Op::Add, slot(1), slot(0)),
                    binary(0, Op::Add, slot(0), imm(USIZE, 1)),
                ],
                Terminator::Jump(BlockId(1)),
            ),
            block(vec![], Terminator::Return(slot(1))),
        ],
    );
    machine(
        &emit(&Compilation {
            data: vec![],
            externs: Default::default(),
            types: table(),
            functions: vec![loop_function],
        })
        .unwrap(),
        Some(10),
    );
    let entry = function(
        0,
        vec![],
        USIZE,
        vec![USIZE],
        vec![block(
            vec![call(1, vec![imm(USIZE, 5)], Some(0))],
            Terminator::Return(slot(0)),
        )],
    );
    let factorial = function(
        1,
        vec![USIZE],
        USIZE,
        vec![USIZE, USIZE, USIZE, USIZE, BOOL],
        vec![
            block(
                vec![binary(4, Op::Eq, slot(0), imm(USIZE, 0))],
                Terminator::Branch {
                    condition: slot(4),
                    yes: BlockId(1),
                    no: BlockId(2),
                },
            ),
            block(vec![], Terminator::Return(imm(USIZE, 1))),
            block(
                vec![
                    binary(2, Op::Sub, slot(0), imm(USIZE, 1)),
                    call(1, vec![slot(2)], Some(3)),
                    binary(1, Op::Mul, slot(0), slot(3)),
                ],
                Terminator::Return(slot(1)),
            ),
        ],
    );
    machine(
        &emit(&Compilation {
            data: vec![],
            externs: Default::default(),
            types: table(),
            functions: vec![entry, factorial],
        })
        .unwrap(),
        Some(120),
    );
}

#[test]
fn aggregate_parameters_are_independent_and_results_use_hidden_slots() {
    let mut types = table();
    let small = ty(
        &mut types,
        TypeKind::Struct {
            name: "Small".into(),
            fields: vec![Field {
                name: "x".into(),
                ty: U32,
            }],
        },
        4,
        4,
        vec![0],
        None,
    );
    let entry = function(
        0,
        vec![],
        U32,
        vec![small, small, U32, U32, U32],
        vec![block(
            vec![
                store(place(0, 0, U32), imm(U32, 9)),
                call(1, vec![slot(0)], Some(1)),
                set(2, Rvalue::Load(place(0, 0, U32))),
                set(3, Rvalue::Load(place(1, 0, U32))),
                binary(4, Op::Add, slot(2), slot(3)),
            ],
            Terminator::Return(slot(4)),
        )],
    );
    let mutate = function(
        1,
        vec![small],
        small,
        vec![small],
        vec![block(
            vec![store(place(0, 0, U32), imm(U32, 77))],
            Terminator::Return(slot(0)),
        )],
    );
    machine(
        &emit(&Compilation {
            data: vec![],
            externs: Default::default(),
            types,
            functions: vec![entry, mutate],
        })
        .unwrap(),
        Some(86),
    );
}

#[test]
fn hidden_result_unit_omission_and_over_eight_positions_follow_lp64() {
    let mut types = table();
    let small = ty(
        &mut types,
        TypeKind::Struct {
            name: "Byte".into(),
            fields: vec![Field {
                name: "x".into(),
                ty: U8,
            }],
        },
        1,
        1,
        vec![0],
        None,
    );
    let result = ty(
        &mut types,
        TypeKind::Result {
            ok: USIZE,
            error: USIZE,
        },
        16,
        8,
        vec![],
        Some(8),
    );
    let parameters = vec![
        UNIT, U8, U32, BOOL, USIZE, small, small, small, small, small,
    ];
    let entry = function(
        0,
        vec![],
        USIZE,
        vec![small, small, small, small, small, result, USIZE],
        vec![block(
            vec![
                store(place(0, 0, U8), imm(U8, 3)),
                store(place(1, 0, U8), imm(U8, 5)),
                store(place(2, 0, U8), imm(U8, 7)),
                store(place(3, 0, U8), imm(U8, 11)),
                store(place(4, 0, U8), imm(U8, 13)),
                call(
                    1,
                    vec![
                        imm(UNIT, 0),
                        imm(U8, 255),
                        imm(U32, u32::MAX as u64),
                        imm(BOOL, 1),
                        imm(USIZE, 10),
                        slot(0),
                        slot(1),
                        slot(2),
                        slot(3),
                        slot(4),
                    ],
                    Some(5),
                ),
                set(6, Rvalue::Load(place(5, 8, USIZE))),
            ],
            Terminator::Return(slot(6)),
        )],
    );
    let mut slots = parameters.clone();
    slots.extend([USIZE, USIZE, U8, result]);
    let mut instructions = vec![
        set(10, Rvalue::Cast { value: slot(1) }),
        set(11, Rvalue::Cast { value: slot(2) }),
        binary(10, Op::Add, slot(10), slot(11)),
        binary(10, Op::Add, slot(10), slot(4)),
    ];
    for parameter in 5..10 {
        instructions.extend([
            set(12, Rvalue::Load(place(parameter, 0, U8))),
            set(11, Rvalue::Cast { value: slot(12) }),
            binary(10, Op::Add, slot(10), slot(11)),
        ]);
    }
    instructions.extend([
        store(place(13, 0, U32), imm(U32, 0)),
        store(place(13, 8, USIZE), slot(10)),
    ]);
    let callee = function(
        1,
        parameters,
        result,
        slots,
        vec![
            block(
                vec![],
                Terminator::Branch {
                    condition: slot(3),
                    yes: BlockId(1),
                    no: BlockId(2),
                },
            ),
            block(instructions, Terminator::Return(slot(13))),
            block(vec![], Terminator::Panic),
        ],
    );
    let assembly = emit(&Compilation {
        data: vec![],
        externs: Default::default(),
        types,
        functions: vec![entry, callee],
    })
    .unwrap();
    // Probe actual incoming register bits before the emitted callee truncates
    // them into slots. This catches wrong caller ABI extension independently.
    let assembly = assembly.replace("__core_s_f1:\n", "__core_s_f1:\nli t0, -1\nbne a2, t0, __test_fail\nli t0, 255\nbne a1, t0, __test_fail\nandi t0, sp, 15\nbnez t0, __test_fail\n");
    machine(&assembly, Some(4294967599));
}

#[test]
fn explicit_bounds_pointer_offset_and_raw_storage_execute() {
    let mut types = table();
    let array = ty(
        &mut types,
        TypeKind::Array {
            element: USIZE,
            length: 3,
        },
        24,
        8,
        vec![],
        None,
    );
    let array_pointer = types.pointer(array, false, Span::default()).unwrap();
    let pointer = types.pointer(USIZE, false, Span::default()).unwrap();
    let entry = function(
        0,
        vec![],
        USIZE,
        vec![array, array_pointer, pointer, USIZE],
        vec![block(
            vec![
                store(place(0, 0, USIZE), imm(USIZE, 11)),
                store(place(0, 8, USIZE), imm(USIZE, 22)),
                store(place(0, 16, USIZE), imm(USIZE, 33)),
                set(1, Rvalue::Address(place(0, 0, array))),
                InstructionKind::Bounds {
                    index: imm(USIZE, 2),
                    length: 3,
                },
                set(
                    2,
                    Rvalue::PointerOffset {
                        base: slot(1),
                        bytes: imm(USIZE, 16),
                    },
                ),
                store(
                    Place {
                        base: PlaceBase::Pointer(slot(2)),
                        byte_offset: 0,
                        ty: USIZE,
                    },
                    imm(USIZE, 42),
                ),
                set(
                    3,
                    Rvalue::Load(Place {
                        base: PlaceBase::Pointer(slot(2)),
                        byte_offset: 0,
                        ty: USIZE,
                    }),
                ),
            ],
            Terminator::Return(slot(3)),
        )],
    );
    machine(
        &emit(&Compilation {
            data: vec![],
            externs: Default::default(),
            types: types.clone(),
            functions: vec![entry],
        })
        .unwrap(),
        Some(42),
    );
    for (index, length) in [(0, 0), (3, 3), (u64::MAX, 3)] {
        let entry = function(
            0,
            vec![],
            UNIT,
            vec![],
            vec![block(
                vec![InstructionKind::Bounds {
                    index: imm(USIZE, index),
                    length,
                }],
                Terminator::Return(imm(UNIT, 0)),
            )],
        );
        machine(
            &emit(&Compilation {
                data: vec![],
                externs: Default::default(),
                types: table(),
                functions: vec![entry],
            })
            .unwrap(),
            None,
        );
    }
    let entry = function(
        0,
        vec![],
        pointer,
        vec![USIZE, pointer, pointer],
        vec![block(
            vec![
                set(1, Rvalue::Address(place(0, 0, USIZE))),
                set(
                    2,
                    Rvalue::PointerOffset {
                        base: slot(1),
                        bytes: imm(USIZE, u64::MAX),
                    },
                ),
            ],
            Terminator::Return(slot(2)),
        )],
    );
    machine(
        &emit(&Compilation {
            data: vec![],
            externs: Default::default(),
            types,
            functions: vec![entry],
        })
        .unwrap(),
        None,
    );
}

#[test]
fn overlap_copy_large_stack_offsets_and_zero_size_values_execute() {
    let mut types = table();
    let bytes = ty(
        &mut types,
        TypeKind::Array {
            element: U8,
            length: 8,
        },
        8,
        1,
        vec![],
        None,
    );
    let four = ty(
        &mut types,
        TypeKind::Array {
            element: U8,
            length: 4,
        },
        4,
        1,
        vec![],
        None,
    );
    let huge = ty(
        &mut types,
        TypeKind::Array {
            element: U8,
            length: 4096,
        },
        4096,
        1,
        vec![],
        None,
    );
    let zero = ty(
        &mut types,
        TypeKind::Array {
            element: USIZE,
            length: 0,
        },
        0,
        8,
        vec![],
        None,
    );
    let mut instructions = vec![];
    for offset in 0..8 {
        instructions.push(store(place(1, offset, U8), imm(U8, offset)));
    }
    instructions.extend([
        InstructionKind::Copy {
            destination: place(1, 2, four),
            source: place(1, 0, four),
        },
        set(2, Rvalue::Load(place(1, 5, U8))),
        set(3, Rvalue::Cast { value: slot(2) }),
        InstructionKind::Copy {
            destination: place(1, 0, four),
            source: place(1, 2, four),
        },
        set(2, Rvalue::Load(place(1, 3, U8))),
        set(4, Rvalue::Cast { value: slot(2) }),
        binary(5, Op::Add, slot(3), slot(4)),
        call(1, vec![slot(6)], Some(7)),
        call(2, vec![imm(UNIT, 0)], None),
    ]);
    let entry = function(
        0,
        vec![],
        USIZE,
        vec![huge, bytes, U8, USIZE, USIZE, USIZE, zero, zero],
        vec![block(instructions, Terminator::Return(slot(5)))],
    );
    let zero_identity = function(
        1,
        vec![zero],
        zero,
        vec![zero],
        vec![block(vec![], Terminator::Return(slot(0)))],
    );
    let unit = function(
        2,
        vec![UNIT],
        UNIT,
        vec![UNIT],
        vec![block(
            vec![
                InstructionKind::StorageLive(SlotId(0)),
                InstructionKind::StorageDead(SlotId(0)),
            ],
            Terminator::Return(imm(UNIT, 0)),
        )],
    );
    machine(
        &emit(&Compilation {
            data: vec![],
            externs: Default::default(),
            types,
            functions: vec![entry, zero_identity, unit],
        })
        .unwrap(),
        Some(6),
    );
}

#[test]
fn malformed_ir_returns_diagnostics_without_source_rechecking() {
    let base = Compilation {
        data: vec![],
        externs: Default::default(),
        types: table(),
        functions: vec![function(
            0,
            vec![],
            USIZE,
            vec![USIZE],
            vec![block(vec![], Terminator::Return(imm(USIZE, 0)))],
        )],
    };
    let mut cases = vec![];
    let mut case = base.clone();
    case.functions[0].entry = BlockId(9);
    cases.push(case);
    let mut case = base.clone();
    case.functions[0].symbol = "user injected symbol".into();
    cases.push(case);
    let mut case = base.clone();
    case.functions[0].blocks[0].terminator = Terminator::Return(imm(BOOL, 1));
    cases.push(case);
    let mut case = base.clone();
    case.functions[0].blocks[0].instructions.push(Instruction {
        kind: set(9, Rvalue::Use(imm(USIZE, 0))),
        span: Span::default(),
    });
    cases.push(case);
    let mut case = base.clone();
    case.functions[0].blocks[0].instructions.push(Instruction {
        kind: call(9, vec![], Some(0)),
        span: Span::default(),
    });
    cases.push(case);
    let mut case = base.clone();
    case.types.entries[2].layout.align = 3;
    cases.push(case);
    let mut case = base.clone();
    case.functions[0].blocks[0].instructions.push(Instruction {
        kind: set(0, Rvalue::Load(place(0, 1, USIZE))),
        span: Span::default(),
    });
    cases.push(case);
    let mut case = base.clone();
    case.functions[0].blocks[0].instructions.push(Instruction {
        kind: set(
            0,
            Rvalue::Cast {
                value: imm(BOOL, 1),
            },
        ),
        span: Span::default(),
    });
    cases.push(case);
    let mut case = base.clone();
    case.functions[0].blocks[0].terminator = Terminator::Return(imm(U8, 256));
    cases.push(case);
    let mut case = base.clone();
    case.functions[0].blocks[0].instructions.push(Instruction {
        kind: set(
            0,
            Rvalue::Unary {
                op: UnaryOp::Deref,
                value: imm(USIZE, 0),
            },
        ),
        span: Span::default(),
    });
    cases.push(case);
    for compilation in cases {
        assert_eq!(emit(&compilation).unwrap_err().code, "I_INTERNAL");
    }
    let compilation = Compilation {
        data: vec![],
        externs: Default::default(),
        types: table(),
        functions: vec![function(
            0,
            vec![],
            BOOL,
            vec![BOOL],
            vec![block(
                vec![binary(0, Op::And, imm(BOOL, 1), imm(BOOL, 1))],
                Terminator::Return(slot(0)),
            )],
        )],
    };
    assert_eq!(emit(&compilation).unwrap_err().code, "I_INTERNAL");
}

#[test]
fn typed_control_flow_preserves_live_temporaries_across_far_edges() {
    use biomiya_core_s::{backend::assembly, machine::*};
    let mut program = Program::default();
    program.instructions.push(Inst::Label("__core_s_f0".into()));
    let temporaries = [(5, 11), (6, 13), (7, 17), (28, 19), (29, 23), (30, 29)];
    for (rd, bits) in temporaries {
        program.instructions.push(Inst::Li { rd, bits });
    }
    // Both a non-taken edge and a jump with >4 KiB displacement retain t0..t5.
    program.instructions.push(Inst::Branch {
        condition: Condition::Eq,
        rs1: 5,
        rs2: ZERO,
        target: "__core_s_panic".into(),
    });
    program.instructions.push(Inst::Jump {
        rd: ZERO,
        target: ".Lfar".into(),
    });
    program.instructions.push(Inst::Label(".Lback".into()));
    program.instructions.push(Inst::Li { rd: 10, bits: 0 });
    for (rs2, _) in temporaries {
        program.instructions.push(Inst::R {
            op: ROp::Add,
            rd: 10,
            rs1: 10,
            rs2,
        });
    }
    program.instructions.push(Inst::Jalr {
        rd: ZERO,
        base: RA,
        offset: 0,
    });
    for _ in 0..1100 {
        program.instructions.push(Inst::I {
            op: IOp::Addi,
            rd: ZERO,
            rs1: ZERO,
            imm: 0,
        });
    }
    program.instructions.push(Inst::Label(".Lfar".into()));
    program.instructions.push(Inst::Branch {
        condition: Condition::Eq,
        rs1: 5,
        rs2: 5,
        target: ".Lback".into(),
    });
    program.instructions.push(Inst::Jump {
        rd: ZERO,
        target: "__core_s_panic".into(),
    });
    machine(&assembly(&program).unwrap(), Some(112));
}

#[test]
fn typed_address_rendering_and_generated_labels_execute_without_collisions() {
    use biomiya_core_s::{backend::assembly, machine::*};
    let program = Program {
        data: vec![],
        instructions: vec![
            Inst::Label("__core_s_f0".into()),
            Inst::Address {
                rd: 5,
                target: ".Lcore_s_render_1".into(),
            },
            Inst::Jalr {
                rd: ZERO,
                base: 5,
                offset: 0,
            },
            Inst::Unimp,
            Inst::Label(".Lcore_s_render_1".into()),
            Inst::Li { rd: 10, bits: 7 },
            Inst::Jalr {
                rd: ZERO,
                base: RA,
                offset: 0,
            },
        ],
    };
    machine(&assembly(&program).unwrap(), Some(7));
}

#[test]
fn typed_lowering_exposes_checks_and_renderer_rejects_malformed_operations() {
    use biomiya_core_s::{backend, machine::*};
    let compilation = Compilation {
        data: vec![],
        externs: Default::default(),
        types: table(),
        functions: vec![function(
            0,
            vec![],
            U32,
            vec![U32],
            vec![block(
                vec![binary(0, Op::Sub, imm(U32, 42), imm(U32, 2))],
                Terminator::Return(slot(0)),
            )],
        )],
    };
    let program = backend::machine(&compilation).unwrap();
    assert!(program.instructions.iter().any(|inst| matches!(
        inst,
        Inst::Branch {
            condition: Condition::GeUnsigned,
            rs1: 6,
            rs2: 7,
            ..
        }
    )));
    assert!(
        program
            .instructions
            .iter()
            .any(|inst| matches!(inst, Inst::Jump {
        rd: RA, target,
    } if target == "__core_s_panic"))
    );
    machine(&backend::assembly(&program).unwrap(), Some(40));
    let bad = [
        Inst::R {
            op: ROp::Add,
            rd: 32,
            rs1: ZERO,
            rs2: ZERO,
        },
        Inst::I {
            op: IOp::Addi,
            rd: 5,
            rs1: ZERO,
            imm: 2048,
        },
        Inst::I {
            op: IOp::Slli,
            rd: 5,
            rs1: ZERO,
            imm: 64,
        },
        Inst::Load {
            op: LoadOp::Double,
            rd: 5,
            base: 32,
            offset: 0,
        },
        Inst::Store {
            op: StoreOp::Word,
            rs: 5,
            base: SP,
            offset: -2049,
        },
        Inst::Li { rd: 32, bits: 0 },
        Inst::Branch {
            condition: Condition::Eq,
            rs1: 32,
            rs2: ZERO,
            target: "external".into(),
        },
        Inst::Jump {
            rd: 5,
            target: "external".into(),
        },
        Inst::Address {
            rd: ZERO,
            target: "external".into(),
        },
        Inst::Jalr {
            rd: RA,
            base: 5,
            offset: 2048,
        },
        Inst::Label("injected:\n.4byte 0".into()),
        Inst::Jump {
            rd: ZERO,
            target: "injected;ecall".into(),
        },
    ];
    for instruction in bad {
        assert_eq!(
            backend::assembly(&Program {
                data: vec![],
                instructions: vec![instruction]
            })
            .unwrap_err()
            .code,
            "I_INTERNAL"
        );
    }
    assert_eq!(
        backend::assembly(&Program {
            data: vec![],
            instructions: vec![
                Inst::Label("duplicate".into()),
                Inst::Label("duplicate".into()),
            ]
        })
        .unwrap_err()
        .code,
        "I_INTERNAL"
    );
    // Reference assembly can defer an unresolved external target to its linker.
    assert!(
        backend::assembly(&Program {
            data: vec![],
            instructions: vec![Inst::Jump {
                rd: RA,
                target: "external".into()
            },]
        })
        .is_ok()
    );
}

#[test]
fn external_c_calls_symbol_addresses_bitcasts_and_dynamic_bounds_execute() {
    use biomiya_core_s::machine::DataObject;
    let mut types = table();
    let pointer = types.pointer(U32, false, Span::default()).unwrap();
    let constant = types.pointer(U32, true, Span::default()).unwrap();
    let mut external = function(1, vec![U32], U32, vec![], vec![]);
    external.signature.unsafe_ = true;
    external.parameter_slots.clear();
    let mut compilation = Compilation {
        types,
        functions: vec![
            function(
                0,
                vec![],
                U32,
                vec![pointer, USIZE, pointer, constant, U32],
                vec![block(
                    vec![
                        set(
                            0,
                            Rvalue::SymbolAddress {
                                symbol: "__global_test".into(),
                            },
                        ),
                        set(1, Rvalue::Bitcast { value: slot(0) }),
                        set(2, Rvalue::Bitcast { value: slot(1) }),
                        set(3, Rvalue::Bitcast { value: slot(2) }),
                        set(
                            4,
                            Rvalue::Load(Place {
                                base: PlaceBase::Pointer(slot(3)),
                                byte_offset: 0,
                                ty: U32,
                            }),
                        ),
                        InstructionKind::BoundsDynamic {
                            index: imm(USIZE, 1),
                            length: imm(USIZE, 2),
                        },
                        call(1, vec![slot(4)], Some(4)),
                    ],
                    Terminator::Return(slot(4)),
                )],
            ),
            external,
        ],
        data: vec![DataObject {
            symbol: "__global_test".into(),
            mutable: false,
            align: 4,
            bytes: 33_u32.to_le_bytes().to_vec(),
            zero_fill: 0,
            relocations: vec![],
        }],
        externs: std::collections::BTreeMap::from([(FunctionId(1), "__external_b2".into())]),
    };
    let provider = "\n.text\n.globl __external_b2\n__external_b2:\naddi x10,x10,1\njalr x0,0(x1)\n";
    let lowered = biomiya_core_s::backend::machine(&compilation).unwrap();
    assert_eq!(lowered.data, compilation.data);
    assert!(
        !lowered
            .instructions
            .iter()
            .any(|i| matches!(i,biomiya_core_s::machine::Inst::Label(s) if s=="__core_s_f1"))
    );
    machine(&(emit(&compilation).unwrap() + provider), Some(34));
    compilation.functions[0].blocks[0].instructions[5].kind = InstructionKind::BoundsDynamic {
        index: imm(USIZE, 2),
        length: imm(USIZE, 2),
    };
    machine(&(emit(&compilation).unwrap() + provider), None);
    compilation.functions[0].blocks[0].instructions[3].kind =
        set(2, Rvalue::Bitcast { value: slot(3) });
    assert!(
        emit(&compilation).is_err(),
        "const pointer removal in IR must reject"
    );
}

#[test]
fn malformed_externs_slices_and_new_ir_are_rejected() {
    let mut external = function(0, vec![USIZE], U32, vec![], vec![]);
    external.signature.unsafe_ = true;
    external.parameter_slots.clear();
    let c = Compilation {
        types: table(),
        functions: vec![external],
        data: vec![],
        externs: std::collections::BTreeMap::from([(FunctionId(0), "provider".into())]),
    };
    assert!(emit(&c).is_ok());
    let mut cases = vec![];
    let mut v = c.clone();
    v.externs.insert(FunctionId(99), "missing".into());
    cases.push(v);
    let mut v = c.clone();
    v.externs.insert(FunctionId(0), "bad\nsymbol".into());
    cases.push(v);
    let mut v = c.clone();
    v.functions[0].signature.unsafe_ = false;
    cases.push(v);
    let mut v = c.clone();
    v.functions[0].signature.parameters[0] = TypeId(99);
    cases.push(v);
    let mut v = c.clone();
    v.functions[0].signature.parameters[0] = UNIT;
    cases.push(v);
    let mut v = c.clone();
    v.functions[0]
        .blocks
        .push(block(vec![], Terminator::Unreachable));
    cases.push(v);
    let mut v = c.clone();
    let slice = ty(
        &mut v.types,
        TypeKind::Slice {
            element: U8,
            const_: true,
        },
        16,
        8,
        vec![0, 8],
        None,
    );
    v.functions[0].signature.parameters[0] = slice;
    cases.push(v);
    let mut v = c.clone();
    ty(
        &mut v.types,
        TypeKind::Slice {
            element: U8,
            const_: true,
        },
        8,
        8,
        vec![0, 8],
        None,
    );
    cases.push(v);
    for v in cases {
        assert!(emit(&v).is_err());
    }
    let c = Compilation {
        types: table(),
        functions: vec![function(
            0,
            vec![],
            U32,
            vec![U32],
            vec![block(
                vec![set(0, Rvalue::Bitcast { value: imm(U32, 0) })],
                Terminator::Return(slot(0)),
            )],
        )],
        data: vec![],
        externs: Default::default(),
    };
    assert!(emit(&c).is_err());
    let mut v = c.clone();
    v.functions[0].blocks[0].instructions[0].kind = InstructionKind::BoundsDynamic {
        index: imm(U32, 0),
        length: imm(USIZE, 1),
    };
    assert!(emit(&v).is_err());
    let mut v = c;
    v.functions[0].blocks[0].instructions[0].kind = set(
        0,
        Rvalue::SymbolAddress {
            symbol: "provider".into(),
        },
    );
    assert!(emit(&v).is_err());
}
