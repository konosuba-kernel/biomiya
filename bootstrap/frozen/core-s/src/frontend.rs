//! Root-module pilot parser and finite constant/layout dependency evaluation.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{
    ast::{self, BinaryOp as Bin, UnaryOp as Un},
    diagnostic::{Diagnostic, Result},
    lexer::{Token, lex},
    model::{self, Integer, Layout, TypeId},
    source::{FileId, Span},
};

fn error(code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, span, message)
}

pub fn parse(file: FileId, source: &str) -> Result<ast::Program> {
    Parser {
        tokens: lex(file, source)?,
        at: 0,
    }
    .program()
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn text(&self) -> &str {
        &self.tokens[self.at].text
    }
    fn span(&self) -> Span {
        self.tokens[self.at].span
    }
    fn bump(&mut self) -> Token {
        let token = self.tokens[self.at].clone();
        if !token.text.is_empty() {
            self.at += 1;
        }
        token
    }
    fn eat(&mut self, text: &str) -> bool {
        if self.text() == text {
            self.bump();
            true
        } else {
            false
        }
    }
    fn need(&mut self, text: &str) -> Result<()> {
        if self.eat(text) {
            Ok(())
        } else {
            Err(error("E_PARSE", self.span(), format!("expected {text}")))
        }
    }
    fn end(&self, start: Span) -> Span {
        if self.at == 0 {
            start
        } else {
            start.join(self.tokens[self.at - 1].span)
        }
    }
    fn unsupported<T>(&self, feature: &str) -> Result<T> {
        Err(error(
            "I_UNSUPPORTED",
            self.span(),
            format!("pilot does not implement {feature}"),
        ))
    }
    fn ident(&mut self) -> Result<String> {
        let word = self.text();
        let starts = word
            .chars()
            .next()
            .is_some_and(|c| c == '_' || c.is_alphabetic() || !c.is_ascii());
        let reserved = [
            "fn", "let", "var", "const", "struct", "enum", "if", "else", "match", "while", "loop",
            "for", "return", "break", "continue", "defer", "unsafe", "true", "false", "null", "as",
            "module", "import", "pub", "extern", "type", "impl", "resource", "static",
        ];
        if !starts || word == "_" || reserved.contains(&word) {
            return Err(error("E_PARSE", self.span(), "expected identifier"));
        }
        Ok(self.bump().text)
    }
    fn close_gt(&mut self) -> Result<()> {
        if self.text() == ">" {
            self.bump();
            return Ok(());
        }
        if matches!(self.text(), ">>" | ">=" | ">>=") {
            self.tokens[self.at].text.remove(0);
            self.tokens[self.at].span.start += 1;
            return Ok(());
        }
        self.need(">")
    }
    fn path(&mut self) -> Result<String> {
        let mut name = self.ident()?;
        while self.text() == "::" && self.tokens.get(self.at + 1).is_some_and(|t| t.text != "<") {
            self.bump();
            name.push_str("::");
            name.push_str(&self.ident()?);
        }
        Ok(name)
    }
    fn program(mut self) -> Result<ast::Program> {
        let module = if self.eat("module") {
            let n = self.path()?;
            self.need(";")?;
            n
        } else {
            String::new()
        };
        let mut imports = vec![];
        while self.eat("import") {
            let sp = self.span();
            let n = self.path()?;
            self.need(";")?;
            imports.push((n, sp));
        }
        let mut declarations = vec![];
        while !self.text().is_empty() {
            let start = self.span();
            let public = self.eat("pub");
            if ["type", "enum", "@", "static_assert"].contains(&self.text()) {
                return self.unsupported("this declaration");
            }
            let kind = if self.eat("struct") {
                let name = self.ident()?;
                self.need("{")?;
                let mut fields = vec![];
                while !self.eat("}") {
                    let public = self.eat("pub");
                    let span = self.span();
                    let name = self.ident()?;
                    self.need(":")?;
                    let ty = self.ty()?;
                    fields.push(ast::Field {
                        public,
                        name,
                        ty,
                        span: self.end(span),
                    });
                    if !self.eat(",") {
                        self.need("}")?;
                        break;
                    }
                }
                ast::DeclarationKind::Struct { name, fields }
            } else if self.text() == "const" || self.text() == "static" {
                let static_ = self.bump().text == "static";
                let mutable = static_ && self.eat("mut");
                let name = self.ident()?;
                self.need(":")?;
                let ty = self.ty()?;
                self.need("=")?;
                let value = self.expr(0, false)?;
                self.need(";")?;
                if static_ {
                    ast::DeclarationKind::Static {
                        name,
                        ty,
                        value,
                        mutable,
                    }
                } else {
                    ast::DeclarationKind::Const { name, ty, value }
                }
            } else {
                let external = self.eat("extern");
                if external {
                    self.need("abi")?;
                    self.need("(")?;
                    self.need("C")?;
                    self.need(")")?;
                }
                let unsafe_ = self.eat("unsafe") || external;
                self.need("fn")?;
                let name = self.ident()?;
                self.need("(")?;
                let mut parameters = vec![];
                while !self.eat(")") {
                    parameters.push(self.parameter()?);
                    if !self.eat(",") {
                        self.need(")")?;
                        break;
                    }
                }
                self.need("->")?;
                let result = self.ty()?;
                let body = if external {
                    self.need(";")?;
                    ast::Block {
                        statements: vec![],
                        span: start,
                    }
                } else {
                    self.block()?
                };
                let function = ast::Function {
                    name,
                    parameters,
                    result,
                    body,
                    unsafe_,
                    span: self.end(start),
                };
                if external {
                    if !function.name.is_ascii() {
                        return Err(error("E_NAME", start, "external symbol must be ASCII"));
                    }
                    ast::DeclarationKind::ExternFunction {
                        symbol: function.name.clone(),
                        function,
                    }
                } else {
                    ast::DeclarationKind::Function(function)
                }
            };
            declarations.push(ast::Declaration {
                public,
                kind,
                span: self.end(start),
            });
        }
        Ok(ast::Program {
            module,
            imports,
            declarations,
        })
    }
    fn parameter(&mut self) -> Result<ast::Parameter> {
        let span = self.span();
        let name = self.ident()?;
        self.need(":")?;
        let ty = self.ty()?;
        Ok(ast::Parameter {
            name,
            ty,
            span: self.end(span),
        })
    }
    fn ty(&mut self) -> Result<ast::Type> {
        let start = self.span();
        let kind = if self.eat("(") {
            self.need(")")?;
            ast::TypeKind::Unit
        } else if self.eat("[") {
            if self.eat("]") {
                let const_ = self.eat("const");
                let element = Box::new(self.ty()?);
                return Ok(ast::Type {
                    kind: ast::TypeKind::Slice { element, const_ },
                    span: self.end(start),
                });
            }
            let length = Box::new(self.expr(0, false)?);
            self.need("]")?;
            ast::TypeKind::Array {
                length,
                element: Box::new(self.ty()?),
            }
        } else if self.eat("*") {
            if ["user", "phys", "mmio"].contains(&self.text()) {
                return self.unsupported("non-kernel pointers");
            }
            self.eat("kernel");
            let const_ = self.eat("const");
            ast::TypeKind::Pointer {
                const_,
                target: Box::new(self.ty()?),
            }
        } else {
            if ["fn", "unsafe", "extern", "!", "Option"].contains(&self.text()) {
                return self.unsupported("this type");
            }
            let name = self.path()?;
            if name == "Result" && self.eat("<") {
                let ok = Box::new(self.ty()?);
                self.need(",")?;
                let error = Box::new(self.ty()?);
                self.close_gt()?;
                ast::TypeKind::Result { ok, error }
            } else {
                if self.text() == "::" {
                    return self.unsupported("qualified type paths");
                }
                if self.text() == "<" {
                    return Err(error(
                        "E_PARSE",
                        self.span(),
                        "user type arguments are excluded",
                    ));
                }
                ast::TypeKind::Named(name)
            }
        };
        Ok(ast::Type {
            kind,
            span: self.end(start),
        })
    }
    fn block(&mut self) -> Result<ast::Block> {
        let start = self.span();
        self.need("{")?;
        let mut statements = vec![];
        while !self.eat("}") {
            if self.text().is_empty() {
                return Err(error("E_PARSE", self.span(), "unclosed block"));
            }
            statements.push(self.statement()?);
        }
        Ok(ast::Block {
            statements,
            span: self.end(start),
        })
    }
    fn statement(&mut self) -> Result<ast::Statement> {
        let start = self.span();
        let kind = if self.eat(";") {
            ast::StatementKind::Empty
        } else if self.text() == "let" || self.text() == "var" {
            let mutable = self.bump().text == "var";
            let name = self.ident()?;
            self.need(":")?;
            let ty = self.ty()?;
            let value = if self.eat("=") {
                Some(self.expr(0, false)?)
            } else if mutable {
                None
            } else {
                return Err(error("E_PARSE", self.span(), "let requires an initializer"));
            };
            self.need(";")?;
            ast::StatementKind::Bind {
                name,
                ty,
                mutable,
                value,
            }
        } else if self.text() == "{" {
            ast::StatementKind::Block(self.block()?)
        } else if self.eat("unsafe") {
            ast::StatementKind::Unsafe(self.block()?)
        } else if self.eat("if") {
            let condition = self.expr(0, true)?;
            let then = self.block()?;
            let else_ = if self.eat("else") {
                if self.text() == "if" {
                    let nested = self.statement()?;
                    Some(ast::Block {
                        span: nested.span,
                        statements: vec![nested],
                    })
                } else {
                    Some(self.block()?)
                }
            } else {
                None
            };
            ast::StatementKind::If {
                condition,
                then,
                else_,
            }
        } else if self.eat("while") {
            let condition = self.expr(0, true)?;
            ast::StatementKind::While {
                condition,
                body: self.block()?,
            }
        } else if self.eat("loop") {
            ast::StatementKind::Loop(self.block()?)
        } else if self.eat("match") {
            let value = self.expr(0, true)?;
            self.need("{")?;
            let mut arms = vec![];
            while !self.eat("}") {
                let pattern = self.pattern()?;
                self.need("=>")?;
                let body = self.block()?;
                arms.push(ast::Arm { pattern, body });
                if !self.eat(",") {
                    self.need("}")?;
                    break;
                }
            }
            ast::StatementKind::Match { value, arms }
        } else if self.eat("return") {
            let value = self.expr(0, false)?;
            self.need(";")?;
            ast::StatementKind::Return(value)
        } else if self.eat("break") {
            self.need(";")?;
            ast::StatementKind::Break
        } else if self.eat("continue") {
            self.need(";")?;
            ast::StatementKind::Continue
        } else if self.eat("defer") {
            ast::StatementKind::Defer(self.block()?)
        } else {
            let target = self.expr(0, false)?;
            let kind = if self.eat("=") {
                ast::StatementKind::Assign {
                    target,
                    value: self.expr(0, false)?,
                }
            } else {
                ast::StatementKind::Expr(target)
            };
            self.need(";")?;
            kind
        };
        Ok(ast::Statement {
            kind,
            span: self.end(start),
        })
    }
    fn result_parts(&mut self) -> Result<(ast::Type, ast::Type, bool)> {
        self.need("::")?;
        self.need("<")?;
        let ok = self.ty()?;
        self.need(",")?;
        let error = self.ty()?;
        self.close_gt()?;
        self.need("::")?;
        let variant = self.ident()?;
        let is_ok = match variant.as_str() {
            "Ok" => true,
            "Err" => false,
            _ => {
                return Err(error_fn(
                    "E_NAME",
                    self.end(ok.span),
                    "unknown Result variant",
                ));
            }
        };
        Ok((ok, error, is_ok))
    }
    fn pattern(&mut self) -> Result<ast::Pattern> {
        let start = self.span();
        let kind = if self.eat("_") {
            ast::PatternKind::Wildcard
        } else if self.eat("true") {
            ast::PatternKind::Bool(true)
        } else if self.eat("false") {
            ast::PatternKind::Bool(false)
        } else if self
            .text()
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
        {
            ast::PatternKind::Integer(self.bump().text)
        } else {
            self.need("Result")?;
            let (ok, error, is_ok) = self.result_parts()?;
            self.need("(")?;
            let binding = if self.eat("_") {
                None
            } else {
                Some(self.parameter()?)
            };
            self.need(")")?;
            ast::PatternKind::Result {
                ok,
                error,
                is_ok,
                binding,
            }
        };
        Ok(ast::Pattern {
            kind,
            span: self.end(start),
        })
    }
    fn expr(&mut self, min: u8, head: bool) -> Result<ast::Expr> {
        let start = self.span();
        let mut value = self.primary(head)?;
        loop {
            let kind = if min <= 13 && self.eat("?") {
                ast::ExprKind::Try(Box::new(value))
            } else if min <= 13 && self.eat(".") {
                let name = self.ident()?;
                ast::ExprKind::Field {
                    base: Box::new(value),
                    name,
                }
            } else if min <= 13 && self.eat("[") {
                let index = Box::new(self.expr(0, false)?);
                self.need("]")?;
                ast::ExprKind::Index {
                    base: Box::new(value),
                    index,
                }
            } else if min <= 13 && self.eat("(") {
                let ast::ExprKind::Name(name) = value.kind else {
                    return self.unsupported("indirect calls");
                };
                let mut arguments = vec![];
                while !self.eat(")") {
                    arguments.push(self.expr(0, false)?);
                    if !self.eat(",") {
                        self.need(")")?;
                        break;
                    }
                }
                ast::ExprKind::Call { name, arguments }
            } else if min <= 11 && self.eat("as") {
                ast::ExprKind::Cast {
                    value: Box::new(value),
                    ty: self.ty()?,
                }
            } else if let Some((op, power)) = binary(self.text()).filter(|(_, power)| *power >= min)
            {
                self.bump();
                let right = Box::new(self.expr(power + 1, head)?);
                ast::ExprKind::Binary {
                    op,
                    left: Box::new(value),
                    right,
                }
            } else {
                break;
            };
            value = ast::Expr {
                kind,
                span: self.end(start),
            };
        }
        Ok(value)
    }
    fn primary(&mut self, head: bool) -> Result<ast::Expr> {
        let start = self.span();
        if self.text().starts_with('"') {
            let raw = self.bump().text;
            let bytes = decode_string(&raw, start)?;
            return Ok(ast::Expr {
                kind: ast::ExprKind::String(bytes),
                span: self.end(start),
            });
        }
        let kind = if self
            .text()
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
        {
            ast::ExprKind::Integer(self.bump().text)
        } else if self.eat("true") {
            ast::ExprKind::Bool(true)
        } else if self.eat("false") {
            ast::ExprKind::Bool(false)
        } else if self.eat("null") {
            ast::ExprKind::Null
        } else if self.eat("(") {
            if self.eat(")") {
                ast::ExprKind::Unit
            } else {
                let mut inner = self.expr(0, false)?;
                self.need(")")?;
                inner.span = self.end(start);
                return Ok(inner);
            }
        } else if self.eat("[") {
            let mut elements = vec![];
            if self.eat("]") {
                ast::ExprKind::Array(elements)
            } else {
                let first = self.expr(0, false)?;
                if self.eat(";") {
                    let length = Box::new(self.expr(0, false)?);
                    self.need("]")?;
                    ast::ExprKind::Repeat {
                        value: Box::new(first),
                        length,
                    }
                } else {
                    elements.push(first);
                    while self.eat(",") && self.text() != "]" {
                        elements.push(self.expr(0, false)?);
                    }
                    self.need("]")?;
                    ast::ExprKind::Array(elements)
                }
            }
        } else if matches!(self.text(), "!" | "~" | "-" | "*") {
            let op = match self.bump().text.as_str() {
                "!" => Un::Not,
                "~" => Un::BitNot,
                "-" => Un::Neg,
                _ => Un::Deref,
            };
            ast::ExprKind::Unary {
                op,
                value: Box::new(self.expr(12, head)?),
            }
        } else if self.text() == "sizeof" || self.text() == "alignof" {
            let size = self.bump().text == "sizeof";
            self.need("(")?;
            let ty = self.ty()?;
            self.need(")")?;
            if size {
                ast::ExprKind::Sizeof(ty)
            } else {
                ast::ExprKind::Alignof(ty)
            }
        } else {
            if ["offsetof", "Option"].contains(&self.text()) {
                return self.unsupported("this expression");
            }
            let name = self.path()?;
            if name == "Result" && self.text() == "::" {
                let (ok, error, is_ok) = self.result_parts()?;
                self.need("(")?;
                let value = Box::new(self.expr(0, false)?);
                self.need(")")?;
                ast::ExprKind::Result {
                    ok,
                    error,
                    is_ok,
                    value,
                }
            } else if name == "core::ptr" && self.eat("::") {
                self.need("<")?;
                let target = self.ty()?;
                self.close_gt()?;
                self.need("(")?;
                let address = Box::new(self.expr(0, false)?);
                self.need(")")?;
                ast::ExprKind::Pointer { target, address }
            } else if !head && self.eat("{") {
                let mut fields = vec![];
                while !self.eat("}") {
                    let field = self.ident()?;
                    self.need(":")?;
                    fields.push((field, self.expr(0, false)?));
                    if !self.eat(",") {
                        self.need("}")?;
                        break;
                    }
                }
                ast::ExprKind::Struct { name, fields }
            } else {
                ast::ExprKind::Name(name)
            }
        };
        Ok(ast::Expr {
            kind,
            span: self.end(start),
        })
    }
}

// Avoid shadowing the diagnostic helper with a parsed Result error type.
fn error_fn(code: &'static str, span: Span, message: &str) -> Diagnostic {
    error(code, span, message)
}

fn binary(text: &str) -> Option<(Bin, u8)> {
    Some(match text {
        "||" => (Bin::Or, 1),
        "&&" => (Bin::And, 2),
        "==" => (Bin::Eq, 3),
        "!=" => (Bin::Ne, 3),
        "<" => (Bin::Lt, 4),
        "<=" => (Bin::Le, 4),
        ">" => (Bin::Gt, 4),
        ">=" => (Bin::Ge, 4),
        "|" => (Bin::BitOr, 5),
        "^" => (Bin::BitXor, 6),
        "&" => (Bin::BitAnd, 7),
        "<<" => (Bin::Shl, 8),
        ">>" => (Bin::Shr, 8),
        "+" => (Bin::Add, 9),
        "-" => (Bin::Sub, 9),
        "+%" => (Bin::WrappingAdd, 9),
        "-%" => (Bin::WrappingSub, 9),
        "*" => (Bin::Mul, 10),
        "/" => (Bin::Div, 10),
        "%" => (Bin::Rem, 10),
        "*%" => (Bin::WrappingMul, 10),
        _ => return None,
    })
}

#[derive(Clone)]
enum Pending {
    Slice {
        element: TypeId,
        const_: bool,
    },
    Scalar(model::TypeKind),
    Struct {
        name: String,
        fields: Vec<model::Field>,
    },
    Pointer {
        target: TypeId,
        const_: bool,
    },
    Array {
        element: TypeId,
        length: Span,
    },
    Result {
        ok: TypeId,
        error: TypeId,
    },
}

#[derive(Clone)]
struct ConstantExpr {
    ty: TypeId,
    span: Span,
    kind: ConstantKind,
}

#[derive(Clone)]
enum ConstantKind {
    Bits(u64),
    Name(String),
    Sizeof(TypeId),
    Alignof(TypeId),
    Unary(Un, Box<ConstantExpr>),
    Binary(Bin, Box<ConstantExpr>, Box<ConstantExpr>),
    Cast(Box<ConstantExpr>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Node {
    Constant(String),
    Length(Span),
    Layout(TypeId),
}

struct Resolver {
    program: ast::Program,
    pending: Vec<Pending>,
    spans: Vec<Span>,
    names: BTreeMap<String, TypeId>,
    uses: HashMap<Span, TypeId>,
    lengths: HashMap<Span, ast::Expr>,
    constants: BTreeMap<String, (TypeId, ast::Expr, Span)>,
    constant_exprs: BTreeMap<String, ConstantExpr>,
    length_exprs: HashMap<Span, ConstantExpr>,
    function_names: HashSet<String>,
    layouts: Vec<Option<Layout>>,
    constant_values: BTreeMap<String, u64>,
    length_values: HashMap<Span, u64>,
    repeat_spans: HashSet<Span>,
}

#[path = "modules.rs"]
mod modules;
pub fn resolve(program: ast::Program) -> Result<model::ResolvedProgram> {
    resolve_files(vec![program])
}
pub fn resolve_files(programs: Vec<ast::Program>) -> Result<model::ResolvedProgram> {
    Resolver::new(modules::merge(programs)?)?.finish()
}

impl Resolver {
    fn new(program: ast::Program) -> Result<Self> {
        let mut r = Self {
            program,
            pending: vec![],
            spans: vec![],
            names: BTreeMap::new(),
            uses: HashMap::new(),
            lengths: HashMap::new(),
            constants: BTreeMap::new(),
            constant_exprs: BTreeMap::new(),
            length_exprs: HashMap::new(),
            function_names: HashSet::new(),
            layouts: vec![],
            constant_values: BTreeMap::new(),
            length_values: HashMap::new(),
            repeat_spans: HashSet::new(),
        };
        for (name, kind) in [
            ("()", model::TypeKind::Unit),
            ("bool", model::TypeKind::Bool),
            ("u8", model::TypeKind::Int(Integer::U8)),
            ("u32", model::TypeKind::Int(Integer::U32)),
            ("usize", model::TypeKind::Int(Integer::Usize)),
        ] {
            let id = r.add(Pending::Scalar(kind), Span::default())?;
            r.names.insert(name.into(), id);
        }
        let declarations = r.program.declarations.clone();
        let mut values = HashSet::new();
        for d in &declarations {
            match &d.kind {
                ast::DeclarationKind::Struct { name, .. } => {
                    if r.names.contains_key(name) {
                        return Err(error("E_NAME", d.span, "duplicate type declaration"));
                    }
                    let id = r.add(
                        Pending::Struct {
                            name: name.clone(),
                            fields: vec![],
                        },
                        d.span,
                    )?;
                    r.names.insert(name.clone(), id);
                }
                ast::DeclarationKind::Function(f)
                | ast::DeclarationKind::ExternFunction { function: f, .. } => {
                    if !values.insert(f.name.clone()) {
                        return Err(error("E_NAME", d.span, "duplicate value declaration"));
                    }
                    r.function_names.insert(f.name.clone());
                }
                ast::DeclarationKind::Const { name, .. }
                | ast::DeclarationKind::Static { name, .. } => {
                    if !values.insert(name.clone()) {
                        return Err(error("E_NAME", d.span, "duplicate value declaration"));
                    }
                }
            }
        }
        for d in &declarations {
            match &d.kind {
                ast::DeclarationKind::Struct { name, fields } => {
                    let mut seen = HashSet::new();
                    let mut out = vec![];
                    for f in fields {
                        if !seen.insert(f.name.clone()) {
                            return Err(error("E_NAME", f.span, "duplicate field"));
                        }
                        out.push(model::Field {
                            name: f.name.clone(),
                            ty: r.ty(&f.ty)?,
                        });
                    }
                    let id = r.names[name];
                    r.pending[id.0 as usize] = Pending::Struct {
                        name: name.clone(),
                        fields: out,
                    };
                }
                ast::DeclarationKind::Static { ty, value, .. } => {
                    r.ty(ty)?;
                    r.expr_types(value)?;
                }
                ast::DeclarationKind::Const { name, ty, value } => {
                    let id = r.ty(ty)?;
                    r.constants
                        .insert(name.clone(), (id, value.clone(), d.span));
                    r.expr_types(value)?;
                }
                ast::DeclarationKind::Function(f)
                | ast::DeclarationKind::ExternFunction { function: f, .. } => {
                    let mut seen = HashSet::new();
                    for p in &f.parameters {
                        if !seen.insert(p.name.clone()) {
                            return Err(error("E_NAME", p.span, "duplicate parameter"));
                        }
                        r.ty(&p.ty)?;
                    }
                    r.ty(&f.result)?;
                    r.block_types(&f.body)?;
                }
            }
        }
        // Signatures/types of every constant are known before checking expressions.
        for (name, (ty, value, span)) in r.constants.clone() {
            if !r.scalar(ty) {
                return Err(error(
                    "I_UNSUPPORTED",
                    span,
                    "pilot constants must be scalar",
                ));
            }
            let expr = r.const_check(&value, Some(ty))?;
            r.constant_exprs.insert(name, expr);
        }
        for (span, value) in r.lengths.clone() {
            let expr = r.const_check(&value, Some(r.names["usize"]))?;
            r.length_exprs.insert(span, expr);
        }
        r.layouts = vec![None; r.pending.len()];
        Ok(r)
    }
    fn add(&mut self, pending: Pending, span: Span) -> Result<TypeId> {
        let id = u32::try_from(self.pending.len())
            .map_err(|_| error("I_LIMIT", span, "too many types"))?;
        self.pending.push(pending);
        self.spans.push(span);
        Ok(TypeId(id))
    }
    fn ty(&mut self, ty: &ast::Type) -> Result<TypeId> {
        if let Some(id) = self.uses.get(&ty.span) {
            return Ok(*id);
        }
        let id = match &ty.kind {
            ast::TypeKind::Named(name) => {
                if ["u16", "u64", "i8", "i16", "i32", "i64", "isize"].contains(&name.as_str()) {
                    return Err(error(
                        "I_UNSUPPORTED",
                        ty.span,
                        "integer type outside pilot",
                    ));
                }
                *self
                    .names
                    .get(name)
                    .ok_or_else(|| error("E_NAME", ty.span, format!("unknown type {name}")))?
            }
            ast::TypeKind::Unit => self.names["()"],
            ast::TypeKind::Slice { element, const_ } => {
                let element = self.ty(element)?;
                self.add(
                    Pending::Slice {
                        element,
                        const_: *const_,
                    },
                    ty.span,
                )?
            }
            ast::TypeKind::Pointer { target, const_ } => {
                let target = self.ty(target)?;
                self.add(
                    Pending::Pointer {
                        target,
                        const_: *const_,
                    },
                    ty.span,
                )?
            }
            ast::TypeKind::Array { length, element } => {
                let element = self.ty(element)?;
                self.lengths.insert(length.span, (**length).clone());
                self.expr_types(length)?;
                self.add(
                    Pending::Array {
                        element,
                        length: length.span,
                    },
                    ty.span,
                )?
            }
            ast::TypeKind::Result { ok, error } => self.result_type(ok, error, ty.span)?,
        };
        self.uses.insert(ty.span, id);
        Ok(id)
    }
    fn result_type(&mut self, ok: &ast::Type, err: &ast::Type, span: Span) -> Result<TypeId> {
        let ok = self.ty(ok)?;
        let error = self.ty(err)?;
        self.add(Pending::Result { ok, error }, span)
    }
    fn expr_types(&mut self, expr: &ast::Expr) -> Result<()> {
        use ast::ExprKind as E;
        match &expr.kind {
            E::Unary { value, .. } | E::Try(value) => self.expr_types(value)?,
            E::Binary { left, right, .. } => {
                self.expr_types(left)?;
                self.expr_types(right)?;
            }
            E::Pointer { target, address } => {
                self.ty(target)?;
                self.expr_types(address)?;
            }
            E::Cast { value, ty } => {
                self.ty(ty)?;
                self.expr_types(value)?;
            }
            E::Call { arguments, .. } | E::Array(arguments) => {
                for x in arguments {
                    self.expr_types(x)?;
                }
            }
            E::Field { base, .. } => self.expr_types(base)?,
            E::Index { base, index } => {
                self.expr_types(base)?;
                self.expr_types(index)?;
            }
            E::Struct { fields, .. } => {
                for (_, x) in fields {
                    self.expr_types(x)?;
                }
            }
            E::Repeat { value, length } => {
                self.expr_types(value)?;
                self.expr_types(length)?;
                self.lengths.insert(length.span, (**length).clone());
                self.repeat_spans.insert(length.span);
            }
            E::Result {
                ok, error, value, ..
            } => {
                self.result_type(ok, error, expr.span)?;
                self.expr_types(value)?;
            }
            E::Sizeof(ty) | E::Alignof(ty) => {
                self.ty(ty)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn block_types(&mut self, block: &ast::Block) -> Result<()> {
        use ast::StatementKind as S;
        for s in &block.statements {
            match &s.kind {
                S::Bind { ty, value, .. } => {
                    self.ty(ty)?;
                    if let Some(x) = value {
                        self.expr_types(x)?;
                    }
                }
                S::Assign { target, value } => {
                    self.expr_types(target)?;
                    self.expr_types(value)?;
                }
                S::Expr(x) | S::Return(x) => self.expr_types(x)?,
                S::Block(b) | S::Unsafe(b) | S::Loop(b) | S::Defer(b) => self.block_types(b)?,
                S::If {
                    condition,
                    then,
                    else_,
                } => {
                    self.expr_types(condition)?;
                    self.block_types(then)?;
                    if let Some(b) = else_ {
                        self.block_types(b)?;
                    }
                }
                S::While { condition, body } => {
                    self.expr_types(condition)?;
                    self.block_types(body)?;
                }
                S::Match { value, arms } => {
                    self.expr_types(value)?;
                    for arm in arms {
                        if let ast::PatternKind::Result {
                            ok, error, binding, ..
                        } = &arm.pattern.kind
                        {
                            self.result_type(ok, error, arm.pattern.span)?;
                            if let Some(p) = binding {
                                self.ty(&p.ty)?;
                            }
                        }
                        self.block_types(&arm.body)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn scalar(&self, ty: TypeId) -> bool {
        matches!(self.pending[ty.0 as usize], Pending::Scalar(_))
    }
    fn integer(&self, ty: TypeId) -> bool {
        matches!(
            self.pending[ty.0 as usize],
            Pending::Scalar(model::TypeKind::Int(_))
        )
    }
    fn width(&self, ty: TypeId) -> u32 {
        match &self.pending[ty.0 as usize] {
            Pending::Scalar(model::TypeKind::Int(Integer::U8)) => 8,
            Pending::Scalar(model::TypeKind::Int(Integer::U32)) => 32,
            _ => 64,
        }
    }
    fn const_check(&self, expr: &ast::Expr, expected: Option<TypeId>) -> Result<ConstantExpr> {
        use ast::ExprKind as E;
        let span = expr.span;
        let (ty, kind) = match &expr.kind {
            E::Integer(raw) => {
                let (bits, suffix) = literal(raw, span)?;
                let ty = if let Some(name) = suffix {
                    *self
                        .names
                        .get(name)
                        .filter(|id| self.integer(**id))
                        .ok_or_else(|| {
                            error("I_UNSUPPORTED", span, "integer suffix outside pilot")
                        })?
                } else {
                    expected
                        .filter(|id| self.integer(*id))
                        .ok_or_else(|| error("E_TYPE", span, "bare integer lacks known type"))?
                };
                if bits as u128 > mask(self.width(ty)) {
                    return Err(error("E_TYPE", span, "integer literal out of range"));
                }
                (ty, ConstantKind::Bits(bits))
            }
            E::Bool(b) => (self.names["bool"], ConstantKind::Bits(u64::from(*b))),
            E::Unit => (self.names["()"], ConstantKind::Bits(0)),
            E::Name(name) => {
                if self.function_names.contains(name) {
                    return Err(error("I_UNSUPPORTED", span, "function address constants"));
                }
                let (ty, _, _) = self
                    .constants
                    .get(name)
                    .ok_or_else(|| error("E_NAME", span, "unknown constant"))?;
                if !self.scalar(*ty) {
                    return Err(error("I_UNSUPPORTED", span, "aggregate constants"));
                }
                (*ty, ConstantKind::Name(name.clone()))
            }
            E::Sizeof(t) | E::Alignof(t) => {
                let id = self.uses[&t.span];
                let kind = if matches!(expr.kind, E::Sizeof(_)) {
                    ConstantKind::Sizeof(id)
                } else {
                    ConstantKind::Alignof(id)
                };
                (self.names["usize"], kind)
            }
            E::Cast { value, ty } => {
                let target = self.uses[&ty.span];
                let value = self.const_check(value, None)?;
                if !self.integer(target) || !self.integer(value.ty) {
                    return Err(error("E_TYPE", span, "as requires integer types"));
                }
                (target, ConstantKind::Cast(Box::new(value)))
            }
            E::Unary { op, value } => {
                let want = if *op == Un::Not {
                    Some(self.names["bool"])
                } else {
                    expected.filter(|id| self.integer(*id))
                };
                let value = self.const_check(value, want)?;
                match op {
                    Un::Not if value.ty == self.names["bool"] => {}
                    Un::BitNot if self.integer(value.ty) => {}
                    Un::Neg => {
                        return Err(error("E_TYPE", span, "negation requires a signed type"));
                    }
                    Un::Deref => {
                        return Err(error("E_CONST", span, "constant memory reads excluded"));
                    }
                    _ => return Err(error("E_TYPE", span, "invalid constant unary operand")),
                }
                (value.ty, ConstantKind::Unary(*op, Box::new(value)))
            }
            E::Binary { op, left, right } => {
                let logical = matches!(op, Bin::And | Bin::Or);
                let comparison = matches!(
                    op,
                    Bin::Eq | Bin::Ne | Bin::Lt | Bin::Le | Bin::Gt | Bin::Ge
                );
                let want = if logical {
                    Some(self.names["bool"])
                } else if comparison {
                    None
                } else {
                    expected.filter(|id| self.integer(*id))
                };
                let left = self.const_check(left, want)?;
                let right = self.const_check(right, want)?;
                if left.ty != right.ty {
                    return Err(error("E_TYPE", span, "constant operand types differ"));
                }
                let is_int = self.integer(left.ty);
                let valid = if logical {
                    left.ty == self.names["bool"]
                } else if matches!(op, Bin::Eq | Bin::Ne) {
                    is_int || left.ty == self.names["bool"] || left.ty == self.names["()"]
                } else {
                    is_int
                };
                if !valid {
                    return Err(error("E_TYPE", span, "unsupported constant operand type"));
                }
                let result = if logical || comparison {
                    self.names["bool"]
                } else {
                    left.ty
                };
                (
                    result,
                    ConstantKind::Binary(*op, Box::new(left), Box::new(right)),
                )
            }
            E::Struct { .. } | E::Array(_) | E::Repeat { .. } | E::Result { .. } | E::Null => {
                return Err(error(
                    "I_UNSUPPORTED",
                    span,
                    "non-scalar constant construction",
                ));
            }
            E::Field { name, .. } if name == "len" => {
                return Err(error("I_UNSUPPORTED", span, "constant length query"));
            }
            _ => return Err(error("E_CONST", span, "operation excluded from constants")),
        };
        if expected.is_some_and(|target| target != ty) {
            return Err(error(
                "E_TYPE",
                span,
                "constant result type differs from annotation",
            ));
        }
        Ok(ConstantExpr { ty, span, kind })
    }
    fn expr_dependencies(expr: &ConstantExpr, out: &mut Vec<Node>) {
        match &expr.kind {
            ConstantKind::Name(n) => out.push(Node::Constant(n.clone())),
            ConstantKind::Sizeof(t) | ConstantKind::Alignof(t) => out.push(Node::Layout(*t)),
            ConstantKind::Unary(_, v) | ConstantKind::Cast(v) => Self::expr_dependencies(v, out),
            ConstantKind::Binary(_, a, b) => {
                Self::expr_dependencies(a, out);
                Self::expr_dependencies(b, out);
            }
            ConstantKind::Bits(_) => {}
        }
    }
    fn graph(&self) -> HashMap<Node, Vec<Node>> {
        let mut graph = HashMap::new();
        for (name, expr) in &self.constant_exprs {
            let mut deps = vec![];
            Self::expr_dependencies(expr, &mut deps);
            graph.insert(Node::Constant(name.clone()), deps);
        }
        for (span, expr) in &self.length_exprs {
            let mut deps = vec![];
            Self::expr_dependencies(expr, &mut deps);
            graph.insert(Node::Length(*span), deps);
        }
        for (index, ty) in self.pending.iter().enumerate() {
            let deps = match ty {
                Pending::Struct { fields, .. } => {
                    fields.iter().map(|f| Node::Layout(f.ty)).collect()
                }
                Pending::Array { element, length } => {
                    vec![Node::Layout(*element), Node::Length(*length)]
                }
                Pending::Result { ok, error } => vec![Node::Layout(*ok), Node::Layout(*error)],
                _ => vec![],
            };
            graph.insert(Node::Layout(TypeId(index as u32)), deps);
        }
        graph
    }
    fn evaluate(&self, expr: &ConstantExpr) -> Result<u64> {
        let fail = || error("E_CONST", expr.span, "constant arithmetic failure");
        let width = self.width(expr.ty);
        let bits = match &expr.kind {
            ConstantKind::Bits(n) => *n,
            ConstantKind::Name(n) => self.constant_values[n],
            ConstantKind::Sizeof(t) => self.layouts[t.0 as usize].as_ref().ok_or_else(fail)?.size,
            ConstantKind::Alignof(t) => self.layouts[t.0 as usize].as_ref().ok_or_else(fail)?.align,
            ConstantKind::Cast(v) => (self.evaluate(v)? as u128 & mask(width)) as u64,
            ConstantKind::Unary(op, v) => {
                let n = self.evaluate(v)?;
                match op {
                    Un::Not => u64::from(n == 0),
                    Un::BitNot => (!n as u128 & mask(width)) as u64,
                    _ => return Err(fail()),
                }
            }
            ConstantKind::Binary(op, a, b) => {
                let a = self.evaluate(a)?;
                if *op == Bin::And && a == 0 {
                    return Ok(0);
                }
                if *op == Bin::Or && a != 0 {
                    return Ok(1);
                }
                let b = self.evaluate(b)?;
                let n = match op {
                    Bin::Eq => u128::from(a == b),
                    Bin::Ne => u128::from(a != b),
                    Bin::Lt => u128::from(a < b),
                    Bin::Le => u128::from(a <= b),
                    Bin::Gt => u128::from(a > b),
                    Bin::Ge => u128::from(a >= b),
                    Bin::And | Bin::Or => u128::from(b != 0),
                    Bin::Add => a as u128 + b as u128,
                    Bin::Mul => a as u128 * b as u128,
                    Bin::Sub => a.checked_sub(b).ok_or_else(fail)? as u128,
                    Bin::Div => a.checked_div(b).ok_or_else(fail)? as u128,
                    Bin::Rem => a.checked_rem(b).ok_or_else(fail)? as u128,
                    Bin::WrappingAdd => (a as u128 + b as u128) & mask(width),
                    Bin::WrappingSub => a.wrapping_sub(b) as u128 & mask(width),
                    Bin::WrappingMul => (a as u128 * b as u128) & mask(width),
                    Bin::BitAnd => (a & b) as u128,
                    Bin::BitOr => (a | b) as u128,
                    Bin::BitXor => (a ^ b) as u128,
                    Bin::Shl | Bin::Shr => {
                        if b >= width as u64 {
                            return Err(fail());
                        }
                        if *op == Bin::Shl {
                            ((a as u128) << b) & mask(width)
                        } else {
                            (a >> b) as u128
                        }
                    }
                };
                if n > mask(width) {
                    return Err(fail());
                }
                n as u64
            }
        };
        Ok(bits)
    }
    fn compute_layout(&self, id: TypeId) -> Result<Layout> {
        let span = self.spans[id.0 as usize];
        let get = |id: TypeId| {
            self.layouts[id.0 as usize]
                .as_ref()
                .ok_or_else(|| error("I_INTERNAL", span, "layout dependency not computed"))
        };
        let mut out = Layout {
            size: 0,
            align: 1,
            field_offsets: vec![],
            payload_offset: None,
        };
        match &self.pending[id.0 as usize] {
            Pending::Scalar(kind) => {
                out.size = match kind {
                    model::TypeKind::Unit => 0,
                    model::TypeKind::Bool | model::TypeKind::Int(Integer::U8) => 1,
                    model::TypeKind::Int(Integer::U32) => 4,
                    _ => 8,
                };
                out.align = out.size.max(1);
            }
            Pending::Slice { .. } => {
                out.size = 16;
                out.align = 8;
                out.field_offsets = vec![0, 8];
            }
            Pending::Pointer { .. } => {
                out.size = 8;
                out.align = 8;
            }
            Pending::Array { element, length } => {
                let element = get(*element)?;
                out.align = element.align;
                out.size = element
                    .size
                    .checked_mul(self.length_values[length])
                    .ok_or_else(|| error("E_LAYOUT", span, "array size overflow"))?;
            }
            Pending::Struct { fields, .. } => {
                for field in fields {
                    let field = get(field.ty)?;
                    out.size = align_up(out.size, field.align, span)?;
                    out.field_offsets.push(out.size);
                    out.size = out
                        .size
                        .checked_add(field.size)
                        .ok_or_else(|| error("E_LAYOUT", span, "field layout overflow"))?;
                    out.align = out.align.max(field.align);
                }
                out.size = align_up(out.size, out.align, span)?;
            }
            Pending::Result { ok, error } => {
                let ok = get(*ok)?;
                let error = get(*error)?;
                out.align = 4.max(ok.align).max(error.align);
                let offset = align_up(4, ok.align.max(error.align), span)?;
                out.payload_offset = Some(offset);
                out.size = align_up(
                    offset
                        .checked_add(ok.size.max(error.size))
                        .ok_or_else(|| error_fn("E_LAYOUT", span, "Result layout overflow"))?,
                    out.align,
                    span,
                )?;
            }
        }
        Ok(out)
    }
    fn finish(mut self) -> Result<model::ResolvedProgram> {
        let graph = self.graph();
        let mut roots: Vec<_> = (0..self.pending.len())
            .map(|n| Node::Layout(TypeId(n as u32)))
            .collect();
        roots.extend(self.constant_exprs.keys().cloned().map(Node::Constant));
        let mut lengths: Vec<_> = self.length_exprs.keys().copied().collect();
        lengths.sort_by_key(|span| (span.file.0, span.start, span.end));
        roots.extend(lengths.into_iter().map(Node::Length));
        let mut done = HashSet::new();
        let mut active = vec![];
        let mut order = vec![];
        let node_span = |node: &Node| match node {
            Node::Layout(id) => self.spans[id.0 as usize],
            Node::Constant(name) => self.constants[name].2,
            Node::Length(span) => *span,
        };
        for root in roots {
            visit(
                &root,
                &graph,
                &mut done,
                &mut active,
                &mut order,
                &node_span,
            )?;
        }
        for node in order {
            match node {
                Node::Constant(name) => {
                    let bits = self.evaluate(&self.constant_exprs[&name])?;
                    self.constant_values.insert(name, bits);
                }
                Node::Length(span) => {
                    let bits = self.evaluate(&self.length_exprs[&span])?;
                    self.length_values.insert(span, bits);
                }
                Node::Layout(id) => {
                    let layout = self.compute_layout(id)?;
                    self.layouts[id.0 as usize] = Some(layout);
                }
            }
        }
        let mut table = model::TypeTable::default();
        let mut canonical = vec![None; self.pending.len()];
        // Nominal identities are allocated before rewriting recursive pointer edges.
        for (index, pending) in self.pending.iter().enumerate() {
            let kind = match pending {
                Pending::Scalar(kind) => Some(kind.clone()),
                Pending::Struct { name, .. } => Some(model::TypeKind::Struct {
                    name: name.clone(),
                    fields: vec![],
                }),
                _ => None,
            };
            if let Some(kind) = kind {
                let layout = self.layouts[index]
                    .clone()
                    .ok_or_else(|| error("I_INTERNAL", self.spans[index], "missing layout"))?;
                canonical[index] = Some(table.intern(kind, layout, self.spans[index])?);
            }
        }
        for index in 0..self.pending.len() {
            self.canonical(TypeId(index as u32), &mut table, &mut canonical)?;
        }
        for (index, pending) in self.pending.iter().enumerate() {
            if let Pending::Struct { name, fields } = pending {
                let id = canonical[index].ok_or_else(|| {
                    error("I_INTERNAL", self.spans[index], "missing nominal identity")
                })?;
                table.entries[id.0 as usize].kind = model::TypeKind::Struct {
                    name: name.clone(),
                    fields: fields
                        .iter()
                        .map(|f| model::Field {
                            name: f.name.clone(),
                            ty: canonical[f.ty.0 as usize].unwrap_or(id),
                        })
                        .collect(),
                };
            }
        }
        for (name, old) in &self.names {
            table.names.insert(
                name.clone(),
                canonical[old.0 as usize].ok_or_else(|| {
                    error(
                        "I_INTERNAL",
                        self.spans[old.0 as usize],
                        "missing named type",
                    )
                })?,
            );
        }
        let remap = |id: TypeId| {
            canonical[id.0 as usize].ok_or_else(|| {
                error(
                    "I_INTERNAL",
                    self.spans[id.0 as usize],
                    "missing canonical type",
                )
            })
        };
        let mut uses = HashMap::new();
        for (span, id) in &self.uses {
            uses.insert(*span, remap(*id)?);
        }
        let mut constants = BTreeMap::new();
        for (name, (ty, _, span)) in &self.constants {
            constants.insert(
                name.clone(),
                model::ResolvedConstant {
                    ty: remap(*ty)?,
                    bits: self.constant_values[name],
                    span: *span,
                },
            );
        }
        let mut functions = vec![];
        let mut externs = BTreeMap::new();
        let mut globals = vec![];
        for declaration in &self.program.declarations {
            if let ast::DeclarationKind::Static {
                name,
                ty,
                mutable,
                value,
            } = &declaration.kind
            {
                globals.push(model::ResolvedGlobal {
                    name: name.clone(),
                    ty: uses[&ty.span],
                    mutable: *mutable,
                    initializer: value.clone(),
                    span: declaration.span,
                });
            }
            if let ast::DeclarationKind::ExternFunction { symbol, .. } = &declaration.kind {
                externs.insert(model::FunctionId(functions.len() as u32), symbol.clone());
            }
            if let ast::DeclarationKind::Function(f)
            | ast::DeclarationKind::ExternFunction { function: f, .. } = &declaration.kind
            {
                let parameters = f.parameters.iter().map(|p| uses[&p.ty.span]).collect();
                functions.push(model::ResolvedFunction {
                    name: f.name.clone(),
                    parameters: f.parameters.clone(),
                    signature: model::Signature {
                        parameters,
                        result: uses[&f.result.span],
                        unsafe_: f.unsafe_,
                    },
                    body: f.body.clone(),
                    span: f.span,
                });
            }
        }
        let repeat_lengths = self
            .repeat_spans
            .iter()
            .map(|s| (*s, self.length_values[s]))
            .collect();
        Ok(model::ResolvedProgram {
            globals,
            externs,
            types: table,
            functions,
            constants,
            type_uses: uses,
            repeat_lengths,
        })
    }
    fn canonical(
        &self,
        id: TypeId,
        table: &mut model::TypeTable,
        mapping: &mut [Option<TypeId>],
    ) -> Result<TypeId> {
        if let Some(id) = mapping[id.0 as usize] {
            return Ok(id);
        }
        let kind = match &self.pending[id.0 as usize] {
            Pending::Slice { element, const_ } => model::TypeKind::Slice {
                element: self.canonical(*element, table, mapping)?,
                const_: *const_,
            },
            Pending::Pointer { target, const_ } => model::TypeKind::Pointer {
                target: self.canonical(*target, table, mapping)?,
                const_: *const_,
            },
            Pending::Array { element, length } => model::TypeKind::Array {
                element: self.canonical(*element, table, mapping)?,
                length: self.length_values[length],
            },
            Pending::Result { ok, error } => model::TypeKind::Result {
                ok: self.canonical(*ok, table, mapping)?,
                error: self.canonical(*error, table, mapping)?,
            },
            _ => {
                return Err(error(
                    "I_INTERNAL",
                    self.spans[id.0 as usize],
                    "unallocated nominal/scalar type",
                ));
            }
        };
        let layout = self.layouts[id.0 as usize].clone().ok_or_else(|| {
            error(
                "I_INTERNAL",
                self.spans[id.0 as usize],
                "missing concrete layout",
            )
        })?;
        let new = table.intern(kind, layout, self.spans[id.0 as usize])?;
        mapping[id.0 as usize] = Some(new);
        Ok(new)
    }
}

fn visit(
    node: &Node,
    graph: &HashMap<Node, Vec<Node>>,
    done: &mut HashSet<Node>,
    active: &mut Vec<Node>,
    order: &mut Vec<Node>,
    node_span: &impl Fn(&Node) -> Span,
) -> Result<()> {
    if done.contains(node) {
        return Ok(());
    }
    if let Some(start) = active.iter().position(|old| old == node) {
        let layout = active[start..].iter().any(|n| matches!(n, Node::Layout(_)));
        let culprit = active[start..]
            .iter()
            .find(|n| matches!(n, Node::Length(_)))
            .unwrap_or(node);
        return Err(error(
            if layout { "E_LAYOUT" } else { "E_CONST" },
            node_span(culprit),
            format!(
                "cyclic constant/length/layout dependency: {:?}",
                &active[start..]
            ),
        ));
    }
    active.push(node.clone());
    let dependencies = graph
        .get(node)
        .ok_or_else(|| error("I_INTERNAL", node_span(node), "missing dependency node"))?;
    for dependency in dependencies {
        visit(dependency, graph, done, active, order, node_span)?;
    }
    active.pop();
    done.insert(node.clone());
    order.push(node.clone());
    Ok(())
}

fn align_up(value: u64, align: u64, span: Span) -> Result<u64> {
    value
        .checked_add(align - 1)
        .map(|n| n & !(align - 1))
        .ok_or_else(|| error("E_LAYOUT", span, "layout alignment overflow"))
}

fn mask(width: u32) -> u128 {
    (1u128 << width) - 1
}

fn literal(raw: &str, span: Span) -> Result<(u64, Option<&str>)> {
    let suffix = [
        "_usize", "_u8", "_u32", "_u16", "_u64", "_i8", "_i16", "_i32", "_i64", "_isize",
    ]
    .into_iter()
    .find(|suffix| raw.ends_with(suffix));
    let number = suffix.map_or(raw, |suffix| &raw[..raw.len() - suffix.len()]);
    let digits = number.replace('_', "");
    let (radix, digits) = if let Some(digits) = digits.strip_prefix("0x") {
        (16, digits)
    } else if let Some(digits) = digits.strip_prefix("0b") {
        (2, digits)
    } else {
        (10, digits.as_str())
    };
    let bits = u64::from_str_radix(digits, radix).map_err(|_| {
        error(
            "E_TYPE",
            span,
            "integer literal exceeds supported representation",
        )
    })?;
    Ok((bits, suffix.map(|suffix| &suffix[1..])))
}

fn decode_string(raw: &str, span: Span) -> Result<Vec<u8>> {
    let b = raw.as_bytes();
    let mut out = vec![];
    let mut i = 1;
    while i + 1 < b.len() {
        let c = b[i];
        i += 1;
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let e = *b
            .get(i)
            .ok_or_else(|| error("E_PARSE", span, "incomplete string escape"))?;
        i += 1;
        out.push(match e {
            b'0' => 0,
            b'n' => 10,
            b'r' => 13,
            b't' => 9,
            b'\\' => b'\\',
            b'"' => b'"',
            b'x' => {
                let hex = b
                    .get(i..i + 2)
                    .ok_or_else(|| error("E_PARSE", span, "incomplete hex escape"))?;
                i += 2;
                u8::from_str_radix(std::str::from_utf8(hex).unwrap_or(""), 16)
                    .map_err(|_| error("E_PARSE", span, "invalid hex escape"))?
            }
            _ => return Err(error("E_PARSE", span, "unknown string escape")),
        });
    }
    Ok(out)
}
