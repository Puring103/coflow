//! CFT/CFD 共享词法、语法树、源码位置与结构限制。
//! 声明编译和语义契约由 coflow-core 持有，CFD 语法解析不依赖契约。

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
mod syntax;

pub mod cfd;
pub mod function;
pub mod limits;
pub mod source;

/// CFT syntax and source modules.
pub mod cft {
    pub use crate::module::*;

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
#[allow(clippy::wildcard_imports)]
pub(crate) use source::*;
