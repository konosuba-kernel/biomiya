use super::*;
impl Checker {
    pub(super) fn template_names(
        &mut self,
        _id: DefId,
        r: &DeclRecord,
        f: &ast::Function,
    ) -> Result<()> {
        let mut visible = BTreeSet::new();
        for p in &f.parameters {
            visible.insert(p.name.clone());
        }
        let mut types = BTreeSet::new();
        for p in r.impl_generics.iter().chain(&f.generics) {
            types.insert(p.name.clone());
        }
        if let Some(b) = &f.body {
            self.name_block(b, &r.module, &visible, &types)?
        }
        Ok(())
    }
    fn name_block(
        &self,
        b: &ast::Block,
        module: &[String],
        outer: &BTreeSet<String>,
        types: &BTreeSet<String>,
    ) -> Result<()> {
        let mut names = outer.clone();
        for s in &b.statements {
            match s {
                ast::Statement::Binding {
                    pattern, ty, value, ..
                } => {
                    if let Some(t) = ty {
                        self.name_type(t, module, types)?
                    }
                    self.name_expr(value, module, &names, types)?;
                    Self::pattern_names(pattern, &mut names)
                }
                ast::Statement::Expr(e) | ast::Statement::Defer(e) | ast::Statement::Assert(e) => {
                    self.name_expr(e, module, &names, types)?
                }
                _ => {}
            }
        }
        if let Some(t) = &b.tail {
            self.name_expr(t, module, &names, types)?
        }
        Ok(())
    }
    fn pattern_names(p: &ast::Pattern, names: &mut BTreeSet<String>) {
        match &p.kind {
            ast::PatternKind::Path(p) if p.len() == 1 => {
                names.insert(p[0].clone());
            }
            ast::PatternKind::Tuple(v) => {
                for p in v {
                    Self::pattern_names(p, names)
                }
            }
            ast::PatternKind::Payload { value, .. } => Self::pattern_names(value, names),
            ast::PatternKind::Record { fields, .. } => {
                for (_, p) in fields {
                    Self::pattern_names(p, names)
                }
            }
            _ => {}
        }
    }
    fn name_type(&self, t: &ast::Type, module: &[String], types: &BTreeSet<String>) -> Result<()> {
        match &t.kind {
            ast::TypeKind::Path(p) => {
                let path = p.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
                let known = p.len() == 1
                    && (types.contains(&p[0].name)
                        || p[0].name == "Self"
                        || p[0].name == "bool"
                        || integer(&p[0].name).is_some());
                if !known
                    && self
                        .resolver
                        .lookup(module, &path, Namespace::Type, t.span)?
                        .is_none()
                    && self
                        .resolver
                        .builtin_path(module, &path, Namespace::Type, t.span)?
                        .is_none()
                {
                    return Err(error("E_NAME", t.span, "unknown type in generic body"));
                }
                for p in p {
                    for a in &p.arguments {
                        self.name_type(a, module, types)?
                    }
                }
            }
            ast::TypeKind::Ref { inner, .. }
            | ast::TypeKind::Pointer { inner, .. }
            | ast::TypeKind::Slice { inner, .. }
            | ast::TypeKind::Array { inner, .. } => self.name_type(inner, module, types)?,
            ast::TypeKind::Tuple(v) => {
                for t in v {
                    self.name_type(t, module, types)?
                }
            }
            ast::TypeKind::Function {
                parameters, result, ..
            } => {
                for t in parameters {
                    self.name_type(t, module, types)?
                }
                self.name_type(result, module, types)?
            }
            _ => {}
        }
        Ok(())
    }
    pub(super) fn name_expr(
        &self,
        e: &ast::Expr,
        module: &[String],
        names: &BTreeSet<String>,
        types: &BTreeSet<String>,
    ) -> Result<()> {
        use ast::ExprKind::*;
        match &e.kind {
            Path(p) | Record { path: p, .. } => {
                let path = p.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
                let local = p.len() == 1 && names.contains(&p[0].name);
                let dependent = types.contains(&p[0].name) || p[0].name == "Self";
                let primitive = p.len() == 2
                    && integer(&p[0].name).is_some()
                    && ["MIN", "MAX"].contains(&p[1].name.as_str());
                let known = self
                    .resolver
                    .lookup(module, &path, Namespace::Value, e.span)?
                    .is_some()
                    || self
                        .resolver
                        .lookup(module, &path, Namespace::Type, e.span)?
                        .is_some()
                    || self
                        .resolver
                        .builtin_path(module, &path, Namespace::Value, e.span)?
                        .is_some();
                let variant=self.resolver.declarations.values().any(|r|matches!(&r.decl.kind,ast::DeclKind::Enum{variants,..} if variants.iter().any(|v|v.name==*path.last().unwrap())));
                if !local && !dependent && !primitive && !known && !variant {
                    return Err(error("E_NAME", e.span, "unknown value in generic body"));
                }
                for seg in p {
                    for t in &seg.arguments {
                        self.name_type(t, module, types)?
                    }
                }
                if let Record { fields, .. } = &e.kind {
                    for (_, e) in fields {
                        self.name_expr(e, module, names, types)?
                    }
                }
            }
            Tuple(v) | Array(v) => {
                for e in v {
                    self.name_expr(e, module, names, types)?
                }
            }
            Repeat { value, length }
            | Binary {
                left: value,
                right: length,
                ..
            }
            | Index {
                base: value,
                index: length,
            } => {
                self.name_expr(value, module, names, types)?;
                self.name_expr(length, module, names, types)?
            }
            Unary { operand, .. } | Try(operand) | Field { base: operand, .. } => {
                self.name_expr(operand, module, names, types)?
            }
            Cast { value, ty } => {
                self.name_expr(value, module, names, types)?;
                self.name_type(ty, module, types)?
            }
            Call { callee, arguments } => {
                self.name_expr(callee, module, names, types)?;
                for e in arguments {
                    self.name_expr(e, module, names, types)?
                }
            }
            Block(b) | Unsafe(b) | Loop(b) => self.name_block(b, module, names, types)?,
            If {
                condition,
                then,
                else_,
            } => {
                self.name_expr(condition, module, names, types)?;
                self.name_block(then, module, names, types)?;
                if let Some(e) = else_ {
                    self.name_expr(e, module, names, types)?
                }
            }
            Match { value, arms } => {
                self.name_expr(value, module, names, types)?;
                for (p, e) in arms {
                    let mut arm = names.clone();
                    Self::pattern_names(p, &mut arm);
                    self.name_expr(e, module, &arm, types)?
                }
            }
            While { condition, body } => {
                self.name_expr(condition, module, names, types)?;
                self.name_block(body, module, names, types)?
            }
            For {
                binding,
                input,
                body,
                ..
            } => {
                self.name_expr(input, module, names, types)?;
                let mut names = names.clone();
                names.insert(binding.clone());
                self.name_block(body, module, &names, types)?
            }
            Return(Some(e)) | Break(Some(e)) => self.name_expr(e, module, names, types)?,
            Layout { ty, .. } => self.name_type(ty, module, types)?,
            Asm { operands, .. } => {
                for o in operands {
                    if let Some(e) = &o.value {
                        self.name_expr(e, module, names, types)?
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
impl Checker {
    pub(super) fn template_body(
        &mut self,
        id: DefId,
        r: &DeclRecord,
        f: &ast::Function,
    ) -> Result<()> {
        let Some(body) = &f.body else { return Ok(()) };
        let sig = self.signatures[&id].clone();
        let mut ctx = Context::new(r.module.clone(), sig.result);
        ctx.template = true;
        for (i, g) in r.impl_generics.iter().chain(&f.generics).enumerate() {
            let t = self.types.intern(TypeKind::Param(i as u32));
            ctx.generics.insert(g.name.clone(), t);
        }
        if let Some(t) = &r.impl_target {
            ctx.self_ty = Some(self.ty(t, &mut ctx)?)
        }
        for (p, t) in f.parameters.iter().zip(&sig.params) {
            self.bind(&p.name, *t, false, p.span, &mut ctx)?;
        }
        self.block(body, Some(sig.result), &mut ctx)?;
        Ok(())
    }
    pub(super) fn dependent(&self, e: &ast::Expr, ctx: &Context) -> bool {
        use ast::ExprKind::*;
        match &e.kind {
            Path(p) => {
                ctx.local(&p[0].name)
                    .is_some_and(|l| self.has_params(ctx.locals[l.0 as usize].ty))
                    || ctx.generics.contains_key(&p[0].name)
                    || p.iter()
                        .flat_map(|p| &p.arguments)
                        .any(|t| self.dependent_type(t, ctx))
            }
            Field { base, .. } | Unary { operand: base, .. } | Try(base) => {
                self.dependent(base, ctx)
            }
            Call { callee, arguments } => {
                self.dependent(callee, ctx) || arguments.iter().any(|e| self.dependent(e, ctx))
            }
            Binary { left, right, .. }
            | Index {
                base: left,
                index: right,
            }
            | Repeat {
                value: left,
                length: right,
            } => self.dependent(left, ctx) || self.dependent(right, ctx),
            Tuple(v) | Array(v) => v.iter().any(|e| self.dependent(e, ctx)),
            Cast { value, ty } => self.dependent(value, ctx) || self.dependent_type(ty, ctx),
            Layout { .. } => false,
            _ => false,
        }
    }
    fn dependent_type(&self, t: &ast::Type, ctx: &Context) -> bool {
        match &t.kind {
            ast::TypeKind::Path(p) => {
                ctx.generics.contains_key(&p[0].name)
                    || p[0].name == "Self" && ctx.self_ty.is_some_and(|t| self.has_params(t))
                    || p.iter()
                        .flat_map(|p| &p.arguments)
                        .any(|t| self.dependent_type(t, ctx))
            }
            ast::TypeKind::Array { inner, .. }
            | ast::TypeKind::Ref { inner, .. }
            | ast::TypeKind::Slice { inner, .. }
            | ast::TypeKind::Pointer { inner, .. } => self.dependent_type(inner, ctx),
            ast::TypeKind::Tuple(v) => v.iter().any(|t| self.dependent_type(t, ctx)),
            _ => false,
        }
    }
    pub(super) fn check_independent_children(
        &mut self,
        e: &ast::Expr,
        ctx: &mut Context,
    ) -> Result<()> {
        use ast::ExprKind::*;
        if let Block(b) | Unsafe(b) = &e.kind {
            self.block(b, None, ctx)?;
            return Ok(());
        }
        let children: Vec<&ast::Expr> = match &e.kind {
            Call { callee, arguments } => {
                std::iter::once(callee.as_ref()).chain(arguments).collect()
            }
            Binary { left, right, .. }
            | Index {
                base: left,
                index: right,
            }
            | Repeat {
                value: left,
                length: right,
            } => vec![left, right],
            Tuple(v) | Array(v) => v.iter().collect(),
            Unary { operand, .. } | Field { base: operand, .. } | Try(operand) => vec![operand],
            _ => vec![],
        };
        for e in children {
            if !self.dependent(e, ctx) {
                self.expr(e, None, ctx)?;
            }
        }
        Ok(())
    }
}
