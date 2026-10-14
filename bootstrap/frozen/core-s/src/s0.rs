//! Deliberately small assembler input for platform adapters. No GNU directives.
use crate::{
    diagnostic::{Diagnostic, Result},
    machine::*,
    source::{FileId, Span},
};

fn symbol(file: FileId, value: &str, span: Span) -> Result<String> {
    let mut chars = value.bytes();
    if !chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_' || c == b'.')
        || value == "."
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.')
    {
        return Err(Diagnostic::new(
            "E_PARSE",
            span,
            "invalid ASCII assembly symbol",
        ));
    }
    Ok(if value.starts_with(".L") {
        format!(".Lfile{}_{}", file.0, &value[2..])
    } else {
        value.into()
    })
}
fn reg(value: &str, span: Span) -> Result<Register> {
    let n = value
        .strip_prefix('x')
        .filter(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
        .and_then(|s| s.parse::<u8>().ok())
        .filter(|n| *n < 32);
    n.ok_or_else(|| Diagnostic::new("E_PARSE", span, "expected register x0..x31"))
}
fn number(value: &str, low: i64, high: i64, span: Span) -> Result<i64> {
    let (negative, digits) = value
        .strip_prefix('-')
        .map_or((false, value), |s| (true, s));
    let parsed = if let Some(digits) = digits.strip_prefix("0x") {
        if digits.is_empty()
            || !digits
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            None
        } else {
            i128::from_str_radix(digits, 16).ok()
        }
    } else if !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) {
        digits.parse::<i128>().ok()
    } else {
        None
    };
    let n = parsed.map(|n| if negative { -n } else { n });
    n.filter(|n| (*n >= low as i128) && (*n <= high as i128))
        .map(|n| n as i64)
        .ok_or_else(|| {
            Diagnostic::new(
                "E_PARSE",
                span,
                format!("expected immediate {low}..={high}"),
            )
        })
}
fn memory(value: &str, span: Span) -> Result<(Register, i64)> {
    let Some((offset, rest)) = value.split_once('(') else {
        return Err(Diagnostic::new("E_PARSE", span, "expected offset(base)"));
    };
    let Some(base) = rest.strip_suffix(')') else {
        return Err(Diagnostic::new(
            "E_PARSE",
            span,
            "expected closing parenthesis",
        ));
    };
    Ok((
        reg(base.trim(), span)?,
        number(offset.trim(), -2048, 2047, span)?,
    ))
}

pub fn parse(file: FileId, source: &str) -> Result<Program> {
    if source.len() > u32::MAX as usize {
        return Err(Diagnostic::new(
            "I_LIMIT",
            Span::default(),
            "assembly source exceeds Span capacity",
        ));
    }
    let mut program = Program::default();
    let mut defined = std::collections::BTreeSet::new();
    let mut offset = 0;
    for raw in source.split_inclusive('\n') {
        let span = Span::new(
            file,
            offset,
            offset + raw.trim_end_matches(['\n', '\r']).len(),
        );
        offset += raw.len();
        let line = raw.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        if let Some(label) = line.strip_suffix(':') {
            let label = symbol(file, label, span)?;
            if !defined.insert(label.clone()) {
                return Err(Diagnostic::new("E_PARSE", span, "duplicate assembly label"));
            }
            program.instructions.push(Inst::Label(label));
            continue;
        }
        let (op, operands) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let args: Vec<_> = if operands.trim().is_empty() {
            vec![]
        } else {
            operands.split(',').map(str::trim).collect()
        };
        let count = |n| {
            if args.len() == n {
                Ok(())
            } else {
                Err(Diagnostic::new(
                    "E_PARSE",
                    span,
                    format!("{op} expects {n} operands"),
                ))
            }
        };
        let r = |i: usize| reg(args[i], span);
        let imm = |i: usize, low, high| number(args[i], low, high, span);
        let target = |i: usize| symbol(file, args[i], span);
        let inst = match op {
            "add" | "sub" | "mul" | "mulhu" | "divu" | "remu" | "and" | "or" | "xor" | "sltu"
            | "sll" | "srl" => {
                count(3)?;
                let op = match op {
                    "add" => ROp::Add,
                    "sub" => ROp::Sub,
                    "mul" => ROp::Mul,
                    "mulhu" => ROp::Mulhu,
                    "divu" => ROp::Divu,
                    "remu" => ROp::Remu,
                    "and" => ROp::And,
                    "or" => ROp::Or,
                    "xor" => ROp::Xor,
                    "sltu" => ROp::Sltu,
                    "sll" => ROp::Sll,
                    _ => ROp::Srl,
                };
                Inst::R {
                    op,
                    rd: r(0)?,
                    rs1: r(1)?,
                    rs2: r(2)?,
                }
            }
            "addi" | "addiw" | "xori" | "sltiu" | "slli" | "srli" => {
                count(3)?;
                let shift = matches!(op, "slli" | "srli");
                let op = match op {
                    "addi" => IOp::Addi,
                    "addiw" => IOp::Addiw,
                    "xori" => IOp::Xori,
                    "sltiu" => IOp::Sltiu,
                    "slli" => IOp::Slli,
                    _ => IOp::Srli,
                };
                Inst::I {
                    op,
                    rd: r(0)?,
                    rs1: r(1)?,
                    imm: imm(
                        2,
                        if shift { 0 } else { -2048 },
                        if shift { 63 } else { 2047 },
                    )?,
                }
            }
            "lbu" | "lwu" | "ld" | "sb" | "sw" | "sd" | "jalr" => {
                count(2)?;
                let (base, offset) = memory(args[1], span)?;
                match op {
                    "jalr" => Inst::Jalr {
                        rd: r(0)?,
                        base,
                        offset,
                    },
                    "lbu" | "lwu" | "ld" => Inst::Load {
                        op: match op {
                            "lbu" => LoadOp::ByteUnsigned,
                            "lwu" => LoadOp::WordUnsigned,
                            _ => LoadOp::Double,
                        },
                        rd: r(0)?,
                        base,
                        offset,
                    },
                    _ => Inst::Store {
                        op: match op {
                            "sb" => StoreOp::Byte,
                            "sw" => StoreOp::Word,
                            _ => StoreOp::Double,
                        },
                        rs: r(0)?,
                        base,
                        offset,
                    },
                }
            }
            "beq" | "bne" | "bltu" | "bgeu" => {
                count(3)?;
                Inst::Branch {
                    condition: match op {
                        "beq" => Condition::Eq,
                        "bne" => Condition::Ne,
                        "bltu" => Condition::LtUnsigned,
                        _ => Condition::GeUnsigned,
                    },
                    rs1: r(0)?,
                    rs2: r(1)?,
                    target: target(2)?,
                }
            }
            "address" => {
                count(2)?;
                let rd = r(0)?;
                if rd == ZERO {
                    return Err(Diagnostic::new(
                        "E_PARSE",
                        span,
                        "address destination cannot be x0",
                    ));
                }
                Inst::Address {
                    rd,
                    target: target(1)?,
                }
            }
            "call" | "jump" => {
                count(1)?;
                Inst::Jump {
                    rd: if op == "call" { RA } else { ZERO },
                    target: target(0)?,
                }
            }
            "ecall" => {
                count(0)?;
                Inst::Ecall
            }
            "unimp" => {
                count(0)?;
                Inst::Unimp
            }
            _ => {
                return Err(Diagnostic::new(
                    "I_UNSUPPORTED",
                    span,
                    format!("unsupported s0 instruction {op}"),
                ));
            }
        };
        program.instructions.push(inst);
    }
    Ok(program)
}
