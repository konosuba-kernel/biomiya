//! Direct RV64 encoding. Symbol layout precedes fixed-size control-flow fixups.
use crate::{
    diagnostic::{Diagnostic, Result},
    machine::*,
    source::Span,
};
use std::collections::BTreeMap;

const MAX_TEXT: usize = 64 * 1024 * 1024;

pub struct Encoded {
    pub bytes: Vec<u8>,
    pub symbols: BTreeMap<String, u64>,
    pub fixups: usize,
}

fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, Span::default(), message)
}
fn internal(message: impl Into<String>) -> Diagnostic {
    error("I_INTERNAL", message)
}
fn limit(message: impl Into<String>) -> Diagnostic {
    error("I_LIMIT", message)
}
fn reg(r: Register) -> Result<u32> {
    if r < 32 {
        Ok(r.into())
    } else {
        Err(internal(format!("invalid register x{r}")))
    }
}
fn immediate(n: i64, low: i64, high: i64) -> Result<u32> {
    if (low..=high).contains(&n) {
        Ok(n as u32)
    } else {
        Err(limit(format!("immediate {n} outside {low}..={high}")))
    }
}
fn iword(opcode: u32, f3: u32, rd: Register, rs: Register, imm: i64) -> Result<u32> {
    Ok(((immediate(imm, -2048, 2047)? & 4095) << 20)
        | (reg(rs)? << 15)
        | (f3 << 12)
        | (reg(rd)? << 7)
        | opcode)
}

// Balanced base-4096 digits avoid host signed overflow, including i64::MAX.
// Every intermediate machine value is taken modulo 2^64, as RV64 specifies.
fn constant(rd: Register, n: i128, out: &mut Vec<u32>) -> Result<()> {
    if (-2048..=2047).contains(&n) {
        out.push(iword(0x13, 0, rd, ZERO, n as i64)?);
    } else {
        let low = ((n + 2048) & 4095) - 2048;
        constant(rd, (n - low) >> 12, out)?;
        out.push((12 << 20) | (reg(rd)? << 15) | (1 << 12) | (reg(rd)? << 7) | 0x13);
        if low != 0 {
            out.push(iword(0x13, 0, rd, rd, low as i64)?);
        }
    }
    Ok(())
}

fn ordinary(inst: &Inst) -> Result<Vec<u32>> {
    let word = match inst {
        Inst::R { op, rd, rs1, rs2 } => {
            let (f7, f3) = match op {
                ROp::Add => (0, 0),
                ROp::Sub => (32, 0),
                ROp::Mul => (1, 0),
                ROp::Mulhu => (1, 3),
                ROp::Divu => (1, 5),
                ROp::Remu => (1, 7),
                ROp::And => (0, 7),
                ROp::Or => (0, 6),
                ROp::Xor => (0, 4),
                ROp::Sltu => (0, 3),
                ROp::Sll => (0, 1),
                ROp::Srl => (0, 5),
            };
            (f7 << 25)
                | (reg(*rs2)? << 20)
                | (reg(*rs1)? << 15)
                | (f3 << 12)
                | (reg(*rd)? << 7)
                | 0x33
        }
        Inst::I { op, rd, rs1, imm } => match op {
            IOp::Slli | IOp::Srli => {
                let f3 = if *op == IOp::Slli { 1 } else { 5 };
                (immediate(*imm, 0, 63)? << 20)
                    | (reg(*rs1)? << 15)
                    | (f3 << 12)
                    | (reg(*rd)? << 7)
                    | 0x13
            }
            _ => {
                let (opcode, f3) = match op {
                    IOp::Addi => (0x13, 0),
                    IOp::Addiw => (0x1b, 0),
                    IOp::Xori => (0x13, 4),
                    IOp::Sltiu => (0x13, 3),
                    _ => unreachable!(),
                };
                iword(opcode, f3, *rd, *rs1, *imm)?
            }
        },
        Inst::Load {
            op,
            rd,
            base,
            offset,
        } => {
            let f3 = match op {
                LoadOp::ByteUnsigned => 4,
                LoadOp::WordUnsigned => 6,
                LoadOp::Double => 3,
            };
            iword(3, f3, *rd, *base, *offset)?
        }
        Inst::Store {
            op,
            rs,
            base,
            offset,
        } => {
            let f3 = match op {
                StoreOp::Byte => 0,
                StoreOp::Word => 2,
                StoreOp::Double => 3,
            };
            let n = immediate(*offset, -2048, 2047)? & 4095;
            ((n >> 5) << 25)
                | (reg(*rs)? << 20)
                | (reg(*base)? << 15)
                | (f3 << 12)
                | ((n & 31) << 7)
                | 0x23
        }
        Inst::Li { rd, bits } => {
            reg(*rd)?;
            let mut out = Vec::new();
            constant(*rd, *bits as i64 as i128, &mut out)?;
            return Ok(out);
        }
        Inst::Jalr { rd, base, offset } => iword(0x67, 0, *rd, *base, *offset)?,
        Inst::Ecall => 0x73,
        Inst::Unimp => 0xc0001073, // CSRRW x0, cycle, x0: writes a read-only CSR.
        _ => return Err(internal("symbolic instruction reached ordinary encoder")),
    };
    Ok(vec![word])
}
fn size(inst: &Inst) -> Result<usize> {
    Ok(match inst {
        Inst::Label(name) => {
            if name.is_empty() {
                return Err(internal("empty symbol name"));
            }
            0
        }
        Inst::Branch { rs1, rs2, .. } => {
            reg(*rs1)?;
            reg(*rs2)?;
            12
        }
        Inst::Jump { rd, .. } => {
            if *rd != ZERO && *rd != RA {
                return Err(internal("Jump destination must be x0 or x1"));
            }
            8
        }
        Inst::Address { rd, .. } => {
            reg(*rd)?;
            if *rd == ZERO {
                return Err(internal("Address destination cannot be x0"));
            }
            8
        }
        _ => ordinary(inst)?.len() * 4,
    })
}
fn pair(rd: Register, scratch: Register, target: u64, pc: u64, jump: bool) -> Result<[u32; 2]> {
    let delta = target as i128 - pc as i128;
    let hi = (delta + 2048) >> 12;
    let lo = delta - (hi << 12);
    if !(-524288..=524287).contains(&hi) {
        return Err(limit("PC-relative address exceeds AUIPC range"));
    }
    let auipc = (((hi as u32) & 0xfffff) << 12) | (reg(scratch)? << 7) | 0x17;
    Ok([
        auipc,
        iword(if jump { 0x67 } else { 0x13 }, 0, rd, scratch, lo as i64)?,
    ])
}
fn target(symbols: &BTreeMap<String, u64>, name: &str) -> Result<u64> {
    symbols
        .get(name)
        .copied()
        .ok_or_else(|| internal(format!("undefined symbol {name}")))
}

pub fn text_size(program: &Program) -> Result<u64> {
    let mut length = 0usize;
    for inst in &program.instructions {
        length = length
            .checked_add(size(inst)?)
            .filter(|n| *n <= MAX_TEXT)
            .ok_or_else(|| limit("text exceeds 64 MiB"))?;
    }
    if length == 0 {
        return Err(internal("empty machine program"));
    }
    Ok(length as u64)
}

pub fn encode(program: &Program, base: u64) -> Result<Encoded> {
    encode_with_symbols(program, base, &BTreeMap::new())
}

pub fn encode_with_symbols(
    program: &Program,
    base: u64,
    external: &BTreeMap<String, u64>,
) -> Result<Encoded> {
    if base % 4 != 0 {
        return Err(internal("text base must be 4-byte aligned"));
    }
    let mut symbols = BTreeMap::new();
    let mut length = 0usize;
    for inst in &program.instructions {
        let bytes = size(inst)?;
        if let Inst::Label(name) = inst {
            let address = base
                .checked_add(length as u64)
                .ok_or_else(|| limit("symbol address overflow"))?;
            if external.contains_key(name) || symbols.insert(name.clone(), address).is_some() {
                return Err(internal(format!("duplicate symbol {name}")));
            }
        }
        length = length
            .checked_add(bytes)
            .filter(|n| *n <= MAX_TEXT)
            .ok_or_else(|| limit("text exceeds 64 MiB"))?;
    }
    if length == 0 {
        return Err(internal("empty machine program"));
    }
    base.checked_add(length as u64)
        .ok_or_else(|| limit("text address overflow"))?;
    let mut bytes = Vec::with_capacity(length);
    let mut fixups = 0;
    for inst in &program.instructions {
        let pc = base + bytes.len() as u64;
        let words = match inst {
            Inst::Label(_) => continue,
            Inst::Jump { rd, target: name } => {
                fixups += 1;
                pair(*rd, SCRATCH, target(&symbols, name)?, pc, true)?.to_vec()
            }
            Inst::Address { rd, target: name } => {
                fixups += 1;
                pair(
                    *rd,
                    *rd,
                    symbols
                        .get(name)
                        .or_else(|| external.get(name))
                        .copied()
                        .ok_or_else(|| internal(format!("undefined symbol {name}")))?,
                    pc,
                    false,
                )?
                .to_vec()
            }
            Inst::Branch {
                condition,
                rs1,
                rs2,
                target: name,
            } => {
                // Inverse condition skips both long-jump instructions (+12).
                let inverse_f3 = match condition {
                    Condition::Eq => 1,
                    Condition::Ne => 0,
                    Condition::LtUnsigned => 7,
                    Condition::GeUnsigned => 6,
                };
                let branch =
                    (reg(*rs2)? << 20) | (reg(*rs1)? << 15) | (inverse_f3 << 12) | (6 << 8) | 0x63;
                let long = pair(ZERO, SCRATCH, target(&symbols, name)?, pc + 4, true)?;
                fixups += 1;
                vec![branch, long[0], long[1]]
            }
            _ => ordinary(inst)?,
        };
        for word in words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    debug_assert_eq!(bytes.len(), length);
    Ok(Encoded {
        bytes,
        symbols,
        fixups,
    })
}
