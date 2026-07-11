mod errors;
mod registration;
mod selection;

pub use errors::{ProviderRegistrationError, SourceProviderSelectionError};

use crate::{
    CodeGenerator, CodegenDescriptor, DataExporter, DimensionSourceManager,
    DimensionSourceManagerDescriptor, ExporterDescriptor, SourceProvider, SourceProviderDescriptor,
    SourceWriter, TableManager, TableManagerDescriptor, WriterDescriptor,
};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

#[derive(Default, Clone)]
pub struct ProviderRegistry {
    source_providers: BTreeMap<&'static str, Arc<dyn SourceProvider>>,
    source_writers: BTreeMap<&'static str, Arc<dyn SourceWriter>>,
    table_managers: BTreeMap<&'static str, Arc<dyn TableManager>>,
    dimension_source_managers: BTreeMap<&'static str, Arc<dyn DimensionSourceManager>>,
    exporters: BTreeMap<&'static str, Arc<dyn DataExporter>>,
    codegens: BTreeMap<&'static str, Arc<dyn CodeGenerator>>,
}

impl fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderRegistry")
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
            .field("exporters", &self.exporters.keys().collect::<Vec<_>>())
            .field("codegens", &self.codegens.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ProviderRegistry {
    /// Registers a source provider.
    ///
    /// # Errors
    ///
    /// Returns an error when another source provider with the same provider id
    /// has already been registered.
    pub fn register_source_provider<L>(
        &mut self,
        source_provider: L,
    ) -> Result<(), ProviderRegistrationError>
    where
        L: SourceProvider + 'static,
    {
        self.register_source_provider_arc(Arc::new(source_provider))
    }

    /// Registers a source writer.
    ///
    /// # Errors
    ///
    /// Returns an error when another source writer with the same provider id
    /// has already been registered.
    pub fn register_source_writer<W>(&mut self, writer: W) -> Result<(), ProviderRegistrationError>
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
    pub fn register_table_manager<T>(&mut self, manager: T) -> Result<(), ProviderRegistrationError>
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
    ) -> Result<(), ProviderRegistrationError>
    where
        D: DimensionSourceManager + 'static,
    {
        self.register_dimension_source_manager_arc(Arc::new(manager))
    }

    /// Registers an exporter provider.
    ///
    /// # Errors
    ///
    /// Returns an error when another exporter with the same provider id has
    /// already been registered.
    pub fn register_exporter<E>(&mut self, exporter: E) -> Result<(), ProviderRegistrationError>
    where
        E: DataExporter + 'static,
    {
        self.register_exporter_arc(Arc::new(exporter))
    }

    /// Registers a code generator provider.
    ///
    /// # Errors
    ///
    /// Returns an error when another code generator with the same provider id
    /// has already been registered.
    pub fn register_codegen<C>(&mut self, codegen: C) -> Result<(), ProviderRegistrationError>
    where
        C: CodeGenerator + 'static,
    {
        self.register_codegen_arc(Arc::new(codegen))
    }

    #[must_use]
    pub fn source_provider(&self, id: &str) -> Option<Arc<dyn SourceProvider>> {
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
    pub fn exporter(&self, id: &str) -> Option<Arc<dyn DataExporter>> {
        self.exporters.get(id).cloned()
    }

    #[must_use]
    pub fn codegen(&self, id: &str) -> Option<Arc<dyn CodeGenerator>> {
        self.codegens.get(id).cloned()
    }

    #[must_use]
    pub fn source_provider_descriptors(&self) -> Vec<&'static SourceProviderDescriptor> {
        self.source_providers
            .values()
            .map(|source_provider| source_provider.descriptor())
            .collect()
    }

    #[must_use]
    pub fn source_providers(&self) -> Vec<Arc<dyn SourceProvider>> {
        self.source_providers.values().cloned().collect()
    }

    #[must_use]
    pub fn exporter_descriptors(&self) -> Vec<&'static ExporterDescriptor> {
        self.exporters
            .values()
            .map(|exporter| exporter.descriptor())
            .collect()
    }

    #[must_use]
    pub fn codegen_descriptors(&self) -> Vec<&'static CodegenDescriptor> {
        self.codegens
            .values()
            .map(|codegen| codegen.descriptor())
            .collect()
    }
}
