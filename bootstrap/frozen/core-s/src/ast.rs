//! Source structures for the bounded pilot, not a second typed tree.

use crate::source::Span;

#[derive(Clone, Debug)]
pub struct Program {
    pub module: String,
    pub imports: Vec<(String, Span)>,
    pub declarations: Vec<Declaration>,
}

#[derive(Clone, Debug)]
pub struct Declaration {
    pub public: bool,
    pub kind: DeclarationKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum DeclarationKind {
    Struct {
        name: String,
        fields: Vec<Field>,
    },
    Function(Function),
    ExternFunction {
        function: Function,
        symbol: String,
    },
    Static {
        name: String,
        ty: Type,
        mutable: bool,
        value: Expr,
    },
    Const {
        name: String,
        ty: Type,
        value: Expr,
    },
}

#[derive(Clone, Debug)]
pub struct Field {
    pub public: bool,
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub result: Type,
    pub body: Block,
    pub unsafe_: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Type {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TypeKind {
    Slice {
        element: Box<Type>,
        const_: bool,
    },
    Named(String),
    Unit,
    Array {
        length: Box<Expr>,
        element: Box<Type>,
    },
    Pointer {
        target: Box<Type>,
        const_: bool,
    },
    Result {
        ok: Box<Type>,
        error: Box<Type>,
    },
}

#[derive(Clone, Debug)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum StatementKind {
    Bind {
        name: String,
        ty: Type,
        mutable: bool,
        value: Option<Expr>,
    },
    Assign {
        target: Expr,
        value: Expr,
    },
    Expr(Expr),
    Block(Block),
    Unsafe(Block),
    If {
        condition: Expr,
        then: Block,
        else_: Option<Block>,
    },
    While {
        condition: Expr,
        body: Block,
    },
    Loop(Block),
    Match {
        value: Expr,
        arms: Vec<Arm>,
    },
    Return(Expr),
    Break,
    Continue,
    Defer(Block),
    Empty,
}

#[derive(Clone, Debug)]
pub struct Arm {
    pub pattern: Pattern,
    pub body: Block,
}

#[derive(Clone, Debug)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum PatternKind {
    Wildcard,
    Bool(bool),
    Integer(String),
    Result {
        ok: Type,
        error: Type,
        is_ok: bool,
        binding: Option<Parameter>,
    },
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    String(Vec<u8>),
    Pointer {
        target: Type,
        address: Box<Expr>,
    },
    Integer(String),
    Bool(bool),
    Unit,
    Null,
    Name(String),
    Unary {
        op: UnaryOp,
        value: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Cast {
        value: Box<Expr>,
        ty: Type,
    },
    Call {
        name: String,
        arguments: Vec<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: String,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Struct {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    Array(Vec<Expr>),
    Repeat {
        value: Box<Expr>,
        length: Box<Expr>,
    },
    Result {
        ok: Type,
        error: Type,
        is_ok: bool,
        value: Box<Expr>,
    },
    Try(Box<Expr>),
    Sizeof(Type),
    Alignof(Type),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    BitNot,
    Neg,
    Deref,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    WrappingAdd,
    WrappingSub,
    WrappingMul,
    Shl,
    Shr,
    BitAnd,
    BitOr,
    BitXor,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}
