use crate::{
    CodeGenerator, DataExporter, DimensionSourceManager, SourceProvider, SourceWriter, TableManager,
};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use super::{ProviderRegistrationError, ProviderRegistry};

/// A set of provider roles that is validated and registered atomically.
#[derive(Default, Clone)]
pub struct ProviderBundle {
    source_providers: BTreeMap<&'static str, Arc<dyn SourceProvider>>,
    source_writers: BTreeMap<&'static str, Arc<dyn SourceWriter>>,
    table_managers: BTreeMap<&'static str, Arc<dyn TableManager>>,
    dimension_source_managers: BTreeMap<&'static str, Arc<dyn DimensionSourceManager>>,
    exporters: BTreeMap<&'static str, Arc<dyn DataExporter>>,
    codegens: BTreeMap<&'static str, Arc<dyn CodeGenerator>>,
}

impl fmt::Debug for ProviderBundle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderBundle")
            .field("source_providers", &self.source_providers.keys())
            .field("source_writers", &self.source_writers.keys())
            .field("table_managers", &self.table_managers.keys())
            .field(
                "dimension_source_managers",
                &self.dimension_source_managers.keys(),
            )
            .field("exporters", &self.exporters.keys())
            .field("codegens", &self.codegens.keys())
            .finish()
    }
}

impl ProviderBundle {
    /// Merges every role from another package bundle into this bundle.
    ///
    /// # Errors
    ///
    /// Returns an error without changing this bundle when any role id is
    /// already present in the same role category.
    pub fn merge(&mut self, additions: Self) -> Result<(), ProviderRegistrationError> {
        ensure_available(
            &self.source_providers,
            &additions.source_providers,
            "source provider",
        )?;
        ensure_available(
            &self.source_writers,
            &additions.source_writers,
            "source writer",
        )?;
        ensure_available(
            &self.table_managers,
            &additions.table_managers,
            "table manager",
        )?;
        ensure_available(
            &self.dimension_source_managers,
            &additions.dimension_source_managers,
            "dimension source manager",
        )?;
        ensure_available(&self.exporters, &additions.exporters, "exporter")?;
        ensure_available(&self.codegens, &additions.codegens, "codegen")?;

        self.source_providers.extend(additions.source_providers);
        self.source_writers.extend(additions.source_writers);
        self.table_managers.extend(additions.table_managers);
        self.dimension_source_managers
            .extend(additions.dimension_source_managers);
        self.exporters.extend(additions.exporters);
        self.codegens.extend(additions.codegens);
        Ok(())
    }

    /// Adds a source provider role to this bundle.
    ///
    /// # Errors
    ///
    /// Returns an error when this bundle already contains the same role id.
    pub fn add_source_provider<L>(&mut self, provider: L) -> Result<(), ProviderRegistrationError>
    where
        L: SourceProvider + 'static,
    {
        self.add_source_provider_arc(Arc::new(provider))
    }

    /// Adds a shared source provider role to this bundle.
    ///
    /// # Errors
    ///
    /// Returns an error when this bundle already contains the same role id.
    pub fn add_source_provider_arc<L>(
        &mut self,
        provider: Arc<L>,
    ) -> Result<(), ProviderRegistrationError>
    where
        L: SourceProvider + 'static,
    {
        let id = provider.descriptor().id;
        let provider: Arc<dyn SourceProvider> = provider;
        insert_role(&mut self.source_providers, "source provider", id, provider)
    }

    /// Adds a source writer role to this bundle.
    ///
    /// # Errors
    ///
    /// Returns an error when this bundle already contains the same role id.
    pub fn add_source_writer<W>(&mut self, writer: W) -> Result<(), ProviderRegistrationError>
    where
        W: SourceWriter + 'static,
    {
        self.add_source_writer_arc(Arc::new(writer))
    }

    /// Adds a shared source writer role to this bundle.
    ///
    /// # Errors
    ///
    /// Returns an error when this bundle already contains the same role id.
    pub fn add_source_writer_arc<W>(
        &mut self,
        writer: Arc<W>,
    ) -> Result<(), ProviderRegistrationError>
    where
        W: SourceWriter + 'static,
    {
        let id = writer.descriptor().id;
        let writer: Arc<dyn SourceWriter> = writer;
        insert_role(&mut self.source_writers, "source writer", id, writer)
    }

    /// Adds a table manager role to this bundle.
    ///
    /// # Errors
    ///
    /// Returns an error when this bundle already contains the same role id.
    pub fn add_table_manager<T>(&mut self, manager: T) -> Result<(), ProviderRegistrationError>
    where
        T: TableManager + 'static,
    {
        self.add_table_manager_arc(Arc::new(manager))
    }

    /// Adds a shared table manager role to this bundle.
    ///
    /// # Errors
    ///
    /// Returns an error when this bundle already contains the same role id.
    pub fn add_table_manager_arc<T>(
        &mut self,
        manager: Arc<T>,
    ) -> Result<(), ProviderRegistrationError>
    where
        T: TableManager + 'static,
    {
        let id = manager.descriptor().id;
        let manager: Arc<dyn TableManager> = manager;
        insert_role(&mut self.table_managers, "table manager", id, manager)
    }

    /// Adds a dimension source manager role to this bundle.
    ///
    /// # Errors
    ///
    /// Returns an error when this bundle already contains the same role id.
    pub fn add_dimension_source_manager<D>(
        &mut self,
        manager: D,
    ) -> Result<(), ProviderRegistrationError>
    where
        D: DimensionSourceManager + 'static,
    {
        self.add_dimension_source_manager_arc(Arc::new(manager))
    }

    /// Adds a shared dimension source manager role to this bundle.
    ///
    /// # Errors
    ///
    /// Returns an error when this bundle already contains the same role id.
    pub fn add_dimension_source_manager_arc<D>(
        &mut self,
        manager: Arc<D>,
    ) -> Result<(), ProviderRegistrationError>
    where
        D: DimensionSourceManager + 'static,
    {
        let id = manager.descriptor().id;
        let manager: Arc<dyn DimensionSourceManager> = manager;
        insert_role(
            &mut self.dimension_source_managers,
            "dimension source manager",
            id,
            manager,
        )
    }

    /// Adds an exporter role to this bundle.
    ///
    /// # Errors
    ///
    /// Returns an error when this bundle already contains the same role id.
    pub fn add_exporter<E>(&mut self, exporter: E) -> Result<(), ProviderRegistrationError>
    where
        E: DataExporter + 'static,
    {
        let id = exporter.descriptor().id;
        let exporter: Arc<dyn DataExporter> = Arc::new(exporter);
        insert_role(&mut self.exporters, "exporter", id, exporter)
    }

    /// Adds a code generator role to this bundle.
    ///
    /// # Errors
    ///
    /// Returns an error when this bundle already contains the same role id.
    pub fn add_codegen<C>(&mut self, codegen: C) -> Result<(), ProviderRegistrationError>
    where
        C: CodeGenerator + 'static,
    {
        let id = codegen.descriptor().id;
        let codegen: Arc<dyn CodeGenerator> = Arc::new(codegen);
        insert_role(&mut self.codegens, "codegen", id, codegen)
    }
}

impl ProviderRegistry {
    /// Validates and registers every role in a provider bundle atomically.
    ///
    /// # Errors
    ///
    /// Returns an error without changing the registry when any role id is
    /// already registered.
    pub fn register_bundle(
        &mut self,
        bundle: ProviderBundle,
    ) -> Result<(), ProviderRegistrationError> {
        ensure_available(
            &self.source_providers,
            &bundle.source_providers,
            "source provider",
        )?;
        ensure_available(
            &self.source_writers,
            &bundle.source_writers,
            "source writer",
        )?;
        ensure_available(
            &self.table_managers,
            &bundle.table_managers,
            "table manager",
        )?;
        ensure_available(
            &self.dimension_source_managers,
            &bundle.dimension_source_managers,
            "dimension source manager",
        )?;
        ensure_available(&self.exporters, &bundle.exporters, "exporter")?;
        ensure_available(&self.codegens, &bundle.codegens, "codegen")?;

        let ProviderBundle {
            source_providers,
            source_writers,
            table_managers,
            dimension_source_managers,
            exporters,
            codegens,
        } = bundle;
        self.source_providers.extend(source_providers);
        self.source_writers.extend(source_writers);
        self.table_managers.extend(table_managers);
        self.dimension_source_managers
            .extend(dimension_source_managers);
        self.exporters.extend(exporters);
        self.codegens.extend(codegens);
        Ok(())
    }
}

fn insert_role<T: ?Sized>(
    roles: &mut BTreeMap<&'static str, Arc<T>>,
    role: &'static str,
    id: &'static str,
    provider: Arc<T>,
) -> Result<(), ProviderRegistrationError> {
    if roles.contains_key(id) {
        return Err(ProviderRegistrationError::duplicate(role, id));
    }
    roles.insert(id, provider);
    Ok(())
}

fn ensure_available<T: ?Sized>(
    registered: &BTreeMap<&'static str, Arc<T>>,
    additions: &BTreeMap<&'static str, Arc<T>>,
    role: &'static str,
) -> Result<(), ProviderRegistrationError> {
    if let Some(id) = additions.keys().find(|id| registered.contains_key(**id)) {
        return Err(ProviderRegistrationError::duplicate(role, *id));
    }
    Ok(())
}
