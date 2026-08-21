use crate::{DimensionSourceManager, CfdSourceAdapter, SourceWriter, TableManager};
use std::collections::BTreeMap;
use std::sync::Arc;

use super::{CfdBindingError, CfdProviderBindings};

impl CfdProviderBindings {
    /// Registers a shared source provider instance.
    ///
    /// # Errors
    ///
    /// Returns an error when another source provider with the same provider id
    /// has already been registered.
    pub fn register_source_provider_arc<L>(
        &mut self,
        source_provider: Arc<L>,
    ) -> Result<(), CfdBindingError>
    where
        L: CfdSourceAdapter + 'static,
    {
        let id = source_provider.descriptor().id;
        let source_provider: Arc<dyn CfdSourceAdapter> = source_provider;
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
    ) -> Result<(), CfdBindingError>
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
    ) -> Result<(), CfdBindingError>
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
    ) -> Result<(), CfdBindingError>
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

}

fn insert_provider<T: ?Sized>(
    providers: &mut BTreeMap<&'static str, Arc<T>>,
    role: &'static str,
    id: &'static str,
    provider: Arc<T>,
) -> Result<(), CfdBindingError> {
    if providers.contains_key(id) {
        return Err(CfdBindingError::duplicate(role, id));
    }
    providers.insert(id, provider);
    Ok(())
}
