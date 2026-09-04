//! Unified CFT/CFD language implementation.
//!
//! CFT schema compilation and CFD syntax parsing intentionally remain separate
//! modules, but share one crate so spans, structural limits and diagnostics do
//! not cross a crate boundary. CFD parsing is schema-free and can be used by
//! editor hosts before a schema is available.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]
#![allow(
    clippy::missing_const_for_fn,
    clippy::redundant_pub_crate,
    clippy::use_self
)]

pub mod diagnostics;
pub mod lexical;
mod module;
mod schema;
mod syntax;

pub mod cfd;
pub mod limits;
pub mod source;

/// CFT syntax, modules, schema compilation, and semantic declarations.
pub mod cft {
    pub use crate::module::*;
    pub use crate::schema::*;

    /// Produces the lossless token stream consumed by source tooling.
    #[must_use]
    pub fn tokenize_cft(source: &str) -> Vec<crate::lexical::LosslessToken> {
        crate::lexical::tokenize_lossless(source)
    }

    pub mod syntax {
        pub use crate::syntax::*;
    }
}

// crate 内部仍使用短名称；对外 API 只通过职责命名空间发布。
pub(crate) use diagnostics::*;
pub(crate) use lexical::*;
pub(crate) use module::*;
pub(crate) use schema::*;
pub(crate) use source::*;
