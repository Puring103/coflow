use crate::{
    CodeGenerator, DataExporter, DimensionSourceManager, LoaderGenerator, SourceProvider,
    SourceWriter, TableManager,
};
use std::collections::BTreeMap;
use std::sync::Arc;

use super::{ProviderRegistrationError, ProviderRegistry};

impl ProviderRegistry {
    /// Registers a shared source provider instance.
    ///
    /// # Errors
    ///
    /// Returns an error when another source provider with the same provider id
    /// has already been registered.
    pub fn register_source_provider_arc<L>(
        &mut self,
        source_provider: Arc<L>,
    ) -> Result<(), ProviderRegistrationError>
    where
        L: SourceProvider + 'static,
    {
        let id = source_provider.descriptor().id;
        let source_provider: Arc<dyn SourceProvider> = source_provider;
        insert_provider(
            &mut self.source_providers,
            "source provider",
            id,
            source_provider,
        )
    }

    /// Registers a shared source writer instance.
    ///
    /// # Errors
    ///
    /// Returns an error when another source writer with the same provider id
    /// has already been registered.
    pub fn register_source_writer_arc<W>(
        &mut self,
        writer: Arc<W>,
    ) -> Result<(), ProviderRegistrationError>
    where
        W: SourceWriter + 'static,
    {
        let id = writer.descriptor().id;
        let writer: Arc<dyn SourceWriter> = writer;
        insert_provider(&mut self.source_writers, "source writer", id, writer)
    }

    /// Registers a shared table manager instance.
    ///
    /// # Errors
    ///
    /// Returns an error when another table manager with the same provider id
    /// has already been registered.
    pub fn register_table_manager_arc<T>(
        &mut self,
        manager: Arc<T>,
    ) -> Result<(), ProviderRegistrationError>
    where
        T: TableManager + 'static,
    {
        let id = manager.descriptor().id;
        let manager: Arc<dyn TableManager> = manager;
        insert_provider(&mut self.table_managers, "table manager", id, manager)
    }

    /// Registers a shared dimension source manager instance.
    ///
    /// # Errors
    ///
    /// Returns an error when another dimension source manager with the same
    /// provider id has already been registered.
    pub fn register_dimension_source_manager_arc<D>(
        &mut self,
        manager: Arc<D>,
    ) -> Result<(), ProviderRegistrationError>
    where
        D: DimensionSourceManager + 'static,
    {
        let id = manager.descriptor().id;
        let manager: Arc<dyn DimensionSourceManager> = manager;
        insert_provider(
            &mut self.dimension_source_managers,
            "dimension source manager",
            id,
            manager,
        )
    }

    /// Registers a shared exporter instance.
    ///
    /// # Errors
    ///
    /// Returns an error when another exporter with the same provider id has
    /// already been registered.
    pub fn register_exporter_arc<E>(
        &mut self,
        exporter: Arc<E>,
    ) -> Result<(), ProviderRegistrationError>
    where
        E: DataExporter + 'static,
    {
        let id = exporter.descriptor().id;
        let exporter: Arc<dyn DataExporter> = exporter;
        insert_provider(&mut self.exporters, "exporter", id, exporter)
    }

    /// Registers a shared code generator instance.
    ///
    /// # Errors
    ///
    /// Returns an error when another code generator with the same provider id
    /// has already been registered.
    pub fn register_codegen_arc<C>(
        &mut self,
        codegen: Arc<C>,
    ) -> Result<(), ProviderRegistrationError>
    where
        C: CodeGenerator + 'static,
    {
        let id = codegen.descriptor().id;
        let codegen: Arc<dyn CodeGenerator> = codegen;
        insert_provider(&mut self.codegens, "codegen", id, codegen)
    }

    /// Registers a shared generated-code loader instance.
    ///
    /// # Errors
    ///
    /// Returns an error when another loader with the same provider id has
    /// already been registered.
    pub fn register_loader_arc<L>(
        &mut self,
        loader: Arc<L>,
    ) -> Result<(), ProviderRegistrationError>
    where
        L: LoaderGenerator + 'static,
    {
        let id = loader.descriptor().id;
        let loader: Arc<dyn LoaderGenerator> = loader;
        insert_provider(&mut self.loaders, "loader generator", id, loader)?;
        self.loader_order.push(id);
        Ok(())
    }
}

fn insert_provider<T: ?Sized>(
    providers: &mut BTreeMap<&'static str, Arc<T>>,
    role: &'static str,
    id: &'static str,
    provider: Arc<T>,
) -> Result<(), ProviderRegistrationError> {
    if providers.contains_key(id) {
        return Err(ProviderRegistrationError::duplicate(role, id));
    }
    providers.insert(id, provider);
    Ok(())
}
