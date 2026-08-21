//! Concrete CFD source services owned by the runtime.
//!
//! This is deliberately a value object, not a registration point. Every
//! project has exactly one text loader and one CFD writer.

use crate::api::CfdDimensionWriter;
use crate::cfd_loader::{CfdLoader, CfdWriter};
use std::sync::Arc;

#[derive(Clone)]
pub struct CfdSourceCatalog {
    pub(crate) loader: Arc<CfdLoader>,
    pub(crate) writer: Arc<CfdWriter>,
}

impl std::fmt::Debug for CfdSourceCatalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CfdSourceCatalog")
            .field("format", &"cfd")
            .finish()
    }
}

impl Default for CfdSourceCatalog {
    fn default() -> Self {
        Self {
            loader: Arc::new(CfdLoader),
            writer: Arc::new(CfdWriter::new()),
        }
    }
}

impl CfdSourceCatalog {
    pub(crate) fn loader(&self) -> Arc<CfdLoader> {
        Arc::clone(&self.loader)
    }

    pub(crate) fn writer(&self) -> Arc<CfdWriter> {
        Arc::clone(&self.writer)
    }

    pub(crate) fn dimension_source_manager(&self) -> Arc<dyn CfdDimensionWriter> {
        Arc::clone(&self.writer) as Arc<dyn CfdDimensionWriter>
    }
}
