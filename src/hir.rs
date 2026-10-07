//! Resolved, typed source semantics retained alongside lowered control flow.
use crate::ir::{Data, ScopeId};
use crate::source::Span;
use crate::types::{DefId, FnType, TyId, TypeContext};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LoopId(pub u32);

#[derive(Clone, Debug)]
pub struct Local {
    pub name: String,
    pub ty: TyId,
    pub mutable: bool,
    pub scope: ScopeId,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub ty: TyId,
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Unit,
    Integer(u64),
    /// Static data bytes without implicit terminator; expression has slice type.
    String(Vec<u8>),
    Local(LocalId),
    Global {
        symbol: String,
        mutable: bool,
        external: bool,
    },
    Function {
        symbol: String,
    },
    Unary {
        op: String,
        value: Box<Expr>,
    },
    Binary {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Assign {
        op: String,
        place: Box<Expr>,
        value: Box<Expr>,
    },
    Cast(Box<Expr>),
    Adapt(Box<Expr>),
    Borrow {
        value: Box<Expr>,
        mutable: bool,
        reborrow: bool,
    },
    RawAddress {
        value: Box<Expr>,
        mutable: bool,
    },
    Deref(Box<Expr>),
    /// Field is the index in the resolved nominal/tuple layout.
    Field {
        base: Box<Expr>,
        field: usize,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Len(Box<Expr>),
    Slice {
        base: Box<Expr>,
        start: Box<Expr>,
        end: Box<Expr>,
    },
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },
    /// Preserve written field order; indices choose declaration-order placement.
    Aggregate {
        variant: Option<usize>,
        fields: Vec<(usize, Expr)>,
    },
    Array(Vec<Expr>),
    Repeat {
        value: Box<Expr>,
        length: u64,
    },
    Call {
        callee: Box<Expr>,
        signature: FnType,
        arguments: Vec<Expr>,
    },
    Intrinsic {
        name: String,
        arguments: Vec<Expr>,
        type_arguments: Vec<TyId>,
    },
    Block {
        scope: ScopeId,
        statements: Vec<Statement>,
        tail: Option<Box<Expr>>,
    },
    If {
        condition: Box<Expr>,
        then: Box<Expr>,
        else_: Option<Box<Expr>>,
    },
    Match {
        value: Box<Expr>,
        arms: Vec<(Pattern, Expr)>,
    },
    Loop {
        id: LoopId,
        body: Box<Expr>,
    },
    While {
        id: LoopId,
        condition: Box<Expr>,
        body: Box<Expr>,
    },
    For {
        id: LoopId,
        binding: Option<LocalId>,
        input: Box<Expr>,
        body: Box<Expr>,
        move_: bool,
    },
    Return(Option<Box<Expr>>),
    Break {
        id: LoopId,
        value: Option<Box<Expr>>,
    },
    Continue(LoopId),
    Try(Box<Expr>),
    Unsafe {
        scope: ScopeId,
        body: Box<Expr>,
    },
    Asm {
        template: String,
        inputs: Vec<(String, Expr)>,
        outputs: Vec<(String, Expr)>,
        clobbers: Vec<String>,
        may_block: bool,
    },
    SpecialTransfer {
        frame: Box<Expr>,
        handler: String,
        irq: bool,
    },
    Uninit,
}

#[derive(Clone, Debug)]
pub enum Statement {
    Bind {
        pattern: Pattern,
        value: Expr,
    },
    Expr(Expr),
    Defer {
        id: u32,
        expression: Expr,
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub struct Pattern {
    pub ty: TyId,
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum PatternKind {
    Wildcard,
    Bind(LocalId),
    BorrowBind {
        local: LocalId,
        mutable: bool,
    },
    Integer(u64),
    /// Covers tuple, struct, newtype and an enum's variant payload.
    Fields {
        variant: Option<usize>,
        fields: Vec<(usize, Pattern)>,
    },
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub definition: DefId,
    pub symbol: String,
    pub signature: FnType,
    pub type_arguments: Vec<TyId>,
    pub parameters: Vec<LocalId>,
    pub locals: Vec<Local>,
    pub body: Expr,
    pub span: Span,
    pub special: Option<String>,
    pub exported: bool,
    pub section: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub declarations: Option<crate::resolve::Resolver>,
    pub types: TypeContext,
    pub functions: Vec<Function>,
    pub data: Vec<Data>,
    pub externals: Vec<String>,
}
