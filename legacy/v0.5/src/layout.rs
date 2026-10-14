//! Deterministic rv64-core-v1 layout, including declaration-level generic dependency validation.
use crate::{
    diagnostic::{Diagnostic, Result},
    source::{FileId, Span},
    types::*,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariantLayout {
    pub fields: Vec<u64>,
    pub size: u64,
    pub align: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub size: u64,
    pub align: u64,
    pub fields: Vec<u64>,
    pub variants: Vec<VariantLayout>,
    pub payload_offset: Option<u64>,
}
impl Layout {
    fn scalar(size: u64, align: u64) -> Self {
        Self {
            size,
            align,
            fields: vec![],
            variants: vec![],
            payload_offset: None,
        }
    }
}
fn err(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_LAYOUT", Span::new(FileId(0), 0, 0), message)
}
pub fn align_up(n: u64, a: u64) -> Result<u64> {
    n.checked_add(a - 1)
        .map(|n| n & !(a - 1))
        .ok_or_else(|| err("layout alignment overflows usize"))
}
pub struct LayoutEngine<'a> {
    pub types: &'a mut TypeContext,
    cache: HashMap<TyId, Layout>,
    active: HashSet<TyId>,
}
impl<'a> LayoutEngine<'a> {
    pub fn new(types: &'a mut TypeContext) -> Self {
        Self {
            types,
            cache: HashMap::new(),
            active: HashSet::new(),
        }
    }
    pub fn layout(&mut self, t: TyId) -> Result<Layout> {
        self.validate_dependencies(t)?;
        self.concrete(t)
    }
    fn dependencies(
        &self,
        t: TyId,
        needed: &BTreeMap<DefId, BTreeSet<u32>>,
        params: &mut BTreeSet<u32>,
        edges: &mut BTreeSet<DefId>,
    ) {
        use TypeKind::*;
        match self.types.kind(t) {
            Param(n) => {
                params.insert(*n);
            }
            Nominal(d, args) => {
                edges.insert(*d);
                if let Some(ns) = needed.get(d) {
                    for n in ns {
                        if let Some(t) = args.get(*n as usize) {
                            self.dependencies(*t, needed, params, edges)
                        }
                    }
                }
            }
            ArrayExpr(x, len) => {
                self.dependencies(*x, needed, params, edges);
                self.len_dependencies(len, needed, params, edges)
            }
            Array(x, _) | Option(x) | MaybeUninit(x) | UnsafeCell(x) | Atomic(x) => {
                self.dependencies(*x, needed, params, edges)
            }
            Result(x, y) => {
                self.dependencies(*x, needed, params, edges);
                self.dependencies(*y, needed, params, edges)
            }
            Tuple(v) => {
                for x in v {
                    self.dependencies(*x, needed, params, edges)
                }
            }
            _ => {}
        }
    }
    pub fn validate_dependencies(&self, t: TyId) -> Result<()> {
        let mut needed: BTreeMap<_, BTreeSet<u32>> = self
            .types
            .definitions
            .keys()
            .map(|d| (*d, BTreeSet::new()))
            .collect();
        loop {
            let mut changed = false;
            for (id, d) in &self.types.definitions {
                let mut ps = BTreeSet::new();
                let mut es = BTreeSet::new();
                for t in d
                    .fields()
                    .iter()
                    .map(|f| f.ty)
                    .chain(d.layout_dependencies.iter().copied())
                {
                    self.dependencies(t, &needed, &mut ps, &mut es)
                }
                let ns = needed.get_mut(id).unwrap();
                let old = ns.len();
                ns.extend(ps);
                changed |= old != ns.len();
            }
            if !changed {
                break;
            }
        }
        let mut graph = BTreeMap::new();
        for (id, d) in &self.types.definitions {
            let mut es = BTreeSet::new();
            for t in d
                .fields()
                .iter()
                .map(|f| f.ty)
                .chain(d.layout_dependencies.iter().copied())
            {
                self.dependencies(t, &needed, &mut BTreeSet::new(), &mut es)
            }
            graph.insert(*id, es);
        }
        fn visit(
            d: DefId,
            g: &BTreeMap<DefId, BTreeSet<DefId>>,
            active: &mut BTreeSet<DefId>,
            done: &mut BTreeSet<DefId>,
        ) -> Result<()> {
            if done.contains(&d) {
                return Ok(());
            }
            if !active.insert(d) {
                return Err(err("cyclic by-value nominal layout dependency"));
            }
            if let Some(es) = g.get(&d) {
                for e in es {
                    visit(*e, g, active, done)?
                }
            } else {
                return Err(err("undefined nominal type"));
            }
            active.remove(&d);
            done.insert(d);
            Ok(())
        }
        let mut roots = BTreeSet::new();
        self.dependencies(t, &needed, &mut BTreeSet::new(), &mut roots);
        let mut done = BTreeSet::new();
        for d in roots {
            visit(d, &graph, &mut BTreeSet::new(), &mut done)?
        }
        Ok(())
    }
    fn aggregate(&mut self, fields: Vec<TyId>, packed: bool) -> Result<Layout> {
        let mut l = Layout::scalar(0, 1);
        for t in fields {
            let f = self.concrete(t)?;
            let a = if packed { 1 } else { f.align };
            l.size = align_up(l.size, a)?;
            l.fields.push(l.size);
            l.size = l
                .size
                .checked_add(f.size)
                .ok_or_else(|| err("layout size overflows usize"))?;
            l.align = l.align.max(a);
        }
        l.size = align_up(l.size, l.align)?;
        Ok(l)
    }
    fn enumeration(&mut self, variants: Vec<Vec<TyId>>) -> Result<Layout> {
        if variants.len() as u64 > u32::MAX as u64 + 1 {
            return Err(err("too many enum variants"));
        }
        let mut l = Layout::scalar(0, 4);
        let mut payload = 0;
        for v in variants {
            let p = self.aggregate(v, false)?;
            payload = payload.max(p.size);
            l.align = l.align.max(p.align);
            l.variants.push(VariantLayout {
                fields: p.fields,
                size: p.size,
                align: p.align,
            });
        }
        let offset = align_up(4, l.align)?;
        l.payload_offset = Some(offset);
        l.size = align_up(
            offset
                .checked_add(payload)
                .ok_or_else(|| err("enum layout overflows usize"))?,
            l.align,
        )?;
        Ok(l)
    }
    fn concrete(&mut self, t: TyId) -> Result<Layout> {
        if let Some(l) = self.cache.get(&t) {
            return Ok(l.clone());
        }
        if !self.active.insert(t) {
            return Err(err("recursive concrete layout"));
        }
        let result = self.compute(t);
        self.active.remove(&t);
        if let Ok(l) = &result {
            self.cache.insert(t, l.clone());
        }
        result
    }
    fn compute(&mut self, t: TyId) -> Result<Layout> {
        use TypeKind::*;
        Ok(match self.types.kind(t).clone() {
            Unit => Layout::scalar(0, 1),
            Never => return Err(err("Never is not an object type")),
            Range(_) => return Err(err("range is not an object type")),
            Param(_) => return Err(err("unsubstituted type parameter has no concrete layout")),
            Bool => Layout::scalar(1, 1),
            Int(i) => {
                let n = i.bits() as u64 / 8;
                Layout::scalar(n, n)
            }
            Ref { .. } | Pointer { .. } | Function(_) => Layout::scalar(8, 8),
            Slice { .. } => {
                let mut l = Layout::scalar(16, 8);
                l.fields = vec![0, 8];
                l
            }
            ArrayExpr(x, len) => {
                let n = self.evaluate_len(&len)?;
                let e = self.concrete(x)?;
                Layout::scalar(
                    e.size
                        .checked_mul(n)
                        .ok_or_else(|| err("array layout overflows usize"))?,
                    e.align,
                )
            }
            Array(x, n) => {
                let e = self.concrete(x)?;
                Layout::scalar(
                    e.size
                        .checked_mul(n)
                        .ok_or_else(|| err("array layout overflows usize"))?,
                    e.align,
                )
            }
            Tuple(v) => self.aggregate(v, false)?,
            Option(x) => self.enumeration(vec![vec![x], vec![]])?,
            Result(x, y) => self.enumeration(vec![vec![x], vec![y]])?,
            MaybeUninit(x) | UnsafeCell(x) => self.concrete(x)?,
            Atomic(x) => match self.types.kind(x) {
                Int(Integer::U8 | Integer::U16) => Layout::scalar(4, 4),
                Int(Integer::U32 | Integer::U64 | Integer::Usize)
                | Pointer {
                    space: AddressSpace::Kernel,
                    ..
                } => self.concrete(x)?,
                _ => {
                    return Err(Diagnostic::new(
                        "E_TYPE",
                        Span::new(FileId(0), 0, 0),
                        "unsupported Atomic payload",
                    ));
                }
            },
            Nominal(id, args) => {
                let d = self
                    .types
                    .definition(id)
                    .cloned()
                    .ok_or_else(|| err("undefined nominal type"))?;
                if args.len() != d.params as usize {
                    return Err(err("nominal type argument count mismatch"));
                }
                if d.resource && d.copy {
                    return Err(err("resource type cannot be Copy"));
                }
                if d.packed && d.align.is_some() || d.repr_c && (d.packed || d.align.is_some()) {
                    return Err(Diagnostic::new(
                        "E_TARGET",
                        d.span,
                        "conflicting layout attributes",
                    ));
                }
                let mut l = match d.kind {
                    DefKind::Struct(fields) => {
                        let ts = fields
                            .into_iter()
                            .map(|f| self.types.substitute(f.ty, &args))
                            .collect();
                        self.aggregate(ts, d.packed)?
                    }
                    DefKind::Enum(v) => {
                        if d.repr_c || d.packed {
                            return Err(err("enum cannot use repr(C) or packed"));
                        }
                        let vs = v
                            .into_iter()
                            .map(|v| {
                                v.fields
                                    .into_iter()
                                    .map(|f| self.types.substitute(f.ty, &args))
                                    .collect()
                            })
                            .collect();
                        self.enumeration(vs)?
                    }
                };
                if let Some(a) = d.align {
                    if !a.is_power_of_two() || a > 4096 {
                        return Err(err("alignment must be a power of two from 1 to 4096"));
                    }
                    l.align = l.align.max(a);
                    l.size = align_up(l.size, l.align)?;
                }
                l
            }
        })
    }
}
impl LayoutEngine<'_> {
    fn len_dependencies(
        &self,
        len: &ConstLen,
        needed: &BTreeMap<DefId, BTreeSet<u32>>,
        params: &mut BTreeSet<u32>,
        edges: &mut BTreeSet<DefId>,
    ) {
        use ConstLenKind::*;
        match &len.kind {
            Deferred { queries, .. } => {
                for query in queries {
                    self.dependencies(*query, needed, params, edges);
                }
            }
            SizeOf(t) | AlignOf(t) | OffsetOf(t, _) => self.dependencies(*t, needed, params, edges),
            Unary(_, x) | Cast(x) | CastFits { value: x, .. } => {
                self.len_dependencies(x, needed, params, edges)
            }
            Binary(_, x, y)
            | Assert(x, y)
            | CheckedFits {
                left: x, right: y, ..
            } => {
                self.len_dependencies(x, needed, params, edges);
                self.len_dependencies(y, needed, params, edges)
            }
            If(c, x, y) => {
                self.len_dependencies(c, needed, params, edges);
                self.len_dependencies(x, needed, params, edges);
                self.len_dependencies(y, needed, params, edges)
            }
            Literal(_) | Panic => {}
        }
    }
    fn validate_len_queries(&mut self, len: &ConstLen) -> Result<()> {
        use ConstLenKind::*;
        match &len.kind {
            Deferred { queries, .. } => {
                // Query types are template dependency metadata. The checker
                // replays the original outer AST with concrete lexical prefixes;
                // evaluating nested metadata would reuse stale template locals.
                for query in queries {
                    self.validate_dependencies(*query)?;
                }
            }
            SizeOf(t) | AlignOf(t) | OffsetOf(t, _) => {
                self.layout(*t)?;
            }
            CastFits { value, .. } => {
                if len.integer != Integer::U8 {
                    return Err(Diagnostic::new(
                        "E_TYPE",
                        Span::default(),
                        "checked cast condition must be boolean",
                    ));
                }
                self.validate_len_queries(value)?;
            }
            CheckedFits { op, left, right } => {
                if len.integer != Integer::U8
                    || left.integer != right.integer
                    || !matches!(
                        op.trim_end_matches('?'),
                        "+" | "-" | "*" | "/" | "%" | "<<" | ">>"
                    )
                {
                    return Err(Diagnostic::new(
                        "E_TYPE",
                        Span::default(),
                        "invalid checked constant operand types or operation",
                    ));
                }
                self.validate_len_queries(left)?;
                self.validate_len_queries(right)?;
            }
            Unary(_, x) | Cast(x) => self.validate_len_queries(x)?,
            Binary(_, x, y) | Assert(x, y) => {
                self.validate_len_queries(x)?;
                self.validate_len_queries(y)?;
            }
            If(c, x, y) => {
                self.validate_len_queries(c)?;
                self.validate_len_queries(x)?;
                self.validate_len_queries(y)?;
            }
            Literal(_) | Panic => {}
        }
        Ok(())
    }
    pub fn evaluate_len(&mut self, len: &ConstLen) -> Result<u64> {
        self.validate_len_queries(len)?;
        let value = self.eval_integer(len)?;
        if value < 0 {
            return Err(err("array length is negative"));
        }
        u64::try_from(value).map_err(|_| err("array length exceeds usize"))
    }
    fn eval_integer(&mut self, len: &ConstLen) -> Result<i128> {
        use ConstLenKind::*;
        let integer = len.integer;
        let bits = integer.bits();
        let signed = integer.signed();
        let min = if signed { -(1_i128 << (bits - 1)) } else { 0 };
        let max = if signed {
            (1_i128 << (bits - 1)) - 1
        } else {
            (1_i128 << bits) - 1
        };
        let cast = |v: i128| {
            let mask = (1_i128 << bits) - 1;
            let n = v & mask;
            if signed && n & (1_i128 << (bits - 1)) != 0 {
                n - (1_i128 << bits)
            } else {
                n
            }
        };
        let value = match &len.kind {
            Deferred { id, args, .. } => {
                if integer != Integer::Usize {
                    return Err(Diagnostic::new(
                        "I_BUG",
                        Span::default(),
                        "deferred length has invalid result representation",
                    ));
                }
                self.types.cached_const(*id, args).ok_or_else(|| {
                    Diagnostic::new(
                        "I_BUG",
                        Span::default(),
                        "dependent constant query was not instantiated before layout",
                    )
                })? as i128
            }
            CheckedFits { op, left, right } => {
                if integer != Integer::U8 || left.integer != right.integer {
                    return Err(Diagnostic::new(
                        "E_TYPE",
                        Span::default(),
                        "invalid checked constant operand types",
                    ));
                }
                let x = self.eval_integer(left)?;
                let y = self.eval_integer(right)?;
                let source = left.integer;
                let bits = source.bits();
                let min = if source.signed() {
                    -(1_i128 << (bits - 1))
                } else {
                    0
                };
                let max = if source.signed() {
                    (1_i128 << (bits - 1)) - 1
                } else {
                    (1_i128 << bits) - 1
                };
                let result = match op.as_str().trim_end_matches('?') {
                    "+" => x.checked_add(y),
                    "-" => x.checked_sub(y),
                    "*" => x.checked_mul(y),
                    "/" | "%" if y == 0 || source.signed() && x == min && y == -1 => None,
                    "/" => x.checked_div(y),
                    "%" => x.checked_rem(y),
                    "<<" | ">>" if y < 0 || y >= bits as i128 => None,
                    "<<" => Some(x << y),
                    ">>" => Some(x >> y),
                    _ => {
                        return Err(Diagnostic::new(
                            "E_TYPE",
                            Span::default(),
                            "unsupported checked constant operation",
                        ));
                    }
                };
                i128::from(result.is_some_and(|v| v >= min && v <= max))
            }
            CastFits { target, value } => {
                if integer != Integer::U8 {
                    return Err(Diagnostic::new(
                        "E_TYPE",
                        Span::default(),
                        "checked cast condition must be boolean",
                    ));
                }
                let value = self.eval_integer(value)?;
                let bits = target.bits();
                let min = if target.signed() {
                    -(1_i128 << (bits - 1))
                } else {
                    0
                };
                let max = if target.signed() {
                    (1_i128 << (bits - 1)) - 1
                } else {
                    (1_i128 << bits) - 1
                };
                i128::from(value >= min && value <= max)
            }
            Literal(n) => cast(*n as i128),
            Panic => {
                return Err(Diagnostic::new(
                    "E_CONST",
                    Span::new(FileId(0), 0, 0),
                    "constant panic",
                ));
            }
            Assert(c, x) => {
                if self.eval_integer(c)? == 0 {
                    return Err(Diagnostic::new(
                        "E_CONST",
                        Span::new(FileId(0), 0, 0),
                        "constant assertion failed",
                    ));
                }
                self.eval_integer(x)?
            }
            SizeOf(t) => self.concrete(*t)?.size as i128,
            AlignOf(t) => self.concrete(*t)?.align as i128,
            OffsetOf(t, field) => {
                let layout = self.concrete(*t)?;
                let index = match self.types.kind(*t) {
                    TypeKind::Nominal(d, _) => {
                        let definition = self
                            .types
                            .definition(*d)
                            .ok_or_else(|| err("undefined offsetof type"))?;
                        match &definition.kind {
                            DefKind::Struct(fields) => fields
                                .iter()
                                .position(|f| &f.name == field)
                                .ok_or_else(|| err("unknown offsetof field"))?,
                            _ => return Err(err("offsetof requires struct")),
                        }
                    }
                    TypeKind::Tuple(_) => field
                        .parse::<usize>()
                        .map_err(|_| err("invalid tuple field index"))?,
                    _ => return Err(err("offsetof requires aggregate")),
                };
                *layout
                    .fields
                    .get(index)
                    .ok_or_else(|| err("unknown offsetof field"))? as i128
            }
            Cast(x) => return Ok(cast(self.eval_integer(x)?)),
            If(c, x, y) => {
                if self.eval_integer(c)? != 0 {
                    self.eval_integer(x)?
                } else {
                    self.eval_integer(y)?
                }
            }
            Unary(op, x) => {
                let x = self.eval_integer(x)?;
                match op.as_str() {
                    "-" => -x,
                    "~" => return Ok(cast(!x)),
                    "!" => i128::from(x == 0),
                    _ => return Err(err("unsupported constant unary operation")),
                }
            }
            Binary(op, x, y) => {
                let x = self.eval_integer(x)?;
                if op == "&&" && x == 0 {
                    return Ok(0);
                }
                if op == "||" && x != 0 {
                    return Ok(1);
                }
                let y = self.eval_integer(y)?;
                let wrap = matches!(op.as_str(), "+%" | "-%" | "*%");
                let base = if wrap { &op[..op.len() - 1] } else { op };
                let n = match base {
                    "+" => x.checked_add(y),
                    "-" => x.checked_sub(y),
                    "*" => {
                        if wrap {
                            Some(x.wrapping_mul(y))
                        } else {
                            x.checked_mul(y)
                        }
                    }
                    "/" | "%" => {
                        if y == 0 || signed && x == min && y == -1 {
                            return Err(Diagnostic::new(
                                "E_CONST",
                                Span::default(),
                                "invalid constant division",
                            ));
                        }
                        Some(if base == "/" { x / y } else { x % y })
                    }
                    "<<" | ">>" => {
                        if y < 0 || y >= bits as i128 {
                            return Err(Diagnostic::new(
                                "E_CONST",
                                Span::default(),
                                "invalid constant shift count",
                            ));
                        }
                        return Ok(cast(if base == "<<" { x << y } else { x >> y }));
                    }
                    "&" => return Ok(cast(x & y)),
                    "|" => return Ok(cast(x | y)),
                    "^" => return Ok(cast(x ^ y)),
                    "==" => Some(i128::from(x == y)),
                    "!=" => Some(i128::from(x != y)),
                    "<" => Some(i128::from(x < y)),
                    ">" => Some(i128::from(x > y)),
                    "<=" => Some(i128::from(x <= y)),
                    ">=" => Some(i128::from(x >= y)),
                    "&&" => Some(i128::from(x != 0 && y != 0)),
                    "||" => Some(i128::from(x != 0 || y != 0)),
                    _ => return Err(err("unsupported array length constant operation")),
                }
                .ok_or_else(|| {
                    Diagnostic::new("E_CONST", Span::default(), "constant arithmetic overflow")
                })?;
                if wrap {
                    return Ok(cast(n));
                }
                n
            }
        };
        if value < min || value > max {
            Err(Diagnostic::new(
                "E_CONST",
                Span::default(),
                "constant arithmetic overflow",
            ))
        } else {
            Ok(value)
        }
    }
}
