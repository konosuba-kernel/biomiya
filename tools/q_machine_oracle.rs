//! Independent oracle for Q's machine tests; never reads or compiles VT.
//! Reuses the frozen Rust instruction, S0, encoding and ELF implementations.
#![allow(dead_code)]
#[path = "../bootstrap/frozen/core-s/src/machine.rs"] mod machine;
#[path = "../bootstrap/frozen/core-s/src/encode.rs"] mod encode;
#[path = "../bootstrap/frozen/core-s/src/image.rs"] mod image;
#[path = "../bootstrap/frozen/core-s/src/s0.rs"] mod s0;
// Only the diagnostic/source carrier is local, avoiding unrelated serde/frontend
// dependencies. The actual machine, S0, encoding and ELF code is unmodified.
mod source {
    #[derive(Clone, Copy, Debug, Default)]
    pub struct FileId(pub u32);
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Span { pub file: FileId, pub start: u32, pub end: u32 }
    impl Span {
        pub fn new(file: FileId, start: usize, end: usize) -> Self {
            Self { file, start: start as u32, end: end as u32 }
        }
    }
}
mod diagnostic {
    #[derive(Debug)]
    pub struct Diagnostic { pub code: &'static str, pub span: crate::source::Span, pub message: String }
    impl Diagnostic {
        pub fn new(code: &'static str, span: crate::source::Span, message: impl Into<String>) -> Self {
            Self { code, span, message: message.into() }
        }
    }
    pub type Result<T> = std::result::Result<T, Diagnostic>;
}
fn constants() -> machine::Program {
    use machine::*;
    let numbers = [0, 2047, 2048, 4095, 32767, 32768, 268435455, 2147483648,
        4294967295, 9223372036854775807, 9223372036854775808,
        18446744073709549568, 18446744073709551615];
    let mut instructions = vec![Inst::Label("entry".into()),
        Inst::I { op: IOp::Addi, rd: 2, rs1: 2, imm: -112 }];
    for (index, bits) in numbers.into_iter().enumerate() {
        instructions.push(Inst::Li { rd: 5, bits });
        instructions.push(Inst::Store { op: StoreOp::Double, rs: 5, base: 2, offset: (index*8) as i64 });
    }
    instructions.extend([
        Inst::Li { rd: 10, bits: 1 },
        Inst::I { op: IOp::Addi, rd: 11, rs1: 2, imm: 0 },
        Inst::Li { rd: 12, bits: 104 }, Inst::Li { rd: 17, bits: 64 }, Inst::Ecall,
        Inst::Li { rd: 10, bits: 0 }, Inst::Li { rd: 17, bits: 93 }, Inst::Ecall,
    ]);
    Program { instructions, data: vec![] }
}
fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let program = match arguments[1].as_str() {
        "constants" => constants(),
        "s0" => s0::parse(source::FileId(7), &std::fs::read_to_string(&arguments[2]).unwrap()).unwrap(),
        _ => panic!("oracle accepts only constants or S0"),
    };
    std::fs::write(&arguments[3], image::executable(&program, "entry").unwrap()).unwrap();
}
