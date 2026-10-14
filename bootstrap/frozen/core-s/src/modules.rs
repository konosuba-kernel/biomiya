//! Module visibility and qualification before concrete type resolution.
use super::*;
fn qualify(module: &str, name: &str) -> String {
    if module.is_empty() {
        name.into()
    } else {
        format!("{module}::{name}")
    }
}
fn decl_name(d: &ast::DeclarationKind) -> &str {
    match d {
        ast::DeclarationKind::Struct { name, .. }
        | ast::DeclarationKind::Const { name, .. }
        | ast::DeclarationKind::Static { name, .. } => name,
        ast::DeclarationKind::Function(f)
        | ast::DeclarationKind::ExternFunction { function: f, .. } => &f.name,
    }
}
struct Context<'a> {
    module: &'a str,
    imports: BTreeMap<String, String>,
    symbols: &'a BTreeMap<String, (String, bool)>,
    locals: Vec<HashSet<String>>,
}
impl Context<'_> {
    fn name(&self, n: &mut String, sp: Span, public_type: bool) -> Result<()> {
        if n.starts_with("core::") {
            return Ok(());
        }
        if !n.contains("::") && self.locals.iter().rev().any(|s| s.contains(n)) {
            return Ok(());
        }
        let candidate = if let Some(n) = n.strip_prefix("root::") {
            n.into()
        } else if let Some((head, tail)) = n.split_once("::") {
            if let Some(module) = self.imports.get(head) {
                format!("{module}::{tail}")
            } else {
                qualify(self.module, n)
            }
        } else {
            qualify(self.module, n)
        };
        if let Some((owner, pub_)) = self.symbols.get(&candidate) {
            if (!*pub_ && owner != self.module) || (public_type && !*pub_) {
                return Err(error(
                    "E_NAME",
                    sp,
                    "private declaration is not visible in this interface",
                ));
            }
            *n = candidate;
        } else if n.contains("::") {
            return Err(error("E_NAME", sp, format!("unknown module path {n}")));
        } else if ![
            "bool",
            "u8",
            "u16",
            "u32",
            "u64",
            "usize",
            "i8",
            "i16",
            "i32",
            "i64",
            "isize",
            "addr_of",
            "addr_of_mut",
            "ptr_const",
            "addr",
            "ptr_add",
            "ptr_add_bytes",
            "slice_of",
            "slice_of_mut",
            "slice_from_raw_parts",
            "slice_from_raw_parts_mut",
            "slice_const",
            "panic",
            "read",
            "write",
            "copy_bytes",
            "fill_bytes",
            "atomic_load",
            "atomic_store",
            "atomic_compare_exchange",
        ]
        .contains(&n.as_str())
        {
            *n = candidate;
        }
        Ok(())
    }
    fn ty(&mut self, t: &mut ast::Type, pub_: bool) -> Result<()> {
        match &mut t.kind {
            ast::TypeKind::Named(n) => {
                if !n.contains("::") && self.locals.iter().rev().any(|s| s.contains(n)) {
                    return Err(error("E_TYPE", t.span, "local value shadows type name"));
                }
                self.name(n, t.span, pub_)?;
            }
            ast::TypeKind::Unit => {}
            ast::TypeKind::Pointer { target, .. } => self.ty(target, pub_)?,
            ast::TypeKind::Slice { element, .. } => self.ty(element, pub_)?,
            ast::TypeKind::Array { element, length } => {
                self.ty(element, pub_)?;
                self.expr(length)?;
            }
            ast::TypeKind::Result { ok, error } => {
                self.ty(ok, pub_)?;
                self.ty(error, pub_)?;
            }
        }
        Ok(())
    }
    fn expr(&mut self, e: &mut ast::Expr) -> Result<()> {
        use ast::ExprKind as E;
        match &mut e.kind {
            E::Name(n) => self.name(n, e.span, false)?,
            E::Call { name, arguments } => {
                self.name(name, e.span, false)?;
                for a in arguments {
                    self.expr(a)?;
                }
            }
            E::Struct { name, fields } => {
                self.name(name, e.span, false)?;
                for (_, v) in fields {
                    self.expr(v)?;
                }
            }
            E::Pointer { target, address } => {
                self.ty(target, false)?;
                self.expr(address)?;
            }
            E::Cast { value, ty } => {
                self.ty(ty, false)?;
                self.expr(value)?;
            }
            E::Unary { value, .. } | E::Try(value) => self.expr(value)?,
            E::Binary { left, right, .. } => {
                self.expr(left)?;
                self.expr(right)?;
            }
            E::Field { base, .. } => self.expr(base)?,
            E::Index { base, index } => {
                self.expr(base)?;
                self.expr(index)?;
            }
            E::Array(es) => {
                for e in es {
                    self.expr(e)?;
                }
            }
            E::Repeat { value, length } => {
                self.expr(value)?;
                self.expr(length)?;
            }
            E::Result {
                ok, error, value, ..
            } => {
                self.ty(ok, false)?;
                self.ty(error, false)?;
                self.expr(value)?;
            }
            E::Sizeof(t) | E::Alignof(t) => self.ty(t, false)?,
            _ => {}
        }
        Ok(())
    }
    fn block(&mut self, b: &mut ast::Block) -> Result<()> {
        use ast::StatementKind as S;
        self.locals.push(HashSet::new());
        for s in &mut b.statements {
            match &mut s.kind {
                S::Bind {
                    name, ty, value, ..
                } => {
                    self.ty(ty, false)?;
                    if let Some(v) = value {
                        self.expr(v)?;
                    }
                    self.locals.last_mut().unwrap().insert(name.clone());
                }
                S::Assign { target, value } => {
                    self.expr(target)?;
                    self.expr(value)?;
                }
                S::Expr(v) | S::Return(v) => self.expr(v)?,
                S::Block(b) | S::Unsafe(b) | S::Loop(b) | S::Defer(b) => self.block(b)?,
                S::If {
                    condition,
                    then,
                    else_,
                } => {
                    self.expr(condition)?;
                    self.block(then)?;
                    if let Some(b) = else_ {
                        self.block(b)?;
                    }
                }
                S::While { condition, body } => {
                    self.expr(condition)?;
                    self.block(body)?;
                }
                S::Match { value, arms } => {
                    self.expr(value)?;
                    for arm in arms {
                        self.locals.push(HashSet::new());
                        if let ast::PatternKind::Result {
                            ok, error, binding, ..
                        } = &mut arm.pattern.kind
                        {
                            self.ty(ok, false)?;
                            self.ty(error, false)?;
                            if let Some(p) = binding {
                                self.ty(&mut p.ty, false)?;
                                self.locals.last_mut().unwrap().insert(p.name.clone());
                            }
                        }
                        self.block(&mut arm.body)?;
                        self.locals.pop();
                    }
                }
                _ => {}
            }
        }
        self.locals.pop();
        Ok(())
    }
}
pub(super) fn merge(programs: Vec<ast::Program>) -> Result<ast::Program> {
    let mut symbols = BTreeMap::new();
    let mut modules = HashSet::new();
    for p in &programs {
        if p.module == "core"
            || p.module.starts_with("core::")
            || p.module == "root"
            || p.module.starts_with("root::")
        {
            return Err(error("E_NAME", Span::default(), "reserved module"));
        }
        let mut prefix = String::new();
        for part in p.module.split("::").filter(|s| !s.is_empty()) {
            prefix = qualify(&prefix, part);
            modules.insert(prefix.clone());
        }
        modules.insert(p.module.clone());
        for d in &p.declarations {
            let name = decl_name(&d.kind);
            if ["core", "root"].contains(&name) {
                return Err(error("E_NAME", d.span, "reserved declaration"));
            }
            if symbols
                .insert(qualify(&p.module, name), (p.module.clone(), d.public))
                .is_some()
            {
                return Err(error("E_NAME", d.span, "duplicate declaration"));
            }
        }
    }
    for module in &modules {
        let mut prefix = String::new();
        for part in module.split("::").filter(|s| !s.is_empty()) {
            prefix = qualify(&prefix, part);
            if symbols.contains_key(&prefix) {
                return Err(error(
                    "E_NAME",
                    Span::default(),
                    "module conflicts with declaration",
                ));
            }
        }
    }
    let mut module_imports: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for p in &programs {
        let imports = module_imports.entry(p.module.clone()).or_default();
        for (name, sp) in &p.imports {
            let n = name.strip_prefix("root::").unwrap_or(name);
            if !modules.contains(n) {
                return Err(error("E_NAME", *sp, "imported module does not exist"));
            }
            let alias = n.rsplit("::").next().unwrap();
            if alias == "core"
                || alias == "root"
                || symbols.contains_key(&qualify(&p.module, alias))
                || modules.contains(&qualify(&p.module, alias))
                || imports.insert(alias.into(), n.into()).is_some()
            {
                return Err(error("E_NAME", *sp, "conflicting import"));
            }
        }
    }
    let mut out = vec![];
    for mut p in programs {
        let mut c = Context {
            module: &p.module,
            imports: module_imports[&p.module].clone(),
            symbols: &symbols,
            locals: vec![],
        };
        for d in &mut p.declarations {
            match &mut d.kind {
                ast::DeclarationKind::Struct { name, fields } => {
                    *name = qualify(&p.module, name);
                    if d.public && fields.iter().any(|f| !f.public) {
                        return Err(error(
                            "I_UNSUPPORTED",
                            d.span,
                            "B2 public structures require public fields",
                        ));
                    }
                    for f in fields {
                        c.ty(&mut f.ty, d.public)?;
                    }
                }
                ast::DeclarationKind::Const { name, ty, value }
                | ast::DeclarationKind::Static {
                    name, ty, value, ..
                } => {
                    *name = qualify(&p.module, name);
                    c.ty(ty, d.public)?;
                    c.expr(value)?;
                }
                ast::DeclarationKind::Function(f)
                | ast::DeclarationKind::ExternFunction { function: f, .. } => {
                    f.name = qualify(&p.module, &f.name);
                    for a in &mut f.parameters {
                        c.ty(&mut a.ty, d.public)?;
                    }
                    c.ty(&mut f.result, d.public)?;
                    c.locals
                        .push(f.parameters.iter().map(|p| p.name.clone()).collect());
                    c.block(&mut f.body)?;
                    c.locals.pop();
                }
            }
            out.push(d.clone());
        }
    }
    Ok(ast::Program {
        module: String::new(),
        imports: vec![],
        declarations: out,
    })
}
