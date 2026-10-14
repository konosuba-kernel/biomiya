//! Typed machine operations shared by assembly rendering and direct encoding.
//! This is a bounded RV64 prototype contract, not a public assembler language.

pub type Register = u8;
pub const ZERO: Register = 0;
pub const RA: Register = 1;
pub const SP: Register = 2;
/// Reserved by the backend at control-flow expansions, never a live IR value.
pub const SCRATCH: Register = 31;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ROp {
    Add,
    Sub,
    Mul,
    Mulhu,
    Divu,
    Remu,
    And,
    Or,
    Xor,
    Sltu,
    Sll,
    Srl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IOp {
    Addi,
    Addiw,
    Xori,
    Sltiu,
    Slli,
    Srli,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadOp {
    ByteUnsigned,
    WordUnsigned,
    Double,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreOp {
    Byte,
    Word,
    Double,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    Eq,
    Ne,
    LtUnsigned,
    GeUnsigned,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inst {
    Label(String),
    R {
        op: ROp,
        rd: Register,
        rs1: Register,
        rs2: Register,
    },
    I {
        op: IOp,
        rd: Register,
        rs1: Register,
        imm: i64,
    },
    Load {
        op: LoadOp,
        rd: Register,
        base: Register,
        offset: i64,
    },
    Store {
        op: StoreOp,
        rs: Register,
        base: Register,
        offset: i64,
    },
    /// Expand before assigning labels; preserves all registers other than rd.
    Li {
        rd: Register,
        bits: u64,
    },
    /// Fixed 12 bytes: inverse short branch, then PC-relative long jump.
    Branch {
        condition: Condition,
        rs1: Register,
        rs2: Register,
        target: String,
    },
    /// Fixed 8 bytes. rd=RA is call, rd=ZERO is an unconditional jump.
    Jump {
        rd: Register,
        target: String,
    },
    /// Fixed 8 bytes, destination must be nonzero.
    Address {
        rd: Register,
        target: String,
    },
    Jalr {
        rd: Register,
        base: Register,
        offset: i64,
    },
    Ecall,
    /// A deliberately illegal noncompressed instruction if an unreachable edge runs.
    Unimp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataRelocation {
    pub offset: u64,
    pub target: String,
    pub addend: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataObject {
    pub symbol: String,
    pub mutable: bool,
    pub align: u64,
    pub bytes: Vec<u8>,
    pub zero_fill: u64,
    pub relocations: Vec<DataRelocation>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Program {
    pub instructions: Vec<Inst>,
    pub data: Vec<DataObject>,
}

/// Validate data independently of address assignment (also used by text rendering).
pub fn validate_data(data: &[DataObject]) -> crate::diagnostic::Result<()> {
    use crate::{diagnostic::Diagnostic, source::Span};
    let invalid = |message| Diagnostic::new("I_INTERNAL", Span::default(), message);
    let mut names = std::collections::BTreeSet::new();
    for object in data {
        let mut chars = object.symbol.bytes();
        if !chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_' || c == b'.')
            || object.symbol == "."
            || !chars.all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.')
            || !names.insert(&object.symbol)
        {
            return Err(invalid("invalid or duplicate data symbol"));
        }
        if !object.align.is_power_of_two() || object.align > 4096 {
            return Err(invalid("invalid data alignment"));
        }
        if (object.bytes.is_empty() && (!object.mutable || object.zero_fill == 0))
            || (!object.bytes.is_empty() && object.zero_fill != 0)
        {
            return Err(invalid("data requires bytes or mutable nonempty BSS"));
        }
        let mut occupied = std::collections::BTreeSet::new();
        for relocation in &object.relocations {
            if relocation
                .offset
                .checked_add(8)
                .is_none_or(|end| end > object.bytes.len() as u64)
            {
                return Err(invalid("ABS64 relocation outside initialized object"));
            }
            for byte in relocation.offset..relocation.offset + 8 {
                if !occupied.insert(byte) {
                    return Err(invalid("overlapping ABS64 relocations"));
                }
            }
        }
    }
    Ok(())
}
