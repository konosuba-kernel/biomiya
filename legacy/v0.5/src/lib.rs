//! Core compilation services. Optional verification never changes Core semantics.
pub mod abi;
pub mod check;
pub mod diagnostic;
pub mod driver;
pub mod extend;
pub mod hir;
pub mod ir;
pub mod layout;
pub mod linkage;
pub mod lower;
pub mod resolve;
pub mod source;
pub mod syntax;
pub mod target;
pub mod toolchain;
pub mod types;

pub const REFERENCE_VERSION: &str = "0.5";
pub const TARGET: &str = "rv64-core-v1";
