//! Real-source entry to the bounded pilot, without the archived compiler.
use crate::{
    check,
    diagnostic::{Diagnostic, Result},
    frontend,
    model::Compilation,
    source::{FileId, Span},
};
/// A per-file resource limit, not a restriction on the language grammar.
pub const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;
pub fn compile(file: FileId, source: &str) -> Result<Compilation> {
    compile_files(&[(file, source)])
}
pub fn compile_files(files: &[(FileId, &str)]) -> Result<Compilation> {
    let mut programs = Vec::new();
    for &(file, source) in files {
        if source.len() > MAX_SOURCE_BYTES {
            return Err(Diagnostic::new(
                "I_LIMIT",
                Span::new(file, 0, 0),
                format!(
                    "source has {} bytes; prototype limit is {MAX_SOURCE_BYTES}",
                    source.len()
                ),
            ));
        }
        programs.push(frontend::parse(file, source)?);
    }
    check::check(&frontend::resolve_files(programs)?)
}
