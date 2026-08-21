mod bundle;
mod errors;
mod registration;
mod selection;

pub use bundle::CfdBindingBundle;
pub use errors::{CfdBindingError, CfdSourceSelectionError};

use crate::{
    DimensionSourceManager, DimensionSourceManagerDescriptor, CfdSourceAdapter,
    CfdSourceAdapterDescriptor, SourceWriter, TableManager, TableManagerDescriptor, WriterDescriptor,
};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

#[derive(Default, Clone)]
pub struct CfdProviderBindings {
    source_providers: BTreeMap<&'static str, Arc<dyn CfdSourceAdapter>>,
    source_writers: BTreeMap<&'static str, Arc<dyn SourceWriter>>,
    table_managers: BTreeMap<&'static str, Arc<dyn TableManager>>,
    dimension_source_managers: BTreeMap<&'static str, Arc<dyn DimensionSourceManager>>,
}

impl fmt::Debug for CfdProviderBindings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CfdProviderBindings")
            .field(
                "source_providers",
                &self.source_providers.keys().collect::<Vec<_>>(),
            )
            .field(
                "source_writers",
                &self.source_writers.keys().collect::<Vec<_>>(),
            )
            .field(
                "table_managers",
                &self.table_managers.keys().collect::<Vec<_>>(),
            )
            .field(
                "dimension_source_managers",
                &self.dimension_source_managers.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl CfdProviderBindings {
    /// Registers a source provider.
    ///
    /// # Errors
    ///
    /// Returns an error when another source provider with the same provider id
    /// has already been registered.
    pub fn register_source_provider<L>(
        &mut self,
        source_provider: L,
    ) -> Result<(), CfdBindingError>
    where
        L: CfdSourceAdapter + 'static,
    {
        self.register_source_provider_arc(Arc::new(source_provider))
    }

    /// Registers a source writer.
    ///
    /// # Errors
    ///
    /// Returns an error when another source writer with the same provider id
    /// has already been registered.
    pub fn register_source_writer<W>(&mut self, writer: W) -> Result<(), CfdBindingError>
    where
        W: SourceWriter + 'static,
    {
        self.register_source_writer_arc(Arc::new(writer))
    }

    /// Registers a table manager provider.
    ///
    /// # Errors
    ///
    /// Returns an error when another table manager with the same provider id
    /// has already been registered.
    pub fn register_table_manager<T>(&mut self, manager: T) -> Result<(), CfdBindingError>
    where
        T: TableManager + 'static,
    {
        self.register_table_manager_arc(Arc::new(manager))
    }

    /// Registers a dimension source manager provider.
    ///
    /// # Errors
    ///
    /// Returns an error when another dimension source manager with the same
    /// provider id has already been registered.
    pub fn register_dimension_source_manager<D>(
        &mut self,
        manager: D,
    ) -> Result<(), CfdBindingError>
    where
        D: DimensionSourceManager + 'static,
    {
        self.register_dimension_source_manager_arc(Arc::new(manager))
    }

    #[must_use]
    pub fn source_provider(&self, id: &str) -> Option<Arc<dyn CfdSourceAdapter>> {
        self.source_providers.get(id).cloned()
    }

    #[must_use]
    pub fn source_writer(&self, id: &str) -> Option<Arc<dyn SourceWriter>> {
        self.source_writers.get(id).cloned()
    }

    #[must_use]
    pub fn table_manager(&self, id: &str) -> Option<Arc<dyn TableManager>> {
        self.table_managers.get(id).cloned()
    }

    #[must_use]
    pub fn dimension_source_manager(&self, id: &str) -> Option<Arc<dyn DimensionSourceManager>> {
        self.dimension_source_managers.get(id).cloned()
    }

    #[must_use]
    pub fn source_writers(&self) -> Vec<Arc<dyn SourceWriter>> {
        self.source_writers.values().cloned().collect()
    }

    #[must_use]
    pub fn source_writer_descriptors(&self) -> Vec<&'static WriterDescriptor> {
        self.source_writers
            .values()
            .map(|writer| writer.descriptor())
            .collect()
    }

    #[must_use]
    pub fn table_manager_descriptors(&self) -> Vec<&'static TableManagerDescriptor> {
        self.table_managers
            .values()
            .map(|manager| manager.descriptor())
            .collect()
    }

    #[must_use]
    pub fn dimension_source_manager_descriptors(
        &self,
    ) -> Vec<&'static DimensionSourceManagerDescriptor> {
        self.dimension_source_managers
            .values()
            .map(|manager| manager.descriptor())
            .collect()
    }

    #[must_use]
    pub fn source_provider_descriptors(&self) -> Vec<&'static CfdSourceAdapterDescriptor> {
        self.source_providers
            .values()
            .map(|source_provider| source_provider.descriptor())
            .collect()
    }

    #[must_use]
    pub fn source_providers(&self) -> Vec<Arc<dyn CfdSourceAdapter>> {
        self.source_providers.values().cloned().collect()
    }

}
