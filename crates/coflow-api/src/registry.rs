use crate::{
    CodeGenerator, CodegenDescriptor, DataExporter, DataLoader, DataWriter, DimensionSourceManager,
    DimensionSourceManagerDescriptor, ExporterDescriptor, LoaderDescriptor, ProjectSourceRef,
    TableManager, TableManagerDescriptor, WriterDescriptor,
};
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

#[derive(Default, Clone)]
pub struct ProviderRegistry {
    loaders: BTreeMap<&'static str, Arc<dyn DataLoader>>,
    writers: BTreeMap<&'static str, Arc<dyn DataWriter>>,
    table_managers: BTreeMap<&'static str, Arc<dyn TableManager>>,
    dimension_source_managers: BTreeMap<&'static str, Arc<dyn DimensionSourceManager>>,
    exporters: BTreeMap<&'static str, Arc<dyn DataExporter>>,
    codegens: BTreeMap<&'static str, Arc<dyn CodeGenerator>>,
}

impl fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("loaders", &self.loaders.keys().collect::<Vec<_>>())
            .field("writers", &self.writers.keys().collect::<Vec<_>>())
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
    /// Registers a loader provider.
    ///
    /// # Errors
    ///
    /// Returns an error when another loader with the same provider id has
    /// already been registered.
    pub fn register_loader<L>(&mut self, loader: L) -> Result<(), ProviderRegistrationError>
    where
        L: DataLoader + 'static,
    {
        let id = loader.descriptor().id;
        if self.loaders.contains_key(id) {
            return Err(ProviderRegistrationError::duplicate("loader", id));
        }
        self.loaders.insert(id, Arc::new(loader));
        Ok(())
    }

    /// Registers a writer provider.
    ///
    /// # Errors
    ///
    /// Returns an error when another writer with the same provider id has
    /// already been registered.
    pub fn register_writer<W>(&mut self, writer: W) -> Result<(), ProviderRegistrationError>
    where
        W: DataWriter + 'static,
    {
        let id = writer.descriptor().id;
        if self.writers.contains_key(id) {
            return Err(ProviderRegistrationError::duplicate("writer", id));
        }
        self.writers.insert(id, Arc::new(writer));
        Ok(())
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
        let id = manager.descriptor().id;
        if self.table_managers.contains_key(id) {
            return Err(ProviderRegistrationError::duplicate("table manager", id));
        }
        self.table_managers.insert(id, Arc::new(manager));
        Ok(())
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
        let id = manager.descriptor().id;
        if self.dimension_source_managers.contains_key(id) {
            return Err(ProviderRegistrationError::duplicate(
                "dimension source manager",
                id,
            ));
        }
        self.dimension_source_managers.insert(id, Arc::new(manager));
        Ok(())
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
        let id = exporter.descriptor().id;
        if self.exporters.contains_key(id) {
            return Err(ProviderRegistrationError::duplicate("exporter", id));
        }
        self.exporters.insert(id, Arc::new(exporter));
        Ok(())
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
        let id = codegen.descriptor().id;
        if self.codegens.contains_key(id) {
            return Err(ProviderRegistrationError::duplicate("codegen", id));
        }
        self.codegens.insert(id, Arc::new(codegen));
        Ok(())
    }

    #[must_use]
    pub fn loader(&self, id: &str) -> Option<Arc<dyn DataLoader>> {
        self.loaders.get(id).cloned()
    }

    #[must_use]
    pub fn writer(&self, id: &str) -> Option<Arc<dyn DataWriter>> {
        self.writers.get(id).cloned()
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
    pub fn writers(&self) -> Vec<Arc<dyn DataWriter>> {
        self.writers.values().cloned().collect()
    }

    #[must_use]
    pub fn writer_descriptors(&self) -> Vec<&'static WriterDescriptor> {
        self.writers
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
    pub fn loader_descriptors(&self) -> Vec<&'static LoaderDescriptor> {
        self.loaders
            .values()
            .map(|loader| loader.descriptor())
            .collect()
    }

    #[must_use]
    pub fn loaders(&self) -> Vec<Arc<dyn DataLoader>> {
        self.loaders.values().cloned().collect()
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

    /// Selects a loader by explicit source type or by provider probe result.
    ///
    /// # Errors
    ///
    /// Returns an error when no provider matches, the explicit provider id is
    /// unknown, or multiple providers report the same highest confidence.
    pub fn select_loader(
        &self,
        source: &ProjectSourceRef<'_>,
    ) -> Result<Arc<dyn DataLoader>, LoaderSelectionError> {
        if let Some(source_type) = source.source_type {
            return self
                .loader(source_type)
                .ok_or_else(|| LoaderSelectionError::UnknownLoader {
                    id: source_type.to_string(),
                });
        }

        let mut matches = self
            .loaders
            .values()
            .filter_map(|loader| {
                let probe = loader.probe(source);
                probe.is_match().then(|| (probe.confidence, loader.clone()))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(confidence, _)| Reverse(*confidence));

        let Some((confidence, loader)) = matches.first().cloned() else {
            return Err(LoaderSelectionError::NoLoader);
        };
        let tied = matches
            .iter()
            .filter(|(candidate_confidence, _)| *candidate_confidence == confidence)
            .map(|(_, candidate)| candidate.descriptor().id.to_string())
            .collect::<Vec<_>>();
        if tied.len() > 1 {
            return Err(LoaderSelectionError::AmbiguousLoaders { ids: tied });
        }
        Ok(loader)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoaderSelectionError {
    UnknownLoader { id: String },
    NoLoader,
    AmbiguousLoaders { ids: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRegistrationError {
    provider_kind: &'static str,
    id: String,
}

impl ProviderRegistrationError {
    #[must_use]
    pub fn duplicate(provider_kind: &'static str, id: impl Into<String>) -> Self {
        Self {
            provider_kind,
            id: id.into(),
        }
    }

    #[must_use]
    pub const fn provider_kind(&self) -> &'static str {
        self.provider_kind
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for ProviderRegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "duplicate {} provider id `{}`",
            self.provider_kind, self.id
        )
    }
}

impl std::error::Error for ProviderRegistrationError {}
