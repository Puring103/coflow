use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use crate::api::DiagnosticSet;
use crate::data_model::{
    CfdDataModel, CfdPath, CfdPathSegment, CfdRecordId, CfdValue, RecordCoordinate,
};
use crate::project::{path_to_slash, Project};
use coflow_language::{CftModuleSet, CftSchema};

use crate::checks::CheckDiagnosticStore;
use crate::dimensions::{dimensions_for_project, DimensionInfo, DimensionRuntimePlan};
use crate::files::{self, DimensionGroup, FileTreeNode, FileTreeOptions};
use crate::indexes::{DiagnosticsStore, FileIndex, RecordIndex, SourceIndex};
use crate::load::SourceDataCache;
use crate::records::{EffectiveFieldWrite, RecordView, RefTargetInfo};
use crate::writes::record_value_at_path;
use crate::ProjectExecutionStats;

#[derive(Debug)]
pub(crate) struct ProjectSession {
    pub(crate) project: Project,
    pub(crate) modules: Arc<CftModuleSet>,
    pub(crate) schema: Arc<CftSchema>,
    pub(crate) dimension_plan: Arc<DimensionRuntimePlan>,
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
    pub(crate) fn id_for_coordinate(&self, actual_type: &str, key: &str) -> Option<CfdRecordId> {
        self.records.id_for_coordinate(actual_type, key)
    }

    /// Inverse of [`Self::id_for_coordinate`]: given an internal record id,
    /// return the wire coordinate. Lives here so model id leakage stays
    /// confined to the engine boundary.
    #[must_use]
    pub(crate) fn coordinate_of(&self, id: CfdRecordId) -> Option<RecordCoordinate> {
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
        dimensions_for_project(&self.project, self.dimension_plan.fields())
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
            .get_by_coordinate(&coordinate.actual_type, &coordinate.key)?;
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
                let Some(file_path) = self.file_for_record(record.actual_type(), &record.key)
                else {
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
                .then_with(|| a.coordinate.key.cmp(&b.coordinate.key))
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

    /// File-tree view of the project. All `.cfd` files are visible, while
    /// dimension `out_dirs` become virtual subtrees.
    #[must_use]
    pub(crate) fn file_tree(&self) -> Vec<FileTreeNode> {
        let mut options = FileTreeOptions {
            dimension_groups: Vec::new(),
            in_sources: BTreeSet::new(),
        };
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
        for info in self.dimensions() {
            if let Some(out_dir) = info.out_dir.as_ref() {
                let absolute = self.project.resolve_path(Path::new(out_dir));
                options.dimension_groups.push(DimensionGroup {
                    display_name: info.display_name.clone(),
                    dir: absolute,
                });
            }
        }
        self.file_tree_with(options)
    }

    /// File-tree view using caller-supplied dimension groups and source paths.
    #[must_use]
    pub(crate) fn file_tree_with(&self, options: FileTreeOptions) -> Vec<FileTreeNode> {
        let mut skip: BTreeSet<String> = BTreeSet::new();
        for group in &options.dimension_groups {
            if let Ok(rel) = group.dir.strip_prefix(self.project.root_dir()) {
                let slash = path_to_slash(rel);
                if !slash.is_empty() {
                    skip.insert(slash);
                }
            }
        }
        let mut tree = files::build_file_tree(self.project.root_dir(), &options.in_sources, &skip);
        for group in options.dimension_groups.iter().rev() {
            if let Some(node) = files::build_dimension_subtree(
                self.project.root_dir(),
                group.display_name.clone(),
                &group.dir,
                &options.in_sources,
            ) {
                tree.insert(0, node);
            }
        }
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
