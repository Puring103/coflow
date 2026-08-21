//! Fixed CFD source catalog used by the runtime.
//!
//! The provider traits remain an implementation detail of the runtime while
//! the host-facing API exposes one concrete catalog. This keeps source
//! selection deterministic: every project reads and writes `.cfd` through the
//! built-in implementation.

use crate::api::CfdProviderBindings;

#[derive(Clone)]
pub struct CfdSourceCatalog {
    pub(crate) bindings: CfdProviderBindings,
}

impl std::fmt::Debug for CfdSourceCatalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CfdSourceCatalog")
            .field("format", &"cfd")
            .finish()
    }
}

impl CfdSourceCatalog {
    pub(crate) const fn from_bindings(bindings: CfdProviderBindings) -> Self {
        Self { bindings }
    }
}

impl std::ops::Deref for CfdSourceCatalog {
    type Target = CfdProviderBindings;

    fn deref(&self) -> &Self::Target {
        &self.bindings
    }
}
