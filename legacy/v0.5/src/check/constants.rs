use super::*;
impl Checker {
    fn external_object(&mut self, t: TyId, span: Span) -> Result<()> {
        match self.types.kind(t).clone() {
            TypeKind::Array(t, _) => self.external_object(t, span),
            _ => crate::abi::validate_c_type(&mut self.types, t, false).map_err(|mut e| {
                e.span = span;
                e
            }),
        }
    }
    pub(super) fn validate_template_const_node(
        &mut self,
        e: &ast::Expr,
        ctx: &mut Context,
    ) -> Result<()> {
        use ast::ExprKind::*;
        match &e.kind {
            Binary { op, .. }
                if op == "="
                    || (op.ends_with('=') && !["==", "!=", "<=", ">="].contains(&op.as_str())) =>
            {
                return Err(error(
                    "E_CONST",
                    e.span,
                    "assignment is forbidden in constants",
                ));
            }
            Try(_)
            | Loop(_)
            | While { .. }
            | For { .. }
            | Return(_)
            | Break(_)
            | Continue
            | Asm { .. }
            | Unsafe(_) => {
                return Err(error(
                    "E_CONST",
                    e.span,
                    "operation not permitted in constant expression",
                ));
            }
            Call { callee, .. } => {
                if let Path(path) = &callee.kind {
                    if path.len() == 1 && ctx.local(&path[0].name).is_some() {
                        return Err(error(
                            "E_CONST",
                            e.span,
                            "ordinary calls are forbidden in constants",
                        ));
                    }
                    let names = path.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
                    if let Some(id) =
                        self.resolver
                            .lookup(&ctx.module, &names, Namespace::Value, e.span)?
                        && matches!(
                            self.resolver.declarations[&id].decl.kind,
                            ast::DeclKind::Function(_)
                        )
                    {
                        return Err(error(
                            "E_CONST",
                            e.span,
                            "ordinary calls are forbidden in constants",
                        ));
                    }
                } else {
                    return Err(error(
                        "E_CONST",
                        e.span,
                        "indirect calls are forbidden in constants",
                    ));
                }
            }
            Binary { op, left, right }
                if !["==", "!=", "<", ">", "<=", ">=", "..", "..="].contains(&op.as_str()) =>
            {
                for value in [left, right] {
                    if let Some(ty) = self.hint(value, ctx)? {
                        let kind = self.types.kind(ty);
                        if matches!(kind, TypeKind::Param(_)) {
                            continue;
                        }
                        let valid = if ["&&", "||"].contains(&op.as_str()) {
                            matches!(kind, TypeKind::Bool)
                        } else {
                            matches!(kind, TypeKind::Int(_))
                        };
                        if !valid {
                            return Err(error(
                                "E_TYPE",
                                e.span,
                                "invalid constant operator operand type",
                            ));
                        }
                    }
                }
            }
            Unary { op, operand, .. } if ["-", "~", "!"].contains(&op.as_str()) => {
                if let Some(ty) = self.hint(operand, ctx)? {
                    let kind = self.types.kind(ty);
                    if !matches!(kind, TypeKind::Param(_)) {
                        let valid = match op.as_str() {
                            "!" => matches!(kind, TypeKind::Bool),
                            "-" => matches!(kind,TypeKind::Int(i) if i.signed()),
                            _ => matches!(kind, TypeKind::Int(_)),
                        };
                        if !valid {
                            return Err(error(
                                "E_TYPE",
                                e.span,
                                "invalid constant unary operand type",
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub(super) fn check_const_template(&mut self, e: &ast::Expr, ctx: &Context) -> Result<h::Expr> {
        let mut scope = ctx.clone();
        scope.template = true;
        scope.const_context = true;
        // Operations on dependent locals await concrete operand types. Structural
        // roots still check their independent rules, such as match exhaustiveness.
        let typed = if self.dependent(e, &scope) {
            self.expr(e, None, &mut scope)?
        } else {
            self.expr_inner(e, None, &mut scope)?
        };
        if scope.locals[ctx.locals.len()..]
            .iter()
            .any(|local| local.mutable)
        {
            return Err(error("E_CONST", e.span, "var is forbidden in constants"));
        }
        let deferred = matches!(&typed.kind, h::ExprKind::Intrinsic { name, .. } if name == "template.deferred");
        if !deferred
            && !self.has_params(typed.ty)
            && !matches!(
                self.types.kind(typed.ty),
                TypeKind::Int(_) | TypeKind::Never
            )
        {
            return Err(error("E_CONST", e.span, "constant integer required"));
        }
        self.const_allowed(&typed)?;
        Ok(typed)
    }
    pub(super) fn constant(&mut self, id: DefId) -> Result<h::Expr> {
        if let Some(e) = self.constants.get(&id) {
            return Ok(e.clone());
        }
        let r = self.resolver.declarations[&id].clone();
        let ast::DeclKind::Const { ty, value } = r.decl.kind else {
            return Err(error("E_CONST", r.decl.span, "not a constant"));
        };
        if !self.constant_active.insert(id) {
            return Err(error("E_CONST", r.decl.span, "constant dependency cycle"));
        }
        let mut ctx = Context::new(r.module, self.unit());
        ctx.const_context = true;
        let ty = self.ty(&ty, &mut ctx)?;
        let e = self.expr(&value, Some(ty), &mut ctx)?;
        if ctx.locals.iter().any(|l| l.mutable) {
            return Err(error(
                "E_CONST",
                value.span,
                "var is forbidden in constants",
            ));
        }
        self.const_allowed(&e)?;
        let e = self.reduce(&e, &mut BTreeMap::new())?;
        self.constants.insert(id, e.clone());
        self.constant_active.remove(&id);
        Ok(e)
    }
    pub(super) fn const_ast(&mut self, e: &ast::Expr, ctx: &mut Context) -> Result<u64> {
        let mut scope = ctx.clone();
        scope.const_context = true;
        let typed = self.expr(e, None, &mut scope)?;
        if scope.locals[ctx.locals.len()..]
            .iter()
            .any(|local| local.mutable)
        {
            return Err(error("E_CONST", e.span, "var is forbidden in constants"));
        }
        if !matches!(self.types.kind(typed.ty), TypeKind::Int(_)) {
            return Err(error("E_CONST", e.span, "constant integer required"));
        }
        self.const_allowed(&typed)?;
        let reduced = self.reduce(&typed, &mut ctx.const_env.clone())?;
        let n = self.eval_inner(&reduced)?;
        if let TypeKind::Int(i) = self.types.kind(typed.ty)
            && i.signed()
            && n >> (i.bits() - 1) != 0
        {
            return Err(error(
                "E_CONST",
                e.span,
                "constant length cannot be negative",
            ));
        }
        Ok(n)
    }
    fn const_allowed(&mut self, e: &h::Expr) -> Result<()> {
        use h::ExprKind::*;
        match &e.kind {
            Unit | Integer(_) | String(_) | Function { .. } | Local(_) | Uninit => {}
            Unary { value, .. } | Cast(value) | Adapt(value) | RawAddress { value, .. } => {
                if matches!(e.kind, RawAddress { .. }) {
                    self.const_place(value)?
                } else {
                    self.const_allowed(value)?
                }
            }
            Binary { left, right, .. } => {
                self.const_allowed(left)?;
                self.const_allowed(right)?
            }
            Aggregate { fields, .. } => {
                for (_, x) in fields {
                    self.const_allowed(x)?
                }
            }
            Array(v) => {
                for x in v {
                    self.const_allowed(x)?
                }
            }
            Repeat { value, .. } => self.const_allowed(value)?,
            Borrow {
                value,
                mutable: false,
                ..
            } => self.const_place(value)?,
            Slice { base, start, end } => {
                self.const_place(base)?;
                self.const_allowed(start)?;
                self.const_allowed(end)?
            }
            If {
                condition,
                then,
                else_,
            } => {
                self.const_allowed(condition)?;
                self.const_allowed(then)?;
                if let Some(e) = else_ {
                    self.const_allowed(e)?
                }
            }
            Block {
                statements, tail, ..
            } => {
                for s in statements {
                    match s {
                        h::Statement::Bind { value, .. } => self.const_allowed(value)?,
                        h::Statement::Expr(value) if matches!(&value.kind, Intrinsic {name,..} if name=="__const.assert") => {
                            self.const_allowed(value)?
                        }
                        _ => {
                            return Err(error(
                                "E_CONST",
                                e.span,
                                "constant block allows only let and assertions",
                            ));
                        }
                    }
                }

                if let Some(e) = tail {
                    self.const_allowed(e)?
                }
            }
            Match { value, arms } => {
                self.const_allowed(value)?;
                for (_, e) in arms {
                    self.const_allowed(e)?
                }
            }
            Intrinsic {
                name, arguments, ..
            } if name.starts_with("__const.")
                || name == "template.deferred"
                || ["checked_cast", "atomic.new", "UnsafeCell.new", "panic"]
                    .contains(&name.as_str()) =>
            {
                for e in arguments {
                    self.const_allowed(e)?
                }
            }
            Len(base) => self.const_allowed(base)?,
            Field { base, .. } => {
                self.const_allowed(base)?;
                if !self.types.properties(base.ty).copy {
                    return Err(error(
                        "E_CONST",
                        e.span,
                        "constant field read requires Copy aggregate",
                    ));
                }
            }
            Index { base, index } => {
                self.const_allowed(base)?;
                self.const_allowed(index)?;
                if !self.types.properties(base.ty).copy {
                    return Err(error(
                        "E_CONST",
                        e.span,
                        "constant index read requires Copy aggregate",
                    ));
                }
            }
            _ => {
                return Err(error(
                    "E_CONST",
                    e.span,
                    "operation not permitted in constant expression",
                ));
            }
        }
        Ok(())
    }
    fn const_place(&self, e: &h::Expr) -> Result<()> {
        match &e.kind {
            h::ExprKind::Global {
                mutable: false,
                external: false,
                ..
            } => Ok(()),
            h::ExprKind::Field { base, .. } | h::ExprKind::Index { base, .. } => {
                self.const_place(base)
            }
            _ => Err(error(
                "E_CONST",
                e.span,
                "constant address must refer to static storage",
            )),
        }
    }
    pub(super) fn reduce(
        &self,
        e: &h::Expr,
        env: &mut BTreeMap<h::LocalId, h::Expr>,
    ) -> Result<h::Expr> {
        use h::ExprKind::*;
        let mut out = e.clone();
        out.kind = match &e.kind {
            Local(id) => {
                let value = env
                    .get(id)
                    .cloned()
                    .ok_or_else(|| error("E_CONST", e.span, "nonconstant local value"))?;
                return self.reduce(&value, env);
            }
            Block {
                statements, tail, ..
            } => {
                let mut scope = env.clone();
                for s in statements {
                    if let h::Statement::Bind { pattern, value } = s {
                        let value = self.reduce(value, &mut scope)?;
                        self.bind_constant(pattern, &value, &mut scope)?
                    } else if let h::Statement::Expr(value) = s {
                        self.reduce(value, &mut scope)?;
                    } else {
                        return Err(error("E_CONST", e.span, "invalid constant statement"));
                    }
                }
                return if let Some(t) = tail {
                    self.reduce(t, &mut scope)
                } else {
                    Ok(h::Expr {
                        ty: e.ty,
                        kind: Unit,
                        span: e.span,
                    })
                };
            }
            If {
                condition,
                then,
                else_,
            } => {
                let c = self.reduce(condition, env)?;
                return if self.eval_inner(&c)? != 0 {
                    self.reduce(then, env)
                } else if let Some(e) = else_ {
                    self.reduce(e, env)
                } else {
                    Ok(h::Expr {
                        ty: e.ty,
                        kind: Unit,
                        span: e.span,
                    })
                };
            }
            Match { value, arms } => {
                let value = self.reduce(value, env)?;
                for (p, arm) in arms {
                    if self.constant_matches(p, &value)? {
                        let mut scope = env.clone();
                        self.bind_constant(p, &value, &mut scope)?;
                        return self.reduce(arm, &mut scope);
                    }
                }
                return Err(error(
                    "E_CONST",
                    e.span,
                    "constant match has no matching arm",
                ));
            }
            Unary { op, value } => Unary {
                op: op.clone(),
                value: Box::new(self.reduce(value, env)?),
            },
            Binary { op, left, right } => {
                let left = self.reduce(left, env)?;
                if op == "&&" && self.eval_inner(&left)? == 0 {
                    return Ok(h::Expr {
                        ty: e.ty,
                        kind: Integer(0),
                        span: e.span,
                    });
                }
                if op == "||" && self.eval_inner(&left)? != 0 {
                    return Ok(h::Expr {
                        ty: e.ty,
                        kind: Integer(1),
                        span: e.span,
                    });
                }
                let right = self.reduce(right, env)?;
                if ["+?", "-?", "*?"].contains(&op.as_str()) {
                    let a = self.eval_inner(&left)?;
                    let b = self.eval_inner(&right)?;
                    let TypeKind::Int(i) = self.types.kind(left.ty) else {
                        unreachable!()
                    };
                    let a = if i.signed() {
                        sign(a, i.bits())
                    } else {
                        a as i128
                    };
                    let b = if i.signed() {
                        sign(b, i.bits())
                    } else {
                        b as i128
                    };
                    let n = match op.as_str() {
                        "+?" => a.checked_add(b),
                        "-?" => a.checked_sub(b),
                        _ => a.checked_mul(b),
                    };
                    let (min, max) = if i.signed() {
                        (-(1i128 << (i.bits() - 1)), (1i128 << (i.bits() - 1)) - 1)
                    } else {
                        (0, mask(i.bits()) as i128)
                    };
                    let good = n.filter(|n| *n >= min && *n <= max);
                    return Ok(h::Expr {
                        ty: e.ty,
                        span: e.span,
                        kind: Aggregate {
                            variant: Some(if good.is_some() { 0 } else { 1 }),
                            fields: good
                                .map(|n| {
                                    vec![(
                                        0,
                                        h::Expr {
                                            ty: left.ty,
                                            span: e.span,
                                            kind: Integer(n as u64 & mask(i.bits())),
                                        },
                                    )]
                                })
                                .unwrap_or_default(),
                        },
                    });
                }
                if ["==", "!="].contains(&op.as_str()) {
                    let equal = self.constant_equal(&left, &right)?;
                    return Ok(h::Expr {
                        ty: e.ty,
                        span: e.span,
                        kind: Integer((if op == "==" { equal } else { !equal }) as u64),
                    });
                }
                Binary {
                    op: op.clone(),
                    left: Box::new(left),
                    right: Box::new(right),
                }
            }
            Cast(v) => Cast(Box::new(self.reduce(v, env)?)),
            Adapt(v) => Adapt(Box::new(self.reduce(v, env)?)),
            Array(v) => Array(
                v.iter()
                    .map(|v| self.reduce(v, env))
                    .collect::<Result<_>>()?,
            ),
            Aggregate { variant, fields } => Aggregate {
                variant: *variant,
                fields: fields
                    .iter()
                    .map(|(i, e)| self.reduce(e, env).map(|e| (*i, e)))
                    .collect::<Result<_>>()?,
            },
            Repeat { value, length } => Repeat {
                value: Box::new(self.reduce(value, env)?),
                length: *length,
            },
            Field { base, field } => {
                let base = self.reduce(base, env)?;
                let Aggregate { fields, .. } = base.kind else {
                    return Err(error("E_CONST", e.span, "constant field needs aggregate"));
                };
                return fields
                    .into_iter()
                    .find(|(i, _)| i == field)
                    .map(|(_, v)| v)
                    .ok_or_else(|| error("E_CONST", e.span, "constant field missing"));
            }
            Index { base, index } => {
                let base = self.reduce(base, env)?;
                let index = self.reduce(index, env)?;
                let i = self.eval_inner(&index)?;
                return match base.kind {
                    Array(v) => v
                        .get(i as usize)
                        .cloned()
                        .ok_or_else(|| error("E_CONST", e.span, "constant index out of bounds")),
                    Repeat { value, length } if i < length => Ok(*value),
                    _ => Err(error("E_CONST", e.span, "constant index requires array")),
                };
            }
            Len(v) => {
                let v = self.reduce(v, env)?;
                let n = match v.kind {
                    Array(v) => v.len() as u64,
                    Repeat { length, .. } => length,
                    String(v) => v.len() as u64,
                    _ => return Err(error("E_CONST", e.span, "constant len requires sequence")),
                };
                Integer(n)
            }
            Intrinsic {
                name,
                type_arguments,
                ..
            } if name == "__const.sizeof"
                || name == "__const.alignof"
                || name.starts_with("__const.offsetof.") =>
            {
                let mut types = self.types.clone();
                let layout =
                    crate::layout::LayoutEngine::new(&mut types).layout(type_arguments[0])?;
                let value = if name == "__const.sizeof" {
                    layout.size
                } else if name == "__const.alignof" {
                    layout.align
                } else {
                    let field = name.strip_prefix("__const.offsetof.").unwrap();
                    let TypeKind::Nominal(id, _) = types.kind(type_arguments[0]) else {
                        return Err(error("E_NAME", e.span, "offsetof requires record type"));
                    };
                    let index = types
                        .definition(*id)
                        .unwrap()
                        .fields()
                        .iter()
                        .position(|f| f.name == field)
                        .ok_or_else(|| error("E_NAME", e.span, "unknown layout field"))?;
                    layout.fields[index]
                };
                Integer(value)
            }
            Intrinsic {
                name, arguments, ..
            } if name == "__const.assert" => {
                let condition = self.reduce(&arguments[0], env)?;
                if self.eval_inner(&condition)? != 1 {
                    return Err(error("E_CONST", e.span, "static assertion false"));
                }
                Unit
            }
            Intrinsic {
                name, arguments, ..
            } if name == "panic" => return Err(error("E_CONST", e.span, "constant panic")),
            Intrinsic {
                name,
                arguments,
                type_arguments,
            } if name == "atomic.new" || name == "UnsafeCell.new" => Intrinsic {
                name: name.clone(),
                arguments: arguments
                    .iter()
                    .map(|e| self.reduce(e, env))
                    .collect::<Result<_>>()?,
                type_arguments: type_arguments.clone(),
            },
            Intrinsic {
                name,
                arguments,
                type_arguments,
            } if name == "checked_cast" => {
                let v = self.reduce(&arguments[0], env)?;
                let n = self.eval_inner(&v)?;
                let TypeKind::Int(src) = self.types.kind(v.ty) else {
                    unreachable!()
                };
                let TypeKind::Int(dst) = self.types.kind(type_arguments[0]) else {
                    unreachable!()
                };
                let n = if src.signed() {
                    sign(n, src.bits())
                } else {
                    n as i128
                };
                let (min, max) = if dst.signed() {
                    (
                        -(1i128 << (dst.bits() - 1)),
                        (1i128 << (dst.bits() - 1)) - 1,
                    )
                } else {
                    (0, mask(dst.bits()) as i128)
                };
                Aggregate {
                    variant: Some(if n >= min && n <= max { 0 } else { 1 }),
                    fields: if n >= min && n <= max {
                        vec![(
                            0,
                            h::Expr {
                                ty: type_arguments[0],
                                kind: Integer(n as u64 & mask(dst.bits())),
                                span: e.span,
                            },
                        )]
                    } else {
                        vec![]
                    },
                }
            }
            _ => e.kind.clone(),
        };
        if matches!(
            out.kind,
            h::ExprKind::Unary { .. } | h::ExprKind::Binary { .. } | h::ExprKind::Cast(_)
        ) && matches!(self.types.kind(out.ty), TypeKind::Int(_) | TypeKind::Bool)
        {
            out.kind = h::ExprKind::Integer(self.eval_inner(&out)?);
        }
        Ok(out)
    }
    fn constant_equal(&self, a: &h::Expr, b: &h::Expr) -> Result<bool> {
        use h::ExprKind::*;
        match (&a.kind, &b.kind) {
            (
                Aggregate {
                    variant: a,
                    fields: af,
                },
                Aggregate {
                    variant: b,
                    fields: bf,
                },
            ) => {
                if a != b {
                    return Ok(false);
                }
                for (i, a) in af {
                    let Some((_, b)) = bf.iter().find(|(j, _)| j == i) else {
                        return Ok(false);
                    };
                    if !self.constant_equal(a, b)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (Array(a), Array(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }
                for (a, b) in a.iter().zip(b) {
                    if !self.constant_equal(a, b)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (
                Repeat {
                    value: a,
                    length: x,
                },
                Repeat {
                    value: b,
                    length: y,
                },
            ) => Ok(x == y && (*x == 0 || self.constant_equal(a, b)?)),
            _ => Ok(self.eval_inner(a)? == self.eval_inner(b)?),
        }
    }
    fn constant_matches(&self, p: &h::Pattern, value: &h::Expr) -> Result<bool> {
        match &p.kind {
            h::PatternKind::Wildcard | h::PatternKind::Bind(_) => Ok(true),
            h::PatternKind::Integer(n) => Ok(self.eval_inner(value)? == *n),
            h::PatternKind::Fields { variant, fields } => {
                let h::ExprKind::Aggregate {
                    variant: v,
                    fields: values,
                } = &value.kind
                else {
                    return Ok(fields.is_empty() && matches!(value.kind, h::ExprKind::Unit));
                };
                if variant != v {
                    return Ok(false);
                }
                for (i, p) in fields {
                    let v = values.iter().find(|(j, _)| i == j).ok_or_else(|| {
                        error("E_CONST", p.span, "constant pattern field missing")
                    })?;
                    if !self.constant_matches(p, &v.1)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Err(error(
                "E_CONST",
                p.span,
                "borrowed patterns are not constants",
            )),
        }
    }
    pub(super) fn bind_constant(
        &self,
        p: &h::Pattern,
        value: &h::Expr,
        env: &mut BTreeMap<h::LocalId, h::Expr>,
    ) -> Result<()> {
        match &p.kind {
            h::PatternKind::Bind(id) => {
                env.insert(*id, value.clone());
            }
            h::PatternKind::Fields { fields, .. } => {
                for (i, p) in fields {
                    let projected = h::Expr {
                        ty: p.ty,
                        span: p.span,
                        kind: h::ExprKind::Field {
                            base: Box::new(value.clone()),
                            field: *i,
                        },
                    };
                    self.bind_constant(p, &projected, env)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub(super) fn eval(&mut self, e: &h::Expr) -> Result<u64> {
        self.const_allowed(e)?;
        self.eval_inner(&self.reduce(e, &mut BTreeMap::new())?)
    }
    fn eval_inner(&self, e: &h::Expr) -> Result<u64> {
        use h::ExprKind::*;
        let fail = |m: &str| error("E_CONST", e.span, m);
        match &e.kind {
            Unit => Ok(0),
            Integer(n) => Ok(*n),
            Cast(v) | Adapt(v) => {
                let n = self.eval_inner(v)?;
                let n = match self.types.kind(v.ty) {
                    TypeKind::Int(i) if i.signed() => sign(n, i.bits()) as u64,
                    _ => n,
                };
                Ok(if let TypeKind::Int(i) = self.types.kind(e.ty) {
                    n & mask(i.bits())
                } else {
                    n
                })
            }
            Unary { op, value } => {
                let n = self.eval_inner(value)?;
                let bits = match self.types.kind(e.ty) {
                    TypeKind::Int(i) => i.bits(),
                    _ => 64,
                };
                Ok(match op.as_str() {
                    "-" => {
                        if n == (1u64 << (bits - 1)) {
                            return Err(fail("constant negation overflow"));
                        }
                        0u64.wrapping_sub(n) & mask(bits)
                    }
                    "~" => !n & mask(bits),
                    "!" => (n == 0) as u64,
                    _ => return Err(fail("unsupported constant unary operation")),
                })
            }
            Binary { op, left, right } => {
                let a = self.eval_inner(left)?;
                if op == "&&" && a == 0 {
                    return Ok(0);
                }
                if op == "||" && a != 0 {
                    return Ok(1);
                }
                let b = self.eval_inner(right)?;
                let (bits, signed) = match self.types.kind(left.ty) {
                    TypeKind::Int(i) => (i.bits(), i.signed()),
                    _ => (64, false),
                };
                let sa = sign(a, bits);
                let sb = sign(b, bits);
                let val = match op.as_str() {
                    "==" => (a == b) as u64,
                    "!=" => (a != b) as u64,
                    "<" => {
                        if signed {
                            (sa < sb) as u64
                        } else {
                            (a < b) as u64
                        }
                    }
                    "<=" => {
                        if signed {
                            (sa <= sb) as u64
                        } else {
                            (a <= b) as u64
                        }
                    }
                    ">" => {
                        if signed {
                            (sa > sb) as u64
                        } else {
                            (a > b) as u64
                        }
                    }
                    ">=" => {
                        if signed {
                            (sa >= sb) as u64
                        } else {
                            (a >= b) as u64
                        }
                    }
                    "&&" | "||" => (b != 0) as u64,
                    "&" => a & b,
                    "|" => a | b,
                    "^" => a ^ b,
                    "<<" | ">>" => {
                        if b >= bits as u64 {
                            return Err(fail("constant shift overflow"));
                        }
                        if op == "<<" {
                            a << b
                        } else if signed {
                            (sa >> b) as u64
                        } else {
                            a >> b
                        }
                    }
                    "+%" => a.wrapping_add(b),
                    "-%" => a.wrapping_sub(b),
                    "*%" => a.wrapping_mul(b),
                    "+" | "-" | "*" | "/" | "%" => {
                        if (op == "/" || op == "%") && b == 0 {
                            return Err(fail("constant division by zero"));
                        }
                        if signed
                            && (op == "/" || op == "%")
                            && sa == -(1i128 << (bits - 1))
                            && sb == -1
                        {
                            return Err(fail("constant division overflow"));
                        }
                        let n = if signed {
                            match op.as_str() {
                                "+" => sa + sb,
                                "-" => sa - sb,
                                "*" => sa * sb,
                                "/" => sa / sb,
                                _ => sa % sb,
                            }
                        } else {
                            match op.as_str() {
                                "+" => a as i128 + b as i128,
                                "-" => a as i128 - b as i128,
                                "*" => (a as i128)
                                    .checked_mul(b as i128)
                                    .ok_or_else(|| fail("constant arithmetic overflow"))?,
                                "/" => (a / b) as i128,
                                _ => (a % b) as i128,
                            }
                        };
                        let (min, max) = if signed {
                            (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
                        } else {
                            (0, mask(bits) as i128)
                        };
                        if n < min || n > max {
                            return Err(fail("constant arithmetic overflow"));
                        }
                        n as u64
                    }
                    _ => return Err(fail("constant operation unsupported")),
                };
                Ok(val & mask(bits))
            }
            If {
                condition,
                then,
                else_,
            } => {
                if self.eval_inner(condition)? != 0 {
                    self.eval_inner(then)
                } else {
                    else_.as_ref().map_or(Ok(0), |e| self.eval_inner(e))
                }
            }
            Block { tail, .. } => tail.as_ref().map_or(Ok(0), |e| self.eval_inner(e)),
            Field { base, field } => {
                if let Aggregate { fields, .. } = &base.kind {
                    self.eval_inner(
                        &fields
                            .iter()
                            .find(|(i, _)| i == field)
                            .ok_or_else(|| fail("missing constant field"))?
                            .1,
                    )
                } else {
                    Err(fail("unsupported constant field"))
                }
            }
            Index { base, index } => {
                let n = self.eval_inner(index)? as usize;
                if let Array(v) = &base.kind {
                    self.eval_inner(
                        v.get(n)
                            .ok_or_else(|| fail("constant index out of bounds"))?,
                    )
                } else {
                    Err(fail("unsupported constant index"))
                }
            }
            _ => Err(fail("constant value is not scalar")),
        }
    }
    pub(super) fn static_data(&mut self, id: DefId, r: &DeclRecord) -> Result<()> {
        let ast::DeclKind::Static {
            mutable,
            external,
            ty,
            value,
        } = &r.decl.kind
        else {
            unreachable!()
        };
        let mut ctx = Context::new(r.module.clone(), self.unit());
        ctx.const_context = true;
        let ty = self.ty(ty, &mut ctx)?;
        if *external {
            self.external_object(ty, r.decl.span)?;
            self.externals.push(r.decl.name.clone());
            return Ok(());
        }
        let value = self.expr(value.as_ref().unwrap(), Some(ty), &mut ctx)?;
        if ctx.locals.iter().any(|l| l.mutable) {
            return Err(error(
                "E_CONST",
                r.decl.span,
                "var forbidden in static initializer",
            ));
        }
        self.const_allowed(&value)?;
        let value = self.reduce(&value, &mut BTreeMap::new())?;
        self.prepare_type(ty)?;
        let layout = crate::layout::LayoutEngine::new(&mut self.types).layout(ty)?;
        const STATIC_IMAGE_LIMIT: u64 = 64 * 1024 * 1024;
        let used = self
            .data
            .iter()
            .try_fold(0u64, |n, d| n.checked_add(d.bytes.len() as u64))
            .unwrap_or(u64::MAX);
        if layout.size > STATIC_IMAGE_LIMIT || used.saturating_add(layout.size) > STATIC_IMAGE_LIMIT
        {
            return Err(error(
                "I_LIMIT",
                r.decl.span,
                "static initializer image exceeds the 64 MiB per-compilation limit",
            ));
        }
        let object_align = if let Some(a) = r.decl.attributes.iter().find(|a| a.name == "align") {
            let align = self
                .const_ast(&a.arguments[0], &mut ctx)
                .map_err(|d| error("E_LAYOUT", a.span, d.message))?;
            if align == 0 || !align.is_power_of_two() || align > 4096 {
                return Err(error("E_LAYOUT", a.span, "invalid static alignment"));
            }
            layout.align.max(align)
        } else {
            layout.align
        };
        let mut initialized = Vec::new();
        initialized
            .try_reserve_exact(layout.size as usize)
            .map_err(|_| {
                error(
                    "I_LIMIT",
                    r.decl.span,
                    "host memory unavailable for static initializer image",
                )
            })?;
        initialized.resize(layout.size as usize, 0);
        let mut data = Data {
            symbol: self.global_symbol(id),
            bytes: initialized,
            relocations: vec![],
            align: object_align,
            writable: *mutable,
            exported: r.decl.attributes.iter().any(|a| a.name == "export"),
            section: r
                .decl
                .attributes
                .iter()
                .find(|a| a.name == "section")
                .and_then(|a| match &a.arguments[0].kind {
                    ast::ExprKind::String(s) => Some(crate::linkage::encode_name(s)),
                    _ => None,
                }),
        };
        self.serialize(&value, 0, &mut data)?;
        self.data.push(data);
        Ok(())
    }
    pub(super) fn global_symbol(&self, id: DefId) -> String {
        let r = &self.resolver.declarations[&id];
        if let Some(a) = r.decl.attributes.iter().find(|a| a.name == "export")
            && let ast::ExprKind::String(s) = &a.arguments[0].kind
        {
            return crate::linkage::encode_name(s);
        }
        if matches!(r.decl.kind, ast::DeclKind::Static { external: true, .. }) {
            r.decl.name.clone()
        } else {
            format!("__biomiya_static_{}", id.0)
        }
    }
    fn serialize(&mut self, e: &h::Expr, offset: u64, data: &mut Data) -> Result<()> {
        self.prepare_type(e.ty)?;
        let layout = crate::layout::LayoutEngine::new(&mut self.types).layout(e.ty)?;
        match &e.kind {
            h::ExprKind::Uninit => {}
            h::ExprKind::Intrinsic {
                name, arguments, ..
            } if name == "atomic.new" || name == "UnsafeCell.new" => {
                self.serialize(&arguments[0], offset, data)?
            }
            h::ExprKind::String(bytes) => {
                let used = self.data.iter().fold(data.bytes.len() as u64, |n, d| {
                    n.saturating_add(d.bytes.len() as u64)
                });
                let length = bytes.len().max(1);
                if used.saturating_add(length as u64) > 64 * 1024 * 1024 {
                    return Err(error(
                        "I_LIMIT",
                        e.span,
                        "static initializer image exceeds the 64 MiB per-compilation limit",
                    ));
                }
                let mut stored = Vec::new();
                stored.try_reserve_exact(length).map_err(|_| {
                    error(
                        "I_LIMIT",
                        e.span,
                        "host memory unavailable for static string image",
                    )
                })?;
                if bytes.is_empty() {
                    stored.push(0)
                } else {
                    stored.extend_from_slice(bytes)
                }
                let symbol = format!("__biomiya_const_string_{}", self.data.len());
                self.data.push(Data {
                    symbol: symbol.clone(),
                    bytes: stored,
                    relocations: vec![],
                    align: 1,
                    writable: false,
                    exported: false,
                    section: None,
                });
                data.relocations.push(crate::ir::Relocation {
                    offset,
                    symbol,
                    addend: 0,
                });
                data.bytes[offset as usize + 8..offset as usize + 16]
                    .copy_from_slice(&(bytes.len() as u64).to_le_bytes());
            }
            h::ExprKind::Array(v) => {
                let TypeKind::Array(elem, _) = self.types.kind(e.ty).clone() else {
                    unreachable!()
                };
                let size = crate::layout::LayoutEngine::new(&mut self.types)
                    .layout(elem)?
                    .size;
                for (i, v) in v.iter().enumerate() {
                    self.serialize(v, offset + size * i as u64, data)?
                }
            }
            h::ExprKind::Repeat { value, length } => {
                let size = crate::layout::LayoutEngine::new(&mut self.types)
                    .layout(value.ty)?
                    .size;
                if size != 0 {
                    for i in 0..*length {
                        self.serialize(value, offset + i * size, data)?
                    }
                }
            }
            h::ExprKind::Aggregate { variant, fields } => {
                let offsets = if let Some(v) = variant {
                    let var = &layout.variants[*v];
                    var.fields
                        .iter()
                        .map(|f| layout.payload_offset.unwrap_or(0) + *f)
                        .collect::<Vec<_>>()
                } else {
                    layout.fields.to_vec()
                };
                if let Some(v) = variant {
                    data.bytes[offset as usize..offset as usize + 4]
                        .copy_from_slice(&(*v as u32).to_le_bytes())
                }
                for (i, e) in fields {
                    self.serialize(e, offset + offsets[*i], data)?
                }
            }
            h::ExprKind::Slice { base, start, end } => {
                let start = self.eval(start)?;
                let end = self.eval(end)?;
                let TypeKind::Array(element, len) = self.types.kind(base.ty).clone() else {
                    return Err(error(
                        "E_CONST",
                        e.span,
                        "constant slice requires static array",
                    ));
                };
                if start > end || end > len {
                    return Err(error("E_CONST", e.span, "constant slice out of bounds"));
                }
                let size = crate::layout::LayoutEngine::new(&mut self.types)
                    .layout(element)?
                    .size;
                let (symbol, addend) = self.address(base)?;
                data.relocations.push(crate::ir::Relocation {
                    offset,
                    symbol,
                    addend: (addend + start * size) as i64,
                });
                data.bytes[offset as usize + 8..offset as usize + 16]
                    .copy_from_slice(&(end - start).to_le_bytes());
            }
            h::ExprKind::Borrow { value, .. } | h::ExprKind::RawAddress { value, .. } => {
                let (symbol, addend) = self.address(value)?;
                data.relocations.push(crate::ir::Relocation {
                    offset,
                    symbol,
                    addend: addend as i64,
                })
            }
            h::ExprKind::Block { tail: Some(t), .. }
            | h::ExprKind::Cast(t)
            | h::ExprKind::Adapt(t) => self.serialize(t, offset, data)?,
            h::ExprKind::If {
                condition,
                then,
                else_,
            } => {
                if self.eval(condition)? != 0 {
                    self.serialize(then, offset, data)?
                } else if let Some(e) = else_ {
                    self.serialize(e, offset, data)?
                }
            }
            _ => {
                let n = self.eval(e)?.to_le_bytes();
                data.bytes[offset as usize..offset as usize + layout.size as usize]
                    .copy_from_slice(&n[..layout.size as usize])
            }
        }
        Ok(())
    }
    fn address(&mut self, e: &h::Expr) -> Result<(String, u64)> {
        match &e.kind {
            h::ExprKind::Global { symbol, .. } => Ok((symbol.clone(), 0)),
            h::ExprKind::Field { base, field } => {
                let (s, n) = self.address(base)?;
                self.prepare_type(base.ty)?;
                let l = crate::layout::LayoutEngine::new(&mut self.types).layout(base.ty)?;
                Ok((s, n + l.fields[*field]))
            }
            h::ExprKind::Index { base, index } => {
                let (s, n) = self.address(base)?;
                let i = self.eval(index)?;
                let TypeKind::Array(t, len) = self.types.kind(base.ty).clone() else {
                    return Err(error(
                        "E_CONST",
                        e.span,
                        "constant index address requires array",
                    ));
                };
                if i >= len {
                    return Err(error(
                        "E_CONST",
                        e.span,
                        "constant address index out of bounds",
                    ));
                }
                let size = crate::layout::LayoutEngine::new(&mut self.types)
                    .layout(t)?
                    .size;
                Ok((s, n + i * size))
            }
            _ => Err(error("E_CONST", e.span, "invalid constant address")),
        }
    }
}
fn mask(bits: u32) -> u64 {
    if bits == 64 {
        u64::MAX
    } else {
        (1 << bits) - 1
    }
}
fn sign(n: u64, bits: u32) -> i128 {
    if bits == 64 {
        n as i64 as i128
    } else {
        ((n as i64) << (64 - bits) >> (64 - bits)) as i128
    }
}
