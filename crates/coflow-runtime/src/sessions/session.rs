use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use crate::api::DiagnosticSet;
use crate::data_model::{
    CfdDataModel, CfdPath, CfdPathSegment, CfdRecordHandle, CfdValue, RecordCoordinate,
};
use crate::project::{normalize_path, path_is_same_or_descendant, path_to_slash, Project};
use coflow_language::cft::{CftModuleSet, CftSchema};

use crate::checks::CheckDiagnosticStore;
use crate::dimensions::{dimensions_for_project, DimensionInfo};
use crate::files::{self, FileTreeNode, FileTreeOptions};
use crate::indexes::{DiagnosticsStore, FileIndex, RecordIndex, SourceIndex};
use crate::inspection::records::{EffectiveFieldWrite, RecordView, RefTargetInfo};
use crate::loading::load::SourceDataCache;
use crate::writes::record_value_at_path;
use crate::ProjectExecutionStats;

#[derive(Debug)]
pub(crate) struct ProjectSession {
    pub(crate) project: Project,
    pub(crate) modules: Arc<CftModuleSet>,
    pub(crate) schema: Arc<CftSchema>,
    pub(crate) model: CfdDataModel,
    pub(crate) diagnostics: DiagnosticsStore,
    pub(crate) sources: SourceIndex,
    pub(crate) records: RecordIndex,
    pub(crate) files: FileIndex,
    pub(crate) source_data: SourceDataCache,
    pub(crate) check_state: CheckDiagnosticStore,
    pub(crate) execution_stats: ProjectExecutionStats,
}

impl ProjectSession {
    #[must_use]
    pub(crate) fn schema(&self) -> &CftSchema {
        &self.schema
    }

    #[must_use]
    pub(crate) const fn model(&self) -> &CfdDataModel {
        &self.model
    }

    #[must_use]
    pub(crate) const fn diagnostics(&self) -> &DiagnosticsStore {
        &self.diagnostics
    }

    #[must_use]
    pub(crate) const fn sources(&self) -> &SourceIndex {
        &self.sources
    }

    #[must_use]
    pub(crate) const fn records(&self) -> &RecordIndex {
        &self.records
    }

    #[must_use]
    pub(crate) const fn files(&self) -> &FileIndex {
        &self.files
    }

    #[must_use]
    pub(crate) const fn execution_stats(&self) -> &ProjectExecutionStats {
        &self.execution_stats
    }

    #[must_use]
    pub(crate) fn into_diagnostics(self) -> DiagnosticSet {
        self.diagnostics.into_set()
    }

    #[must_use]
    pub(crate) fn into_schema_session(self) -> ProjectSchemaSession {
        ProjectSchemaSession {
            project: self.project,
            modules: self.modules,
            schema: Some(self.schema),
            diagnostics: self.diagnostics,
        }
    }

    #[must_use]
    pub(crate) fn has_diagnostics(&self) -> bool {
        !self.diagnostics.is_empty()
    }

    /// Resolve a wire `(actual_type, key)` coordinate to its internal model
    /// id. Returns `None` when no record matches — callers surface an
    /// `EditorError::NotFound` rather than panic.
    #[must_use]
    pub(crate) fn id_for_coordinate(
        &self,
        actual_type: &str,
        key: &str,
    ) -> Option<CfdRecordHandle> {
        self.records.id_for_coordinate(actual_type, key)
    }

    /// Inverse of [`Self::id_for_coordinate`]: given an internal record id,
    /// return the wire coordinate. Lives here so model id leakage stays
    /// confined to the engine boundary.
    #[must_use]
    pub(crate) fn coordinate_of(&self, id: CfdRecordHandle) -> Option<RecordCoordinate> {
        self.records.get(id).map(|r| r.coordinate.clone())
    }

    /// Look up the project-relative file that backs a record, addressed by
    /// its wire coordinate.
    #[must_use]
    pub(crate) fn file_for_record(&self, actual_type: &str, key: &str) -> Option<&str> {
        self.records.file_for_coordinate(actual_type, key)
    }

    /// Resolved dimension metadata for the project.
    #[must_use]
    pub(crate) fn dimensions(&self) -> Vec<DimensionInfo> {
        dimensions_for_project(&self.project, &self.schema, &self.model, &self.source_data)
    }

    /// Compose a read-only [`RecordView`] for a coordinate. Returns `None`
    /// when no record matches — typically a stale coordinate after a rename.
    #[must_use]
    pub(crate) fn record_view(&self, actual_type: &str, key: &str) -> Option<RecordView<'_>> {
        let record_ref = self.records.get_by_coordinate(actual_type, key)?;
        let record = self.model.record(record_ref.id)?;
        Some(RecordView {
            coordinate: record_ref.coordinate.clone(),
            display_path: record_ref.display_path.as_str(),
            record,
            origin: &record_ref.origin,
        })
    }

    /// Read a record field by model path through the same path resolver the
    /// write engine uses for current-value checks.
    #[must_use]
    pub(crate) fn field_value(
        &self,
        actual_type: &str,
        key: &str,
        path: &[CfdPathSegment],
    ) -> Option<&CfdValue> {
        let record = self.record_view(actual_type, key)?;
        record_value_at_path(
            record.record,
            &CfdPath {
                segments: path.to_vec(),
            },
        )
    }

    #[must_use]
    pub(crate) fn effective_field_write(
        &self,
        coordinate: &RecordCoordinate,
        path: &[CfdPathSegment],
    ) -> Option<EffectiveFieldWrite> {
        let record_ref = self
            .records
            .get_by_coordinate(&coordinate.actual_type, &coordinate.id)?;
        let target_path = path.to_vec();
        let target_record = self.model.record(record_ref.id)?;
        let old_value = record_value_at_path(
            target_record,
            &CfdPath {
                segments: target_path.clone(),
            },
        )
        .cloned();
        Some(EffectiveFieldWrite {
            host: coordinate.clone(),
            target: record_ref.coordinate.clone(),
            file_path: record_ref.display_path.clone(),
            field_path: target_path,
            old_value,
        })
    }

    #[must_use]
    pub(crate) fn ref_targets(&self, expected_type: &str) -> Vec<RefTargetInfo> {
        let mut targets = Vec::new();
        let schema = self.schema();
        let Some(assignable_types) = schema.concrete_assignable_types(expected_type) else {
            return targets;
        };
        for type_name in assignable_types {
            for (_, record) in self.model.records_of_type(&type_name) {
                let Some(file_path) = self.file_for_record(record.actual_type(), &record.id) else {
                    continue;
                };
                targets.push(RefTargetInfo {
                    coordinate: record.coordinate(),
                    file_path: file_path.to_string(),
                });
            }
        }
        targets.sort_by(|a, b| {
            a.coordinate
                .actual_type
                .cmp(&b.coordinate.actual_type)
                .then_with(|| a.coordinate.id.cmp(&b.coordinate.id))
        });
        targets.dedup_by(|a, b| a.coordinate == b.coordinate);
        targets
    }

    /// Iterate read-only views of every record backed by `file`.
    pub(crate) fn record_views_in_file<'a>(
        &'a self,
        file: &str,
    ) -> impl Iterator<Item = RecordView<'a>> + 'a {
        self.records.ids_in_file(file).iter().filter_map(move |id| {
            let record_ref = self.records.get(*id)?;
            let record = self.model.record(*id)?;
            Some(RecordView {
                coordinate: record_ref.coordinate.clone(),
                display_path: record_ref.display_path.as_str(),
                record,
                origin: &record_ref.origin,
            })
        })
    }

    /// File-tree view of the project and its selected source roots.
    #[must_use]
    pub(crate) fn file_tree(&self) -> Vec<FileTreeNode> {
        let mut options = FileTreeOptions {
            in_sources: BTreeSet::new(),
            schema_roots: BTreeSet::new(),
            data_roots: BTreeSet::new(),
        };
        for path in self.project.config().schema.paths() {
            let absolute = self.project.resolve_path(path);
            if let Ok(relative) = absolute.strip_prefix(self.project.root_dir()) {
                options.schema_roots.insert(path_to_slash(relative));
            }
        }
        for path in self.project.cfd_input_paths() {
            let absolute = self.project.resolve_path(path);
            if let Ok(relative) = absolute.strip_prefix(self.project.root_dir()) {
                options.data_roots.insert(path_to_slash(relative));
            }
        }
        for source in self.files.source_files() {
            options.in_sources.insert(display_source_path(source));
        }
        if let Ok(schema_sources) = self.project.schema_sources() {
            for source in schema_sources {
                if let Ok(relative) = source.canonical_path.strip_prefix(self.project.root_dir()) {
                    options.in_sources.insert(path_to_slash(relative));
                }
            }
        }
        self.file_tree_with(&options)
    }

    /// File-tree view using source paths and schema/data membership.
    #[must_use]
    pub(crate) fn file_tree_with(&self, options: &FileTreeOptions) -> Vec<FileTreeNode> {
        let mut tree = files::build_file_tree(
            self.project.root_dir(),
            &options.in_sources,
            &options.schema_roots,
            &options.data_roots,
            &BTreeSet::new(),
        );
        let project_root = normalize_path(self.project.root_dir());
        for (configured, in_schema, in_data) in self
            .project
            .config()
            .schema
            .paths()
            .iter()
            .map(|path| (path.as_path(), true, false))
            .chain(
                self.project
                    .cfd_input_paths()
                    .map(|path| (path, false, true)),
            )
        {
            let absolute = normalize_path(&self.project.resolve_path(configured));
            if path_is_same_or_descendant(&absolute, &project_root) {
                continue;
            }
            let display_path = path_to_slash(&normalize_path(&absolute));
            if tree.iter().any(|node| {
                node.path == display_path
                    || node.is_dir && display_path.starts_with(&format!("{}/", node.path))
            }) {
                continue;
            }
            if absolute.is_dir() {
                tree.retain(|node| !node.path.starts_with(&format!("{display_path}/")));
            }
            if let Some(node) = files::build_external_source_subtree(
                &absolute,
                &display_path,
                &options.in_sources,
                in_schema,
                in_data,
            ) {
                tree.push(node);
            }
        }
        files::sort_file_tree(&mut tree);
        let dimension_files = self.source_data.dimension_only_files(&self.project);

        annotate_dimensions(&mut tree, &dimension_files);

        tree
    }
}

#[derive(Debug, Clone)]
pub struct ProjectSchemaSession {
    pub(crate) project: Project,
    pub(crate) modules: Arc<CftModuleSet>,
    pub(crate) schema: Option<Arc<CftSchema>>,
    pub(crate) diagnostics: DiagnosticsStore,
}

impl ProjectSchemaSession {
    #[must_use]
    pub fn schema(&self) -> Option<&CftSchema> {
        self.schema.as_deref()
    }

    /// Parsed CFT modules paired with this schema attempt for language hosts.
    #[must_use]
    pub fn modules(&self) -> &CftModuleSet {
        &self.modules
    }

    #[must_use]
    pub const fn diagnostics(&self) -> &DiagnosticsStore {
        &self.diagnostics
    }

    #[must_use]
    pub fn into_diagnostics(self) -> DiagnosticSet {
        self.diagnostics.into_set()
    }

    #[must_use]
    pub fn has_diagnostics(&self) -> bool {
        !self.diagnostics.is_empty()
    }
}

fn display_source_path(source: &str) -> String {
    if source.contains("://") {
        source.to_string()
    } else {
        path_to_slash(Path::new(source))
    }
}

fn annotate_dimensions(nodes: &mut [FileTreeNode], files: &BTreeSet<&str>) {
    for node in nodes {
        node.dimension_only = !node.is_dir && files.contains(node.path.as_str());
        annotate_dimensions(&mut node.children, files);
    }
}
