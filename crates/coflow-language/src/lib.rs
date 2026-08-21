//! Unified language boundary for CFT schemas, CFD syntax and structural limits.
//!
//! The facade deliberately keeps the two grammars independent: [`cft`] owns
//! schema compilation, while [`cfd`] remains schema-free and is safe to use by
//! editor tooling before a schema is available.  The implementation modules
//! are currently sourced from the mature parsers; consumers must depend on
//! this crate so the implementation can be merged without another API break.

pub mod cft {
    pub use coflow_cft::*;
}

pub mod cfd {
    pub use coflow_cfd::*;
    pub use coflow_cfd::ast;
}

pub mod limits {
    pub use coflow_structure::*;
}

pub use cfd::{parse_cfd, parse_cfd_with_options, CfdAst, CfdParseOptions};
pub use cft::{build_schema, parse_modules, CftFile, CftModuleSet, CftSchema};
pub use limits::{Span, StructuralLimits};
