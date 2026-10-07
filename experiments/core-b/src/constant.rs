//! Bounded execution of trusted typed HIR. Typing remains the checker's authority.
use biomiya::{
    diagnostic::{Diagnostic, Result},
    hir::{self, ExprKind as E},
    ir,
    layout::LayoutEngine,
    source::Span,
    types::{TyId, TypeContext, TypeKind as T},
};
use std::collections::BTreeMap;

pub const IMAGE_LIMIT: u64 = 64 * 1024 * 1024;
const FUEL: usize = 100_000;
const DEPTH: usize = 128;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstValue {
    pub ty: TyId,
    pub kind: ConstKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wrapper {
    MaybeUninit,
    UnsafeCell,
    Atomic,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstKind {
    Unit,
    Scalar(u64),
    Aggregate {
        variant: Option<usize>,
        fields: Vec<(usize, ConstValue)>,
    },
    Relocation {
        symbol: String,
        addend: i64,
    },
    Uninit,
    Wrapped {
        wrapper: Wrapper,
        value: Box<ConstValue>,
    },
}
#[derive(Clone, Debug, Default)]
pub struct ConstInput {
    pub locals: BTreeMap<hir::LocalId, TyId>,
    pub symbols: BTreeMap<String, TyId>,
}
#[derive(Clone, Debug)]
pub struct DataSpec {
    pub symbol: String,
    pub writable: bool,
    pub exported: bool,
    pub section: Option<String>,
    pub span: Span,
}
fn error(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new("E_CONST", span, message)
}
fn unsupported(span: Span) -> Diagnostic {
    Diagnostic::new(
        "E_CORE_B_UNSUPPORTED",
        span,
        "constant operation outside prototype subset",
    )
}
fn budget(fuel: &mut usize, depth: usize, span: Span) -> Result<()> {
    if *fuel == 0 || depth > DEPTH {
        return Err(Diagnostic::new(
            "I_LIMIT",
            span,
            "constant evaluation budget exceeded",
        ));
    }
    *fuel -= 1;
    Ok(())
}
fn pattern(p: &hir::Pattern) -> Result<()> {
    match p.kind {
        hir::PatternKind::Bind(_) | hir::PatternKind::Wildcard => Ok(()),
        _ => Err(unsupported(p.span)),
    }
}
// Const capability, not source typing: valid Core aggregate/address comparisons
// exist, but this bounded executor implements operations on scalar bits only.
fn scalar_capable(types: &TypeContext, ty: TyId) -> bool {
    matches!(types.kind(ty), T::Bool | T::Int(_))
}
pub fn validate(types: &TypeContext, expr: &hir::Expr, input: &ConstInput) -> Result<()> {
    fn walk(
        types: &TypeContext,
        e: &hir::Expr,
        input: &ConstInput,
        fuel: &mut usize,
        depth: usize,
    ) -> Result<()> {
        budget(fuel, depth, e.span)?;
        let mut child = |e: &hir::Expr| walk(types, e, input, fuel, depth + 1);
        match &e.kind {
            E::Unit | E::Integer(_) | E::Local(_) => {}
            E::Function { symbol } => {
                if !input.symbols.contains_key(symbol) {
                    return Err(error(e.span, "undeclared constant address"));
                }
            }
            E::RawAddress { value, .. } => match &value.kind {
                E::Global { symbol, .. } if input.symbols.contains_key(symbol) => {}
                _ => return Err(unsupported(e.span)),
            },
            E::Unary { op, value }
                if matches!(op.as_str(), "-" | "!" | "~") && scalar_capable(types, value.ty) =>
            {
                child(value)?
            }
            E::Binary { op, left, right }
                if matches!(
                    op.as_str(),
                    "+" | "-"
                        | "*"
                        | "/"
                        | "%"
                        | "=="
                        | "!="
                        | "<"
                        | ">"
                        | "<="
                        | ">="
                        | "&&"
                        | "||"
                        | "&"
                        | "|"
                        | "^"
                ) && scalar_capable(types, left.ty)
                    && scalar_capable(types, right.ty) =>
            {
                child(left)?;
                child(right)?;
            }
            E::Aggregate { fields, .. } => {
                for (_, e) in fields {
                    child(e)?;
                }
            }
            E::Field { base, .. } => child(base)?,
            E::Block {
                statements, tail, ..
            } => {
                for s in statements {
                    match s {
                        hir::Statement::Bind { pattern: p, value } => {
                            pattern(p)?;
                            child(value)?;
                        }
                        hir::Statement::Expr(e) => child(e)?,
                        _ => return Err(unsupported(e.span)),
                    }
                }
                if let Some(t) = tail {
                    child(t)?;
                }
            }
            E::If {
                condition,
                then,
                else_,
            } => {
                child(condition)?;
                child(then)?;
                if let Some(e) = else_ {
                    child(e)?;
                }
            }
            E::Intrinsic {
                name,
                arguments,
                type_arguments,
            } => match name.as_str() {
                "__const.sizeof" | "__const.alignof"
                    if arguments.is_empty() && type_arguments.len() == 1 =>
                {
                    if !concrete(types, type_arguments[0], 0) {
                        return Err(unsupported(e.span));
                    }
                }
                "atomic.new" | "UnsafeCell.new" if arguments.len() == 1 => child(&arguments[0])?,
                _ => return Err(unsupported(e.span)),
            },
            E::Uninit if matches!(types.kind(e.ty), T::MaybeUninit(_)) => {}
            E::Uninit => return Err(error(e.span, "Uninit only allowed in MaybeUninit")),
            _ => return Err(unsupported(e.span)),
        }
        Ok(())
    }
    {
        let mut fuel = FUEL;
        walk(types, expr, input, &mut fuel, 0)
    }
}
// Scope bound, not a new dependency engine: concrete layouts still use the existing engine.
fn concrete(t: &TypeContext, ty: TyId, depth: usize) -> bool {
    if depth > DEPTH {
        return false;
    }
    match t.kind(ty) {
        T::Param(_) | T::ArrayExpr(_, _) => false,
        T::Tuple(xs) => xs.iter().all(|x| concrete(t, *x, depth + 1)),
        T::Nominal(_, xs) => xs.iter().all(|x| concrete(t, *x, depth + 1)),
        T::Array(x, _) | T::Option(x) | T::MaybeUninit(x) | T::UnsafeCell(x) | T::Atomic(x) => {
            concrete(t, *x, depth + 1)
        }
        T::Result(x, y) => concrete(t, *x, depth + 1) && concrete(t, *y, depth + 1),
        _ => true,
    }
}
fn scalar(v: ConstValue, span: Span) -> Result<u64> {
    if let ConstKind::Scalar(n) = v.kind {
        Ok(n)
    } else {
        Err(error(
            span,
            "constant scalar required; uninitialized storage cannot be read",
        ))
    }
}
fn number(t: &TypeContext, ty: TyId, n: u64) -> (i128, i128, i128, u32) {
    match t.kind(ty) {
        T::Bool => (n as i128, 0, 1, 1),
        T::Int(i) => {
            let bits = i.bits();
            let mask = if bits == 64 {
                u64::MAX
            } else {
                (1u64 << bits) - 1
            };
            let n = n & mask;
            if i.signed() {
                let x = ((n as i128) << (128 - bits)) >> (128 - bits);
                (x, -(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1, bits)
            } else {
                (n as i128, 0, (1i128 << bits) - 1, bits)
            }
        }
        _ => unreachable!("trusted HIR scalar type"),
    }
}
fn checked(n: i128, min: i128, max: i128, bits: u32, span: Span) -> Result<u64> {
    if n < min || n > max {
        return Err(error(span, "constant arithmetic overflow"));
    }
    Ok(n as u64
        & if bits == 64 {
            u64::MAX
        } else {
            (1u64 << bits) - 1
        })
}
pub fn evaluate(
    types: &mut TypeContext,
    expr: &hir::Expr,
    input: &ConstInput,
) -> Result<ConstValue> {
    validate(types, expr, input)?;
    fn exec(
        t: &mut TypeContext,
        e: &hir::Expr,
        env: &mut BTreeMap<hir::LocalId, ConstValue>,
        fuel: &mut usize,
        depth: usize,
    ) -> Result<ConstValue> {
        budget(fuel, depth, e.span)?;
        let k = match &e.kind {
            E::Unit => ConstKind::Unit,
            E::Integer(n) => ConstKind::Scalar(*n),
            E::Local(id) => {
                return env
                    .get(id)
                    .cloned()
                    .ok_or_else(|| error(e.span, "constant local not initialized"));
            }
            E::Function { symbol } => ConstKind::Relocation {
                symbol: symbol.clone(),
                addend: 0,
            },
            E::RawAddress { value, .. } => {
                let E::Global { symbol, .. } = &value.kind else {
                    unreachable!()
                };
                ConstKind::Relocation {
                    symbol: symbol.clone(),
                    addend: 0,
                }
            }
            E::Uninit => ConstKind::Uninit,
            E::Unary { op, value } => {
                let v = scalar(exec(t, value, env, fuel, depth + 1)?, e.span)?;
                let (x, min, max, bits) = number(t, value.ty, v);
                ConstKind::Scalar(match op.as_str() {
                    "-" => checked(-x, min, max, bits, e.span)?,
                    "!" => u64::from(v == 0),
                    "~" => {
                        !v & if bits == 64 {
                            u64::MAX
                        } else {
                            (1u64 << bits) - 1
                        }
                    }
                    _ => unreachable!(),
                })
            }
            E::Binary { op, left, right } => {
                let a = scalar(exec(t, left, env, fuel, depth + 1)?, e.span)?;
                if op == "&&" && a == 0 {
                    return Ok(ConstValue {
                        ty: e.ty,
                        kind: ConstKind::Scalar(0),
                    });
                }
                if op == "||" && a != 0 {
                    return Ok(ConstValue {
                        ty: e.ty,
                        kind: ConstKind::Scalar(1),
                    });
                }
                let b = scalar(exec(t, right, env, fuel, depth + 1)?, e.span)?;
                let (x, min, max, bits) = number(t, left.ty, a);
                let (y, ..) = number(t, right.ty, b);
                let n = match op.as_str() {
                    "+" => x.checked_add(y),
                    "-" => x.checked_sub(y),
                    "*" => x.checked_mul(y),
                    "/" | "%" => {
                        if y == 0 || x == min && y == -1 {
                            return Err(error(e.span, "invalid constant division"));
                        }
                        Some(if op == "/" { x / y } else { x % y })
                    }
                    "==" => Some(i128::from(x == y)),
                    "!=" => Some(i128::from(x != y)),
                    "<" => Some(i128::from(x < y)),
                    ">" => Some(i128::from(x > y)),
                    "<=" => Some(i128::from(x <= y)),
                    ">=" => Some(i128::from(x >= y)),
                    "&&" => Some(i128::from(b != 0)),
                    "||" => Some(i128::from(b != 0)),
                    "&" => Some((a & b) as i128),
                    "|" => Some((a | b) as i128),
                    "^" => Some((a ^ b) as i128),
                    _ => unreachable!(),
                }
                .ok_or_else(|| error(e.span, "constant arithmetic overflow"))?;
                let n = if matches!(
                    op.as_str(),
                    "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||"
                ) {
                    n as u64
                } else if matches!(op.as_str(), "&" | "|" | "^") {
                    n as u64
                } else {
                    checked(n, min, max, bits, e.span)?
                };
                ConstKind::Scalar(n)
            }
            E::Aggregate { variant, fields } => {
                let mut values = Vec::new();
                for (id, e) in fields {
                    values.push((*id, exec(t, e, env, fuel, depth + 1)?));
                }
                ConstKind::Aggregate {
                    variant: *variant,
                    fields: values,
                }
            }
            E::Field { base, field } => {
                let base = exec(t, base, env, fuel, depth + 1)?;
                let ConstKind::Aggregate { fields, .. } = base.kind else {
                    return Err(error(e.span, "constant field requires aggregate"));
                };
                return fields
                    .into_iter()
                    .find(|(i, _)| i == field)
                    .map(|(_, v)| v)
                    .ok_or_else(|| error(e.span, "missing constant field"));
            }
            E::Block {
                statements, tail, ..
            } => {
                let mut inner = env.clone();
                for s in statements {
                    match s {
                        hir::Statement::Bind { pattern, value } => {
                            let v = exec(t, value, &mut inner, fuel, depth + 1)?;
                            if let hir::PatternKind::Bind(id) = pattern.kind {
                                inner.insert(id, v);
                            }
                        }
                        hir::Statement::Expr(e) => {
                            exec(t, e, &mut inner, fuel, depth + 1)?;
                        }
                        _ => unreachable!(),
                    }
                }
                return if let Some(tail) = tail {
                    exec(t, tail, &mut inner, fuel, depth + 1)
                } else {
                    Ok(ConstValue {
                        ty: e.ty,
                        kind: ConstKind::Unit,
                    })
                };
            }
            E::If {
                condition,
                then,
                else_,
            } => {
                if scalar(exec(t, condition, env, fuel, depth + 1)?, e.span)? != 0 {
                    return exec(t, then, env, fuel, depth + 1);
                }
                return if let Some(e) = else_ {
                    exec(t, e, env, fuel, depth + 1)
                } else {
                    Ok(ConstValue {
                        ty: e.ty,
                        kind: ConstKind::Unit,
                    })
                };
            }
            E::Intrinsic {
                name,
                arguments,
                type_arguments,
            } => match name.as_str() {
                "__const.sizeof" | "__const.alignof" => {
                    let l = LayoutEngine::new(t).layout(type_arguments[0])?;
                    ConstKind::Scalar(if name == "__const.sizeof" {
                        l.size
                    } else {
                        l.align
                    })
                }
                name => ConstKind::Wrapped {
                    wrapper: match name {
                        "atomic.new" => Wrapper::Atomic,
                        "UnsafeCell.new" => Wrapper::UnsafeCell,
                        _ => unreachable!(),
                    },
                    value: Box::new(exec(t, &arguments[0], env, fuel, depth + 1)?),
                },
            },
            _ => unreachable!("validated constant subset"),
        };
        Ok(ConstValue { ty: e.ty, kind: k })
    }
    {
        let mut fuel = FUEL;
        exec(types, expr, &mut BTreeMap::new(), &mut fuel, 0)
    }
}
pub fn serialize(types: &mut TypeContext, value: &ConstValue, spec: &DataSpec) -> Result<ir::Data> {
    let layout = LayoutEngine::new(types).layout(value.ty)?;
    if layout.size > IMAGE_LIMIT {
        return Err(Diagnostic::new(
            "I_LIMIT",
            spec.span,
            "constant image exceeds 64 MiB",
        ));
    }
    let mut data = ir::Data {
        symbol: spec.symbol.clone(),
        bytes: vec![0; layout.size as usize],
        relocations: vec![],
        align: layout.align,
        writable: spec.writable,
        exported: spec.exported,
        section: spec.section.clone(),
    };
    fn write(
        t: &mut TypeContext,
        v: &ConstValue,
        off: u64,
        data: &mut ir::Data,
        span: Span,
        depth: usize,
    ) -> Result<()> {
        if depth > DEPTH {
            return Err(Diagnostic::new(
                "I_LIMIT",
                span,
                "constant serialization depth exceeded",
            ));
        }
        let l = LayoutEngine::new(t).layout(v.ty)?;
        if off
            .checked_add(l.size)
            .is_none_or(|end| end > data.bytes.len() as u64)
        {
            return Err(error(span, "constant object boundary exceeded"));
        }
        match &v.kind {
            ConstKind::Unit | ConstKind::Uninit => {}
            ConstKind::Scalar(n) => {
                if l.size > 8 {
                    return Err(error(span, "scalar layout too large"));
                }
                data.bytes[off as usize..(off + l.size) as usize]
                    .copy_from_slice(&n.to_le_bytes()[..l.size as usize]);
            }
            ConstKind::Relocation { symbol, addend } => {
                if l.size != 8 {
                    return Err(error(span, "relocation requires RV64 address"));
                }
                data.relocations.push(ir::Relocation {
                    offset: off,
                    symbol: symbol.clone(),
                    addend: *addend,
                });
            }
            ConstKind::Wrapped { wrapper, value } => {
                let inner = match (wrapper, t.kind(v.ty)) {
                    (Wrapper::MaybeUninit, T::MaybeUninit(inner))
                    | (Wrapper::UnsafeCell, T::UnsafeCell(inner))
                    | (Wrapper::Atomic, T::Atomic(inner)) => *inner,
                    _ => return Err(error(span, "wrapper does not match object type")),
                };
                if value.ty != inner {
                    return Err(error(span, "wrapper payload does not match inner type"));
                }
                write(t, value, off, data, span, depth + 1)?;
            }
            ConstKind::Aggregate { variant, fields } => {
                let offsets = if let Some(i) = variant {
                    let vl = l
                        .variants
                        .get(*i)
                        .ok_or_else(|| error(span, "variant outside layout"))?;
                    let tag_size = 4; // LayoutEngine uses a fixed u32 discriminant.
                    data.bytes[off as usize..off as usize + tag_size]
                        .copy_from_slice(&(*i as u64).to_le_bytes()[..tag_size]);
                    vl.fields
                        .iter()
                        .map(|x| x + l.payload_offset.unwrap_or(0))
                        .collect::<Vec<_>>()
                } else {
                    l.fields
                };
                for (i, v) in fields {
                    let offset = *offsets
                        .get(*i)
                        .ok_or_else(|| error(span, "field outside layout"))?;
                    write(t, v, off + offset, data, span, depth + 1)?;
                }
            }
        }
        Ok(())
    }
    write(types, value, 0, &mut data, spec.span, 0)?;
    Ok(data)
}
