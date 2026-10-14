//! Interned semantic types. Effects remain part of identity even when Core accepts a conversion.
use crate::source::Span;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TyId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefId(pub u32);
/// Identity of an original constant AST retained by the semantic checker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConstExprId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Abi {
    Biomiya,
    C,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AddressSpace {
    Kernel,
    User,
    Phys,
    Mmio,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Integer {
    U8,
    U16,
    U32,
    U64,
    Usize,
    I8,
    I16,
    I32,
    I64,
    Isize,
}
impl Integer {
    pub fn bits(self) -> u32 {
        match self {
            Self::U8 | Self::I8 => 8,
            Self::U16 | Self::I16 => 16,
            Self::U32 | Self::I32 => 32,
            _ => 64,
        }
    }
    pub fn signed(self) -> bool {
        matches!(
            self,
            Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::Isize
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FnType {
    pub params: Vec<TyId>,
    pub result: TyId,
    pub abi: Abi,
    pub unsafe_: bool,
    pub effects: BTreeSet<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TypeKind {
    Unit,
    Never,
    Bool,
    Int(Integer),
    Param(u32),
    Range(TyId),
    Array(TyId, u64),
    ArrayExpr(TyId, ConstLen),
    Tuple(Vec<TyId>),
    Ref {
        target: TyId,
        mutable: bool,
    },
    Pointer {
        target: TyId,
        space: AddressSpace,
        const_: bool,
    },
    Slice {
        element: TyId,
        const_: bool,
    },
    Function(FnType),
    Nominal(DefId, Vec<TyId>),
    Option(TyId),
    Result(TyId, TyId),
    MaybeUninit(TyId),
    UnsafeCell(TyId),
    Atomic(TyId),
}
#[derive(Clone, Debug)]
pub struct FieldDef {
    pub name: String,
    pub ty: TyId,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct VariantDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
}
#[derive(Clone, Debug)]
pub enum DefKind {
    Struct(Vec<FieldDef>),
    Enum(Vec<VariantDef>),
}
#[derive(Clone, Debug)]
pub struct TypeDef {
    pub name: String,
    pub params: u32,
    pub kind: DefKind,
    pub copy: bool,
    pub resource: bool,
    pub repr_c: bool,
    pub packed: bool,
    pub align: Option<u64>,
    pub span: Span,
    /// Types queried by field layout constants, including unselected constant branches.
    pub layout_dependencies: Vec<TyId>,
}
impl TypeDef {
    pub fn fields(&self) -> Vec<&FieldDef> {
        match &self.kind {
            DefKind::Struct(f) => f.iter().collect(),
            DefKind::Enum(v) => v.iter().flat_map(|v| v.fields.iter()).collect(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Properties {
    pub copy: bool,
    pub resource: bool,
    pub borrow_carrying: bool,
}
#[derive(Clone, Debug, Default)]
pub struct TypeContext {
    types: Vec<TypeKind>,
    interned: HashMap<TypeKind, TyId>,
    pub definitions: BTreeMap<DefId, TypeDef>,
    checked_constants: HashMap<(ConstExprId, Vec<TyId>), u64>,
}
impl TypeContext {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn intern(&mut self, k: TypeKind) -> TyId {
        if let Some(t) = self.interned.get(&k) {
            return *t;
        }
        let t = TyId(self.types.len() as u32);
        self.types.push(k.clone());
        self.interned.insert(k, t);
        t
    }
    pub fn kind(&self, t: TyId) -> &TypeKind {
        &self.types[t.0 as usize]
    }
    pub fn define(&mut self, id: DefId, d: TypeDef) {
        self.definitions.insert(id, d);
    }
    pub fn definition(&self, id: DefId) -> Option<&TypeDef> {
        self.definitions.get(&id)
    }
    /// Values enter this cache only after checking the original AST in its concrete context.
    pub fn cache_const(&mut self, id: ConstExprId, args: &[TyId], value: u64) {
        self.checked_constants.insert((id, args.to_vec()), value);
    }
    pub fn cached_const(&self, id: ConstExprId, args: &[TyId]) -> Option<u64> {
        self.checked_constants.get(&(id, args.to_vec())).copied()
    }
    pub fn substitute(&mut self, t: TyId, args: &[TyId]) -> TyId {
        let k = self.kind(t).clone();
        let k = match k {
            TypeKind::Param(n) => return args.get(n as usize).copied().unwrap_or(t),
            TypeKind::Range(x) => TypeKind::Range(self.substitute(x, args)),
            TypeKind::ArrayExpr(x, n) => {
                let element = self.substitute(x, args);
                let len = self.substitute_len(n, args);
                if let ConstLen {
                    integer: Integer::Usize,
                    kind: ConstLenKind::Literal(n),
                } = len
                {
                    TypeKind::Array(element, n)
                } else {
                    TypeKind::ArrayExpr(element, len)
                }
            }
            TypeKind::Array(x, n) => TypeKind::Array(self.substitute(x, args), n),
            TypeKind::Tuple(v) => {
                TypeKind::Tuple(v.into_iter().map(|x| self.substitute(x, args)).collect())
            }
            TypeKind::Nominal(d, v) => {
                TypeKind::Nominal(d, v.into_iter().map(|x| self.substitute(x, args)).collect())
            }
            TypeKind::Option(x) => TypeKind::Option(self.substitute(x, args)),
            TypeKind::Result(x, y) => {
                TypeKind::Result(self.substitute(x, args), self.substitute(y, args))
            }
            TypeKind::MaybeUninit(x) => TypeKind::MaybeUninit(self.substitute(x, args)),
            TypeKind::UnsafeCell(x) => TypeKind::UnsafeCell(self.substitute(x, args)),
            TypeKind::Atomic(x) => TypeKind::Atomic(self.substitute(x, args)),
            TypeKind::Ref { target, mutable } => TypeKind::Ref {
                target: self.substitute(target, args),
                mutable,
            },
            TypeKind::Pointer {
                target,
                space,
                const_,
            } => TypeKind::Pointer {
                target: self.substitute(target, args),
                space,
                const_,
            },
            TypeKind::Slice { element, const_ } => TypeKind::Slice {
                element: self.substitute(element, args),
                const_,
            },
            TypeKind::Function(mut f) => {
                f.params = f
                    .params
                    .into_iter()
                    .map(|x| self.substitute(x, args))
                    .collect();
                f.result = self.substitute(f.result, args);
                TypeKind::Function(f)
            }
            k => k,
        };
        self.intern(k)
    }
    pub fn core_compatible(&self, a: TyId, b: TyId) -> bool {
        use TypeKind::*;
        match (self.kind(a), self.kind(b)) {
            (Function(a), Function(b)) => {
                a.abi == b.abi
                    && a.unsafe_ == b.unsafe_
                    && a.params.len() == b.params.len()
                    && a.params
                        .iter()
                        .zip(&b.params)
                        .all(|(a, b)| self.core_compatible(*a, *b))
                    && self.core_compatible(a.result, b.result)
            }
            (Range(a), Range(b)) => self.core_compatible(*a, *b),
            (ArrayExpr(a, n), ArrayExpr(b, m)) => {
                self.lengths_equal(n, m) && self.core_compatible(*a, *b)
            }
            (Array(a, n), ArrayExpr(b, m)) | (ArrayExpr(b, m), Array(a, n)) => {
                let mut context = self.clone();
                crate::layout::LayoutEngine::new(&mut context)
                    .evaluate_len(m)
                    .is_ok_and(|m| m == *n)
                    && self.core_compatible(*a, *b)
            }
            (Array(a, n), Array(b, m)) => n == m && self.core_compatible(*a, *b),
            (Tuple(a), Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| self.core_compatible(*a, *b))
            }
            (Nominal(x, a), Nominal(y, b)) => {
                x == y
                    && a.len() == b.len()
                    && a.iter().zip(b).all(|(a, b)| self.core_compatible(*a, *b))
            }
            (
                Ref {
                    target: a,
                    mutable: x,
                },
                Ref {
                    target: b,
                    mutable: y,
                },
            ) => x == y && self.core_compatible(*a, *b),
            (
                Pointer {
                    target: a,
                    space: x,
                    const_: c,
                },
                Pointer {
                    target: b,
                    space: y,
                    const_: d,
                },
            ) => x == y && c == d && self.core_compatible(*a, *b),
            (
                Slice {
                    element: a,
                    const_: x,
                },
                Slice {
                    element: b,
                    const_: y,
                },
            ) => x == y && self.core_compatible(*a, *b),
            (Option(a), Option(b))
            | (MaybeUninit(a), MaybeUninit(b))
            | (UnsafeCell(a), UnsafeCell(b))
            | (Atomic(a), Atomic(b)) => self.core_compatible(*a, *b),
            (Result(a, b), Result(c, d)) => {
                self.core_compatible(*a, *c) && self.core_compatible(*b, *d)
            }
            _ => a == b,
        }
    }
    fn lengths_equal(&self, a: &ConstLen, b: &ConstLen) -> bool {
        let mut context = self.clone();
        let mut layout = crate::layout::LayoutEngine::new(&mut context);
        if let (Ok(a), Ok(b)) = (layout.evaluate_len(a), layout.evaluate_len(b)) {
            return a == b;
        }
        if a.integer != b.integer {
            return false;
        }
        use ConstLenKind::*;
        match (&a.kind, &b.kind) {
            (
                Deferred {
                    id: a,
                    args: b,
                    queries: c,
                },
                Deferred {
                    id: d,
                    args: e,
                    queries: f,
                },
            ) => {
                a == d
                    && b.len() == e.len()
                    && c.len() == f.len()
                    && b.iter()
                        .zip(e)
                        .chain(c.iter().zip(f))
                        .all(|(x, y)| self.core_compatible(*x, *y))
            }
            (
                CheckedFits {
                    op: a,
                    left: b,
                    right: c,
                },
                CheckedFits {
                    op: d,
                    left: e,
                    right: f,
                },
            ) => a == d && self.lengths_equal(b, e) && self.lengths_equal(c, f),
            (
                CastFits {
                    target: a,
                    value: b,
                },
                CastFits {
                    target: c,
                    value: d,
                },
            ) => a == c && self.lengths_equal(b, d),
            (Literal(a), Literal(b)) => a == b,
            (Panic, Panic) => true,
            (Assert(a, b), Assert(c, d)) => self.lengths_equal(a, c) && self.lengths_equal(b, d),
            (SizeOf(a), SizeOf(b)) | (AlignOf(a), AlignOf(b)) => self.core_compatible(*a, *b),
            (OffsetOf(a, x), OffsetOf(b, y)) => x == y && self.core_compatible(*a, *b),
            (Unary(x, a), Unary(y, b)) => x == y && self.lengths_equal(a, b),
            (Cast(a), Cast(b)) => self.lengths_equal(a, b),
            (Binary(x, a, b), Binary(y, c, d)) => {
                x == y && self.lengths_equal(a, c) && self.lengths_equal(b, d)
            }
            (If(a, b, c), If(d, e, f)) => {
                self.lengths_equal(a, d) && self.lengths_equal(b, e) && self.lengths_equal(c, f)
            }
            _ => false,
        }
    }
    pub fn try_properties(&mut self, t: TyId) -> crate::diagnostic::Result<Properties> {
        crate::layout::LayoutEngine::new(self).validate_dependencies(t)?;
        Ok(self.properties_inner(t, &mut HashSet::new()))
    }
    pub fn properties(&mut self, t: TyId) -> Properties {
        if crate::layout::LayoutEngine::new(self)
            .validate_dependencies(t)
            .is_err()
        {
            return Properties::default();
        }
        self.properties_inner(t, &mut HashSet::new())
    }
    fn properties_inner(&mut self, t: TyId, seen: &mut HashSet<TyId>) -> Properties {
        use TypeKind::*;
        if !seen.insert(t) {
            return Properties::default();
        }
        let p = match self.kind(t).clone() {
            Unit | Bool | Int(_) | Pointer { .. } | Function(_) => Properties {
                copy: true,
                ..Properties::default()
            },
            Ref { mutable, .. } => Properties {
                copy: !mutable,
                borrow_carrying: true,
                resource: false,
            },
            Slice { const_, .. } => Properties {
                copy: const_,
                borrow_carrying: true,
                resource: false,
            },
            Array(x, _) | ArrayExpr(x, _) | Option(x) => self.properties_inner(x, seen),
            Tuple(v) => self.combine(v, seen),
            Result(x, y) => self.combine(vec![x, y], seen),
            MaybeUninit(x) => {
                let mut p = self.properties_inner(x, seen);
                p.copy = false;
                p.resource = false;
                p
            }
            UnsafeCell(x) => {
                let mut p = self.properties_inner(x, seen);
                p.copy = false;
                p
            }
            Atomic(_) | Never | Param(_) | Range(_) => Properties::default(),
            Nominal(id, args) => {
                if let Some(d) = self.definition(id).cloned() {
                    let v = d.fields().into_iter().map(|f| f.ty).collect::<Vec<_>>();
                    let v = v.into_iter().map(|x| self.substitute(x, &args)).collect();
                    let mut p = self.combine(v, seen);
                    p.resource |= d.resource;
                    p.copy &= d.copy && !p.resource;
                    p
                } else {
                    Properties::default()
                }
            }
        };
        seen.remove(&t);
        p
    }
    fn combine(&mut self, v: Vec<TyId>, seen: &mut HashSet<TyId>) -> Properties {
        let mut p = Properties {
            copy: true,
            ..Properties::default()
        };
        for t in v {
            let q = self.properties_inner(t, seen);
            p.copy &= q.copy;
            p.resource |= q.resource;
            p.borrow_carrying |= q.borrow_carrying;
        }
        p
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemoryOrder {
    Relaxed,
    Acquire,
    Release,
    AcqRel,
    SeqCst,
}
impl TypeContext {
    pub fn display(&self, t: TyId) -> String {
        format!("{:?}", self.kind(t))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConstLen {
    pub kind: ConstLenKind,
    pub integer: Integer,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ConstLenKind {
    /// A length whose original expression must be checked after generic substitution.
    /// Queries retain every branch's layout dependencies. Generic arguments identify
    /// evaluation contexts; they are not themselves layout dependencies.
    Deferred {
        id: ConstExprId,
        args: Vec<TyId>,
        queries: Vec<TyId>,
    },
    Assert(Box<ConstLen>, Box<ConstLen>),
    CheckedFits {
        op: String,
        left: Box<ConstLen>,
        right: Box<ConstLen>,
    },
    CastFits {
        target: Integer,
        value: Box<ConstLen>,
    },
    Panic,
    Literal(u64),
    SizeOf(TyId),
    AlignOf(TyId),
    OffsetOf(TyId, String),
    Unary(String, Box<ConstLen>),
    Binary(String, Box<ConstLen>, Box<ConstLen>),
    Cast(Box<ConstLen>),
    If(Box<ConstLen>, Box<ConstLen>, Box<ConstLen>),
}
impl TypeContext {
    fn substitute_len(&mut self, len: ConstLen, args: &[TyId]) -> ConstLen {
        use ConstLenKind::*;
        let kind = match len.kind {
            Deferred {
                id,
                args: bindings,
                queries,
            } => {
                let bindings = bindings
                    .into_iter()
                    .map(|t| self.substitute(t, args))
                    .collect::<Vec<_>>();
                if let Some(value) = self.cached_const(id, &bindings) {
                    Literal(value)
                } else {
                    Deferred {
                        id,
                        args: bindings,
                        queries: queries
                            .into_iter()
                            .map(|t| self.substitute(t, args))
                            .collect(),
                    }
                }
            }
            CheckedFits { op, left, right } => CheckedFits {
                op,
                left: Box::new(self.substitute_len(*left, args)),
                right: Box::new(self.substitute_len(*right, args)),
            },
            CastFits { target, value } => CastFits {
                target,
                value: Box::new(self.substitute_len(*value, args)),
            },
            Literal(n) => Literal(n),
            Panic => Panic,
            Assert(c, x) => Assert(
                Box::new(self.substitute_len(*c, args)),
                Box::new(self.substitute_len(*x, args)),
            ),
            SizeOf(t) => SizeOf(self.substitute(t, args)),
            AlignOf(t) => AlignOf(self.substitute(t, args)),
            OffsetOf(t, f) => OffsetOf(self.substitute(t, args), f),
            Unary(op, x) => Unary(op, Box::new(self.substitute_len(*x, args))),
            Binary(op, x, y) => Binary(
                op,
                Box::new(self.substitute_len(*x, args)),
                Box::new(self.substitute_len(*y, args)),
            ),
            Cast(x) => Cast(Box::new(self.substitute_len(*x, args))),
            If(c, x, y) => If(
                Box::new(self.substitute_len(*c, args)),
                Box::new(self.substitute_len(*x, args)),
                Box::new(self.substitute_len(*y, args)),
            ),
        };
        ConstLen {
            kind,
            integer: len.integer,
        }
    }
}
