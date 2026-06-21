//! Editor backend session/store.
//!
//! Read-only first cut: load a project, expose file tree, table data,
//! record details, and the reference graph to the frontend.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

mod convert;
mod session;
pub mod types;

pub use session::{EditorSession, SessionStore};
pub use types::*;
