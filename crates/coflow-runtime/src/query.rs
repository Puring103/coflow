use std::collections::BTreeSet;

use coflow_api::{ProviderRegistry, WriterCapabilities};
use coflow_cft::{CftContainer, CompiledSchema};
use coflow_data_model::{CfdDataModel, CfdPathSegment, CfdRecordId, CfdValue};
use coflow_project::Project;

use crate::{
    DiagnosticsStore, DimensionInfo, EffectiveFieldWrite, FileIndex, FileTreeNode, FileTreeOptions,
    ProjectSession, RecordCoordinate, RecordIndex, RecordView, RefTargetInfo, SourceIndex,
};

/// Read-only capability over one immutable project generation.
///
/// Hosts receive this view instead of the owning runtime session, so query
/// code cannot reach mutation methods or replace the active generation.
#[derive(Debug, Clone, Copy)]
pub struct ProjectQueries<'a> {
    session: &'a ProjectSession,
    revision: u64,
}

impl<'a> ProjectQueries<'a> {
    pub(crate) const fn new(session: &'a ProjectSession, revision: u64) -> Self {
        Self { session, revision }
    }

    #[must_use]
    pub const fn revision(self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn project(self) -> &'a Project {
        self.session.project()
    }

    #[must_use]
    pub const fn schema(self) -> &'a CftContainer {
        self.session.schema()
    }

    #[must_use]
    pub const fn compiled_schema(self) -> &'a CompiledSchema {
        self.session.compiled_schema()
    }

    #[must_use]
    pub const fn model(self) -> &'a CfdDataModel {
        self.session.model()
    }

    #[must_use]
    pub const fn diagnostics(self) -> &'a DiagnosticsStore {
        self.session.diagnostics()
    }

    #[must_use]
    pub const fn sources(self) -> &'a SourceIndex {
        self.session.sources()
    }

    #[must_use]
    pub const fn records(self) -> &'a RecordIndex {
        self.session.records()
    }

    #[must_use]
    pub const fn files(self) -> &'a FileIndex {
        self.session.files()
    }

    #[must_use]
    pub fn has_diagnostics(self) -> bool {
        self.session.has_diagnostics()
    }

    #[must_use]
    pub fn id_for_coordinate(self, actual_type: &str, key: &str) -> Option<CfdRecordId> {
        self.session.id_for_coordinate(actual_type, key)
    }

    #[must_use]
    pub fn coordinate_of(self, id: CfdRecordId) -> Option<RecordCoordinate> {
        self.session.coordinate_of(id)
    }

    #[must_use]
    pub fn file_for_record(self, actual_type: &str, key: &str) -> Option<&'a str> {
        self.session.file_for_record(actual_type, key)
    }

    pub fn coordinates_in_file(self, file: &str) -> impl Iterator<Item = &'a RecordCoordinate> + 'a {
        self.session.coordinates_in_file(file)
    }

    #[must_use]
    pub fn enum_int_value(self, enum_name: &str, variant: &str) -> Option<i64> {
        self.session.enum_int_value(enum_name, variant)
    }

    #[must_use]
    pub fn enum_variants(self, enum_name: &str) -> Vec<String> {
        self.session.enum_variants(enum_name)
    }

    #[must_use]
    pub fn dimensions(self) -> Vec<DimensionInfo> {
        self.session.dimensions()
    }

    #[must_use]
    pub fn dimension_synthesized_types(self) -> BTreeSet<String> {
        self.session.dimension_synthesized_types()
    }

    #[must_use]
    pub fn dimension(self, name: &str) -> Option<DimensionInfo> {
        self.session.dimension(name)
    }

    #[must_use]
    pub fn record_view(self, actual_type: &str, key: &str) -> Option<RecordView<'a>> {
        self.session.record_view(actual_type, key)
    }

    #[must_use]
    pub fn field_value(
        self,
        actual_type: &str,
        key: &str,
        path: &[CfdPathSegment],
    ) -> Option<&'a CfdValue> {
        self.session.field_value(actual_type, key, path)
    }

    #[must_use]
    pub fn effective_field_write(
        self,
        coordinate: &RecordCoordinate,
        path: &[CfdPathSegment],
    ) -> Option<EffectiveFieldWrite> {
        self.session.effective_field_write(coordinate, path)
    }

    #[must_use]
    pub fn ref_targets(self, expected_type: &str) -> Vec<RefTargetInfo> {
        self.session.ref_targets(expected_type)
    }

    pub fn record_views_in_file(self, file: &str) -> impl Iterator<Item = RecordView<'a>> + 'a {
        self.session.record_views_in_file(file)
    }

    #[must_use]
    pub fn file_tree(self) -> Vec<FileTreeNode> {
        self.session.file_tree()
    }

    #[must_use]
    pub fn file_tree_with(self, options: FileTreeOptions) -> Vec<FileTreeNode> {
        self.session.file_tree_with(options)
    }

    #[must_use]
    pub const fn loader_extensions(self) -> &'a BTreeSet<String> {
        self.session.loader_extensions()
    }

    pub(crate) fn writer_capabilities_for_file(
        self,
        registry: &ProviderRegistry,
        file: &str,
    ) -> WriterCapabilities {
        let Some(entry) = self
            .files()
            .source_for_display(file)
            .and_then(|source_id| self.sources().entries().get(source_id.index()))
        else {
            return WriterCapabilities::read_only().with_provider_id("unknown");
        };
        registry.source_writer(&entry.provider_id).map_or_else(
            || WriterCapabilities::read_only().with_provider_id(entry.provider_id.clone()),
            |writer| {
                writer
                    .capabilities(&entry.source)
                    .with_provider_id(entry.provider_id.clone())
            },
        )
    }
}
