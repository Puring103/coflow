//! Concrete CFD source services owned by the runtime.
//!
//! This is deliberately a value object, not a registration point. Every
//! project has exactly one text format and one staged CFD writer.

use crate::cfd_loader::CfdWriter;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct CfdSourceCatalog {
    writer: Arc<CfdWriter>,
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
            writer: Arc::new(CfdWriter::new()),
        }
    }
}

impl CfdSourceCatalog {
    pub(crate) fn staged_writes() -> Self {
        Self {
            writer: Arc::new(CfdWriter::new()),
        }
    }

    pub(crate) fn writer(&self) -> Arc<CfdWriter> {
        Arc::clone(&self.writer)
    }

    pub(crate) fn dimension_source_manager(&self) -> Arc<CfdWriter> {
        Arc::clone(&self.writer)
    }
}
