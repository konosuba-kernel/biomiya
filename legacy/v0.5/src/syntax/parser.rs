use super::*;
use crate::{
    diagnostic::{Diagnostic, Result},
    source::{FileId, Span},
};
pub fn parse(file: FileId, source: &str) -> Result<AstFile> {
    Parser {
        tokens: lex(file, source)?,
        i: 0,
        last: Span::new(file, 0, 0),
    }
    .file()
}
struct Parser {
    tokens: Vec<Token>,
    i: usize,
    last: Span,
}
impl Parser {
    fn t(&self) -> &str {
        &self.tokens[self.i].text
    }
    fn span(&self) -> Span {
        self.tokens[self.i].span
    }
    fn bump(&mut self) -> Token {
        let t = self.tokens[self.i].clone();
        self.last = t.span;
        if !t.text.is_empty() {
            self.i += 1
        }
        t
    }
    fn eat(&mut self, t: &str) -> bool {
        if self.t() == t {
            self.bump();
            true
        } else {
            false
        }
    }
    fn err<T>(&self, msg: &str) -> Result<T> {
        Err(Diagnostic::new("E_PARSE", self.span(), msg))
    }
    fn need(&mut self, t: &str) -> Result<()> {
        if self.eat(t) {
            Ok(())
        } else {
            self.err(&format!("expected {t}, found {}", self.t()))
        }
    }
    fn end(&self, start: Span) -> Span {
        start.join(self.last)
    }
    fn ident(&mut self) -> Result<String> {
        let t = self.t();
        if t.is_empty()
            || t == "_"
            || t.chars().next().unwrap().is_ascii_digit()
            || !matches!(t.chars().next().unwrap(),'_'| 'a'..='z'|'A'..='Z')
                && t.chars().next().unwrap().is_ascii()
            || [
                "module", "import", "pub", "type", "struct", "enum", "fn", "impl", "const",
                "static", "mut", "let", "var", "if", "else", "match", "loop", "while", "for",
                "break", "continue", "return", "defer", "unsafe", "extern", "effects", "true",
                "false", "null", "as", "resource",
            ]
            .contains(&t)
        {
            return self.err("expected identifier");
        }
        Ok(self.bump().text)
    }
    fn path(&mut self) -> Result<Vec<String>> {
        let mut p = vec![self.ident()?];
        while self.t() == "::" && self.tokens[self.i + 1].text != "{" {
            self.bump();
            p.push(self.ident()?)
        }
        Ok(p)
    }
    fn file(&mut self) -> Result<AstFile> {
        let module = if self.eat("module") {
            let p = self.path()?;
            self.need(";")?;
            p
        } else {
            vec![]
        };
        let mut imports = vec![];
        while self.eat("import") {
            let start = self.tokens[self.i - 1].span;
            let path = self.path()?;
            let mut names = vec![];
            if self.eat("::") {
                self.need("{")?;
                loop {
                    names.push(self.ident()?);
                    if !self.eat(",") || self.t() == "}" {
                        break;
                    }
                }
                self.need("}")?
            }
            self.need(";")?;
            imports.push(Import {
                path,
                names,
                span: self.end(start),
            })
        }
        let mut declarations = vec![];
        while !self.t().is_empty() {
            declarations.push(self.decl()?)
        }
        Ok(AstFile {
            module,
            imports,
            declarations,
        })
    }
    fn attrs(&mut self) -> Result<Vec<Attribute>> {
        let mut a = vec![];
        while self.eat("@") {
            let start = self.tokens[self.i - 1].span;
            let name = self.ident()?;
            let arguments = if self.eat("(") {
                self.args(")")?
            } else {
                vec![]
            };
            a.push(Attribute {
                name,
                arguments,
                span: self.end(start),
            })
        }
        Ok(a)
    }
    fn close_gt(&mut self) -> Result<()> {
        if self.eat(">") {
            return Ok(());
        }
        if [">>", ">=", ">>="].contains(&self.t()) {
            let token = &mut self.tokens[self.i];
            self.last = Span {
                file: token.span.file,
                start: token.span.start,
                end: token.span.start + 1,
            };
            token.text.remove(0);
            token.span.start += 1;
            return Ok(());
        }
        self.err("expected generic closing >")
    }
    fn generics(&mut self) -> Result<Vec<GenericParam>> {
        if !self.eat("<") {
            return Ok(vec![]);
        }
        let mut v = vec![];
        loop {
            let span = self.span();
            let name = self.ident()?;
            let copy = if self.eat(":") {
                self.need("Copy")?;
                true
            } else {
                false
            };
            v.push(GenericParam { name, copy, span });
            if !self.eat(",") || self.t().starts_with('>') {
                break;
            }
        }
        self.close_gt()?;
        Ok(v)
    }
    fn typeargs(&mut self) -> Result<Vec<Type>> {
        self.need("<")?;
        let mut v = vec![];
        loop {
            v.push(self.ty()?);
            if !self.eat(",") || self.t().starts_with('>') {
                break;
            }
        }
        self.close_gt()?;
        Ok(v)
    }
    fn fields(&mut self) -> Result<Vec<Field>> {
        let mut v = vec![];
        while self.t() != "}" {
            let start = self.span();
            let public = self.eat("pub");
            let name = self.ident()?;
            self.need(":")?;
            let ty = self.ty()?;
            v.push(Field {
                name,
                public,
                ty,
                span: self.end(start),
            });
            if !self.eat(",") {
                break;
            }
        }
        self.need("}")?;
        Ok(v)
    }
    fn abi(&mut self) -> Result<bool> {
        if !self.eat("extern") {
            return Ok(false);
        }
        self.need("abi")?;
        self.need("(")?;
        self.need("C")?;
        self.need(")")?;
        Ok(true)
    }
    fn effects(&mut self) -> Result<Vec<String>> {
        let mut v = vec![];
        if self.eat("effects") {
            self.need("{")?;
            while self.t() != "}" {
                v.push(self.ident()?);
                if !self.eat(",") {
                    break;
                }
            }
            self.need("}")?
        }
        Ok(v)
    }
    fn unit(&self, span: Span) -> Type {
        Type {
            kind: TypeKind::Tuple(vec![]),
            span,
        }
    }
    fn decl(&mut self) -> Result<Decl> {
        let start = self.span();
        let attributes = self.attrs()?;
        let public = self.eat("pub");
        let unsafe_ = self.eat("unsafe");
        let external = self.abi()?;
        let mut name = String::new();
        let kind = if self.eat("fn") {
            name = self.ident()?;
            DeclKind::Function(self.function(unsafe_, external)?)
        } else if self.eat("type") {
            name = self.ident()?;
            self.need("=")?;
            let t = self.ty()?;
            self.need(";")?;
            DeclKind::Alias(t)
        } else if self.t() == "resource" || self.t() == "struct" {
            let resource = self.eat("resource");
            self.need("struct")?;
            name = self.ident()?;
            let generics = self.generics()?;
            let (fields, newtype) = if self.eat("{") {
                (self.fields()?, None)
            } else {
                self.need("(")?;
                let t = self.ty()?;
                self.need(")")?;
                self.need(";")?;
                (vec![], Some(t))
            };
            DeclKind::Struct {
                resource,
                generics,
                fields,
                newtype,
            }
        } else if self.eat("enum") {
            name = self.ident()?;
            let generics = self.generics()?;
            self.need("{")?;
            let mut variants = vec![];
            loop {
                let s = self.span();
                let n = self.ident()?;
                let mut fields = vec![];
                let mut payload = None;
                if self.eat("(") {
                    payload = Some(self.ty()?);
                    self.need(")")?
                } else if self.eat("{") {
                    fields = self.fields()?
                }
                variants.push(Variant {
                    name: n,
                    fields,
                    payload,
                    span: self.end(s),
                });
                if !self.eat(",") || self.t() == "}" {
                    break;
                }
            }
            self.need("}")?;
            DeclKind::Enum { generics, variants }
        } else if self.eat("impl") {
            let generics = self.generics()?;
            let target = self.ty()?;
            self.need("{")?;
            let mut methods = vec![];
            while self.t() != "}" {
                let d = self.decl()?;
                if !matches!(d.kind, DeclKind::Function(_)) {
                    return self.err("implementation requires methods");
                }
                methods.push(d)
            }
            self.need("}")?;
            DeclKind::Impl {
                generics,
                target,
                methods,
            }
        } else if self.t() == "const" || self.t() == "static" {
            let constant = self.eat("const");
            if !constant {
                self.need("static")?
            }
            let mutable = !constant && self.eat("mut");
            name = self.ident()?;
            self.need(":")?;
            let ty = self.ty()?;
            let value = if external && !constant {
                None
            } else {
                self.need("=")?;
                Some(self.expr(0, false)?)
            };
            self.need(";")?;
            if constant {
                DeclKind::Const {
                    ty,
                    value: value.unwrap(),
                }
            } else {
                DeclKind::Static {
                    mutable,
                    external,
                    ty,
                    value,
                }
            }
        } else if self.eat("static_assert") {
            self.need("(")?;
            let e = self.expr(0, false)?;
            self.need(")")?;
            self.need(";")?;
            DeclKind::Assert(e)
        } else {
            return self.err("expected declaration");
        };
        if (unsafe_ || external)
            && !matches!(
                kind,
                DeclKind::Function(_) | DeclKind::Static { external: true, .. }
            )
        {
            return self.err("invalid declaration prefix");
        }
        Ok(Decl {
            name,
            public,
            attributes,
            kind,
            span: self.end(start),
        })
    }
    fn function(&mut self, unsafe_: bool, external: bool) -> Result<Function> {
        let generics = self.generics()?;
        self.need("(")?;
        let mut parameters = vec![];
        while self.t() != ")" {
            let start = self.span();
            let receiver = if self.t() == "self" || self.t() == "&" {
                let mut r = String::new();
                if self.eat("&") {
                    r.push('&');
                    if self.eat("mut") {
                        r.push_str("mut ")
                    }
                }
                self.need("self")?;
                r.push_str("self");
                Some(r)
            } else {
                None
            };
            if receiver.is_some() && !parameters.is_empty() {
                return self.err("receiver must be first parameter");
            }
            let (name, ty) = if let Some(r) = &receiver {
                let inner = Type {
                    kind: TypeKind::Path(vec![PathSegment {
                        name: "Self".into(),
                        arguments: vec![],
                        span: start,
                    }]),
                    span: start,
                };
                (
                    "self".into(),
                    if r.starts_with('&') {
                        Type {
                            kind: TypeKind::Ref {
                                mutable: r.contains("mut"),
                                inner: Box::new(inner),
                            },
                            span: start,
                        }
                    } else {
                        inner
                    },
                )
            } else {
                let name = self.ident()?;
                self.need(":")?;
                (name, self.ty()?)
            };
            parameters.push(Parameter {
                name,
                ty,
                receiver,
                span: self.end(start),
            });
            if !self.eat(",") {
                break;
            }
        }
        self.need(")")?;
        let result = if self.eat("->") {
            self.ty()?
        } else {
            self.unit(self.span())
        };
        let effects = self.effects()?;
        let body = if self.eat(";") {
            if !external {
                return self.err("only extern function may omit body");
            }
            None
        } else {
            Some(self.block()?)
        };
        Ok(Function {
            generics,
            parameters,
            result,
            unsafe_,
            external,
            effects,
            body,
        })
    }
    fn ty(&mut self) -> Result<Type> {
        let start = self.span();
        let kind = if self.eat("&") {
            TypeKind::Ref {
                mutable: self.eat("mut"),
                inner: Box::new(self.ty()?),
            }
        } else if self.eat("*") {
            let space = if ["kernel", "user", "phys", "mmio"].contains(&self.t()) {
                Some(self.bump().text)
            } else {
                None
            };
            let constant = self.eat("const");
            TypeKind::Pointer {
                space,
                constant,
                inner: Box::new(self.ty()?),
            }
        } else if self.eat("[") {
            if self.eat("]") {
                let constant = self.eat("const");
                TypeKind::Slice {
                    constant,
                    inner: Box::new(self.ty()?),
                }
            } else {
                let length = Box::new(self.expr(0, false)?);
                self.need("]")?;
                TypeKind::Array {
                    length,
                    inner: Box::new(self.ty()?),
                }
            }
        } else if self.eat("!") {
            TypeKind::Never
        } else if self.eat("(") {
            if self.eat(")") {
                TypeKind::Tuple(vec![])
            } else {
                let first = self.ty()?;
                if !self.eat(",") {
                    self.need(")")?;
                    return Ok(first);
                }
                let mut v = vec![first];
                if self.t() == ")" {
                    return self.err("one-tuples are excluded");
                }
                loop {
                    v.push(self.ty()?);
                    if !self.eat(",") || self.t() == ")" {
                        break;
                    }
                }
                self.need(")")?;
                TypeKind::Tuple(v)
            }
        } else if ["fn", "unsafe", "extern"].contains(&self.t()) {
            let unsafe_ = self.eat("unsafe");
            let external = self.abi()?;
            self.need("fn")?;
            self.need("(")?;
            let mut parameters = vec![];
            while self.t() != ")" {
                parameters.push(self.ty()?);
                if !self.eat(",") {
                    break;
                }
            }
            self.need(")")?;
            let result = Box::new(if self.eat("->") {
                self.ty()?
            } else {
                self.unit(start)
            });
            let effects = self.effects()?;
            TypeKind::Function {
                parameters,
                result,
                unsafe_,
                external,
                effects,
            }
        } else {
            TypeKind::Path(self.segments(false)?)
        };
        Ok(Type {
            kind,
            span: self.end(start),
        })
    }
    fn segments(&mut self, value: bool) -> Result<Vec<PathSegment>> {
        let mut p = vec![];
        loop {
            let span = self.span();
            let name = self.ident()?;
            let arguments = if !value && self.t() == "<" {
                self.typeargs()?
            } else if value && self.t() == "::" && self.tokens[self.i + 1].text == "<" {
                self.bump();
                self.typeargs()?
            } else {
                vec![]
            };
            p.push(PathSegment {
                name,
                arguments,
                span,
            });
            if !self.eat("::") {
                break;
            }
        }
        Ok(p)
    }
    fn args(&mut self, end: &str) -> Result<Vec<Expr>> {
        let mut v = vec![];
        while self.t() != end {
            v.push(self.expr(0, false)?);
            if !self.eat(",") {
                break;
            }
        }
        self.need(end)?;
        Ok(v)
    }
    fn expr(&mut self, min: u8, head: bool) -> Result<Expr> {
        self.expr_mode(min, head, false)
    }
    fn expr_mode(&mut self, min: u8, head: bool, statement: bool) -> Result<Expr> {
        let mut grouped = self.t() == "(";
        let mut left = self.primary(head)?;
        loop {
            if statement
                && !grouped
                && matches!(
                    left.kind,
                    ExprKind::Block(_)
                        | ExprKind::Unsafe(_)
                        | ExprKind::If { .. }
                        | ExprKind::Match { .. }
                        | ExprKind::Loop(_)
                        | ExprKind::While { .. }
                        | ExprKind::For { .. }
                )
                && matches!(self.t(), "*" | "&" | "-")
            {
                break;
            }
            let s = left.span;
            let postfix_allowed = grouped || !matches!(left.kind, ExprKind::Cast { .. });
            let kind = if min <= 15 && postfix_allowed && self.eat("(") {
                if let ExprKind::Path(p) = &left.kind
                    && p.len() == 2
                    && p[0].name == "arch"
                    && p[1].name == "asm"
                {
                    left = self.asm(s)?;
                    continue;
                }
                ExprKind::Call {
                    callee: Box::new(left),
                    arguments: self.args(")")?,
                }
            } else if min <= 15 && postfix_allowed && self.eat("[") {
                let index = Box::new(self.expr(0, false)?);
                self.need("]")?;
                ExprKind::Index {
                    base: Box::new(left),
                    index,
                }
            } else if min <= 15 && postfix_allowed && self.eat("?") {
                ExprKind::Try(Box::new(left))
            } else if min <= 15 && postfix_allowed && self.eat(".") {
                let name = if self.t().chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    let n = self.bump().text;
                    if !n.chars().all(|c| c.is_ascii_digit()) || n.len() > 1 && n.starts_with('0') {
                        return self.err("invalid decimal field index");
                    }
                    n
                } else {
                    self.ident()?
                };
                let arguments = if self.eat("::") {
                    self.typeargs()?
                } else {
                    vec![]
                };
                if !arguments.is_empty() && self.t() != "(" {
                    return self.err("generic method requires call");
                }
                ExprKind::Field {
                    base: Box::new(left),
                    name,
                    arguments,
                    grouped: false,
                }
            } else if min <= 13 && self.eat("as") {
                ExprKind::Cast {
                    value: Box::new(left),
                    ty: self.ty()?,
                }
            } else {
                let op = self.t();
                let prec = match op {
                    "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>="
                    | "+%=" | "-%=" | "*%=" => 1,
                    ".." | "..=" => 2,
                    "||" => 3,
                    "&&" => 4,
                    "==" | "!=" => 5,
                    "<" | "<=" | ">" | ">=" => 6,
                    "|" => 7,
                    "^" => 8,
                    "&" => 9,
                    "<<" | ">>" => 10,
                    "+" | "-" | "+%" | "-%" | "+?" | "-?" => 11,
                    "*" | "/" | "%" | "*%" | "*?" => 12,
                    _ => 0,
                };
                if prec == 0 || prec < min {
                    break;
                }
                if prec == 2 && matches!(&left.kind,ExprKind::Binary{op,..} if op==".."||op=="..=")
                {
                    return self.err("ranges cannot be chained");
                }
                let op = self.bump().text;
                let right = self.expr(if prec == 1 { prec } else { prec + 1 }, head)?;
                ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                }
            };
            grouped = false;
            left = Expr {
                kind,
                span: self.end(s),
            }
        }
        Ok(left)
    }
    fn primary(&mut self, head: bool) -> Result<Expr> {
        let start = self.span();
        let t = self.t().to_string();
        let kind = if ["!", "~", "-", "*", "&"].contains(&t.as_str()) {
            let mut op = self.bump().text;
            let end = self.tokens[self.i - 1].span.end;
            if op == "&" && self.eat("mut") {
                op.push_str("mut")
            }
            let operand = Box::new(self.expr(14, head)?);
            let adjacent = end == operand.span.start;
            ExprKind::Unary {
                op,
                operand,
                adjacent,
            }
        } else if t.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            ExprKind::Integer(self.bump().text)
        } else if t.starts_with('"') {
            ExprKind::String(self.bump().bytes.unwrap())
        } else if self.eat("true") {
            ExprKind::Bool(true)
        } else if self.eat("false") {
            ExprKind::Bool(false)
        } else if self.eat("null") {
            ExprKind::Null
        } else if self.eat("(") {
            if self.eat(")") {
                ExprKind::Tuple(vec![])
            } else {
                let mut first = self.expr(0, false)?;
                if !self.eat(",") {
                    self.need(")")?;
                    if let ExprKind::Field { grouped, .. } = &mut first.kind {
                        *grouped = true;
                    }
                    first.span = self.end(start);
                    return Ok(first);
                }
                if self.t() == ")" {
                    return self.err("one-tuples are excluded");
                }
                let mut v = vec![first];
                loop {
                    v.push(self.expr(0, false)?);
                    if !self.eat(",") || self.t() == ")" {
                        break;
                    }
                }
                self.need(")")?;
                ExprKind::Tuple(v)
            }
        } else if self.eat("[") {
            if self.eat("]") {
                ExprKind::Array(vec![])
            } else {
                let first = self.expr(0, false)?;
                if self.eat(";") {
                    let length = Box::new(self.expr(0, false)?);
                    self.need("]")?;
                    ExprKind::Repeat {
                        value: Box::new(first),
                        length,
                    }
                } else {
                    let mut v = vec![first];
                    while self.eat(",") && self.t() != "]" {
                        v.push(self.expr(0, false)?)
                    }
                    self.need("]")?;
                    ExprKind::Array(v)
                }
            }
        } else if t == "{" {
            ExprKind::Block(self.block()?)
        } else if self.eat("unsafe") {
            ExprKind::Unsafe(self.block()?)
        } else if self.eat("if") {
            let condition = Box::new(self.expr(0, true)?);
            let then = self.block()?;
            let else_ = if self.eat("else") {
                Some(Box::new(if self.t() == "if" {
                    self.primary(false)?
                } else {
                    let b = self.block()?;
                    Expr {
                        span: b.span,
                        kind: ExprKind::Block(b),
                    }
                }))
            } else {
                None
            };
            ExprKind::If {
                condition,
                then,
                else_,
            }
        } else if self.eat("match") {
            let value = Box::new(self.expr(0, true)?);
            self.need("{")?;
            let mut arms = vec![];
            loop {
                let p = self.pattern()?;
                self.need("=>")?;
                let e = self.expr(0, false)?;
                arms.push((p, e));
                if !self.eat(",") || self.t() == "}" {
                    break;
                }
            }
            self.need("}")?;
            ExprKind::Match { value, arms }
        } else if self.eat("loop") {
            ExprKind::Loop(self.block()?)
        } else if self.eat("while") {
            let condition = Box::new(self.expr(0, true)?);
            let body = self.block()?;
            ExprKind::While { condition, body }
        } else if self.eat("for") {
            let move_ = self.eat("move");
            let binding = if self.eat("_") {
                "_".into()
            } else {
                self.ident()?
            };
            self.need("in")?;
            let input = Box::new(self.expr(0, true)?);
            let body = self.block()?;
            ExprKind::For {
                move_,
                binding,
                input,
                body,
            }
        } else if self.eat("return") {
            ExprKind::Return(self.optional_expr(head)?)
        } else if self.eat("break") {
            ExprKind::Break(self.optional_expr(head)?)
        } else if self.eat("continue") {
            ExprKind::Continue
        } else if ["sizeof", "alignof", "offsetof"].contains(&t.as_str()) {
            let operation = self.bump().text;
            self.need("(")?;
            let ty = self.ty()?;
            let field = if operation == "offsetof" {
                self.need(",")?;
                Some(self.layout_field()?)
            } else {
                None
            };
            self.need(")")?;
            ExprKind::Layout {
                operation,
                ty,
                field,
            }
        } else {
            let path = self.segments(true)?;
            if !head && self.eat("{") {
                let mut fields = vec![];
                while self.t() != "}" {
                    let name = self.ident()?;
                    self.need(":")?;
                    fields.push((name, self.expr(0, false)?));
                    if !self.eat(",") {
                        break;
                    }
                }
                self.need("}")?;
                ExprKind::Record { path, fields }
            } else {
                ExprKind::Path(path)
            }
        };
        Ok(Expr {
            kind,
            span: self.end(start),
        })
    }
    fn layout_field(&mut self) -> Result<String> {
        if self.t().chars().next().is_some_and(|c| c.is_ascii_digit()) {
            let n = self.bump().text;
            if !n.chars().all(|c| c.is_ascii_digit()) || n.len() > 1 && n.starts_with('0') {
                return self.err("invalid offsetof decimal index");
            }
            Ok(n)
        } else {
            self.ident()
        }
    }
    fn optional_expr(&mut self, head: bool) -> Result<Option<Box<Expr>>> {
        if [";", "}", ")", "]", ",", ""].contains(&self.t()) {
            Ok(None)
        } else {
            Ok(Some(Box::new(self.expr(0, head)?)))
        }
    }
    fn block(&mut self) -> Result<Block> {
        let start = self.span();
        self.need("{")?;
        let mut statements = vec![];
        let mut tail = None;
        while self.t() != "}" {
            if self.t().is_empty() {
                return self.err("unclosed block");
            }
            let s = self.span();
            if self.eat(";") {
                statements.push(Statement::Empty(s));
                continue;
            }
            if self.t() == "let" || self.t() == "var" {
                let mutable = self.bump().text == "var";
                let pattern = self.pattern()?;
                let ty = if self.eat(":") {
                    Some(self.ty()?)
                } else {
                    None
                };
                self.need("=")?;
                let value = self.expr(0, false)?;
                self.need(";")?;
                statements.push(Statement::Binding {
                    mutable,
                    pattern,
                    ty,
                    value,
                    span: self.end(s),
                });
                continue;
            }
            if self.eat("static_assert") {
                self.need("(")?;
                let e = self.expr(0, false)?;
                self.need(")")?;
                self.need(";")?;
                statements.push(Statement::Assert(e));
                continue;
            }
            if self.eat("defer") {
                let e = if self.t() == "{" {
                    let b = self.block()?;
                    Expr {
                        span: b.span,
                        kind: ExprKind::Block(b),
                    }
                } else {
                    let e = self.expr(0, false)?;
                    self.need(";")?;
                    e
                };
                statements.push(Statement::Defer(e));
                continue;
            }
            let e = self.expr_mode(0, false, true)?;
            if self.eat(";") {
                statements.push(Statement::Expr(e))
            } else if self.t() == "}" {
                tail = Some(Box::new(e));
                break;
            } else if matches!(
                e.kind,
                ExprKind::Block(_)
                    | ExprKind::Unsafe(_)
                    | ExprKind::If { .. }
                    | ExprKind::Match { .. }
                    | ExprKind::Loop(_)
                    | ExprKind::While { .. }
                    | ExprKind::For { .. }
            ) {
                statements.push(Statement::Expr(e))
            } else {
                return self.err("expected statement semicolon");
            }
        }
        self.need("}")?;
        Ok(Block {
            statements,
            tail,
            span: self.end(start),
        })
    }
    fn pattern(&mut self) -> Result<Pattern> {
        let start = self.span();
        let kind = if self.eat("_") {
            PatternKind::Wildcard
        } else if self.eat("true") {
            PatternKind::Bool(true)
        } else if self.eat("false") {
            PatternKind::Bool(false)
        } else if self.t() == "-" || self.t().chars().next().is_some_and(|c| c.is_ascii_digit()) {
            let negative = self.eat("-");
            let end = self.tokens[self.i.saturating_sub(1)].span.end;
            let adjacent = end == self.span().start;
            let value = self.bump().text;
            if !value.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return self.err("negative pattern requires integer");
            }
            PatternKind::Integer {
                value,
                negative,
                adjacent,
            }
        } else if self.eat("(") {
            if self.eat(")") {
                PatternKind::Tuple(vec![])
            } else {
                let first = self.pattern()?;
                if !self.eat(",") {
                    self.need(")")?;
                    return Ok(first);
                }
                if self.t() == ")" {
                    return self.err("one-tuples are excluded");
                }
                let mut v = vec![first];
                loop {
                    v.push(self.pattern()?);
                    if !self.eat(",") || self.t() == ")" {
                        break;
                    }
                }
                self.need(")")?;
                PatternKind::Tuple(v)
            }
        } else {
            let path = self.path()?;
            if self.eat("(") {
                let value = Box::new(self.pattern()?);
                self.need(")")?;
                PatternKind::Payload { path, value }
            } else if self.eat("{") {
                let mut fields = vec![];
                while self.t() != "}" {
                    let name = self.ident()?;
                    self.need(":")?;
                    fields.push((name, self.pattern()?));
                    if !self.eat(",") {
                        break;
                    }
                }
                self.need("}")?;
                PatternKind::Record { path, fields }
            } else {
                PatternKind::Path(path)
            }
        };
        Ok(Pattern {
            kind,
            span: self.end(start),
        })
    }
    fn asm(&mut self, start: Span) -> Result<Expr> {
        if !self.t().starts_with('"') {
            return self.err("asm requires string template");
        }
        let template = self.bump().bytes.unwrap();
        let mut operands = vec![];
        let mut effects = vec![];
        while self.eat(",") && self.t() != ")" {
            if self.t() == "effects" {
                effects = self.effects()?;
                self.eat(",");
                break;
            }
            let s = self.span();
            let kind = self.ident()?;
            if !["in", "out", "clobber"].contains(&kind.as_str()) {
                return self.err("unknown asm operand");
            };
            self.need("(")?;
            if !self.t().starts_with('"') {
                return self.err("asm register requires string");
            }
            let register = self.bump().bytes.unwrap();
            self.need(")")?;
            let value = if kind == "clobber" {
                None
            } else {
                Some(self.expr(0, false)?)
            };
            operands.push(AsmOperand {
                kind,
                register,
                value,
                span: self.end(s),
            })
        }
        self.need(")")?;
        Ok(Expr {
            kind: ExprKind::Asm {
                template,
                operands,
                effects,
            },
            span: self.end(start),
        })
    }
}
