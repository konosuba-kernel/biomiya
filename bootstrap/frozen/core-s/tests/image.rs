//! Machine-level acceptance; reference tools are mandatory, never silently skipped.
use biomiya_core_s::{encode, image, machine::*};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "core-s-image-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn tool(command: &mut Command) {
    let output = command
        .output()
        .unwrap_or_else(|e| panic!("required reference tool {command:?}: {e}"));
    assert!(
        output.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn qemu(path: &Path) -> std::process::Output {
    let mut child = Command::new("qemu-riscv64")
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("qemu-riscv64 is required");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("qemu execution timeout");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn run(program: &Program) -> std::process::Output {
    use std::os::unix::fs::PermissionsExt;
    let dir = Temp::new();
    let p = dir.0.join("direct.elf");
    fs::write(&p, image::executable(program, "_start").unwrap()).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    qemu(&p)
}
fn li(rd: u8, bits: u64) -> Inst {
    Inst::Li { rd, bits }
}
fn label(s: &str) -> Inst {
    Inst::Label(s.into())
}
fn exit(n: u64) -> Vec<Inst> {
    vec![li(10, n), li(17, 93), Inst::Ecall]
}
fn u16_at(v: &[u8], n: usize) -> u16 {
    u16::from_le_bytes(v[n..n + 2].try_into().unwrap())
}
fn u32_at(v: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(v[n..n + 4].try_into().unwrap())
}
fn u64_at(v: &[u8], n: usize) -> u64 {
    u64::from_le_bytes(v[n..n + 8].try_into().unwrap())
}

#[test]
fn instructions_match_independent_clang_lld_bytes() {
    let mut instructions = vec![label("_start")];
    let mut asm = String::from(".option norvc\n.option norelax\n.text\n.global _start\n_start:\n");
    for (op, name) in [
        (ROp::Add, "add"),
        (ROp::Sub, "sub"),
        (ROp::Mul, "mul"),
        (ROp::Mulhu, "mulhu"),
        (ROp::Divu, "divu"),
        (ROp::Remu, "remu"),
        (ROp::And, "and"),
        (ROp::Or, "or"),
        (ROp::Xor, "xor"),
        (ROp::Sltu, "sltu"),
        (ROp::Sll, "sll"),
        (ROp::Srl, "srl"),
    ] {
        instructions.push(Inst::R {
            op,
            rd: 5,
            rs1: 6,
            rs2: 7,
        });
        asm.push_str(&format!("{name} x5,x6,x7\n"));
    }
    for (op, name) in [
        (IOp::Addi, "addi"),
        (IOp::Addiw, "addiw"),
        (IOp::Xori, "xori"),
        (IOp::Sltiu, "sltiu"),
    ] {
        for imm in [-2048, -1, 0, 2047] {
            instructions.push(Inst::I {
                op,
                rd: 8,
                rs1: 9,
                imm,
            });
            asm.push_str(&format!("{name} x8,x9,{imm}\n"));
        }
    }
    for (op, name) in [(IOp::Slli, "slli"), (IOp::Srli, "srli")] {
        for imm in [0, 31, 32, 63] {
            instructions.push(Inst::I {
                op,
                rd: 10,
                rs1: 11,
                imm,
            });
            asm.push_str(&format!("{name} x10,x11,{imm}\n"));
        }
    }
    for (op, name) in [
        (LoadOp::ByteUnsigned, "lbu"),
        (LoadOp::WordUnsigned, "lwu"),
        (LoadOp::Double, "ld"),
    ] {
        for offset in [-2048, 2047] {
            instructions.push(Inst::Load {
                op,
                rd: 12,
                base: 13,
                offset,
            });
            asm.push_str(&format!("{name} x12,{offset}(x13)\n"));
        }
    }
    for (op, name) in [
        (StoreOp::Byte, "sb"),
        (StoreOp::Word, "sw"),
        (StoreOp::Double, "sd"),
    ] {
        for offset in [-2048, 2047] {
            instructions.push(Inst::Store {
                op,
                rs: 14,
                base: 15,
                offset,
            });
            asm.push_str(&format!("{name} x14,{offset}(x15)\n"));
        }
    }
    instructions.push(Inst::Jalr {
        rd: 1,
        base: 16,
        offset: -2048,
    });
    asm.push_str("jalr x1,-2048(x16)\n");
    instructions.extend([Inst::Ecall, Inst::Unimp]);
    asm.push_str("ecall\ncsrrw x0,cycle,x0\n");
    for (n, (condition, inverse)) in [
        (Condition::Eq, "bne"),
        (Condition::Ne, "beq"),
        (Condition::LtUnsigned, "bgeu"),
        (Condition::GeUnsigned, "bltu"),
    ]
    .into_iter()
    .enumerate()
    {
        instructions.push(Inst::Branch {
            condition,
            rs1: 5,
            rs2: 6,
            target: "far".into(),
        });
        asm.push_str(&format!("{inverse} x5,x6,.Lskip{n}\n.Lpair{n}:\nauipc x31,%pcrel_hi(far)\njalr x0,%pcrel_lo(.Lpair{n})(x31)\n.Lskip{n}:\n"));
    }
    instructions.push(Inst::Jump {
        rd: 1,
        target: "far".into(),
    });
    asm.push_str(".Lcall:\nauipc x31,%pcrel_hi(far)\njalr x1,%pcrel_lo(.Lcall)(x31)\n");
    instructions.push(Inst::Address {
        rd: 7,
        target: "far".into(),
    });
    asm.push_str(".Laddr:\nauipc x7,%pcrel_hi(far)\naddi x7,x7,%pcrel_lo(.Laddr)\n");
    for _ in 0..1700 {
        instructions.push(Inst::I {
            op: IOp::Addi,
            rd: 0,
            rs1: 0,
            imm: 0,
        });
    }
    asm.push_str(".rept 1700\naddi x0,x0,0\n.endr\nfar:\n");
    instructions.extend([
        label("far"),
        Inst::Jump {
            rd: 0,
            target: "_start".into(),
        },
    ]);
    asm.push_str(".Lback:\nauipc x31,%pcrel_hi(_start)\njalr x0,%pcrel_lo(.Lback)(x31)\n");
    let dir = Temp::new();
    let src = dir.0.join("oracle.s");
    let obj = dir.0.join("oracle.o");
    let bin = dir.0.join("oracle.bin");
    fs::write(&src, asm).unwrap();
    tool(
        Command::new("clang")
            .args([
                "--target=riscv64-unknown-elf",
                "-march=rv64im_zicsr",
                "-mabi=lp64",
                "-mno-relax",
                "-c",
            ])
            .arg(&src)
            .arg("-o")
            .arg(&obj),
    );
    tool(
        Command::new("ld.lld")
            .args(["--no-relax", "--oformat=binary", "-Ttext=0x10000"])
            .arg(&obj)
            .arg("-o")
            .arg(&bin),
    );
    let encoded = encode::encode(
        &Program {
            instructions,
            data: vec![],
        },
        image::TEXT_BASE,
    )
    .unwrap();
    assert_eq!(encoded.fixups, 7);
    assert_eq!(encoded.bytes, fs::read(bin).unwrap());
}

#[test]
fn arbitrary_u64_constants_execute_and_write_exact_bytes() {
    let mut values = vec![
        0,
        1,
        2047,
        2048,
        4095,
        4096,
        0x7fffffff,
        0x80000000,
        0xffffffff,
        0x100000000,
        0x7fffffffffffffff,
        0x8000000000000000,
        0xffffffffffffffff,
        0x123456789abcdef0,
    ];
    let mut bits = 0xdca182bc59480801u64;
    for _ in 0..64 {
        bits ^= bits << 13;
        bits ^= bits >> 7;
        bits ^= bits << 17;
        values.push(bits);
    }
    let mut instructions = vec![
        label("_start"),
        Inst::I {
            op: IOp::Addi,
            rd: SP,
            rs1: SP,
            imm: -1024,
        },
    ];
    for (n, bits) in values.iter().enumerate() {
        instructions.extend([
            li(5, *bits),
            Inst::Store {
                op: StoreOp::Double,
                rs: 5,
                base: SP,
                offset: (n * 8) as i64,
            },
        ]);
    }
    instructions.extend([
        li(10, 1),
        Inst::I {
            op: IOp::Addi,
            rd: 11,
            rs1: SP,
            imm: 0,
        },
        li(12, (values.len() * 8) as u64),
        li(17, 64),
        Inst::Ecall,
    ]);
    instructions.extend(exit(0));
    let output = run(&Program {
        instructions,
        data: vec![],
    });
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    );
}

#[test]
fn far_control_flow_calls_addresses_and_scratch_contract_execute() {
    let mut instructions = vec![label("_start")];
    for r in [5, 6, 7, 28, 29, 30] {
        instructions.push(li(r, r as u64));
    }
    instructions.extend([
        Inst::Branch {
            condition: Condition::Eq,
            rs1: 5,
            rs2: 6,
            target: "fail".into(),
        },
        Inst::Branch {
            condition: Condition::Ne,
            rs1: 5,
            rs2: 5,
            target: "fail".into(),
        },
        Inst::Branch {
            condition: Condition::LtUnsigned,
            rs1: 6,
            rs2: 5,
            target: "fail".into(),
        },
        Inst::Branch {
            condition: Condition::GeUnsigned,
            rs1: 5,
            rs2: 6,
            target: "fail".into(),
        },
        Inst::Jump {
            rd: RA,
            target: "far_function".into(),
        },
        Inst::Address {
            rd: 11,
            target: "far_function".into(),
        },
        Inst::Branch {
            condition: Condition::Ne,
            rs1: 10,
            rs2: 11,
            target: "fail".into(),
        },
    ]);
    for r in [5, 6, 7, 28, 29, 30] {
        instructions.extend([
            li(12, r as u64),
            Inst::Branch {
                condition: Condition::Ne,
                rs1: r,
                rs2: 12,
                target: "fail".into(),
            },
        ]);
    }
    instructions.push(Inst::Jump {
        rd: ZERO,
        target: "far_branch".into(),
    });
    instructions.push(label("success"));
    instructions.extend(exit(0));
    instructions.push(label("fail"));
    instructions.extend(exit(42));
    for _ in 0..1700 {
        instructions.push(Inst::Unimp);
    }
    instructions.extend([
        label("far_function"),
        Inst::Address {
            rd: 10,
            target: "far_function".into(),
        },
        Inst::Jalr {
            rd: ZERO,
            base: RA,
            offset: 0,
        },
        label("far_branch"),
        Inst::Branch {
            condition: Condition::Eq,
            rs1: 5,
            rs2: 5,
            target: "eq".into(),
        },
        Inst::Unimp,
        label("eq"),
        Inst::Branch {
            condition: Condition::Ne,
            rs1: 5,
            rs2: 6,
            target: "ne".into(),
        },
        Inst::Unimp,
        label("ne"),
        Inst::Branch {
            condition: Condition::LtUnsigned,
            rs1: 5,
            rs2: 6,
            target: "lt".into(),
        },
        Inst::Unimp,
        label("lt"),
        Inst::Branch {
            condition: Condition::GeUnsigned,
            rs1: 6,
            rs2: 5,
            target: "success".into(),
        },
        Inst::Unimp,
    ]);
    let output = run(&Program {
        instructions,
        data: vec![],
    });
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn elf_headers_segments_entry_and_padding_are_exact() {
    for padding in [0, 1021, 1022, 1023, 1024] {
        let mut instructions = vec![label("_start")];
        instructions.extend(exit(0));
        instructions.extend((0..padding).map(|_| Inst::Unimp));
        let program = Program {
            instructions,
            data: vec![],
        };
        let encoded = encode::encode(&program, image::TEXT_BASE).unwrap();
        let bytes = image::executable(&program, "_start").unwrap();
        assert_eq!(&bytes[..7], b"\x7fELF\x02\x01\x01");
        assert_eq!(u16_at(&bytes, 16), 2);
        assert_eq!(u16_at(&bytes, 18), 243);
        assert_eq!(u64_at(&bytes, 24), image::TEXT_BASE);
        assert_eq!(u64_at(&bytes, 32), 64);
        assert_eq!(u64_at(&bytes, 40), 0);
        assert_eq!(u32_at(&bytes, 48), 0);
        assert_eq!(u16_at(&bytes, 52), 64);
        assert_eq!(u16_at(&bytes, 54), 56);
        assert_eq!(u16_at(&bytes, 56), 3);
        assert_eq!(u16_at(&bytes, 58), 0);
        assert_eq!(u16_at(&bytes, 60), 0);
        let mut file_end = 232;
        let mut memory_end = image::TEXT_BASE;
        for n in 0..3 {
            let at = 64 + 56 * n;
            assert_eq!(u32_at(&bytes, at), 1);
            assert_eq!(u32_at(&bytes, at + 4), [5, 4, 6][n]);
            let offset = u64_at(&bytes, at + 8) as usize;
            let address = u64_at(&bytes, at + 16);
            let size = u64_at(&bytes, at + 32) as usize;
            assert_eq!(offset % 4096, 0);
            assert_eq!(address % 4096, 0);
            assert_eq!(u64_at(&bytes, at + 48), 4096);
            assert_eq!(u64_at(&bytes, at + 24), address);
            assert_eq!(u64_at(&bytes, at + 40), size as u64);
            assert!(offset >= file_end && address >= memory_end);
            assert!(bytes[file_end..offset].iter().all(|b| *b == 0));
            if n == 0 {
                assert_eq!(&bytes[offset..offset + size], &encoded.bytes);
            } else {
                assert_eq!(size, 1);
                assert_eq!(bytes[offset], 0);
            }
            file_end = offset + size;
            memory_end = address + size as u64;
        }
        assert_eq!(file_end, bytes.len());
        assert_eq!(bytes, image::executable(&program, "_start").unwrap());
    }
}

#[test]
fn malformed_operands_symbols_entries_and_ranges_are_rejected() {
    let bad = vec![
        vec![],
        vec![label("only_label")],
        vec![label(""), Inst::Ecall],
        vec![label("dup"), label("dup"), Inst::Ecall],
        vec![Inst::Jump {
            rd: 0,
            target: "absent".into(),
        }],
        vec![Inst::Branch {
            condition: Condition::Eq,
            rs1: 0,
            rs2: 0,
            target: "absent".into(),
        }],
        vec![Inst::Address {
            rd: 1,
            target: "absent".into(),
        }],
        vec![
            Inst::Jump {
                rd: 2,
                target: "x".into(),
            },
            label("x"),
        ],
        vec![
            Inst::Address {
                rd: 0,
                target: "x".into(),
            },
            label("x"),
        ],
        vec![li(32, 0)],
        vec![Inst::R {
            op: ROp::Add,
            rd: 1,
            rs1: 32,
            rs2: 0,
        }],
        vec![
            Inst::Branch {
                condition: Condition::Eq,
                rs1: 0,
                rs2: 32,
                target: "x".into(),
            },
            label("x"),
        ],
        vec![Inst::I {
            op: IOp::Addi,
            rd: 1,
            rs1: 0,
            imm: 2048,
        }],
        vec![Inst::I {
            op: IOp::Slli,
            rd: 1,
            rs1: 0,
            imm: 64,
        }],
        vec![Inst::I {
            op: IOp::Srli,
            rd: 1,
            rs1: 0,
            imm: -1,
        }],
        vec![Inst::Load {
            op: LoadOp::Double,
            rd: 1,
            base: 0,
            offset: -2049,
        }],
        vec![Inst::Store {
            op: StoreOp::Double,
            rs: 32,
            base: 0,
            offset: 0,
        }],
        vec![Inst::Jalr {
            rd: 1,
            base: 0,
            offset: 2048,
        }],
    ];
    for instructions in bad {
        assert!(
            encode::encode(
                &Program {
                    instructions,
                    data: vec![]
                },
                image::TEXT_BASE
            )
            .is_err()
        );
    }
    let program = Program {
        data: vec![],
        instructions: vec![label("_start"), Inst::Ecall, label("end")],
    };
    assert_eq!(
        encode::encode(&program, 2).err().unwrap().code,
        "I_INTERNAL"
    );
    assert_eq!(
        encode::encode(&program, u64::MAX - 3).err().unwrap().code,
        "I_LIMIT"
    );
    assert!(image::executable(&program, "end").is_err());
    assert!(image::executable(&program, "absent").is_err());
}

#[test]
fn text_capacity_is_checked_before_output_allocation() {
    let inst = li(5, 0x123456789abcdef0);
    let width = encode::encode(
        &Program {
            data: vec![],
            instructions: vec![inst.clone()],
        },
        0,
    )
    .unwrap()
    .bytes
    .len();
    let program = Program {
        data: vec![],
        instructions: vec![inst; (64 * 1024 * 1024) / width + 1],
    };
    let error = encode::encode(&program, image::TEXT_BASE).err().unwrap();
    assert_eq!(error.code, "I_LIMIT");
    assert!(error.message.contains("64 MiB"));
}

fn data_program() -> Program {
    let mut p = biomiya_core_s::s0::parse(biomiya_core_s::source::FileId(0), "_start:\naddress x5,ptr\nld x11,0(x5)\naddi x10,x0,1\naddi x12,x0,5\naddi x17,x0,64\necall\naddress x5,buffer\nld x6,0(x5)\nbne x6,x0,fail\naddi x6,x0,77\nsd x6,0(x5)\nld x10,0(x5)\naddi x17,x0,93\necall\nfail:\naddi x10,x0,99\naddi x17,x0,93\necall\n").unwrap();
    p.data = vec![
        DataObject {
            symbol: "ptr".into(),
            mutable: true,
            align: 8,
            bytes: vec![0; 8],
            zero_fill: 0,
            relocations: vec![DataRelocation {
                offset: 0,
                target: "greeting".into(),
                addend: 1,
            }],
        },
        DataObject {
            symbol: "greeting".into(),
            mutable: false,
            align: 16,
            bytes: b"xHELLO".to_vec(),
            zero_fill: 0,
            relocations: vec![],
        },
        DataObject {
            symbol: "buffer".into(),
            mutable: true,
            align: 64,
            bytes: vec![],
            zero_fill: 2 * 1024 * 1024,
            relocations: vec![],
        },
        DataObject {
            symbol: "tail".into(),
            mutable: true,
            align: 128,
            bytes: vec![],
            zero_fill: 1,
            relocations: vec![],
        },
    ];
    p
}

#[test]
fn data_abs64_bss_and_segment_permissions_execute_against_reference() {
    use std::os::unix::fs::PermissionsExt;
    let p = data_program();
    let elf = image::executable(&p, "_start").unwrap();
    assert!(elf.len() < 16384);
    let rw_header = 64 + 2 * 56;
    assert_eq!(u64_at(&elf, rw_header + 32), 8);
    assert_eq!(u64_at(&elf, rw_header + 40), 128 + 2 * 1024 * 1024 + 1);
    let ro_base = u64_at(&elf, 64 + 56 + 16);
    let rw_offset = u64_at(&elf, rw_header + 8) as usize;
    assert_eq!(u64_at(&elf, rw_offset), ro_base + 1);
    let direct = run(&p);
    assert_eq!(direct.status.code(), Some(77));
    assert_eq!(direct.stdout, b"HELLO");
    let dir = Temp::new();
    let src = dir.0.join("data.s");
    let obj = dir.0.join("data.o");
    let binary = dir.0.join("reference.elf");
    fs::write(&src, biomiya_core_s::backend::assembly(&p).unwrap()).unwrap();
    tool(
        Command::new("clang")
            .args([
                "--target=riscv64-unknown-elf",
                "-march=rv64im",
                "-mabi=lp64",
                "-mno-relax",
                "-c",
            ])
            .arg(src)
            .arg("-o")
            .arg(&obj),
    );
    tool(
        Command::new("ld.lld")
            .args(["--no-relax", "-e", "_start"])
            .arg(obj)
            .arg("-o")
            .arg(&binary),
    );
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    let reference = qemu(&binary);
    assert_eq!(reference.status.code(), Some(77));
    assert_eq!(reference.stdout, direct.stdout);
    let mut write_ro = p.clone();
    write_ro.instructions = vec![
        label("_start"),
        Inst::Address {
            rd: 5,
            target: "greeting".into(),
        },
        Inst::Store {
            op: StoreOp::Byte,
            rs: ZERO,
            base: 5,
            offset: 0,
        },
    ];
    write_ro.instructions.extend(exit(0));
    assert!(
        !run(&write_ro).status.success(),
        "read-only segment must reject stores"
    );
}

#[test]
fn data_and_external_symbol_validation_rejects_malformed_images() {
    use std::collections::BTreeMap;
    let p = data_program();
    let mut cases = vec![];
    let mut v = p.clone();
    v.data[1].symbol = "_start".into();
    cases.push(v);
    let mut v = p.clone();
    v.data[1].symbol = "ptr".into();
    cases.push(v);
    let mut v = p.clone();
    v.data[0].align = 3;
    cases.push(v);
    let mut v = p.clone();
    v.data[0].align = 8192;
    cases.push(v);
    let mut v = p.clone();
    v.data[0].zero_fill = 1;
    cases.push(v);
    let mut v = p.clone();
    v.data[2].mutable = false;
    cases.push(v);
    let mut v = p.clone();
    v.data[2].zero_fill = u64::MAX;
    cases.push(v);
    let mut v = p.clone();
    v.data[2].zero_fill = 64 * 1024 * 1024;
    cases.push(v);
    let mut v = p.clone();
    v.data[0].relocations[0].offset = 1;
    cases.push(v);
    let mut v = p.clone();
    v.data[0].relocations[0].offset = u64::MAX;
    cases.push(v);
    let mut v = p.clone();
    v.data[0].relocations[0].target = "missing".into();
    cases.push(v);
    let mut v = p.clone();
    v.data[0].relocations[0].addend = i64::MIN;
    cases.push(v);
    let mut v = p.clone();
    let duplicate = v.data[0].relocations[0].clone();
    v.data[0].relocations.push(duplicate);
    cases.push(v);
    let mut v = p.clone();
    v.instructions.push(Inst::Jump {
        rd: RA,
        target: "greeting".into(),
    });
    cases.push(v);
    let mut v = p.clone();
    v.instructions.push(Inst::Branch {
        condition: Condition::Eq,
        rs1: 0,
        rs2: 0,
        target: "ptr".into(),
    });
    cases.push(v);
    for v in cases {
        assert!(
            image::executable(&v, "_start").is_err(),
            "malformed image accepted"
        );
    }
    let ext = BTreeMap::from([("data".into(), 0x20000)]);
    let mut v = Program {
        instructions: vec![Inst::Address {
            rd: 5,
            target: "data".into(),
        }],
        data: vec![],
    };
    assert_eq!(encode::text_size(&v).unwrap(), 8);
    assert_eq!(
        encode::encode_with_symbols(&v, 0x10000, &ext)
            .unwrap()
            .bytes
            .len(),
        8
    );
    v.instructions.push(label("data"));
    assert!(encode::encode_with_symbols(&v, 0x10000, &ext).is_err());
    v.instructions = vec![Inst::Jump {
        rd: RA,
        target: "data".into(),
    }];
    assert!(encode::encode_with_symbols(&v, 0x10000, &ext).is_err());
}
