//! Host-facing project runtime for Coflow.
//!
//! This crate is the boundary consumed by CLI, editor, and automation hosts.
//! It currently delegates to `coflow-engine`; keeping this facade separate
//! makes the runtime role explicit without forcing a large file move in the
//! same change.

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
#![allow(clippy::multiple_crate_versions)]

pub use coflow_engine::*;
