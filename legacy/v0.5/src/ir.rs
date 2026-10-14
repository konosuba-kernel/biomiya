//! Sequential, typed control-flow IR shared by the emitter and optional analyses.
use crate::source::Span;
use crate::types::{DefId, FnType, TyId, TypeContext};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopeId(pub u32);

#[derive(Clone, Debug)]
pub struct Slot {
    pub ty: TyId,
    pub name: Option<String>,
    pub span: Span,
    pub scope: ScopeId,
}

#[derive(Clone, Debug)]
pub enum Operand {
    Slot(SlotId),
    Integer { bits: u64, ty: TyId },
    Symbol { name: String, addend: i64, ty: TyId },
}

impl Operand {
    pub fn ty(&self, function: &Function) -> TyId {
        match self {
            Self::Slot(id) => function.slots[id.0 as usize].ty,
            Self::Integer { ty, .. } | Self::Symbol { ty, .. } => *ty,
        }
    }
}

#[derive(Clone, Debug)]
pub enum PlaceBase {
    Slot(SlotId),
    Global(String),
    Pointer(Operand),
}

#[derive(Clone, Debug)]
pub struct Place {
    pub base: PlaceBase,
    pub offset: u64,
    pub ty: TyId,
    /// Known alignment of this address; packed projections may be less aligned.
    pub align: u64,
}

#[derive(Clone, Debug)]
pub enum Rvalue {
    Use(Operand),
    Load(Place),
    Address(Place),
    Unary {
        op: String,
        value: Operand,
    },
    /// Operators retain the Core spelling, including wrapping and Option forms.
    Binary {
        op: String,
        left: Operand,
        right: Operand,
    },
    Cast {
        value: Operand,
    },
}

#[derive(Clone, Debug)]
pub enum CallTarget {
    Direct(String),
    Indirect(Operand),
}

#[derive(Clone, Debug)]
pub struct AsmInput {
    pub register: String,
    pub value: Operand,
}
#[derive(Clone, Debug)]
pub struct AsmOutput {
    pub register: String,
    pub place: Place,
}

#[derive(Clone, Debug)]
pub enum InstructionKind {
    Set {
        dst: SlotId,
        value: Rvalue,
    },
    Store {
        dst: Place,
        value: Operand,
    },
    Copy {
        dst: Place,
        src: Place,
        size: u64,
    },
    Zero {
        dst: Place,
        size: u64,
    },
    Call {
        target: CallTarget,
        signature: FnType,
        arguments: Vec<Operand>,
        result: Option<SlotId>,
    },
    /// Identity is assigned by resolution, never by matching a user's spelling.
    Intrinsic {
        name: String,
        arguments: Vec<Operand>,
        result: Option<SlotId>,
        type_arguments: Vec<TyId>,
    },
    Asm {
        template: String,
        inputs: Vec<AsmInput>,
        outputs: Vec<AsmOutput>,
        clobbers: Vec<String>,
        may_block: bool,
    },
    /// Runtime-free semantic information consumed by extension views.
    Event(SemanticEvent),
}

#[derive(Clone, Debug)]
pub enum SemanticEvent {
    StorageLive(SlotId),
    StorageDead(SlotId),
    /// Read-only observation; does not consume or assert Copy of a descriptor/value.
    Inspect(Place),
    Copy(Place),
    Move(Place),
    Borrow {
        place: Place,
        mutable: bool,
        reborrow: bool,
        result: SlotId,
    },
    RawAddress {
        place: Place,
        result: SlotId,
    },
    /// Nonmoving weakening of a mutable slice descriptor to a readonly view.
    /// Retains the source view (address and length), result and both source types.
    ReborrowView {
        source: Operand,
        result: SlotId,
        from: TyId,
        to: TyId,
    },
    Conversion {
        from: TyId,
        to: TyId,
        result: SlotId,
    },
    Defer {
        source: u32,
        scope: ScopeId,
        registration: bool,
    },
    FullExpression {
        id: u32,
        entering: bool,
    },
    SplitSlice {
        input: Operand,
        result: SlotId,
    },
    Unsafe {
        scope: ScopeId,
        entering: bool,
    },
}

#[derive(Clone, Debug)]
pub struct Instruction {
    pub kind: InstructionKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Terminator {
    Goto(BlockId),
    Branch {
        condition: Operand,
        yes: BlockId,
        no: BlockId,
    },
    Switch {
        value: Operand,
        cases: Vec<(u64, BlockId)>,
        otherwise: BlockId,
    },
    Return(Option<Operand>),
    /// Message is a slice value, or None for an implicit static failure message.
    Panic(Option<Operand>),
    SpecialTransfer {
        frame: Operand,
        handler: String,
        irq: bool,
    },
    Unreachable,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub definition: DefId,
    pub symbol: String,
    pub signature: FnType,
    pub parameters: Vec<SlotId>,
    pub slots: Vec<Slot>,
    pub blocks: Vec<Block>,
    pub entry: BlockId,
    pub span: Span,
    pub special: Option<String>,
    pub exported: bool,
    pub section: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Relocation {
    pub offset: u64,
    pub symbol: String,
    pub addend: i64,
}

#[derive(Clone, Debug)]
pub struct Data {
    pub symbol: String,
    pub bytes: Vec<u8>,
    pub relocations: Vec<Relocation>,
    pub align: u64,
    pub writable: bool,
    pub exported: bool,
    pub section: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Module {
    pub functions: Vec<Function>,
    pub data: Vec<Data>,
    pub externals: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Compilation {
    pub types: TypeContext,
    pub module: Module,
}
