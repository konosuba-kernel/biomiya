use super::*;
impl Checker {
    pub(super) fn intrinsic(
        &mut self,
        canonical: &str,
        p: &[ast::PathSegment],
        args: &[ast::Expr],
        expected: Option<TyId>,
        span: Span,
        ctx: &mut Context,
    ) -> Result<h::Expr> {
        let name = canonical.strip_prefix("core::").unwrap_or(canonical);
        let wrapper = name.starts_with("Atomic::")
            || name.starts_with("UnsafeCell::")
            || name.starts_with("MaybeUninit::");
        for (i, seg) in p.iter().enumerate() {
            if !seg.arguments.is_empty() {
                let valid = if wrapper {
                    i + 2 == p.len()
                } else {
                    i + 1 == p.len()
                };
                if !valid {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "type arguments attached to nongeneric path segment",
                    ));
                }
            }
        }
        let generic = [
            "replace",
            "swap",
            "split_at_mut",
            "forget",
            "volatile_load",
            "volatile_store",
            "mmio_load",
            "mmio_store",
            "slice_from_raw_parts",
            "slice_from_raw_parts_mut",
            "checked_cast",
            "ptr",
        ]
        .contains(&name)
            || wrapper;
        let written = p.iter().map(|p| p.arguments.len()).sum::<usize>();
        if !generic && written != 0 {
            return Err(error("E_TYPE", span, "intrinsic takes no type arguments"));
        }
        if generic && written > if name == "ptr" { 2 } else { 1 } {
            return Err(error(
                "E_TYPE",
                span,
                "intrinsic type argument count mismatch",
            ));
        }
        let mut explicit = vec![];
        for seg in p {
            for (i, t) in seg.arguments.iter().enumerate() {
                if name == "ptr" && i == 1 {
                    continue;
                }
                explicit.push(self.ty(t, ctx)?)
            }
        }
        let known_payload = explicit.first().copied().or_else(|| {
            expected.and_then(|t| match (name, self.types.kind(t)) {
                ("volatile_load" | "mmio_load" | "replace", _) => Some(t),
                (
                    "slice_from_raw_parts" | "slice_from_raw_parts_mut",
                    TypeKind::Slice { element, .. },
                ) => Some(*element),
                ("split_at_mut", TypeKind::Tuple(v)) if v.len() == 2 => match self.types.kind(v[0])
                {
                    TypeKind::Slice { element, .. } => Some(*element),
                    _ => None,
                },
                _ => None,
            })
        });
        let count = |n: usize| {
            if args.len() == n {
                Ok(())
            } else {
                Err(error("E_TYPE", span, "intrinsic argument count mismatch"))
            }
        };
        let unsaf = |ctx: &Context| {
            if ctx.unsafe_depth > 0 {
                Ok(())
            } else {
                Err(error("E_UNSAFE", span, "intrinsic requires unsafe block"))
            }
        };
        let mut arguments = vec![];
        let mut type_arguments = explicit.clone();
        let mut output_name = name.to_string();
        let ty = match name {
            "addr_of" | "addr_of_mut" => {
                count(1)?;
                if !explicit.is_empty() {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "raw address does not accept type arguments",
                    ));
                }
                let old = ctx.place_context;
                ctx.place_context = true;
                let value = self.expr(&args[0], None, ctx)?;
                ctx.place_context = old;
                let mutable = name == "addr_of_mut";
                self.place(&value, mutable, ctx)?;
                let ty = self.types.intern(TypeKind::Pointer {
                    target: value.ty,
                    space: AddressSpace::Kernel,
                    const_: !mutable,
                });
                return Ok(h::Expr {
                    ty,
                    kind: h::ExprKind::RawAddress {
                        value: Box::new(value),
                        mutable,
                    },
                    span,
                });
            }
            "panic" => {
                count(1)?;
                let u8 = self.int(Integer::U8);
                let slice = self.types.intern(TypeKind::Slice {
                    element: u8,
                    const_: true,
                });
                arguments.push(self.expr(&args[0], Some(slice), ctx)?);
                self.never()
            }
            "forget" => {
                count(1)?;
                unsaf(ctx)?;
                arguments.push(self.expr(&args[0], explicit.first().copied(), ctx)?);
                self.unit()
            }
            "addr" => {
                count(1)?;
                let p = self.expr(&args[0], None, ctx)?;
                if !matches!(self.types.kind(p.ty), TypeKind::Pointer { .. }) {
                    return Err(error("E_TYPE", span, "addr requires raw pointer"));
                }
                arguments.push(p);
                self.int(Integer::Usize)
            }
            "ptr" => {
                count(1)?;
                unsaf(ctx)?;
                if explicit.len() != 1 {
                    return Err(error("E_TYPE", span, "ptr requires explicit target type"));
                }
                let u = self.int(Integer::Usize);
                arguments.push(self.expr(&args[0], Some(u), ctx)?);
                let space = match p.iter().flat_map(|s| s.arguments.iter()).nth(1) {
                    None => "kernel",
                    Some(t) => match &t.kind {
                        ast::TypeKind::Path(p) if p.len() == 1 && p[0].arguments.is_empty() => {
                            p[0].name.as_str()
                        }
                        _ => {
                            return Err(error(
                                "E_TYPE",
                                span,
                                "pointer address space requires a bare space name",
                            ));
                        }
                    },
                };
                let space = match space {
                    "kernel" => AddressSpace::Kernel,
                    "user" => AddressSpace::User,
                    "phys" => AddressSpace::Phys,
                    "mmio" => AddressSpace::Mmio,
                    _ => return Err(error("E_TYPE", span, "invalid pointer address space")),
                };
                self.types.intern(TypeKind::Pointer {
                    target: explicit[0],
                    space,
                    const_: false,
                })
            }
            "ptr_add" | "ptr_add_bytes" => {
                count(2)?;
                unsaf(ctx)?;
                let p = self.expr(&args[0], None, ctx)?;
                let TypeKind::Pointer { target, .. } = self.types.kind(p.ty) else {
                    return Err(error("E_TYPE", span, "pointer offset requires pointer"));
                };
                type_arguments = vec![*target];
                let ty = p.ty;
                arguments.push(p);
                let u = self.int(Integer::Usize);
                arguments.push(self.expr(&args[1], Some(u), ctx)?);
                ty
            }
            "slice_from_raw_parts" | "slice_from_raw_parts_mut" => {
                count(2)?;
                unsaf(ctx)?;
                let want = known_payload.map(|target| {
                    self.types.intern(TypeKind::Pointer {
                        target,
                        space: if name.starts_with("mmio") {
                            AddressSpace::Mmio
                        } else {
                            AddressSpace::Kernel
                        },
                        const_: !name.ends_with("_mut") && !name.ends_with("store"),
                    })
                });
                let p = self.expr(&args[0], want, ctx)?;
                let TypeKind::Pointer {
                    target,
                    space,
                    const_,
                } = self.types.kind(p.ty).clone()
                else {
                    return Err(error("E_TYPE", span, "raw slice requires pointer"));
                };
                if space != AddressSpace::Kernel || name.ends_with("_mut") && const_ {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "raw slice requires compatible kernel pointer",
                    ));
                }
                if !explicit.is_empty() && explicit != vec![target] {
                    return Err(error("E_TYPE", span, "raw slice type argument mismatch"));
                }
                type_arguments = vec![target];
                arguments.push(p);
                let u = self.int(Integer::Usize);
                arguments.push(self.expr(&args[1], Some(u), ctx)?);
                self.types.intern(TypeKind::Slice {
                    element: target,
                    const_: !name.ends_with("_mut"),
                })
            }
            "volatile_load" | "mmio_load" | "volatile_store" | "mmio_store" => {
                let store = name.ends_with("store");
                count(if store { 2 } else { 1 })?;
                unsaf(ctx)?;
                let want = known_payload.map(|target| {
                    self.types.intern(TypeKind::Pointer {
                        target,
                        space: if name.starts_with("mmio") {
                            AddressSpace::Mmio
                        } else {
                            AddressSpace::Kernel
                        },
                        const_: !name.ends_with("_mut") && !name.ends_with("store"),
                    })
                });
                let p = self.expr(&args[0], want, ctx)?;
                let TypeKind::Pointer {
                    target,
                    space,
                    const_,
                } = self.types.kind(p.ty).clone()
                else {
                    return Err(error("E_TYPE", span, "load/store requires pointer"));
                };
                if !matches!(self.types.kind(target), TypeKind::Int(_))
                    || store && const_
                    || name.starts_with("mmio") && space != AddressSpace::Mmio
                    || name.starts_with("volatile") && space != AddressSpace::Kernel
                {
                    return Err(error("E_TYPE", span, "invalid load/store pointer type"));
                }
                arguments.push(p);
                if store {
                    arguments.push(self.expr(&args[1], Some(target), ctx)?)
                }
                type_arguments = vec![target];
                if store { self.unit() } else { target }
            }
            "checked_cast" => {
                count(1)?;
                if explicit.len() != 1 || !matches!(self.types.kind(explicit[0]), TypeKind::Int(_))
                {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "checked_cast requires target integer type",
                    ));
                }
                let v = self.expr(&args[0], None, ctx)?;
                if !matches!(self.types.kind(v.ty), TypeKind::Int(_)) {
                    return Err(error("E_TYPE", span, "checked_cast source must be integer"));
                }
                arguments.push(v);
                self.types.intern(TypeKind::Option(explicit[0]))
            }
            "replace" | "swap" => {
                count(2)?;
                let want = known_payload.map(|target| {
                    self.types.intern(TypeKind::Ref {
                        target,
                        mutable: true,
                    })
                });
                let first = self.expr(&args[0], want, ctx)?;
                let TypeKind::Ref {
                    target,
                    mutable: true,
                } = self.types.kind(first.ty).clone()
                else {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "operation requires mutable reference",
                    ));
                };
                let ty = first.ty;
                arguments.push(first);
                arguments.push(self.expr(
                    &args[1],
                    Some(if name == "swap" { ty } else { target }),
                    ctx,
                )?);
                type_arguments = vec![target];
                if name == "swap" { self.unit() } else { target }
            }
            "split_at_mut" => {
                count(2)?;
                let want = known_payload.map(|element| {
                    self.types.intern(TypeKind::Slice {
                        element,
                        const_: false,
                    })
                });
                let v = self.expr(&args[0], want, ctx)?;
                if !matches!(self.types.kind(v.ty), TypeKind::Slice { const_: false, .. }) {
                    return Err(error("E_TYPE", span, "split_at_mut requires mutable slice"));
                }
                let ty = v.ty;
                arguments.push(v);
                let u = self.int(Integer::Usize);
                arguments.push(self.expr(&args[1], Some(u), ctx)?);
                self.types.intern(TypeKind::Tuple(vec![ty, ty]))
            }
            "Atomic::new" | "UnsafeCell::new" => {
                count(1)?;
                let inner = explicit.first().copied().or_else(|| {
                    expected.and_then(|t| match self.types.kind(t) {
                        TypeKind::Atomic(t) | TypeKind::UnsafeCell(t) => Some(*t),
                        _ => None,
                    })
                });
                let v = self.expr(&args[0], inner, ctx)?;
                let t = v.ty;
                if name.starts_with("Atomic")
                    && !matches!(
                        self.types.kind(t),
                        TypeKind::Int(
                            Integer::U8
                                | Integer::U16
                                | Integer::U32
                                | Integer::U64
                                | Integer::Usize
                        ) | TypeKind::Pointer {
                            space: AddressSpace::Kernel,
                            ..
                        }
                    )
                {
                    return Err(error("E_TYPE", span, "unsupported Atomic element type"));
                }
                type_arguments = vec![t];
                arguments.push(v);
                output_name = if name.starts_with("Atomic") {
                    "atomic.new".into()
                } else {
                    "UnsafeCell.new".into()
                };
                self.types.intern(if name.starts_with("Atomic") {
                    TypeKind::Atomic(t)
                } else {
                    TypeKind::UnsafeCell(t)
                })
            }
            "MaybeUninit::uninit" => {
                count(0)?;
                let t = explicit
                    .first()
                    .copied()
                    .or_else(|| {
                        expected.and_then(|t| {
                            if let TypeKind::MaybeUninit(t) = self.types.kind(t) {
                                Some(*t)
                            } else {
                                None
                            }
                        })
                    })
                    .ok_or_else(|| error("E_TYPE", span, "uninit requires type context"))?;
                return Ok(h::Expr {
                    ty: self.types.intern(TypeKind::MaybeUninit(t)),
                    kind: h::ExprKind::Uninit,
                    span,
                });
            }
            "arch::enter_trap_handler" | "arch::enter_irq_handler" => {
                count(2)?;
                if ctx.special.as_deref() != Some("trap_entry") || ctx.in_defer {
                    return Err(error(
                        "E_CONTEXT",
                        span,
                        "special transfer requires trap_entry context",
                    ));
                }
                let trap = self
                    .types
                    .intern(TypeKind::Nominal(DefId(0xffff_ff01), vec![]));
                let r = self.types.intern(TypeKind::Ref {
                    target: trap,
                    mutable: true,
                });
                let frame = self.expr(&args[0], Some(r), ctx)?;
                let ast::ExprKind::Path(p) = &args[1].kind else {
                    return Err(error("E_CONTEXT", span, "handler must be named function"));
                };
                let path = p.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
                let id = self.resolver.lookup_visible(
                    &ctx.module,
                    &path,
                    Namespace::Value,
                    args[1].span,
                )?;
                let rec = self.resolver.declarations[&id].clone();
                let irq = name.ends_with("irq_handler");
                if !rec
                    .decl
                    .attributes
                    .iter()
                    .any(|a| a.name == if irq { "irq" } else { "trap" })
                {
                    return Err(error("E_CONTEXT", span, "wrong handler context"));
                }
                let handler = self.instance(id, vec![])?;
                return Ok(h::Expr {
                    ty: self.never(),
                    kind: h::ExprKind::SpecialTransfer {
                        frame: Box::new(frame),
                        handler,
                        irq,
                    },
                    span,
                });
            }
            "arch::riscv::cpu_relax" | "arch::device_read_fence" | "arch::device_write_fence" => {
                count(0)?;
                self.unit()
            }
            "arch::riscv::wfi" | "arch::riscv::fence_i" | "arch::riscv::sfence_vma_all" => {
                count(0)?;
                unsaf(ctx)?;
                self.unit()
            }
            "arch::riscv::csr_read" | "arch::riscv::csr_write" => {
                count(if name.ends_with("write") { 2 } else { 1 })?;
                unsaf(ctx)?;
                let u = self.int(Integer::Usize);
                let csr = self.expr(&args[0], Some(u), ctx)?;
                let number = self.eval(&csr)?;
                if ![
                    0x100, 0x104, 0x105, 0x140, 0x141, 0x142, 0x143, 0x144, 0x180,
                ]
                .contains(&number)
                {
                    return Err(error(
                        "E_TARGET",
                        span,
                        "CSR is outside the RV64 Core whitelist",
                    ));
                }
                arguments.push(h::Expr {
                    ty: u,
                    span: csr.span,
                    kind: h::ExprKind::Integer(number),
                });
                if name.ends_with("write") {
                    arguments.push(self.expr(&args[1], Some(u), ctx)?);
                    self.unit()
                } else {
                    u
                }
            }
            _ => {
                return Err(error(
                    "I_UNSUPPORTED",
                    span,
                    format!("intrinsic {canonical} not implemented"),
                ));
            }
        };
        if let Some(t) = expected {
            self.coerce(ty, t, span)?
        }
        Ok(h::Expr {
            ty,
            kind: h::ExprKind::Intrinsic {
                name: output_name,
                arguments,
                type_arguments,
            },
            span,
        })
    }
    pub(super) fn method(
        &mut self,
        base: &ast::Expr,
        name: &str,
        typeargs: &[ast::Type],
        site: CallSite<'_>,
        ctx: &mut Context,
    ) -> Result<h::Expr> {
        let CallSite {
            arguments: args,
            expected,
            span,
        } = site;
        let old = ctx.place_context;
        ctx.place_context = true;
        let base = self.expr(base, None, ctx)?;
        ctx.place_context = old;
        let base = self.autoderef(base);
        let shared_receiver = self.readonly_chain(&base);
        let k = self.types.kind(base.ty).clone();
        if let TypeKind::Nominal(id, nominalargs) = k.clone()
            && let Some(method) = self.resolver.methods.get(&(id, name.into())).copied()
        {
            let r = self.resolver.declarations[&method].clone();
            if ctx.module != r.module && !r.decl.public {
                return Err(error("E_NAME", span, "private method"));
            }
            let ast::DeclKind::Function(f) = r.decl.kind else {
                unreachable!()
            };
            let receiver = f
                .parameters
                .first()
                .and_then(|p| p.receiver.as_deref())
                .ok_or_else(|| error("E_TYPE", span, "associated function requires type path"))?;
            let recv = if receiver.starts_with('&') {
                let mutable = receiver.contains("mut");
                if matches!(
                    base.kind,
                    h::ExprKind::Local(_)
                        | h::ExprKind::Global { .. }
                        | h::ExprKind::Deref(_)
                        | h::ExprKind::Field { .. }
                        | h::ExprKind::Index { .. }
                ) {
                    self.place(&base, mutable, ctx)?;
                }
                h::Expr {
                    ty: self.types.intern(TypeKind::Ref {
                        target: base.ty,
                        mutable,
                    }),
                    span: base.span,
                    kind: h::ExprKind::Borrow {
                        value: Box::new(base),
                        mutable,
                        reborrow: true,
                    },
                }
            } else {
                if shared_receiver && !self.types.properties(base.ty).copy {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "by-value method cannot move through shared reference",
                    ));
                }
                base
            };
            let mut explicit = nominalargs;
            explicit.extend(
                typeargs
                    .iter()
                    .map(|t| self.ty(t, ctx))
                    .collect::<Result<Vec<_>>>()?,
            );
            return self.function_call(method, &f, explicit, Some(recv), site, ctx);
        }
        if !typeargs.is_empty() {
            return Err(error(
                "E_TYPE",
                span,
                "builtin method takes no method type arguments",
            ));
        }
        let mut arguments = vec![];
        let mut payload = vec![];
        let output;
        let ty = match k {
            TypeKind::Array(element, _) | TypeKind::Slice { element, .. } => {
                let constant = match k {
                    TypeKind::Slice { const_, .. } => const_,
                    _ => self.place(&base, true, ctx).is_err(),
                };
                match name {
                    "as_ptr" | "as_mut_ptr" => {
                        if !args.is_empty() {
                            return Err(error(
                                "E_TYPE",
                                span,
                                "slice pointer method takes no arguments",
                            ));
                        }
                        if name == "as_mut_ptr" && constant {
                            return Err(error(
                                "E_TYPE",
                                span,
                                "readonly view has no mutable pointer method",
                            ));
                        }
                        payload = vec![element];
                        arguments.push(base);
                        output = if name == "as_ptr" {
                            "slice.as_ptr"
                        } else {
                            "slice.as_mut_ptr"
                        };
                        self.types.intern(TypeKind::Pointer {
                            target: element,
                            space: AddressSpace::Kernel,
                            const_: name == "as_ptr",
                        })
                    }
                    _ => return Err(error("E_NAME", span, "unknown slice method")),
                }
            }
            TypeKind::MaybeUninit(inner) => {
                payload = vec![inner];
                match name {
                    "write" | "as_ptr" | "as_mut_ptr" => {
                        let mutable = name != "as_ptr";
                        self.place(&base, mutable, ctx)?;
                        let ty = self.types.intern(TypeKind::Ref {
                            target: base.ty,
                            mutable,
                        });
                        arguments.push(h::Expr {
                            ty,
                            span: base.span,
                            kind: h::ExprKind::Borrow {
                                value: Box::new(base),
                                mutable,
                                reborrow: true,
                            },
                        });
                        if name == "write" {
                            if args.len() != 1 {
                                return Err(error("E_TYPE", span, "write takes one argument"));
                            }
                            arguments.push(self.expr(&args[0], Some(inner), ctx)?);
                            output = "MaybeUninit.write";
                            self.unit()
                        } else {
                            if !args.is_empty() {
                                return Err(error(
                                    "E_TYPE",
                                    span,
                                    "pointer method takes no arguments",
                                ));
                            }
                            output = if mutable {
                                "MaybeUninit.as_mut_ptr"
                            } else {
                                "MaybeUninit.as_ptr"
                            };
                            self.types.intern(TypeKind::Pointer {
                                target: inner,
                                space: AddressSpace::Kernel,
                                const_: !mutable,
                            })
                        }
                    }
                    "assume_init" => {
                        if ctx.unsafe_depth == 0 {
                            return Err(error(
                                "E_UNSAFE",
                                span,
                                "assume_init requires unsafe block",
                            ));
                        }
                        if !args.is_empty() {
                            return Err(error("E_TYPE", span, "assume_init takes no arguments"));
                        }
                        arguments.push(base);
                        output = "MaybeUninit.assume_init";
                        inner
                    }
                    _ => return Err(error("E_NAME", span, "unknown MaybeUninit method")),
                }
            }
            TypeKind::UnsafeCell(inner) => {
                if name != "get" || !args.is_empty() {
                    return Err(error("E_NAME", span, "unknown UnsafeCell method"));
                }
                self.place(&base, false, ctx)?;
                let r = self.types.intern(TypeKind::Ref {
                    target: base.ty,
                    mutable: false,
                });
                arguments.push(h::Expr {
                    ty: r,
                    span: base.span,
                    kind: h::ExprKind::Borrow {
                        value: Box::new(base),
                        mutable: false,
                        reborrow: true,
                    },
                });
                payload = vec![inner];
                output = "UnsafeCell.get";
                self.types.intern(TypeKind::Pointer {
                    target: inner,
                    space: AddressSpace::Kernel,
                    const_: false,
                })
            }
            TypeKind::Atomic(inner) => {
                if name.starts_with("fetch_")
                    && matches!(self.types.kind(inner), TypeKind::Pointer { .. })
                {
                    return Err(error(
                        "E_TYPE",
                        span,
                        "pointer atomics do not support fetch operations",
                    ));
                }

                self.place(&base, false, ctx)?;
                let r = self.types.intern(TypeKind::Ref {
                    target: base.ty,
                    mutable: false,
                });
                arguments.push(h::Expr {
                    ty: r,
                    span: base.span,
                    kind: h::ExprKind::Borrow {
                        value: Box::new(base),
                        mutable: false,
                        reborrow: true,
                    },
                });
                payload = vec![inner];
                let value_count = match name {
                    "load" => 0,
                    "store" | "exchange" | "fetch_add" | "fetch_sub" | "fetch_and" | "fetch_or" => {
                        1
                    }
                    "compare_exchange" => 2,
                    _ => return Err(error("E_NAME", span, "unknown Atomic method")),
                };
                let order_count = if name == "compare_exchange" { 2 } else { 1 };
                if args.len() != value_count + order_count {
                    return Err(error("E_TYPE", span, "atomic argument count mismatch"));
                }
                for a in &args[..value_count] {
                    arguments.push(self.expr(a, Some(inner), ctx)?)
                }
                let orderty = self
                    .types
                    .intern(TypeKind::Nominal(DefId(0xffff_ff00), vec![]));
                let mut orders = vec![];
                for a in &args[value_count..] {
                    let e = self.expr(a, Some(orderty), ctx)?;
                    let h::ExprKind::Aggregate {
                        variant: Some(v), ..
                    } = e.kind
                    else {
                        return Err(error("E_CONST", a.span, "atomic order must be constant"));
                    };
                    orders.push(v);
                    arguments.push(h::Expr {
                        ty: self.int(Integer::Usize),
                        kind: h::ExprKind::Integer(v as u64),
                        span: a.span,
                    })
                }
                if name == "load" && [2, 3].contains(&orders[0])
                    || name == "store" && [1, 3].contains(&orders[0])
                    || name == "compare_exchange" && [2, 3].contains(&orders[1])
                {
                    return Err(error("E_CONST", span, "invalid atomic memory order"));
                }
                if name == "compare_exchange" {
                    let valid = match orders[0] {
                        0 => orders[1] == 0,
                        1 => [0, 1].contains(&orders[1]),
                        2 => orders[1] == 0,
                        3 => [0, 1].contains(&orders[1]),
                        4 => [0, 1, 4].contains(&orders[1]),
                        _ => false,
                    };
                    if !valid {
                        return Err(error(
                            "E_CONST",
                            span,
                            "failure order stronger than success",
                        ));
                    }
                }
                output = match name {
                    "load" => "atomic.load",
                    "store" => "atomic.store",
                    "exchange" => "atomic.exchange",
                    "compare_exchange" => "atomic.compare_exchange",
                    "fetch_add" => "atomic.fetch_add",
                    "fetch_sub" => "atomic.fetch_sub",
                    "fetch_and" => "atomic.fetch_and",
                    _ => "atomic.fetch_or",
                };
                if name == "store" {
                    self.unit()
                } else if name == "compare_exchange" {
                    self.types.intern(TypeKind::Result(inner, inner))
                } else {
                    inner
                }
            }
            _ => return Err(error("E_NAME", span, "unknown method")),
        };
        if let Some(e) = expected {
            self.coerce(ty, e, span)?
        }
        Ok(h::Expr {
            ty,
            kind: h::ExprKind::Intrinsic {
                name: output.into(),
                arguments,
                type_arguments: payload,
            },
            span,
        })
    }
}
