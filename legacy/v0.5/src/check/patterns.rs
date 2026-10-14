use super::*;
type PatternCase = (Option<usize>, Option<u64>, Vec<TyId>);
impl Checker {
    pub(super) fn pattern(
        &mut self,
        p: &ast::Pattern,
        ty: TyId,
        mutable: bool,
        refutable: bool,
        ctx: &mut Context,
    ) -> Result<h::Pattern> {
        let span = p.span;
        match &p.kind {
            ast::PatternKind::Record { path, .. } | ast::PatternKind::Payload { path, .. } => {
                self.pattern_constructor(path, ty, span, ctx)?
            }
            ast::PatternKind::Path(path) if path.len() > 1 => {
                self.pattern_constructor(path, ty, span, ctx)?
            }
            _ => {}
        }
        let kind = match &p.kind {
            ast::PatternKind::Wildcard => h::PatternKind::Wildcard,
            ast::PatternKind::Path(path) => {
                let name = path.last().unwrap();
                if let Some(variant) = self.variant(ty, name) {
                    let fields = self.fields(ty, Some(variant), span)?;
                    if !fields.is_empty() {
                        return Err(error(
                            "E_PATTERN",
                            span,
                            "payload variant requires payload pattern",
                        ));
                    }
                    if !refutable && self.constructor_count(ty) != 1 {
                        return Err(error("E_PATTERN", span, "refutable binding pattern"));
                    }
                    h::PatternKind::Fields {
                        variant: Some(variant),
                        fields: vec![],
                    }
                } else if path.len() == 1 {
                    if let Some(borrow_mutable) = ctx.pattern_borrow {
                        let bound_ty = self.types.intern(TypeKind::Ref {
                            target: ty,
                            mutable: borrow_mutable,
                        });
                        h::PatternKind::BorrowBind {
                            local: self.bind(name, bound_ty, mutable, span, ctx)?,
                            mutable: borrow_mutable,
                        }
                    } else {
                        h::PatternKind::Bind(self.bind(name, ty, mutable, span, ctx)?)
                    }
                } else {
                    return Err(error("E_PATTERN", span, "unknown pattern path"));
                }
            }
            ast::PatternKind::Bool(b) => {
                if self.types.kind(ty) != &TypeKind::Bool {
                    return Err(error("E_PATTERN", span, "boolean pattern type mismatch"));
                }
                if !refutable {
                    return Err(error("E_PATTERN", span, "refutable binding pattern"));
                }
                h::PatternKind::Integer(*b as u64)
            }
            ast::PatternKind::Integer {
                value,
                negative,
                adjacent,
            } => {
                if !refutable {
                    return Err(error("E_PATTERN", span, "refutable binding pattern"));
                }
                let (mut n, suffix) = super::expressions::literal(value, span)?;
                if let Some(i) = suffix
                    && self.types.kind(ty) != &TypeKind::Int(i)
                {
                    return Err(error("E_PATTERN", span, "integer pattern suffix mismatch"));
                }
                if *negative && !matches!(self.types.kind(ty),TypeKind::Int(i) if i.signed()) {
                    return Err(error(
                        "E_PATTERN",
                        span,
                        "negative pattern requires signed integer",
                    ));
                }
                self.literal_fit(n, ty, *negative && *adjacent, span)
                    .map_err(|d| error("E_PATTERN", span, d.message))?;
                if *negative {
                    n = 0u64.wrapping_sub(n);
                    if let TypeKind::Int(i) = self.types.kind(ty)
                        && i.bits() < 64
                    {
                        n &= (1 << i.bits()) - 1
                    }
                }
                h::PatternKind::Integer(n)
            }
            ast::PatternKind::Tuple(v) => {
                let tys = match self.types.kind(ty) {
                    TypeKind::Tuple(t) => t.clone(),
                    TypeKind::Unit => vec![],
                    _ => return Err(error("E_PATTERN", span, "tuple pattern on non-tuple")),
                };
                if tys.len() != v.len() {
                    return Err(error("E_PATTERN", span, "tuple pattern arity mismatch"));
                }
                let mut fields = vec![];
                for (i, (p, t)) in v.iter().zip(tys).enumerate() {
                    fields.push((i, self.pattern(p, t, mutable, refutable, ctx)?))
                }
                h::PatternKind::Fields {
                    variant: None,
                    fields,
                }
            }
            ast::PatternKind::Payload { path, value } => {
                let name = path.last().unwrap();
                let variant = self.variant(ty, name);
                if variant.is_none() {
                    if let TypeKind::Nominal(id, _) = self.types.kind(ty) {
                        if &self.types.definition(*id).unwrap().name != name {
                            return Err(error("E_PATTERN", span, "newtype name mismatch"));
                        }
                    } else {
                        return Err(error("E_PATTERN", span, "unknown payload constructor"));
                    }
                }
                if !refutable && variant.is_some() && self.constructor_count(ty) != 1 {
                    return Err(error("E_PATTERN", span, "refutable binding pattern"));
                }
                let fields = self.fields(ty, variant, span)?;
                if fields.len() != 1 || fields[0].name != "0" {
                    return Err(error(
                        "E_PATTERN",
                        span,
                        "constructor does not have single payload",
                    ));
                }
                h::PatternKind::Fields {
                    variant,
                    fields: vec![(
                        0,
                        self.pattern(value, fields[0].ty, mutable, refutable, ctx)?,
                    )],
                }
            }
            ast::PatternKind::Record {
                path,
                fields: written,
            } => {
                let variant = self.variant(ty, path.last().unwrap());
                if !refutable && variant.is_some() && self.constructor_count(ty) != 1 {
                    return Err(error("E_PATTERN", span, "refutable binding pattern"));
                }
                let fields = self.fields(ty, variant, span)?;
                let mut used = BTreeSet::new();
                let mut out = vec![];
                for (name, p) in written {
                    let i = fields
                        .iter()
                        .position(|f| &f.name == name)
                        .ok_or_else(|| error("E_PATTERN", p.span, "unknown pattern field"))?;
                    if !used.insert(i) {
                        return Err(error("E_PATTERN", p.span, "duplicate pattern field"));
                    }
                    out.push((i, self.pattern(p, fields[i].ty, mutable, refutable, ctx)?))
                }
                if used.len() != fields.len() {
                    return Err(error("E_PATTERN", span, "pattern must specify every field"));
                }
                h::PatternKind::Fields {
                    variant,
                    fields: out,
                }
            }
        };
        Ok(h::Pattern { ty, kind, span })
    }
    fn pattern_constructor(
        &mut self,
        path: &[String],
        ty: TyId,
        span: Span,
        ctx: &Context,
    ) -> Result<()> {
        let id = self
            .resolver
            .lookup(&ctx.module, path, Namespace::Value, span)?
            .or(self
                .resolver
                .lookup(&ctx.module, path, Namespace::Type, span)?);
        if let Some(id) = id {
            if !matches!(self.types.kind(ty),TypeKind::Nominal(expected,_) if *expected==id) {
                return Err(error(
                    "E_PATTERN",
                    span,
                    "pattern constructor nominal identity mismatch",
                ));
            }
            let r = &self.resolver.declarations[&id];
            if r.module != ctx.module
                && let ast::DeclKind::Struct {
                    fields, newtype, ..
                } = &r.decl.kind
                && (newtype.is_some() || fields.iter().any(|f| !f.public))
            {
                return Err(error("E_NAME", span, "pattern has private fields"));
            }
            return Ok(());
        }
        if path.len() == 1 && self.variant(ty, &path[0]).is_some() {
            return Ok(());
        }
        if let Some(b) = self
            .resolver
            .builtin_path(&ctx.module, path, Namespace::Value, span)?
        {
            let name = b.last().unwrap();
            if self.variant(ty, name).is_some() {
                return Ok(());
            }
            return Err(error("E_PATTERN", span, "builtin pattern type mismatch"));
        }
        Err(error("E_NAME", span, "unknown pattern constructor"))
    }
    fn constructor_count(&self, t: TyId) -> usize {
        match self.types.kind(t) {
            TypeKind::Option(_) | TypeKind::Result(..) | TypeKind::Bool => 2,
            TypeKind::Nominal(id, _) => match &self.types.definition(*id).unwrap().kind {
                DefKind::Enum(v) => v.len(),
                _ => 1,
            },
            _ => 1,
        }
    }
    pub(super) fn match_expr(
        &mut self,
        value: &ast::Expr,
        arms: &[(ast::Pattern, ast::Expr)],
        expected: Option<TyId>,
        span: Span,
        ctx: &mut Context,
    ) -> Result<h::Expr> {
        let mut value = self.expr(value, None, ctx)?;
        let old_borrow = ctx.pattern_borrow;
        if let TypeKind::Ref { target, mutable } = self.types.kind(value.ty).clone() {
            ctx.pattern_borrow = Some(mutable);
            let span = value.span;
            value = h::Expr {
                ty: target,
                span,
                kind: h::ExprKind::Deref(Box::new(value)),
            };
        }
        let mut typed = vec![];
        let mut result = expected;
        if result.is_none() {
            for (p, e) in arms {
                let mut trial = ctx.clone();
                trial.scopes.push(BTreeMap::new());
                self.pattern(p, value.ty, false, true, &mut trial)?;
                if let Some(t) = self.hint(e, &mut trial)? {
                    result = Some(t);
                    break;
                }
            }
        }
        for (p, e) in arms {
            ctx.scopes.push(BTreeMap::new());
            let p = self.pattern(p, value.ty, false, true, ctx)?;
            let e = self.expr(e, result, ctx)?;
            if !matches!(self.types.kind(e.ty), TypeKind::Never) {
                if let Some(t) = result {
                    self.coerce(e.ty, t, e.span)?
                } else {
                    result = Some(e.ty)
                }
            }
            ctx.scopes.pop();
            typed.push((p, e))
        }
        ctx.pattern_borrow = old_borrow;
        let matrix = typed
            .iter()
            .map(|(p, _)| vec![p.clone()])
            .collect::<Vec<_>>();
        if !self.exhaustive(&[value.ty], &matrix)? {
            return Err(error("E_PATTERN", span, "nonexhaustive match"));
        }
        Ok(h::Expr {
            ty: result.unwrap_or_else(|| self.never()),
            kind: h::ExprKind::Match {
                value: Box::new(value),
                arms: typed,
            },
            span,
        })
    }
    pub(super) fn exhaustive(&mut self, types: &[TyId], rows: &[Vec<h::Pattern>]) -> Result<bool> {
        if types.is_empty() {
            return Ok(!rows.is_empty());
        }
        if rows.is_empty() {
            return Ok(false);
        }
        let t = types[0];
        let wildcard = |p: &h::Pattern| {
            matches!(
                p.kind,
                h::PatternKind::Wildcard
                    | h::PatternKind::Bind(_)
                    | h::PatternKind::BorrowBind { .. }
            )
        };
        let constructors: Option<Vec<PatternCase>> = match self.types.kind(t).clone() {
            TypeKind::Bool => Some(vec![(None, Some(0), vec![]), (None, Some(1), vec![])]),
            TypeKind::Unit => Some(vec![(None, None, vec![])]),
            TypeKind::Tuple(v) => Some(vec![(None, None, v)]),
            TypeKind::Option(x) => Some(vec![(Some(0), None, vec![x]), (Some(1), None, vec![])]),
            TypeKind::Result(x, y) => {
                Some(vec![(Some(0), None, vec![x]), (Some(1), None, vec![y])])
            }
            TypeKind::Nominal(id, _) => {
                let count = match &self.types.definition(id).unwrap().kind {
                    DefKind::Enum(v) => v.len(),
                    _ => 1,
                };
                let isenum = matches!(self.types.definition(id).unwrap().kind, DefKind::Enum(_));
                let mut c = vec![];
                for i in 0..count {
                    let v = if isenum { Some(i) } else { None };
                    c.push((
                        v,
                        None,
                        self.fields(t, v, Span::default())?
                            .iter()
                            .map(|f| f.ty)
                            .collect(),
                    ))
                }
                Some(c)
            }
            _ => None,
        };
        if let Some(constructors) = constructors {
            for (variant, integer, fields) in constructors {
                let mut specialized = vec![];
                for row in rows {
                    let p = &row[0];
                    let mut front = vec![];
                    if wildcard(p) {
                        for ty in &fields {
                            front.push(h::Pattern {
                                ty: *ty,
                                kind: h::PatternKind::Wildcard,
                                span: p.span,
                            })
                        }
                    } else {
                        match &p.kind {
                            h::PatternKind::Integer(n) if integer == Some(*n) => {}
                            h::PatternKind::Fields {
                                variant: v,
                                fields: ps,
                            } if *v == variant && integer.is_none() => {
                                for (i, ty) in fields.iter().enumerate() {
                                    front.push(
                                        ps.iter()
                                            .find(|(j, _)| *j == i)
                                            .map(|(_, p)| p.clone())
                                            .unwrap_or(h::Pattern {
                                                ty: *ty,
                                                kind: h::PatternKind::Wildcard,
                                                span: p.span,
                                            }),
                                    )
                                }
                            }
                            _ => continue,
                        }
                    }
                    front.extend_from_slice(&row[1..]);
                    specialized.push(front)
                }
                let mut next = fields;
                next.extend_from_slice(&types[1..]);
                if !self.exhaustive(&next, &specialized)? {
                    return Ok(false);
                }
            }
            Ok(true)
        } else {
            let defaults = rows
                .iter()
                .filter(|r| wildcard(&r[0]))
                .map(|r| r[1..].to_vec())
                .collect::<Vec<_>>();
            self.exhaustive(&types[1..], &defaults)
        }
    }
}
