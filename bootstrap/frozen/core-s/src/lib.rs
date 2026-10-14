//! Core S pilot interfaces. No old semantic compiler modules are linked.

#[path = "../../src/diagnostic.rs"]
pub mod diagnostic;
#[path = "../../src/syntax/lexer.rs"]
pub mod lexer;
#[path = "../../src/source.rs"]
pub mod source;
#[path = "../../src/syntax/unicode.rs"]
pub mod unicode;

pub mod ast;
pub mod model;
pub mod machine;

#[cfg(feature = "pipeline")]
pub mod pipeline;

#[cfg(feature = "image")]
pub mod encode;
#[cfg(feature = "image")]
pub mod image;
#[cfg(feature = "image")]
pub mod s0;
#[cfg(feature = "pipeline")]
pub mod project;

// Each implementation owner can build independently after H acceptance.
#[cfg(feature = "backend")]
pub mod backend;
#[cfg(feature = "check")]
pub mod check;
#[cfg(feature = "frontend")]
pub mod frontend;

pub type ParseService = fn(source::FileId, &str) -> diagnostic::Result<ast::Program>;
pub type ResolveService = fn(ast::Program) -> diagnostic::Result<model::ResolvedProgram>;
pub type CheckService = fn(&model::ResolvedProgram) -> diagnostic::Result<model::Compilation>;
pub type EmitService = fn(&model::Compilation) -> diagnostic::Result<String>;
