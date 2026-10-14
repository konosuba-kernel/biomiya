//! LP64 integer calling convention pinned by architecture-rv64.md R3.
use crate::{
    diagnostic::{Diagnostic, Result},
    layout::{LayoutEngine, align_up},
    source::{FileId, Span},
    types::*,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extension {
    None,
    SignTo32Then64 { bits: u32 },
    ZeroTo32ThenSign64 { bits: u32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Register(u8),
    Stack { offset: u64 },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub location: Location,
    pub value_offset: u64,
    pub size: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Passing {
    Ignore,
    Direct,
    IndirectSnapshot,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgLoc {
    pub ty: TyId,
    pub passing: Passing,
    pub pieces: Vec<Piece>,
    pub extension: Extension,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReturnLoc {
    Void,
    Never,
    Direct {
        pieces: Vec<Piece>,
        extension: Extension,
    },
    Indirect {
        pointer: Location,
        size: u64,
        align: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallPlan {
    pub abi: Abi,
    pub args: Vec<ArgLoc>,
    pub result: ReturnLoc,
    pub stack_size: u64,
    pub stack_align: u64,
    pub hidden_result: Option<Location>,
}
fn error(s: &str) -> Diagnostic {
    Diagnostic::new("E_TYPE", Span::new(FileId(0), 0, 0), s)
}
fn scalar(k: &TypeKind) -> bool {
    matches!(
        k,
        TypeKind::Int(_)
            | TypeKind::Bool
            | TypeKind::Pointer { .. }
            | TypeKind::Ref { .. }
            | TypeKind::Function(_)
    )
}
fn extension(k: &TypeKind) -> Extension {
    match k {
        TypeKind::Int(i) if i.bits() < 64 => {
            if i.signed() {
                Extension::SignTo32Then64 { bits: i.bits() }
            } else {
                Extension::ZeroTo32ThenSign64 { bits: i.bits() }
            }
        }
        TypeKind::Bool => Extension::ZeroTo32ThenSign64 { bits: 1 },
        _ => Extension::None,
    }
}
/// Validate source-level C ABI eligibility independently of physical classification.
pub fn validate_c_type(types: &mut TypeContext, t: TyId, return_: bool) -> Result<()> {
    c_type(
        types,
        t,
        return_,
        false,
        &mut std::collections::HashSet::new(),
    )
}
fn c_type(
    types: &mut TypeContext,
    t: TyId,
    return_: bool,
    field: bool,
    seen: &mut std::collections::HashSet<TyId>,
) -> Result<()> {
    if !seen.insert(t) {
        return Ok(());
    }
    let k = types.kind(t).clone();
    let r = match k {
        TypeKind::Unit if return_ => Ok(()),
        TypeKind::Int(_) | TypeKind::Bool => Ok(()),
        TypeKind::Pointer {
            target,
            space: AddressSpace::Kernel,
            ..
        } => LayoutEngine::new(types).layout(target).map(|_| ()),
        TypeKind::Function(f) if f.abi == Abi::C => {
            for p in f.params {
                c_type(types, p, false, false, seen)?
            }
            c_type(types, f.result, true, false, seen)
        }
        TypeKind::ArrayExpr(x, n) if field => {
            let length = LayoutEngine::new(types).evaluate_len(&n)?;
            if length == 0 {
                return Err(error("C ABI array fields need positive length"));
            }
            c_type(types, x, false, true, seen)
        }
        TypeKind::Array(x, n) if field && n > 0 => c_type(types, x, false, true, seen),
        TypeKind::Nominal(d, args) => {
            let def = types
                .definition(d)
                .cloned()
                .ok_or_else(|| error("undefined C type"))?;
            let p = types.properties(t);
            if !def.repr_c || p.resource || p.borrow_carrying {
                return Err(error("type is not eligible for C ABI"));
            }
            let fields = match def.kind {
                DefKind::Struct(f) if !f.is_empty() => f,
                _ => return Err(error("C ABI requires nonempty repr(C) struct")),
            };
            if LayoutEngine::new(types).layout(t)?.size == 0 {
                return Err(error("C ABI struct must have nonzero size"));
            }
            for f in fields {
                let ty = types.substitute(f.ty, &args);
                c_type(types, ty, false, true, seen)?
            }
            Ok(())
        }
        _ => Err(error("type is not eligible for C ABI")),
    };
    seen.remove(&t);
    r
}
struct Alloc {
    register: u8,
    stack: u64,
}
impl Alloc {
    fn pieces(&mut self, size: u64, align: u64) -> Result<Vec<Piece>> {
        let mut pieces = vec![];
        let mut offset = 0;
        while offset < size && self.register < 8 {
            pieces.push(Piece {
                location: Location::Register(self.register),
                value_offset: offset,
                size: (size - offset).min(8),
            });
            self.register += 1;
            offset += 8;
        }
        if offset < size {
            self.stack = align_up(self.stack, align.clamp(8, 16))?;
            while offset < size {
                pieces.push(Piece {
                    location: Location::Stack { offset: self.stack },
                    value_offset: offset,
                    size: (size - offset).min(8),
                });
                self.stack = self
                    .stack
                    .checked_add(8)
                    .ok_or_else(|| error("argument stack size overflows usize"))?;
                offset += 8;
            }
        }
        Ok(pieces)
    }
}
pub fn classify(types: &mut TypeContext, f: &FnType) -> Result<CallPlan> {
    if f.abi == Abi::C {
        for t in &f.params {
            validate_c_type(types, *t, false)?
        }
        validate_c_type(types, f.result, true)?
    }
    let mut alloc = Alloc {
        register: 0,
        stack: 0,
    };
    let mut hidden = None;
    let result = if matches!(types.kind(f.result), TypeKind::Never) {
        ReturnLoc::Never
    } else {
        let l = LayoutEngine::new(types).layout(f.result)?;
        if l.size == 0 {
            ReturnLoc::Void
        } else if !scalar(types.kind(f.result)) && (f.abi == Abi::Biomiya || l.size > 16) {
            let p = alloc.pieces(8, 8)?[0].location;
            hidden = Some(p);
            ReturnLoc::Indirect {
                pointer: p,
                size: l.size,
                align: l.align,
            }
        } else {
            ReturnLoc::Direct {
                pieces: Alloc {
                    register: 0,
                    stack: 0,
                }
                .pieces(l.size, l.align)?,
                extension: extension(types.kind(f.result)),
            }
        }
    };
    let mut args = vec![];
    for t in &f.params {
        let l = LayoutEngine::new(types).layout(*t)?;
        let indirect = !scalar(types.kind(*t)) && (f.abi == Abi::Biomiya || l.size > 16);
        let passing = if l.size == 0 {
            Passing::Ignore
        } else if indirect {
            Passing::IndirectSnapshot
        } else {
            Passing::Direct
        };
        let pieces = if l.size == 0 {
            vec![]
        } else if indirect {
            alloc.pieces(8, 8)?
        } else {
            alloc.pieces(l.size, l.align)?
        };
        args.push(ArgLoc {
            ty: *t,
            passing,
            pieces,
            extension: if indirect {
                Extension::None
            } else {
                extension(types.kind(*t))
            },
        });
    }
    Ok(CallPlan {
        abi: f.abi,
        args,
        result,
        stack_size: align_up(alloc.stack, 16)?,
        stack_align: 16,
        hidden_result: hidden,
    })
}
