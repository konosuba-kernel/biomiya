//! Concrete declarations and typed CFG. Backend inputs contain no source trees.

use std::collections::{BTreeMap, HashMap};

use crate::{
    ast,
    diagnostic::{Diagnostic, Result},
    source::Span,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TypeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlotId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Integer {
    U8,
    U32,
    Usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: TypeId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeKind {
    Unit,
    Bool,
    Int(Integer),
    Pointer { target: TypeId, const_: bool },
    Array { element: TypeId, length: u64 },
    Slice { element: TypeId, const_: bool },
    Struct { name: String, fields: Vec<Field> },
    Result { ok: TypeId, error: TypeId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub size: u64,
    pub align: u64,
    pub field_offsets: Vec<u64>,
    pub payload_offset: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeInfo {
    pub kind: TypeKind,
    pub layout: Layout,
}

#[derive(Clone, Debug, Default)]
pub struct TypeTable {
    pub entries: Vec<TypeInfo>,
    pub names: BTreeMap<String, TypeId>,
}

impl TypeTable {
    /// Only completed layouts are published to consumers.
    pub fn intern(&mut self, kind: TypeKind, layout: Layout, span: Span) -> Result<TypeId> {
        if let Some(index) = self.entries.iter().position(|entry| entry.kind == kind) {
            if self.entries[index].layout != layout {
                return Err(Diagnostic::new(
                    "I_INTERNAL",
                    span,
                    "conflicting concrete layout",
                ));
            }
            return Ok(TypeId(index as u32));
        }
        let index = u32::try_from(self.entries.len())
            .map_err(|_| Diagnostic::new("I_LIMIT", span, "too many concrete types"))?;
        self.entries.push(TypeInfo { kind, layout });
        Ok(TypeId(index))
    }

    /// Checking may add pointer types needed by explicit address expressions.
    pub fn pointer(&mut self, target: TypeId, const_: bool, span: Span) -> Result<TypeId> {
        if self.entries.get(target.0 as usize).is_none() {
            return Err(Diagnostic::new(
                "I_INTERNAL",
                span,
                "invalid pointer target type",
            ));
        }
        self.intern(
            TypeKind::Pointer { target, const_ },
            Layout {
                size: 8,
                align: 8,
                field_offsets: vec![],
                payload_offset: None,
            },
            span,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub parameters: Vec<TypeId>,
    pub result: TypeId,
    pub unsafe_: bool,
}

#[derive(Clone, Debug)]
pub struct ResolvedConstant {
    pub ty: TypeId,
    pub bits: u64,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ResolvedFunction {
    pub name: String,
    pub parameters: Vec<ast::Parameter>,
    pub signature: Signature,
    pub body: ast::Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ResolvedGlobal {
    pub name: String,
    pub ty: TypeId,
    pub mutable: bool,
    pub initializer: ast::Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ResolvedProgram {
    pub globals: Vec<ResolvedGlobal>,
    pub externs: BTreeMap<FunctionId, String>,
    pub types: TypeTable,
    pub functions: Vec<ResolvedFunction>,
    pub constants: BTreeMap<String, ResolvedConstant>,
    /// Every source Type occurrence, including locals, patterns and constructors.
    pub type_uses: HashMap<Span, TypeId>,
    /// Repeat expression lengths are checked/evaluated by P, keyed by length span.
    pub repeat_lengths: HashMap<Span, u64>,
}

#[derive(Clone, Debug)]
pub struct Compilation {
    pub data: Vec<crate::machine::DataObject>,
    pub externs: BTreeMap<FunctionId, String>,
    pub types: TypeTable,
    /// FunctionId is this vector's index, preserving resolved declaration order.
    pub functions: Vec<Function>,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub symbol: String,
    pub signature: Signature,
    pub parameter_slots: Vec<SlotId>,
    pub slots: Vec<Slot>,
    pub blocks: Vec<Block>,
    pub entry: BlockId,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Slot {
    pub ty: TypeId,
    pub name: Option<String>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Operand {
    Slot(SlotId),
    /// Unit/bool/integers, or typed null bits=0; aggregates always occupy slots.
    Immediate {
        bits: u64,
        ty: TypeId,
    },
}

#[derive(Clone, Debug)]
pub enum PlaceBase {
    Slot(SlotId),
    Pointer(Operand),
}

#[derive(Clone, Debug)]
pub struct Place {
    pub base: PlaceBase,
    pub byte_offset: u64,
    pub ty: TypeId,
}

#[derive(Clone, Debug)]
pub enum Rvalue {
    SymbolAddress {
        symbol: String,
    },
    Bitcast {
        value: Operand,
    },
    Use(Operand),
    Load(Place),
    Address(Place),
    /// Internal address calculation; never a source pointer/integer cast.
    PointerOffset {
        base: Operand,
        bytes: Operand,
    },
    Unary {
        op: ast::UnaryOp,
        value: Operand,
    },
    Binary {
        op: ast::BinaryOp,
        left: Operand,
        right: Operand,
    },
    Cast {
        value: Operand,
    },
}

#[derive(Clone, Debug)]
pub enum InstructionKind {
    BoundsDynamic {
        index: Operand,
        length: Operand,
    },
    Set {
        dst: SlotId,
        value: Rvalue,
    },
    Store {
        destination: Place,
        value: Operand,
    },
    /// Whole typed value, including aggregates; size comes from destination.ty.
    Copy {
        destination: Place,
        source: Place,
    },
    /// C emits this explicitly before indexing; B does not infer checks from Place.
    Bounds {
        index: Operand,
        length: u64,
    },
    Call {
        function: FunctionId,
        arguments: Vec<Operand>,
        result: Option<SlotId>,
    },
    StorageLive(SlotId),
    StorageDead(SlotId),
}

#[derive(Clone, Debug)]
pub struct Instruction {
    pub kind: InstructionKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Terminator {
    Jump(BlockId),
    Branch {
        condition: Operand,
        yes: BlockId,
        no: BlockId,
    },
    Return(Operand),
    Panic,
    /// Structurally unreachable source continuation: never executed.
    Unreachable,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
    pub span: Span,
}
