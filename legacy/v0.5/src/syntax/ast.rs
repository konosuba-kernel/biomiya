use crate::source::Span;
#[derive(Clone, Debug)]
pub struct AstFile {
    pub module: Vec<String>,
    pub imports: Vec<Import>,
    pub declarations: Vec<Decl>,
}
#[derive(Clone, Debug)]
pub struct Import {
    pub path: Vec<String>,
    pub names: Vec<String>,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Attribute {
    pub name: String,
    pub arguments: Vec<Expr>,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Decl {
    pub name: String,
    pub public: bool,
    pub attributes: Vec<Attribute>,
    pub kind: DeclKind,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum DeclKind {
    Alias(Type),
    Struct {
        resource: bool,
        generics: Vec<GenericParam>,
        fields: Vec<Field>,
        newtype: Option<Type>,
    },
    Enum {
        generics: Vec<GenericParam>,
        variants: Vec<Variant>,
    },
    Function(Function),
    Impl {
        generics: Vec<GenericParam>,
        target: Type,
        methods: Vec<Decl>,
    },
    Const {
        ty: Type,
        value: Expr,
    },
    Static {
        mutable: bool,
        external: bool,
        ty: Type,
        value: Option<Expr>,
    },
    Assert(Expr),
}
#[derive(Clone, Debug)]
pub struct GenericParam {
    pub name: String,
    pub copy: bool,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub public: bool,
    pub ty: Type,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Field>,
    pub payload: Option<Type>,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Function {
    pub generics: Vec<GenericParam>,
    pub parameters: Vec<Parameter>,
    pub result: Type,
    pub unsafe_: bool,
    pub external: bool,
    pub effects: Vec<String>,
    /// None is an external declaration; an external ABI definition still has a body.
    pub body: Option<Block>,
}
#[derive(Clone, Debug)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
    /// Preserves self, &self, or &mut self; ty contains its expanded Self type.
    pub receiver: Option<String>,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Type {
    pub kind: TypeKind,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum TypeKind {
    Path(Vec<PathSegment>),
    Ref {
        mutable: bool,
        inner: Box<Type>,
    },
    Pointer {
        space: Option<String>,
        constant: bool,
        inner: Box<Type>,
    },
    Slice {
        constant: bool,
        inner: Box<Type>,
    },
    Array {
        length: Box<Expr>,
        inner: Box<Type>,
    },
    Tuple(Vec<Type>),
    Function {
        parameters: Vec<Type>,
        result: Box<Type>,
        unsafe_: bool,
        external: bool,
        effects: Vec<String>,
    },
    Never,
}
#[derive(Clone, Debug)]
/// Type and value paths share segments; value generic arguments use ::<...>.
pub struct PathSegment {
    pub name: String,
    pub arguments: Vec<Type>,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum ExprKind {
    Integer(String),
    String(Vec<u8>),
    Bool(bool),
    Null,
    Path(Vec<PathSegment>),
    Tuple(Vec<Expr>),
    Array(Vec<Expr>),
    Repeat {
        value: Box<Expr>,
        length: Box<Expr>,
    },
    Record {
        path: Vec<PathSegment>,
        fields: Vec<(String, Expr)>,
    },
    Unary {
        op: String,
        operand: Box<Expr>,
        adjacent: bool,
    },
    Binary {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Cast {
        value: Box<Expr>,
        ty: Type,
    },
    Call {
        callee: Box<Expr>,
        arguments: Vec<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: String,
        arguments: Vec<Type>,
        /// Parenthesized selection is a field value even when a method shares its name.
        grouped: bool,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Try(Box<Expr>),
    Block(Block),
    Unsafe(Block),
    If {
        condition: Box<Expr>,
        then: Block,
        else_: Option<Box<Expr>>,
    },
    Match {
        value: Box<Expr>,
        arms: Vec<(Pattern, Expr)>,
    },
    Loop(Block),
    While {
        condition: Box<Expr>,
        body: Block,
    },
    For {
        move_: bool,
        binding: String,
        input: Box<Expr>,
        body: Block,
    },
    Return(Option<Box<Expr>>),
    Break(Option<Box<Expr>>),
    Continue,
    Layout {
        operation: String,
        ty: Type,
        field: Option<String>,
    },
    Asm {
        template: Vec<u8>,
        operands: Vec<AsmOperand>,
        effects: Vec<String>,
    },
}
#[derive(Clone, Debug)]
pub struct AsmOperand {
    pub kind: String,
    pub register: Vec<u8>,
    pub value: Option<Expr>,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum Statement {
    Empty(Span),
    Binding {
        mutable: bool,
        pattern: Pattern,
        ty: Option<Type>,
        value: Expr,
        span: Span,
    },
    Assert(Expr),
    Defer(Expr),
    Expr(Expr),
}
#[derive(Clone, Debug)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum PatternKind {
    Wildcard,
    Path(Vec<String>),
    Integer {
        value: String,
        negative: bool,
        adjacent: bool,
    },
    Bool(bool),
    Tuple(Vec<Pattern>),
    Payload {
        path: Vec<String>,
        value: Box<Pattern>,
    },
    Record {
        path: Vec<String>,
        fields: Vec<(String, Pattern)>,
    },
}
